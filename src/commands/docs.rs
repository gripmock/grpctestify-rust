use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::cli::args::DocsArgs;
use crate::parser::ast::{SectionContent, SectionType};
use crate::parser::{self};
use crate::report::coverage::CoverageReport;
use crate::report::kernel;
use crate::utils::FileUtils;

#[derive(Clone)]
struct FlowStep {
    endpoint: Option<String>,
    method: Option<String>,
    request: Option<serde_json::Value>,
    response: Option<serde_json::Value>,
    error: Option<serde_json::Value>,
    asserts: Vec<String>,
}

#[derive(Clone)]
struct MethodFlow {
    file: PathBuf,
    title: String,
    summary: Option<String>,
    tags: Vec<String>,
    owner: Option<String>,
    steps: Vec<FlowStep>,
}

pub fn handle_docs(args: &DocsArgs) -> Result<()> {
    let paths: Vec<PathBuf> = if args.paths.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        args.paths.clone()
    };

    let mut files = Vec::new();
    for path in &paths {
        if path.is_dir() {
            files.extend(FileUtils::collect_test_files(path, &[]));
        } else if path.is_file() {
            files.push(path.clone());
        }
    }
    files.sort();
    files.dedup();

    if files.is_empty() {
        println!("No .gctf test files found.");
        return Ok(());
    }

    let coverage = load_coverage(args)?;
    let by_service = collect_flows_by_service(&files);

    if by_service.is_empty() {
        println!("No ENDPOINT-bearing tests found — nothing to document.");
        return Ok(());
    }

    std::fs::create_dir_all(&args.output).with_context(|| {
        format!(
            "failed to create output directory: {}",
            args.output.display()
        )
    })?;

    let index = render_index(&by_service, coverage.as_ref());
    let index_path = args.output.join("index.md");
    std::fs::write(&index_path, index)
        .with_context(|| format!("failed to write {}", index_path.display()))?;

    let mut pages_written = 1; // index.md
    for (service, flows) in &by_service {
        let page = render_service_page(service, flows, coverage.as_ref());
        let page_path = args.output.join(format!("{service}.md"));
        std::fs::write(&page_path, page)
            .with_context(|| format!("failed to write {}", page_path.display()))?;
        pages_written += 1;
    }

    println!(
        "{} Wrote {pages_written} page(s) to {}",
        crate::report::style::pass_icon(),
        args.output.display()
    );
    Ok(())
}

fn load_coverage(args: &DocsArgs) -> Result<Option<CoverageReport>> {
    let Some(path) = &args.coverage else {
        return Ok(None);
    };
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read coverage report: {}", path.display()))?;
    let report: CoverageReport = serde_json::from_str(&content)
        .with_context(|| format!("failed to parse coverage report: {}", path.display()))?;
    Ok(Some(report))
}

/// Group every ENDPOINT-bearing test file into a flow under each gRPC service
/// it touches — usually one, but a cross-service multi-document chain lands
/// under every service its steps call so no path through the API is hidden
/// behind whichever service happens to come first.
fn collect_flows_by_service(files: &[PathBuf]) -> BTreeMap<String, Vec<MethodFlow>> {
    let mut by_service: BTreeMap<String, Vec<MethodFlow>> = BTreeMap::new();

    for file in files {
        let Ok(doc) = parser::parse_gctf(file) else {
            continue;
        };
        let labels = kernel::collect_grpc_labels(&file.to_string_lossy());
        if labels.services.is_empty() {
            continue;
        }
        let package = labels.packages.first().cloned().unwrap_or_default();

        let meta = crate::commands::run::extract_test_meta(&doc);
        let title = meta.name.clone().unwrap_or_else(|| {
            file.file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| file.display().to_string())
        });

        let steps: Vec<FlowStep> = doc.iter_chain().map(build_flow_step).collect();

        let flow = MethodFlow {
            file: file.clone(),
            title,
            summary: meta.summary.clone(),
            tags: meta.tags.clone(),
            owner: meta.owner.clone(),
            steps,
        };

        for service in &labels.services {
            let key = service_key(&package, service);
            by_service.entry(key).or_default().push(flow.clone());
        }
    }

    for flows in by_service.values_mut() {
        flows.sort_by(|a, b| a.title.cmp(&b.title));
    }
    by_service
}

