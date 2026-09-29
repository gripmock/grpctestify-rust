use crate::cli::args::HealthArgs;
use crate::grpc::{GrpcClient, GrpcClientConfig, TlsConfig, WireProtocol};
use anyhow::Result;
use std::time::Duration;

fn resolve_tls_config(tls: bool, insecure: bool) -> Option<TlsConfig> {
    (tls || insecure).then(|| super::tls_config_from_flags(None, None, None, insecure))
}

fn client_config(args: &HealthArgs) -> GrpcClientConfig {
    GrpcClientConfig {
        address: args.address.clone(),
        timeout_seconds: args.timeout,
        tls_config: resolve_tls_config(args.tls, args.insecure),
        proto_config: None,
        metadata: None,
        target_service: None,
        compression: crate::grpc::CompressionMode::None,
        connection_id: 0,
        protocol: args
            .protocol
            .parse::<WireProtocol>()
            .unwrap_or(crate::grpc::WireProtocol::Grpc),
        version: env!("CARGO_PKG_VERSION").to_string(),
    }
}

struct HealthResult {
    service: String,
    status: String,
    duration_ms: u64,
}

impl HealthResult {
    fn is_serving(&self) -> bool {
        self.status == "SERVING"
    }

    fn display_target(&self) -> &str {
        if self.service.is_empty() {
            "overall"
        } else {
            &self.service
        }
    }

    fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "status": self.status,
            "duration_ms": self.duration_ms,
            "service": self.service,
        })
    }
}

fn target_services(args: &HealthArgs) -> Vec<String> {
    if args.service.is_empty() {
        vec![String::new()]
    } else {
        args.service.clone()
    }
}

async fn probe_once(args: &HealthArgs, services: &[String]) -> Vec<HealthResult> {
    let config = client_config(args);
    let mut client = match GrpcClient::new(config).await {
        Ok(c) => c,
        Err(_) => {
            return services
                .iter()
                .map(|s| HealthResult {
                    service: s.clone(),
                    status: "UNREACHABLE".to_string(),
                    duration_ms: 0,
                })
                .collect();
        }
    };

    let mut results = Vec::with_capacity(services.len());
    for service in services {
        let start = std::time::Instant::now();
        let request = serde_json::json!({"service": service});
        let status = match client
            .call("grpc.health.v1.Health", "Check", vec![request])
            .await
        {
            Ok(response) => response
                .messages
                .first()
                .and_then(|m| m.get("status").and_then(|s| s.as_str()))
                .unwrap_or("UNKNOWN")
                .to_string(),
            Err(_) => "UNREACHABLE".to_string(),
        };
        results.push(HealthResult {
            service: service.clone(),
            status,
            duration_ms: start.elapsed().as_millis() as u64,
        });
    }
    results
}

fn print_text_result(r: &HealthResult) {
    use crate::report::style::{dim_style, fail_icon, fail_style, pass_icon, pass_style};
    let (icon, status_styled) = if r.is_serving() {
        (
            pass_icon().to_string(),
            pass_style().apply_to(&r.status).to_string(),
        )
    } else {
        (
            fail_icon().to_string(),
            fail_style().apply_to(&r.status).to_string(),
        )
    };
    println!(
        "{icon} {status_styled} {}",
        dim_style().apply_to(format!(
            "· {} · {}",
            r.display_target(),
            crate::report::style::format_duration_ms(r.duration_ms)
        ))
    );
}