fn service_key(package: &str, service: &str) -> String {
    if package.is_empty() {
        service.to_string()
    } else {
        format!("{package}.{service}")
    }
}

fn build_flow_step(d: &parser::ast::GctfDocument) -> FlowStep {
    let (endpoint, method) = match d.parse_endpoint() {
        Some((pkg, service, method)) => {
            let ep = if pkg.is_empty() {
                format!("{service}/{method}")
            } else {
                format!("{pkg}.{service}/{method}")
            };
            (Some(ep), Some(method))
        }
        None => (d.get_endpoint(), None),
    };

    let request = d.get_requests().into_iter().next();
    let response = d
        .first_section(SectionType::Response)
        .and_then(|s| match &s.content {
            SectionContent::Json(v) => Some(v.clone()),
            SectionContent::JsonLines(vs) => vs.first().cloned(),
            _ => None,
        });
    let error = d
        .first_section(SectionType::Error)
        .and_then(|s| match &s.content {
            SectionContent::Json(v) => Some(v.clone()),
            _ => None,
        });
    let asserts = d.get_assertions().into_iter().flatten().collect();

    FlowStep {
        endpoint,
        method,
        request,
        response,
        error,
        asserts,
    }
}

fn coverage_line(coverage: Option<&CoverageReport>, service: &str) -> Option<String> {
    let report = coverage?;
    // `CoverageFile.uri` is built from `Service::name()` (the bare service
    // name), never the `package.Service` key used to group pages here — a
    // packaged service's coverage would never match otherwise.
    let short_name = service.rsplit('.').next().unwrap_or(service);
    let uri = format!("grpc://{short_name}");
    let file = report.files.iter().find(|f| f.uri == uri)?;
    let pct = if file.statements.total > 0 {
        (file.statements.covered as f64 / file.statements.total as f64) * 100.0
    } else {
        0.0
    };
    Some(format!(
        "**Coverage:** {}/{} methods called ({:.1}%)\n",
        file.statements.covered, file.statements.total, pct
    ))
}

fn render_index(
    by_service: &BTreeMap<String, Vec<MethodFlow>>,
    coverage: Option<&CoverageReport>,
) -> String {
    let mut out = String::from("# API Documentation\n\nGenerated by `grpctestify docs`.\n\n");

    if let Some(report) = coverage {
        let pct = if report.summary.total > 0 {
            (report.summary.covered as f64 / report.summary.total as f64) * 100.0
        } else {
            0.0
        };
        out.push_str(&format!(
            "**Overall coverage:** {}/{} methods called ({:.1}%)\n\n",
            report.summary.covered, report.summary.total, pct
        ));
    }

    out.push_str("| Service | Methods | Tests |\n|---|---|---|\n");
    for (service, flows) in by_service {
        let methods: std::collections::BTreeSet<&str> = flows
            .iter()
            .flat_map(|f| f.steps.iter().filter_map(|s| s.method.as_deref()))
            .collect();
        out.push_str(&format!(
            "| [{service}]({service}.md) | {} | {} |\n",
            methods.len(),
            flows.len()
        ));
    }
    out
}