fn print_results(args: &HealthArgs, results: &[HealthResult]) -> Result<()> {
    if args.format == "json" {
        let value = match results {
            [single] => single.to_json(),
            many => serde_json::Value::Array(many.iter().map(HealthResult::to_json).collect()),
        };
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else {
        for r in results {
            print_text_result(r);
        }
    }
    Ok(())
}

fn failure_summary(results: &[HealthResult]) -> String {
    results
        .iter()
        .filter(|r| !r.is_serving())
        .map(|r| format!("{}={}", r.display_target(), r.status))
        .collect::<Vec<_>>()
        .join(", ")
}

async fn handle_health_watch(args: &HealthArgs, services: &[String]) -> Result<()> {
    let deadline = std::time::Instant::now() + Duration::from_secs_f64(args.watch_timeout.max(0.0));
    let mut attempt: u32 = 0;

    loop {
        attempt += 1;
        let results = probe_once(args, services).await;
        let all_serving = results.iter().all(HealthResult::is_serving);

        if args.format != "json" {
            use crate::report::style::dim_style;
            for r in &results {
                print_text_result(r);
            }
            if !all_serving {
                println!(
                    "{}",
                    dim_style().apply_to(format!("  (attempt {attempt}, retrying...)"))
                );
            }
        }

        if all_serving {
            if args.format == "json" {
                print_results(args, &results)?;
            } else {
                use crate::report::style::{dim_style, pass_icon, pass_style};
                println!(
                    "{} {} {}",
                    pass_icon(),
                    pass_style().apply_to("healthy"),
                    dim_style().apply_to(format!("· {attempt} attempt(s)"))
                );
            }
            return Ok(());
        }

        if std::time::Instant::now() >= deadline {
            if args.format == "json" {
                print_results(args, &results)?;
            }
            anyhow::bail!(
                "timed out after {}s waiting for healthy: {}",
                args.watch_timeout,
                failure_summary(&results)
            );
        }

        tokio::time::sleep(Duration::from_secs_f64(args.interval.max(0.0))).await;
    }
}

pub async fn handle_health(args: &HealthArgs) -> Result<()> {
    let services = target_services(args);

    if args.watch {
        return handle_health_watch(args, &services).await;
    }

    let results = probe_once(args, &services).await;
    print_results(args, &results)?;

    if !results.iter().all(HealthResult::is_serving) {
        anyhow::bail!("Health check failed: {}", failure_summary(&results));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_flags_stays_plaintext() {
        assert!(resolve_tls_config(false, false).is_none());
    }

    #[test]
    fn tls_flag_requests_verified_tls() {
        let cfg = resolve_tls_config(true, false).unwrap();
        assert!(!cfg.insecure_skip_verify);
    }

    #[test]
    fn insecure_flag_requests_unverified_tls() {
        let cfg = resolve_tls_config(false, true).unwrap();
        assert!(cfg.insecure_skip_verify);
    }

    #[test]
    fn tls_and_insecure_together_stays_unverified() {
        let cfg = resolve_tls_config(true, true).unwrap();
        assert!(
            cfg.insecure_skip_verify,
            "--insecure wins when both are set"
        );
    }

    fn base_args() -> HealthArgs {
        HealthArgs {
            address: "localhost:1".to_string(),
            protocol: "grpc".to_string(),
            service: Vec::new(),
            format: "json".to_string(),
            tls: false,
            insecure: false,
            timeout: 5,
            watch: false,
            interval: 1.0,
            watch_timeout: 60.0,
        }
    }

    #[test]
    fn no_service_flags_checks_overall_only() {
        assert_eq!(target_services(&base_args()), vec![String::new()]);
    }

    #[test]
    fn repeated_service_flags_check_each_named_service() {
        let mut args = base_args();
        args.service = vec!["svc.A".to_string(), "svc.B".to_string()];
        assert_eq!(
            target_services(&args),
            vec!["svc.A".to_string(), "svc.B".to_string()]
        );
    }

    #[test]
    fn failure_summary_lists_only_non_serving_targets() {
        let results = vec![
            HealthResult {
                service: "a".to_string(),
                status: "SERVING".to_string(),
                duration_ms: 1,
            },
            HealthResult {
                service: "b".to_string(),
                status: "NOT_SERVING".to_string(),
                duration_ms: 1,
            },
        ];
        assert_eq!(failure_summary(&results), "b=NOT_SERVING");
    }
}