fn render_service_page(
    service: &str,
    flows: &[MethodFlow],
    coverage: Option<&CoverageReport>,
) -> String {
    let mut out = format!("# {service}\n\n");
    if let Some(line) = coverage_line(coverage, service) {
        out.push_str(&line);
        out.push('\n');
    }

    for flow in flows {
        out.push_str(&format!("## {}\n\n", flow.title));
        if let Some(summary) = &flow.summary {
            out.push_str(summary);
            out.push_str("\n\n");
        }
        if !flow.tags.is_empty() {
            out.push_str(&format!(
                "Tags: {}\n\n",
                flow.tags
                    .iter()
                    .map(|t| format!("`{t}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if let Some(owner) = &flow.owner {
            out.push_str(&format!("Owner: {owner}\n\n"));
        }
        out.push_str(&format!("Source: `{}`\n\n", flow.file.display()));

        if flow.steps.len() > 1 {
            out.push_str(&render_sequence_diagram(flow));
        }

        for (i, step) in flow.steps.iter().enumerate() {
            if flow.steps.len() > 1 {
                out.push_str(&format!("### Step {}\n\n", i + 1));
            }
            if let Some(endpoint) = &step.endpoint {
                out.push_str(&format!("**Endpoint:** `{endpoint}`\n\n"));
            }
            if let Some(request) = &step.request {
                out.push_str("Request:\n\n```json\n");
                out.push_str(&pretty_json(request));
                out.push_str("\n```\n\n");
            }
            if let Some(response) = &step.response {
                out.push_str("Response:\n\n```json\n");
                out.push_str(&pretty_json(response));
                out.push_str("\n```\n\n");
            }
            if let Some(error) = &step.error {
                out.push_str("Error:\n\n```json\n");
                out.push_str(&pretty_json(error));
                out.push_str("\n```\n\n");
            }
            if !step.asserts.is_empty() {
                out.push_str("Asserts:\n\n");
                for assertion in &step.asserts {
                    out.push_str(&format!("- `{assertion}`\n"));
                }
                out.push('\n');
            }
        }
        out.push_str("---\n\n");
    }
    out
}

fn render_sequence_diagram(flow: &MethodFlow) -> String {
    let mut out = String::from(
        "```mermaid\nsequenceDiagram\n    participant Client\n    participant Server\n",
    );
    for step in &flow.steps {
        let method = step.method.as_deref().unwrap_or("?");
        out.push_str(&format!("    Client->>Server: {method}\n"));
        if step.error.is_some() {
            out.push_str("    Server-->>Client: error\n");
        } else {
            out.push_str("    Server-->>Client: response\n");
        }
    }
    out.push_str("```\n\n");
    out
}

fn pretty_json(value: &serde_json::Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_key_joins_package_and_service() {
        assert_eq!(service_key("users", "UserService"), "users.UserService");
    }

    #[test]
    fn service_key_bare_service_when_no_package() {
        assert_eq!(service_key("", "Calculator"), "Calculator");
    }

    fn report_with_uri(uri: &str, covered: usize, total: usize) -> CoverageReport {
        use crate::report::coverage::{CoverageFile, CoverageStats};
        CoverageReport {
            files: vec![CoverageFile {
                uri: uri.to_string(),
                statements: CoverageStats { covered, total },
                branches: None,
                functions: None,
                fields: None,
                methods: Vec::new(),
            }],
            messages: Vec::new(),
            summary: CoverageStats { covered, total },
            field_summary: CoverageStats {
                covered: 0,
                total: 0,
            },
        }
    }

    #[test]
    fn coverage_line_matches_packaged_service_by_bare_name() {
        // CoverageFile.uri is built from the bare service name (see coverage.rs),
        // never `package.Service` — this is the exact mismatch that shipped
        // silently in the first version of this command.
        let report = report_with_uri("grpc://UserService", 2, 5);
        let line = coverage_line(Some(&report), "users.UserService").unwrap();
        assert!(line.contains("2/5 methods called (40.0%)"), "{line}");
    }

    #[test]
    fn coverage_line_none_when_service_not_in_report() {
        let report = report_with_uri("grpc://OtherService", 1, 1);
        assert!(coverage_line(Some(&report), "users.UserService").is_none());
    }

    #[test]
    fn coverage_line_none_without_report() {
        assert!(coverage_line(None, "users.UserService").is_none());
    }

    #[test]
    fn render_sequence_diagram_marks_error_steps_distinctly() {
        let flow = MethodFlow {
            file: PathBuf::from("t.gctf"),
            title: "t".to_string(),
            summary: None,
            tags: Vec::new(),
            owner: None,
            steps: vec![
                FlowStep {
                    endpoint: None,
                    method: Some("Get".to_string()),
                    request: None,
                    response: Some(serde_json::json!({})),
                    error: None,
                    asserts: Vec::new(),
                },
                FlowStep {
                    endpoint: None,
                    method: Some("Delete".to_string()),
                    request: None,
                    response: None,
                    error: Some(serde_json::json!({"code": 5})),
                    asserts: Vec::new(),
                },
            ],
        };
        let diagram = render_sequence_diagram(&flow);
        assert!(diagram.contains("Client->>Server: Get"));
        assert!(diagram.contains("Client->>Server: Delete"));
        assert!(diagram.contains("Server-->>Client: response"));
        assert!(diagram.contains("Server-->>Client: error"));
    }
}
