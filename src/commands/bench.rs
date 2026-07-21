// Bench command - run benchmark tests with load generation

use crate::bench::schema::bench_value;
use crate::cli::args::BenchArgs;
use crate::parser::ast::{GctfDocument, SectionContent, SectionType};
use crate::report::bench::{
    BENCH_REPORT_SCHEMA_VERSION, BenchHistogramBucket, BenchPercentile, BenchReport, BenchRunInfo,
    BenchThresholdResult,
};
use crate::utils::FileUtils;
use anyhow::{Context, Result};
use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::task::JoinSet;
use tracing::{info, warn};

/// Safety cap on the number of per-response `details` retained for the report.
/// Latency percentiles no longer use this — they come from the bounded
/// [`LatencyHistogram`] — so this only bounds the memory of the raw detail log.
const MAX_LATENCY_SAMPLES: usize = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurationStopMode {
    Close,
    Wait,
    Ignore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BenchOptionSource {
    Cli,
    BenchSection,
    Default,
}

impl BenchOptionSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cli => "cli",
            Self::BenchSection => "bench_section",
            Self::Default => "default",
        }
    }
}

impl DurationStopMode {
    fn parse(raw: &str) -> Result<Self> {
        match raw.trim_ascii().to_ascii_lowercase().as_str() {
            "close" => Ok(Self::Close),
            "wait" => Ok(Self::Wait),
            "ignore" => Ok(Self::Ignore),
            other => anyhow::bail!(
                "invalid duration-stop mode '{}', expected close|wait|ignore",
                other
            ),
        }
    }
}

/// Resolved benchmark configuration from CLI + BENCH section + defaults
#[derive(Debug, Clone)]
pub struct BenchConfigResolved {
    pub profile: String,
    pub mode: String,
    /// Wire protocol override for the whole bench run (from `--protocol`).
    /// Mirrors `run`/`call`: takes priority over each file's OPTIONS.protocol.
    pub protocol: crate::grpc::WireProtocol,
    pub concurrency: u32,
    pub requests: Option<u64>,
    pub duration: Option<Duration>,
    pub ramp_up: Option<Duration>,
    pub warmup: Option<Duration>,
    pub warmup_mode: String,
    pub max_duration: Option<Duration>,
    pub cool_down: Option<Duration>,
    pub max_rps: Option<f64>,
    pub load_schedule: String,
    pub load_start: Option<f64>,
    pub load_step: Option<f64>,
    pub load_end: Option<f64>,
    pub load_step_duration: Option<Duration>,
    pub load_max_duration: Option<Duration>,
    pub load_midpoint: Option<f64>,
    pub load_amplitude: Option<f64>,
    pub load_frequency: Option<f64>,
    pub load_spike_target: Option<f64>,
    pub load_spike_after: Option<f64>,
    pub load_spike_duration: Option<f64>,
    pub load_profile: Option<Vec<(f64, f64)>>,
    pub connections: u32,
    pub connect_timeout: Duration,
    pub keepalive: Option<Duration>,
    pub cpus: Option<usize>,
    pub name: Option<String>,
    pub assert_mode: String,
    pub no_assert: bool,
    pub sample_rate: f64,
    pub cache: bool,
    pub cache_ttl: Option<Duration>,
    pub skip_first: u32,
    pub count_errors_in_latency: bool,
    pub duration_stop: DurationStopMode,
    pub latency_percentiles: Vec<String>,
    pub progress_interval: Duration,
    pub thresholds: HashMap<String, String>,
    pub option_sources: HashMap<String, BenchOptionSource>,
    pub sources: Vec<crate::bench::sources::SourceDefinition>,
}

impl Default for BenchConfigResolved {
    fn default() -> Self {
        Self {
            profile: "functional".to_string(),
            mode: "fixed".to_string(),
            protocol: crate::grpc::WireProtocol::Grpc,
            concurrency: 1,
            requests: Some(100),
            duration: None,
            ramp_up: None,
            warmup: None,
            warmup_mode: "warmup".to_string(),
            cool_down: None,
            max_duration: None,
            max_rps: None,
            load_schedule: "const".to_string(),
            load_start: None,
            load_step: None,
            load_end: None,
            load_step_duration: None,
            load_max_duration: None,
            load_midpoint: None,
            load_amplitude: None,
            load_frequency: None,
            load_spike_target: None,
            load_spike_after: None,
            load_spike_duration: None,
            load_profile: None,
            connections: 1,
            connect_timeout: Duration::from_secs(10),
            keepalive: None,
            cpus: None,
            name: None,
            assert_mode: "collect_all".to_string(),
            no_assert: false,
            sample_rate: 1.0,
            cache: true,
            cache_ttl: None,
            skip_first: 0,
            count_errors_in_latency: false,
            duration_stop: DurationStopMode::Wait,
            latency_percentiles: vec![
                "p50".to_string(),
                "p90".to_string(),
                "p95".to_string(),
                "p99".to_string(),
            ],
            progress_interval: Duration::from_secs(5),
            thresholds: HashMap::new(),
            option_sources: {
                let mut s = HashMap::new();
                for key in [
                    "concurrency",
                    "load_schedule",
                    "load_start",
                    "load_step",
                    "load_end",
                    "load_step_duration",
                    "load_max_duration",
                    "progress_interval",
                ] {
                    s.insert(key.to_string(), BenchOptionSource::Default);
                }
                s
            },
            sources: Vec::new(),
        }
    }
}

/// Linear interpolation between custom profile points: [(time_secs, rps), ...]
fn interpolate_custom_profile(profile: &[(f64, f64)], t: f64) -> f64 {
    if profile.is_empty() {
        return 0.0;
    }
    if t <= profile[0].0 {
        return profile[0].1.max(0.0);
    }
    if let Some(last) = profile.last()
        && t >= last.0
    {
        return last.1.max(0.0);
    }
    for i in 0..profile.len() - 1 {
        let (t1, r1) = profile[i];
        let (t2, r2) = profile[i + 1];
        if t >= t1 && t <= t2 {
            let fraction = (t - t1) / (t2 - t1);
            return (r1 + (r2 - r1) * fraction).max(0.0);
        }
    }
    0.0
}

/// Parse `load_profile` string: "0s:10, 10s:100, 30s:50"
fn parse_custom_profile(s: &str) -> Option<Vec<(f64, f64)>> {
    let mut points: Vec<(f64, f64)> = s
        .split(',')
        .filter_map(|part| {
            let part = part.trim();
            let (time_str, rps_str) = part.split_once(':')?;
            let time_str = time_str.trim();
            let rps_str = rps_str.trim();
            let t = parse_duration_sec(time_str)?;
            let rps: f64 = rps_str.parse().ok()?;
            Some((t, rps))
        })
        .collect();
    if points.is_empty() {
        return None;
    }
    points.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    Some(points)
}

/// Number of round-robin passes to schedule in fixed-request mode.
///
/// Each pass issues one request per test doc, so to honour `--requests` as the
/// *total* request budget across all endpoints (per its help text), the pass
/// count is the budget divided by the number of docs. This keeps the overall
/// request count at ~`total_requests` instead of `total_requests * docs.len()`.
fn request_passes(total_requests: u64, doc_count: usize) -> u64 {
    total_requests / (doc_count as u64).max(1)
}

fn parse_duration_sec(s: &str) -> Option<f64> {
    let s = s.trim().to_ascii_lowercase();
    // Check longest / most-specific suffixes first: "ms" must be matched before
    // the single-char "s", otherwise "500ms" gets stripped to "500m" and fails.
    if let Some(rest) = s.strip_suffix('h') {
        rest.parse::<f64>().ok().map(|v| v * 3600.0)
    } else if let Some(rest) = s.strip_suffix("ms") {
        rest.parse::<f64>().ok().map(|v| v / 1000.0)
    } else if let Some(rest) = s.strip_suffix('s') {
        rest.parse::<f64>().ok()
    } else if let Some(rest) = s.strip_suffix('m') {
        rest.parse::<f64>().ok().map(|v| v * 60.0)
    } else {
        s.parse::<f64>().ok()
    }
}

/// Macro for CLI-only config field overrides.
/// Reduces repetitive `if let Some(v) = &cli.field { config.field = v; }` patterns.
macro_rules! cli_config_field {
    (string_clone, $config:expr, $cli:expr, $field:ident, $key:literal) => {
        if let Some(v) = &$cli.$field {
            $config.$field = v.clone();
        }
    };
    (direct, $config:expr, $cli:expr, $field:ident, $key:literal) => {
        if let Some(v) = $cli.$field {
            $config.$field = v;
            $config
                .option_sources
                .insert($key.to_string(), BenchOptionSource::Cli);
        }
    };
    (option_direct, $config:expr, $cli:expr, $field:ident, $key:literal) => {
        if let Some(v) = $cli.$field {
            $config.$field = Some(v);
            $config
                .option_sources
                .insert($key.to_string(), BenchOptionSource::Cli);
        }
    };
    (duration, $config:expr, $cli:expr, $field:ident) => {
        if let Some(v) = &$cli.$field {
            $config.$field = Some(parse_duration(v)?);
        }
    };
    (bool_flag, $config:expr, $cli:expr, $field:ident) => {
        if $cli.$field {
            $config.$field = true;
        }
    };
    (string_source, $config:expr, $cli:expr, $field:ident, $key:literal) => {
        if let Some(v) = &$cli.$field {
            $config.$field = v.clone();
            $config
                .option_sources
                .insert($key.to_string(), BenchOptionSource::Cli);
        }
    };
    (f64_source, $config:expr, $cli:expr, $field:ident, $key:literal) => {
        if let Some(v) = $cli.$field {
            $config.$field = Some(v);
            $config
                .option_sources
                .insert($key.to_string(), BenchOptionSource::Cli);
        }
    };
    (duration_source, $config:expr, $cli:expr, $field:ident, $key:literal) => {
        if let Some(v) = &$cli.$field {
            $config.$field = Some(parse_duration(v)?);
            $config
                .option_sources
                .insert($key.to_string(), BenchOptionSource::Cli);
        }
    };
}

impl BenchConfigResolved {
    pub fn from_bench_section(bench_section: Option<&HashMap<String, String>>) -> Result<Self> {
        let mut config = Self::default();

        if let Some(bench) = bench_section {
            if let Some(mode) = bench.get("mode") {
                config.mode = mode.clone();
            }
            if let Some(p) = bench.get("profile") {
                config.profile = p.clone();
            }
            if let Some(c) = bench.get("concurrency") {
                config.concurrency = c.parse().unwrap_or(1);
                config
                    .option_sources
                    .insert("concurrency".to_string(), BenchOptionSource::BenchSection);
            }
            if let Some(n) = bench.get("requests") {
                config.requests = Some(n.parse().unwrap_or(100));
            }
            if let Some(d) = bench.get("duration") {
                config.duration = Some(parse_duration(d)?);
            }
            if let Some(d) = bench_value(bench, "ramp_up") {
                config.ramp_up = Some(parse_duration(d)?);
            }
            if let Some(d) = bench.get("warmup") {
                config.warmup = Some(parse_duration(d)?);
            }
            if let Some(v) = bench.get("warmup_mode") {
                config.warmup_mode = v.clone();
            }
            if let Some(d) = bench.get("cool_down") {
                config.cool_down = Some(parse_duration(d)?);
            }
            if let Some(d) = bench_value(bench, "max_duration") {
                config.max_duration = Some(parse_duration(d)?);
            }
            if let Some(rps) = bench_value(bench, "max_rps") {
                config.max_rps = Some(rps.parse().unwrap_or(0.0));
            }
            if let Some(v) = bench_value(bench, "load_schedule") {
                config.load_schedule = v.clone();
                config
                    .option_sources
                    .insert("load_schedule".to_string(), BenchOptionSource::BenchSection);
            }
            if let Some(v) = bench.get("load_profile") {
                config.load_profile = parse_custom_profile(v);
            }
            if let Some(v) = bench.get("load_midpoint") {
                config.load_midpoint = v.parse::<f64>().ok();
            }
            if let Some(v) = bench.get("load_amplitude") {
                config.load_amplitude = v.parse::<f64>().ok();
            }
            if let Some(v) = bench.get("load_frequency") {
                config.load_frequency = v.parse::<f64>().ok();
            }
            if let Some(v) = bench.get("load_spike_target") {
                config.load_spike_target = v.parse::<f64>().ok();
            }
            if let Some(v) = bench.get("load_spike_after") {
                config.load_spike_after = v.parse::<f64>().ok();
            }
            if let Some(v) = bench.get("load_spike_duration") {
                config.load_spike_duration = v.parse::<f64>().ok();
            }
            if let Some(v) = bench_value(bench, "load_start") {
                config.load_start = v.parse::<f64>().ok();
                config
                    .option_sources
                    .insert("load_start".to_string(), BenchOptionSource::BenchSection);
            }
            if let Some(v) = bench_value(bench, "load_step") {
                config.load_step = v.parse::<f64>().ok();
                config
                    .option_sources
                    .insert("load_step".to_string(), BenchOptionSource::BenchSection);
            }
            if let Some(v) = bench_value(bench, "load_end") {
                config.load_end = v.parse::<f64>().ok();
                config
                    .option_sources
                    .insert("load_end".to_string(), BenchOptionSource::BenchSection);
            }
            if let Some(v) = bench_value(bench, "load_step_duration") {
                config.load_step_duration = Some(parse_duration(v)?);
                config.option_sources.insert(
                    "load_step_duration".to_string(),
                    BenchOptionSource::BenchSection,
                );
            }
            if let Some(v) = bench_value(bench, "load_max_duration") {
                config.load_max_duration = Some(parse_duration(v)?);
                config.option_sources.insert(
                    "load_max_duration".to_string(),
                    BenchOptionSource::BenchSection,
                );
            }
            if let Some(v) = bench.get("connections") {
                config.connections = v.parse().unwrap_or(1);
            }
            if let Some(v) = bench_value(bench, "connect_timeout") {
                config.connect_timeout = parse_duration(v)?;
            }
            if let Some(v) = bench.get("keepalive") {
                config.keepalive = Some(parse_duration(v)?);
            }
            if let Some(v) = bench.get("cpus") {
                config.cpus = Some(v.parse().unwrap_or(1));
            }
            if let Some(v) = bench.get("name") {
                config.name = Some(v.clone());
            }
            if let Some(am) = bench_value(bench, "assert_mode") {
                config.assert_mode = am.clone();
            }
            if let Some(v) = bench_value(bench, "no_assert") {
                config.no_assert = v == "true" || v == "1";
            }
            if let Some(v) = bench_value(bench, "duration_stop") {
                config.duration_stop = DurationStopMode::parse(v)?;
            }
            if let Some(sr) = bench_value(bench, "sample_rate") {
                config.sample_rate = sr.parse().unwrap_or(1.0);
            }
            if let Some(v) = bench_value(bench, "skip_first") {
                config.skip_first = v.parse().unwrap_or(0);
            }
            if let Some(v) = bench_value(bench, "count_errors_in_latency") {
                config.count_errors_in_latency = v == "true" || v == "1";
            }
            if let Some(v) = bench_value(bench, "latency_percentiles") {
                config.latency_percentiles = parse_latency_percentiles(v);
            }
            if let Some(v) = bench_value(bench, "progress_interval") {
                config.progress_interval = parse_duration(v)?;
                config.option_sources.insert(
                    "progress_interval".to_string(),
                    BenchOptionSource::BenchSection,
                );
            }
            if let Some(cache) = bench.get("cache") {
                config.cache = cache == "true" || cache == "1";
            }
            if let Some(ttl) = bench.get("cache_ttl") {
                config.cache_ttl = Some(parse_duration(ttl)?);
            }

            for (key, value) in bench {
                if let Some(metric) = key.strip_prefix("threshold.") {
                    config.thresholds.insert(metric.to_string(), value.clone());
                }
            }

            if let Some(sources_yaml) = bench.get("sources")
                && let Ok(defs) = serde_yaml_ng::from_str::<
                    Vec<crate::bench::sources::SourceDefinition>,
                >(sources_yaml)
            {
                config.sources = defs;
            }
        }

        if config.connections == 0 {
            anyhow::bail!("connections must be greater than 0");
        }
        if config.connections > config.concurrency {
            anyhow::bail!(
                "connections ({}) cannot exceed concurrency ({})",
                config.connections,
                config.concurrency
            );
        }
        if config.duration.is_some() {
            config.requests = None;
        }

        Ok(config)
    }

    /// Merge CLI args -> BENCH section -> defaults (precedence: CLI > BENCH > defaults)
    pub fn from_cli_and_bench(
        cli: &BenchArgs,
        bench_section: Option<&HashMap<String, String>>,
    ) -> Result<Self> {
        let defaults = Self::default();
        let mut config = defaults;

        // Apply BENCH section first (if present)
        if let Some(bench) = bench_section {
            if let Some(mode) = bench.get("mode") {
                config.mode = mode.clone();
            }
            if let Some(p) = bench.get("profile") {
                config.profile = p.clone();
            }
            if let Some(c) = bench.get("concurrency") {
                config.concurrency = c.parse().unwrap_or(1);
                config
                    .option_sources
                    .insert("concurrency".to_string(), BenchOptionSource::BenchSection);
            }
            if let Some(n) = bench.get("requests") {
                config.requests = Some(n.parse().unwrap_or(100));
            }
            if let Some(d) = bench.get("duration") {
                config.duration = Some(parse_duration(d)?);
            }
            if let Some(d) = bench_value(bench, "ramp_up") {
                config.ramp_up = Some(parse_duration(d)?);
            }
            if let Some(d) = bench.get("warmup") {
                config.warmup = Some(parse_duration(d)?);
            }
            if let Some(v) = bench.get("warmup_mode") {
                config.warmup_mode = v.clone();
            }
            if let Some(d) = bench.get("cool_down") {
                config.cool_down = Some(parse_duration(d)?);
            }
            if let Some(d) = bench_value(bench, "max_duration") {
                config.max_duration = Some(parse_duration(d)?);
            }
            if let Some(rps) = bench_value(bench, "max_rps") {
                config.max_rps = Some(rps.parse().unwrap_or(0.0));
            }
            if let Some(v) = bench_value(bench, "load_schedule") {
                config.load_schedule = v.clone();
                config
                    .option_sources
                    .insert("load_schedule".to_string(), BenchOptionSource::BenchSection);
            }
            if let Some(v) = bench.get("load_profile") {
                config.load_profile = parse_custom_profile(v);
            }
            if let Some(v) = bench.get("load_midpoint") {
                config.load_midpoint = v.parse::<f64>().ok();
            }
            if let Some(v) = bench.get("load_amplitude") {
                config.load_amplitude = v.parse::<f64>().ok();
            }
            if let Some(v) = bench.get("load_frequency") {
                config.load_frequency = v.parse::<f64>().ok();
            }
            if let Some(v) = bench.get("load_spike_target") {
                config.load_spike_target = v.parse::<f64>().ok();
            }
            if let Some(v) = bench.get("load_spike_after") {
                config.load_spike_after = v.parse::<f64>().ok();
            }
            if let Some(v) = bench.get("load_spike_duration") {
                config.load_spike_duration = v.parse::<f64>().ok();
            }
            if let Some(v) = bench_value(bench, "load_start") {
                config.load_start = v.parse::<f64>().ok();
                config
                    .option_sources
                    .insert("load_start".to_string(), BenchOptionSource::BenchSection);
            }
            if let Some(v) = bench_value(bench, "load_step") {
                config.load_step = v.parse::<f64>().ok();
                config
                    .option_sources
                    .insert("load_step".to_string(), BenchOptionSource::BenchSection);
            }
            if let Some(v) = bench_value(bench, "load_end") {
                config.load_end = v.parse::<f64>().ok();
                config
                    .option_sources
                    .insert("load_end".to_string(), BenchOptionSource::BenchSection);
            }
            if let Some(v) = bench_value(bench, "load_step_duration") {
                config.load_step_duration = Some(parse_duration(v)?);
                config.option_sources.insert(
                    "load_step_duration".to_string(),
                    BenchOptionSource::BenchSection,
                );
            }
            if let Some(v) = bench_value(bench, "load_max_duration") {
                config.load_max_duration = Some(parse_duration(v)?);
                config.option_sources.insert(
                    "load_max_duration".to_string(),
                    BenchOptionSource::BenchSection,
                );
            }
            if let Some(v) = bench.get("connections") {
                config.connections = v.parse().unwrap_or(1);
            }
            if let Some(v) = bench_value(bench, "connect_timeout") {
                config.connect_timeout = parse_duration(v)?;
            }
            if let Some(v) = bench.get("keepalive") {
                config.keepalive = Some(parse_duration(v)?);
            }
            if let Some(v) = bench.get("cpus") {
                config.cpus = Some(v.parse().unwrap_or(1));
            }
            if let Some(v) = bench.get("name") {
                config.name = Some(v.clone());
            }
            if let Some(am) = bench_value(bench, "assert_mode") {
                config.assert_mode = am.clone();
            }
            if let Some(v) = bench_value(bench, "no_assert") {
                config.no_assert = v == "true" || v == "1";
            }
            if let Some(v) = bench_value(bench, "duration_stop") {
                config.duration_stop = DurationStopMode::parse(v)?;
            }
            if let Some(sr) = bench_value(bench, "sample_rate") {
                config.sample_rate = sr.parse().unwrap_or(1.0);
            }
            if let Some(v) = bench_value(bench, "skip_first") {
                config.skip_first = v.parse().unwrap_or(0);
            }
            if let Some(v) = bench_value(bench, "count_errors_in_latency") {
                config.count_errors_in_latency = v == "true" || v == "1";
            }
            if let Some(v) = bench_value(bench, "latency_percentiles") {
                config.latency_percentiles = parse_latency_percentiles(v);
            }
            if let Some(v) = bench_value(bench, "progress_interval") {
                config.progress_interval = parse_duration(v)?;
                config.option_sources.insert(
                    "progress_interval".to_string(),
                    BenchOptionSource::BenchSection,
                );
            }
            if let Some(cache) = bench.get("cache") {
                config.cache = cache == "true" || cache == "1";
            }
            if let Some(ttl) = bench.get("cache_ttl") {
                config.cache_ttl = Some(parse_duration(ttl)?);
            }

            // Collect thresholds (keys starting with "threshold.")
            for (key, value) in bench {
                if let Some(metric) = key.strip_prefix("threshold.") {
                    config.thresholds.insert(metric.to_string(), value.clone());
                }
            }

            // Parse sources (YAML array of SourceDefinition)
            if let Some(sources_yaml) = bench.get("sources")
                && let Ok(defs) = serde_yaml_ng::from_str::<
                    Vec<crate::bench::sources::SourceDefinition>,
                >(sources_yaml)
            {
                config.sources = defs;
            }
        }

        // Apply profile defaults (lowest priority — fills in values not set by BENCH section)
        let profile_name = config.profile.clone();
        if profile_name != "functional" {
            apply_profile_defaults(&mut config, &profile_name);
        }

        // Override with CLI args (highest priority)
        // `--protocol` selects the wire protocol for the whole run, overriding
        // each file's OPTIONS.protocol — consistent with `run`/`call`, which
        // parse this flag and apply it as a runner-level override.
        config.protocol = cli.protocol.parse().unwrap_or_default();
        cli_config_field!(string_clone, config, cli, profile, "profile");
        cli_config_field!(string_clone, config, cli, mode, "mode");
        cli_config_field!(direct, config, cli, concurrency, "concurrency");
        cli_config_field!(option_direct, config, cli, requests, "requests");
        cli_config_field!(duration, config, cli, duration);
        cli_config_field!(duration, config, cli, ramp_up);
        cli_config_field!(duration, config, cli, warmup);
        cli_config_field!(duration, config, cli, max_duration);
        cli_config_field!(option_direct, config, cli, max_rps, "max_rps");
        cli_config_field!(string_source, config, cli, load_schedule, "load_schedule");
        cli_config_field!(f64_source, config, cli, load_start, "load_start");
        cli_config_field!(f64_source, config, cli, load_step, "load_step");
        cli_config_field!(f64_source, config, cli, load_end, "load_end");
        cli_config_field!(
            duration_source,
            config,
            cli,
            load_step_duration,
            "load_step_duration"
        );
        cli_config_field!(
            duration_source,
            config,
            cli,
            load_max_duration,
            "load_max_duration"
        );
        cli_config_field!(direct, config, cli, connections, "connections");
        if let Some(v) = &cli.connect_timeout {
            config.connect_timeout = parse_duration(v)?;
        }
        if let Some(v) = &cli.keepalive {
            config.keepalive = Some(parse_duration(v)?);
        }
        if let Some(v) = cli.cpus {
            config.cpus = Some(v);
        }
        if let Some(v) = &cli.name {
            config.name = Some(v.clone());
        }
        if let Some(am) = &cli.assert_mode {
            config.assert_mode = am.clone();
        }
        cli_config_field!(bool_flag, config, cli, no_assert);
        if let Some(sr) = cli.sample_rate {
            config.sample_rate = sr;
        }
        if let Some(cache) = cli.cache {
            config.cache = cache;
        }
        if let Some(skip) = cli.skip_first {
            config.skip_first = skip;
        }
        if let Some(count_err) = cli.count_errors_in_latency {
            config.count_errors_in_latency = count_err;
        }
        if let Some(dur_stop) = &cli.duration_stop {
            config.duration_stop = DurationStopMode::parse(dur_stop)?;
        }
        if let Some(v) = &cli.latency_percentiles {
            config.latency_percentiles = parse_latency_percentiles(v);
        }
        if let Some(v) = &cli.progress_interval {
            config.progress_interval = parse_duration(v)?;
            config
                .option_sources
                .insert("progress_interval".to_string(), BenchOptionSource::Cli);
        }

        if config.connections == 0 {
            anyhow::bail!("connections must be greater than 0");
        }
        if config.connections > config.concurrency {
            anyhow::bail!(
                "connections ({}) cannot exceed concurrency ({})",
                config.connections,
                config.concurrency
            );
        }

        if config.duration.is_some() {
            config.requests = None;
        }

        Ok(config)
    }
}

/// Parse duration string (e.g., "30s", "5m", "1h")
fn parse_duration(s: &str) -> Result<Duration> {
    let s = s.trim_ascii();
    if s.is_empty() {
        anyhow::bail!("empty duration string");
    }

    let (num_str, unit) = if let Some(stripped) = s.strip_suffix("ms") {
        (stripped, "ms")
    } else if let Some(stripped) = s.strip_suffix('s') {
        (stripped, "s")
    } else if let Some(stripped) = s.strip_suffix('m') {
        (stripped, "m")
    } else if let Some(stripped) = s.strip_suffix('h') {
        (stripped, "h")
    } else {
        anyhow::bail!("invalid duration format: {}", s);
    };

    let num: f64 = num_str
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid duration number: {}", num_str))?;

    let millis = match unit {
        "ms" => num,
        "s" => num * 1000.0,
        "m" => num * 60.0 * 1000.0,
        "h" => num * 60.0 * 60.0 * 1000.0,
        _ => anyhow::bail!("unknown duration unit: {}", unit),
    };

    Ok(Duration::from_millis(millis as u64))
}

fn parse_latency_percentiles(s: &str) -> Vec<String> {
    s.split(',')
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToString::to_string)
        .collect()
}

/// Extract BENCH section content from document
fn extract_bench_section(doc: &GctfDocument) -> Option<HashMap<String, String>> {
    for section in &doc.sections {
        if section.section_type == SectionType::Bench
            && let SectionContent::KeyValues(kv) = &section.content
        {
            return Some(kv.clone());
        }
    }
    None
}

/// Apply profile defaults to config for keys not already set in the BENCH section.
fn apply_profile_defaults(config: &mut BenchConfigResolved, profile_name: &str) {
    for (key, value) in crate::bench::schema::apply_profile_dynamic(profile_name) {
        // Only apply if not explicitly set via BENCH section or CLI
        let is_explicit = config
            .option_sources
            .get(&key)
            .is_some_and(|s| *s != BenchOptionSource::Default);
        if is_explicit {
            continue;
        }
        match key.as_str() {
            "mode" => config.mode = value,
            "concurrency" => {
                if let Ok(v) = value.parse::<u32>() {
                    config.concurrency = v;
                }
            }
            "requests" => {
                if let Ok(v) = value.parse::<u64>() {
                    config.requests = Some(v);
                }
            }
            "duration" => {
                if let Ok(d) = parse_duration(&value) {
                    config.duration = Some(d);
                }
            }
            "load_schedule" => config.load_schedule = value,
            "load_start" => {
                if let Ok(v) = value.parse::<f64>() {
                    config.load_start = Some(v);
                }
            }
            "load_step" => {
                if let Ok(v) = value.parse::<f64>() {
                    config.load_step = Some(v);
                }
            }
            "load_end" => {
                if let Ok(v) = value.parse::<f64>() {
                    config.load_end = Some(v);
                }
            }
            "load_step_duration" => {
                if let Ok(d) = parse_duration(&value) {
                    config.load_step_duration = Some(d);
                }
            }
            "load_spike_target" => {
                if let Ok(v) = value.parse::<f64>() {
                    config.load_spike_target = Some(v);
                }
            }
            "load_spike_after" => {
                if let Ok(v) = value.parse::<f64>() {
                    config.load_spike_after = Some(v);
                }
            }
            "load_spike_duration" => {
                if let Ok(v) = value.parse::<f64>() {
                    config.load_spike_duration = Some(v);
                }
            }
            _ => {}
        }
    }
}

// Bounded log-linear (HDR-style) latency histogram.
//
// Buckets are laid out by octave: each power-of-two range `[2^e, 2^(e+1))` is
// split into `SUB_BUCKETS` equal-width linear sub-buckets, preceded by a linear
// region of unit-width buckets for values below `SUB_BUCKETS`. A bucket in
// octave `e` spans `2^(e-SUB_BUCKET_BITS)` and the smallest value it can hold is
// `2^e`, so the width relative to the value is at most
// `2^(e-SUB_BUCKET_BITS)/2^e = 2^-SUB_BUCKET_BITS = 1/SUB_BUCKETS`. With 128
// sub-buckets that is a guaranteed relative error of ~0.78%, independent of the
// number of samples recorded — percentiles interpolate within the containing
// bucket, so the reported value is within one bucket-width of the true value.
//
// Memory is O(number of buckets) (`HIST_BUCKETS` = 4608 u64 counts ≈ 37 KB) no
// matter how many samples are recorded, and two histograms merge losslessly by
// bucket-wise addition — which is what makes cross-worker aggregation unbiased.
const SUB_BUCKET_BITS: u32 = 7;
const SUB_BUCKETS: u64 = 1 << SUB_BUCKET_BITS; // 128
/// Highest octave tracked; ~2^42 ns ≈ 73 min. Larger values saturate the top bucket.
const MAX_EXPONENT: u32 = 41;
const HIST_BUCKETS: usize =
    SUB_BUCKETS as usize + (MAX_EXPONENT - SUB_BUCKET_BITS + 1) as usize * SUB_BUCKETS as usize;

/// Index of the bucket containing `v` (latency in ns).
fn hist_bucket_index(v: u64) -> usize {
    if v < SUB_BUCKETS {
        return v as usize;
    }
    let e = (63 - v.leading_zeros()).min(MAX_EXPONENT); // floor(log2 v), clamped
    let base = SUB_BUCKETS as usize + (e - SUB_BUCKET_BITS) as usize * SUB_BUCKETS as usize;
    let shift = e - SUB_BUCKET_BITS;
    let sub = ((v - (1u64 << e)) >> shift).min(SUB_BUCKETS - 1) as usize;
    base + sub
}

/// Inclusive-lower / exclusive-upper ns bounds of bucket `index`.
fn hist_bucket_bounds(index: usize) -> (u64, u64) {
    if (index as u64) < SUB_BUCKETS {
        return (index as u64, index as u64 + 1);
    }
    let rel = index - SUB_BUCKETS as usize;
    let e = SUB_BUCKET_BITS + (rel / SUB_BUCKETS as usize) as u32;
    let sub = (rel % SUB_BUCKETS as usize) as u64;
    let width = 1u64 << (e - SUB_BUCKET_BITS);
    let lower = (1u64 << e) + sub * width;
    (lower, lower + width)
}

#[derive(Debug, Clone)]
struct LatencyHistogram {
    buckets: Vec<u64>,
    total: u64,
    min: u64,
    max: u64,
}

impl Default for LatencyHistogram {
    fn default() -> Self {
        Self {
            buckets: vec![0; HIST_BUCKETS],
            total: 0,
            min: u64::MAX,
            max: 0,
        }
    }
}

impl LatencyHistogram {
    fn record(&mut self, v: u64) {
        self.buckets[hist_bucket_index(v)] += 1;
        self.total += 1;
        self.min = self.min.min(v);
        self.max = self.max.max(v);
    }

    fn merge(&mut self, other: &Self) {
        for (a, b) in self.buckets.iter_mut().zip(other.buckets.iter()) {
            *a += *b;
        }
        self.total += other.total;
        if other.total > 0 {
            self.min = self.min.min(other.min);
            self.max = self.max.max(other.max);
        }
    }

    fn is_empty(&self) -> bool {
        self.total == 0
    }

    /// Value at the `p`-th percentile (0..=100), interpolated within the
    /// containing bucket and clamped to the exact recorded [min, max].
    fn percentile(&self, p: f64) -> u64 {
        if self.total == 0 {
            return 0;
        }
        if self.min == self.max {
            return self.min;
        }
        let p = p.clamp(0.0, 100.0);
        // 1-indexed target rank into the sorted sample set.
        let target = ((p / 100.0 * self.total as f64).ceil().max(1.0) as u64).min(self.total);
        let mut cumulative = 0u64;
        for (i, &c) in self.buckets.iter().enumerate() {
            if c == 0 {
                continue;
            }
            if cumulative + c >= target {
                let (lower, upper) = hist_bucket_bounds(i);
                let frac = (target - cumulative) as f64 / c as f64;
                let val = lower as f64 + frac * (upper - lower) as f64;
                return (val.round() as u64).clamp(self.min, self.max);
            }
            cumulative += c;
        }
        self.max
    }
}

/// Shared metrics accumulator for bench results
#[derive(Default, Debug)]
struct BenchMetrics {
    count: u64,
    ok: u64,
    errors: u64,
    total_ns: u64,
    fastest_ns: u64,
    slowest_ns: u64,
    grpc_status: BTreeMap<String, u64>,
    error_dist: BTreeMap<String, u64>,
    latency: LatencyHistogram,
    per_endpoint: BTreeMap<String, PerEndpointData>,
    details: Vec<crate::report::bench::BenchDetail>,
    /// When false, latencies of error responses are excluded from the latency
    /// distribution (percentiles/histogram). Throughput and overall timing
    /// counters (count/rps/fastest/slowest/average) are never affected.
    count_errors_in_latency: bool,
    /// Deterministic latency sampling stride: record one latency sample every
    /// `sample_stride` requests (`0` or `1` records all). Derived from
    /// `sample_rate` via [`sample_stride_from_rate`].
    sample_stride: u64,
    /// Running request counter that drives `sample_stride`.
    sample_counter: u64,
    /// Warm-up outliers to discard from the latency distribution: the first
    /// `skip_first_remaining` sampled latencies are counted for throughput but
    /// held out of the histogram (decremented as they are skipped). Applied per
    /// accumulator; per-endpoint stats are unaffected, matching prior behaviour.
    skip_first_remaining: u32,
}

/// Convert a `sample_rate` in `[0.0, 1.0]` into a deterministic recording
/// stride: record one latency sample every `N` requests where `N = round(1/rate)`.
/// `rate >= 1.0` records every request (stride 1); `rate <= 0.0` records none.
fn sample_stride_from_rate(rate: f64) -> u64 {
    if rate >= 1.0 {
        1
    } else if rate <= 0.0 {
        u64::MAX
    } else {
        (1.0 / rate).round().max(1.0) as u64
    }
}

impl BenchMetrics {
    fn with_capacity(_hint: usize) -> Self {
        let mut grpc_status = BTreeMap::new();
        grpc_status.insert("OK".to_string(), 0);
        grpc_status.insert("ERROR".to_string(), 0);
        Self {
            grpc_status,
            ..Default::default()
        }
    }

    /// Per-worker metrics accumulator preconfigured with the latency-sampling
    /// options from the resolved bench config.
    fn for_worker(
        hint: usize,
        count_errors_in_latency: bool,
        sample_stride: u64,
        skip_first: u32,
    ) -> Self {
        let mut m = Self::with_capacity(hint);
        m.count_errors_in_latency = count_errors_in_latency;
        m.sample_stride = sample_stride;
        m.skip_first_remaining = skip_first;
        m
    }
}

#[derive(Default, Debug)]
struct PerEndpointData {
    count: u64,
    errors: u64,
    latency: LatencyHistogram,
}

impl BenchMetrics {
    fn record(&mut self, latency_ns: u64, status: &str, error: Option<&str>, endpoint: &str) {
        self.count += 1;
        let is_ok = status == "OK" || status.is_empty();
        if is_ok {
            self.ok += 1;
        } else {
            self.errors += 1;
        }

        // Use static key strings to avoid allocation in hot path
        let status_key = if status.is_empty() { "OK" } else { status };
        *self.grpc_status.entry(status_key.to_string()).or_insert(0) += 1;

        if let Some(err) = error {
            let category = categorize_error(err);
            *self.error_dist.entry(category).or_insert(0) += 1;
        }

        // Decide whether this request contributes a *latency sample* (percentiles
        // and histogram). Governed by `sample_rate` (deterministic every-Nth
        // sampling) and `count_errors_in_latency` (exclude error responses when
        // false). Throughput and overall-timing counters below are unaffected.
        self.sample_counter += 1;
        let sampled = self.sample_stride <= 1 || (self.sample_counter % self.sample_stride == 1);
        let contributes = sampled && (is_ok || self.count_errors_in_latency);

        // Per-endpoint tracking
        let ep = self.per_endpoint.entry(endpoint.to_string()).or_default();
        ep.count += 1;
        if !is_ok {
            ep.errors += 1;
        }
        if contributes {
            ep.latency.record(latency_ns);
        }

        self.total_ns += latency_ns;

        if self.fastest_ns == 0 || latency_ns < self.fastest_ns {
            self.fastest_ns = latency_ns;
        }
        if latency_ns > self.slowest_ns {
            self.slowest_ns = latency_ns;
        }

        // `skip_first` gates only the global distribution (per-endpoint keeps
        // all sampled latencies, as before): hold out the first N sampled
        // values as warm-up outliers.
        if contributes {
            if self.skip_first_remaining > 0 {
                self.skip_first_remaining -= 1;
            } else {
                self.latency.record(latency_ns);
            }
        }

        // Collect per-response detail (capped at 100k)
        if self.details.len() < MAX_LATENCY_SAMPLES {
            self.details.push(crate::report::bench::BenchDetail {
                timestamp: crate::polyfill::runtime::now_timestamp(),
                latency_ns,
                status: status.to_string(),
                error: error.map(|s| s.to_string()),
            });
        }
    }

    fn compute_percentile(&self, p: f64) -> u64 {
        self.latency.percentile(p)
    }

    fn to_percentiles(&self, requested: &[String]) -> Vec<BenchPercentile> {
        let mut result = Vec::new();
        for token in requested {
            let t = token.trim_ascii();
            if let Some(stripped) = t.strip_prefix('p')
                && let Ok(pct) = stripped.trim_ascii().parse::<f64>()
            {
                result.push(BenchPercentile {
                    percentile: pct,
                    latency_ns: self.latency.percentile(pct),
                });
            }
        }
        result.sort_by(|a, b| a.percentile.partial_cmp(&b.percentile).unwrap());
        result
    }

    /// Render the bounded histogram as `bucket_count` linear display buckets
    /// spanning the exact [min, max] range. Counts are folded in from the
    /// log-linear buckets by their representative (mid-point) value, keeping the
    /// report's `histogram` shape (lower_ns/upper_ns/count/frequency) unchanged.
    fn to_histogram(&self, bucket_count: usize) -> Vec<BenchHistogramBucket> {
        if self.latency.is_empty() || bucket_count == 0 {
            return vec![];
        }

        let min = self.latency.min;
        let max = self.latency.max;

        if min == max {
            return vec![BenchHistogramBucket {
                lower_ns: min,
                upper_ns: max,
                count: self.latency.total,
                frequency: 1.0,
            }];
        }

        // Guard against zero-width display buckets when min/max are close.
        let width = ((max - min) / bucket_count as u64).max(1);
        let mut buckets: Vec<BenchHistogramBucket> = (0..bucket_count)
            .map(|i| BenchHistogramBucket {
                lower_ns: min + i as u64 * width,
                upper_ns: min + (i + 1) as u64 * width,
                count: 0,
                frequency: 0.0,
            })
            .collect();

        for (i, &c) in self.latency.buckets.iter().enumerate() {
            if c == 0 {
                continue;
            }
            let (lo, hi) = hist_bucket_bounds(i);
            let mid = ((lo + hi) / 2).clamp(min, max); // representative value of this source bucket
            let idx = (((mid - min) / width).min((bucket_count - 1) as u64)) as usize;
            buckets[idx].count += c;
        }

        let total = self.latency.total as f64;
        for b in &mut buckets {
            b.frequency = b.count as f64 / total;
        }

        buckets
    }

    fn merge_from(&mut self, other: Self) {
        self.count += other.count;
        self.ok += other.ok;
        self.errors += other.errors;
        self.total_ns += other.total_ns;

        if self.fastest_ns == 0 || (other.fastest_ns > 0 && other.fastest_ns < self.fastest_ns) {
            self.fastest_ns = other.fastest_ns;
        }
        if other.slowest_ns > self.slowest_ns {
            self.slowest_ns = other.slowest_ns;
        }

        for (k, v) in other.grpc_status {
            *self.grpc_status.entry(k).or_insert(0) += v;
        }
        for (k, v) in other.error_dist {
            *self.error_dist.entry(k).or_insert(0) += v;
        }

        // Lossless bucket-wise merge of the per-worker latency distribution —
        // the key correctness win over the old downsample-on-merge.
        self.latency.merge(&other.latency);
        for (endpoint, data) in other.per_endpoint {
            let ep = self.per_endpoint.entry(endpoint).or_default();
            ep.count += data.count;
            ep.errors += data.errors;
            ep.latency.merge(&data.latency);
        }

        self.details.extend(other.details);
        if self.details.len() > MAX_LATENCY_SAMPLES {
            self.details.truncate(MAX_LATENCY_SAMPLES);
        }
    }
}

fn categorize_error(message: &str) -> String {
    let msg = message.to_lowercase();
    if msg.contains("assertion") || msg.contains("assert") {
        "assert_failure".to_string()
    } else if msg.contains("timeout") || msg.contains("deadline") {
        "timeout".to_string()
    } else if msg.contains("connection") || msg.contains("refused") || msg.contains("reset") {
        "connection_error".to_string()
    } else if msg.contains("unavailable") {
        "unavailable".to_string()
    } else if msg.contains("invalid") || msg.contains("malformed") {
        "invalid_input".to_string()
    } else {
        "other".to_string()
    }
}

/// Emit warnings for bench options that are accepted but do not (yet) influence
/// measurement, so users are not misled into thinking they took effect.
///
/// - `keepalive`: the bench harness constructs a fresh `TestRunner` (and thus a
///   fresh transport) per request, so there is no persistent gRPC channel on
///   which to set keepalive without channel pooling in the (frozen) transport
///   layer. Currently a no-op.
/// - `ramp_up` without a target RPS: with unbounded load there is no target to
///   ramp toward, so ramp-up has no observable effect.
/// - `cpus`: the tokio worker-thread count is fixed when the runtime starts in
///   `main.rs`; the bench harness cannot repartition it, so `cpus` is a no-op.
///   Use `--concurrency` to control the number of parallel workers.
/// - `mode`: only the closed-loop execution model is implemented. Any other
///   mode (e.g. `open`/`adaptive`) is accepted but ignored.
fn warn_ineffective_options(config: &BenchConfigResolved) {
    if config.cpus.is_some() {
        warn!(
            "bench: `cpus` is parsed but not honored — the tokio worker-thread count is fixed at runtime startup and the bench harness cannot repartition it; use `--concurrency` to control parallel workers"
        );
    }
    let mode = config.mode.trim_ascii().to_ascii_lowercase();
    match exec_model_for(&mode) {
        ExecModel::Open if mode == "adaptive" => {
            warn!(
                "bench: `mode` = 'adaptive' runs the open-model (arrival-rate) executor; adaptive rate control is not yet implemented"
            );
        }
        ExecModel::Open => {}
        ExecModel::Closed
            if !matches!(
                mode.as_str(),
                "fixed" | "closed" | "closed-loop" | "closed_loop"
            ) =>
        {
            warn!(
                "bench: `mode` = '{}' is not recognized — using the closed-loop execution model",
                config.mode
            );
        }
        ExecModel::Closed => {}
    }
    if config.keepalive.is_some() {
        warn!(
            "bench: `keepalive` is parsed but not applied — the harness builds a fresh transport per request and cannot set channel keepalive without gRPC channel pooling; option is currently a no-op"
        );
    }
    if config.ramp_up.is_some() {
        let has_target = config.max_rps.is_some()
            || config.load_start.is_some()
            || !config
                .load_schedule
                .trim_ascii()
                .eq_ignore_ascii_case("const");
        if !has_target {
            warn!(
                "bench: `ramp_up` is set but no target RPS (max_rps / load_start / load schedule) is configured; with unbounded load there is nothing to ramp and the option has no effect"
            );
        }
    }
}

/// Run actual benchmark with the given config
async fn run_benchmark(
    test_paths: &[std::path::PathBuf],
    config: &BenchConfigResolved,
    exclude: &[String],
) -> Result<BenchReport> {
    let start_ts = crate::polyfill::runtime::now_timestamp();

    let mut test_files = Vec::new();
    for path in test_paths {
        if path.is_dir() {
            test_files.extend(FileUtils::collect_test_files(path, exclude));
        } else if path.is_file() {
            test_files.push(path.clone());
        }
    }

    if test_files.is_empty() {
        warn!("No test files found for bench");
    }

    // Pre-parse all test files for performance (avoid re-parsing on every iteration)
    let test_docs: Vec<(std::path::PathBuf, crate::parser::GctfDocument)> = test_files
        .iter()
        .map(|f| {
            let result = crate::parser::parse_with_recovery(f);
            (f.clone(), result.document)
        })
        .collect();

    info!("Bench: found {} test files", test_files.len());
    warn_ineffective_options(config);

    // Graceful shutdown via SIGINT/SIGTERM
    let shutdown_requested = Arc::new(AtomicBool::new(false));
    {
        let flag = Arc::clone(&shutdown_requested);
        tokio::spawn(async move {
            tokio::signal::ctrl_c().await.ok();
            flag.store(true, Ordering::Relaxed);
            eprintln!("\nShutdown requested — finishing in-flight requests...");
        });
    }

    // Metrics collector (merged from per-worker local metrics)
    let mut metrics = BenchMetrics::default();
    let progress_count = Arc::new(AtomicU64::new(0));
    let progress_errors = Arc::new(AtomicU64::new(0));
    let progress_done = Arc::new(AtomicBool::new(false));

    // Calculate total iterations
    let total_requests = config.requests.unwrap_or(0);
    let has_duration = config.duration.is_some();
    let warmup = config.warmup;

    // Warmup phase
    if let Some(warmup_dur) = warmup {
        if config.warmup_mode == "dry_run" {
            eprintln!("Warmup phase (dry run — template parsing only, no gRPC)...");
        } else {
            eprintln!("Warmup phase for {:?}...", warmup_dur);
        }
        let warmup_start = Instant::now();
        while warmup_start.elapsed() < warmup_dur {
            for file in &test_files {
                if config.warmup_mode == "dry_run" {
                    // Parse template variables without making gRPC calls
                    let _ = crate::parser::parse_with_recovery(file);
                } else {
                    let _ = execute_single_bench_iteration(file, config).await;
                }
            }
        }
        eprintln!("Warmup complete.");
    }

    let source_config = if !config.sources.is_empty() {
        match crate::bench::sources::SourceDrivenConfig::prepare(&config.sources, &test_files[0]) {
            Ok(Some(sc)) => {
                let headers = sc.primary_headers();
                eprintln!(
                    "Data source: {} columns ({})",
                    headers.len(),
                    headers.join(", ")
                );
                Some(Arc::new(sc))
            }
            Ok(None) => None,
            Err(e) => {
                warn!("Source preparation failed: {e}");
                None
            }
        }
    } else {
        None
    };

    eprintln!("Starting benchmark...");
    let run_start = Instant::now();
    let progress_task = {
        let count = Arc::clone(&progress_count);
        let errors = Arc::clone(&progress_errors);
        let done = Arc::clone(&progress_done);
        let cfg = config.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(cfg.progress_interval);
            interval.tick().await;
            loop {
                interval.tick().await;
                if done.load(Ordering::Relaxed) {
                    break;
                }
                print_progress_snapshot(run_start, &count, &errors, &cfg);
            }
        })
    };

    // Select the execution model. `open`/`adaptive` need a target rate; without
    // one the open model has no defined arrival schedule, so fall back to
    // closed-loop with a warning.
    let exec_model = exec_model_for(&config.mode);
    let use_open = exec_model == ExecModel::Open && has_target_rate(config);
    if exec_model == ExecModel::Open && !has_target_rate(config) {
        warn!(
            "bench: open/adaptive mode needs a target rate (max_rps / load_start / load schedule); none configured — falling back to closed-loop"
        );
    }
    eprintln!(
        "Execution model: {}",
        if use_open {
            "open (arrival-rate)"
        } else {
            "closed-loop"
        }
    );

    // Run with duration or count limit
    if use_open && (has_duration || total_requests > 0) {
        let bound = if let Some(dur) = config.duration {
            RunBound::Duration(dur)
        } else {
            RunBound::Count(request_passes(total_requests, test_docs.len()))
        };
        metrics = run_open_model(
            &test_docs,
            config,
            bound,
            run_start,
            Arc::clone(&progress_count),
            Arc::clone(&progress_errors),
            Arc::clone(&shutdown_requested),
            source_config.clone(),
        )
        .await;
    } else if let Some(dur) = config.duration {
        let mut join_set = JoinSet::new();
        let schedule_start = run_start;

        for worker_id in 0..config.concurrency {
            let docs = test_docs.clone();
            let cfg = config.clone();
            let progress_count = Arc::clone(&progress_count);
            let progress_errors = Arc::clone(&progress_errors);
            let sc = source_config.clone();
            let shutdown = Arc::clone(&shutdown_requested);
            // Spread workers across `connections` distinct client channels.
            let connection_id = worker_connection_id(worker_id, config.connections);
            join_set.spawn(async move {
                let mut local = BenchMetrics::for_worker(
                    1000,
                    cfg.count_errors_in_latency,
                    sample_stride_from_rate(cfg.sample_rate),
                    cfg.skip_first,
                );
                let mut next_slot = Instant::now();
                let deadline = Instant::now() + dur;
                while Instant::now() < deadline && !shutdown.load(Ordering::Relaxed) {
                    for (_file, gctf_doc) in &docs {
                        if Instant::now() >= deadline || shutdown.load(Ordering::Relaxed) {
                            break;
                        }
                        wait_for_rps_slot(&cfg, schedule_start, &mut next_slot).await;

                        let vars = match &sc {
                            Some(sdc) => match sdc.next_row_variables() {
                                Ok(Some(v)) => v,
                                Ok(None) => {
                                    if let Err(e) = sdc
                                        .primary
                                        .lock()
                                        .map_err(|e| anyhow::anyhow!("{e}"))
                                        .and_then(|mut r| r.reset())
                                    {
                                        warn!("source reset failed: {e}");
                                    }
                                    match sdc.next_row_variables() {
                                        Ok(Some(v)) => v,
                                        _ => std::collections::HashMap::new(),
                                    }
                                }
                                Err(_) => std::collections::HashMap::new(),
                            },
                            None => std::collections::HashMap::new(),
                        };

                        let (lat_ns, status, error, endpoint) =
                            execute_single_bench_iteration_with_vars(
                                gctf_doc,
                                &cfg,
                                vars,
                                connection_id,
                            )
                            .await;
                        let finished_at = Instant::now();
                        if should_record_after_deadline(cfg.duration_stop, finished_at, deadline) {
                            local.record(lat_ns, &status, error.as_deref(), &endpoint);
                            progress_count.fetch_add(1, Ordering::Relaxed);
                            if status != "OK" {
                                progress_errors.fetch_add(1, Ordering::Relaxed);
                            }
                        }

                        if finished_at >= deadline
                            && matches!(cfg.duration_stop, DurationStopMode::Close)
                        {
                            break;
                        }
                    }
                }

                local
            });
        }

        while let Some(joined) = join_set.join_next().await {
            if let Ok(worker_metrics) = joined {
                metrics.merge_from(worker_metrics);
            }
        }
    } else if total_requests > 0 {
        let mut join_set = JoinSet::new();
        // `--requests` is the TOTAL request budget across all endpoints (per its
        // help text: "Total number of requests to send"). Each worker pass below
        // issues one request per doc, so scale the pass count by the number of
        // docs to keep the overall request count equal to `total_requests`
        // instead of `total_requests * docs.len()`.
        let total_passes = request_passes(total_requests, test_docs.len());
        let passes_per_worker = total_passes / config.concurrency as u64;
        let max_deadline = config.max_duration.map(|d| Instant::now() + d);
        let schedule_start = run_start;

        for worker_id in 0..config.concurrency {
            let docs = test_docs.clone();
            let cfg = config.clone();
            let progress_count = Arc::clone(&progress_count);
            let progress_errors = Arc::clone(&progress_errors);
            let is_last = worker_id == config.concurrency - 1;
            let worker_requests = if is_last {
                passes_per_worker + (total_passes % config.concurrency as u64)
            } else {
                passes_per_worker
            };
            let sc = source_config.clone();
            let shutdown = Arc::clone(&shutdown_requested);
            // Spread workers across `connections` distinct client channels.
            let connection_id = worker_connection_id(worker_id, config.connections);

            join_set.spawn(async move {
                let mut local = BenchMetrics::for_worker(
                    worker_requests as usize,
                    cfg.count_errors_in_latency,
                    sample_stride_from_rate(cfg.sample_rate),
                    cfg.skip_first,
                );
                let mut next_slot = Instant::now();
                for _ in 0..worker_requests {
                    if shutdown.load(Ordering::Relaxed) {
                        break;
                    }
                    if let Some(deadline) = max_deadline
                        && Instant::now() >= deadline
                    {
                        break;
                    }

                    for (_file, gctf_doc) in &docs {
                        if shutdown.load(Ordering::Relaxed) {
                            break;
                        }
                        if let Some(deadline) = max_deadline
                            && Instant::now() >= deadline
                        {
                            break;
                        }

                        wait_for_rps_slot(&cfg, schedule_start, &mut next_slot).await;

                        let vars = match &sc {
                            Some(sdc) => match sdc.next_row_variables() {
                                Ok(Some(v)) => v,
                                Ok(None) => {
                                    if let Err(e) = sdc
                                        .primary
                                        .lock()
                                        .map_err(|e| anyhow::anyhow!("{e}"))
                                        .and_then(|mut r| r.reset())
                                    {
                                        warn!("source reset failed: {e}");
                                    }
                                    match sdc.next_row_variables() {
                                        Ok(Some(v)) => v,
                                        _ => std::collections::HashMap::new(),
                                    }
                                }
                                Err(_) => std::collections::HashMap::new(),
                            },
                            None => std::collections::HashMap::new(),
                        };

                        let (lat_ns, status, error, endpoint) =
                            execute_single_bench_iteration_with_vars(
                                gctf_doc,
                                &cfg,
                                vars,
                                connection_id,
                            )
                            .await;
                        local.record(lat_ns, &status, error.as_deref(), &endpoint);
                        progress_count.fetch_add(1, Ordering::Relaxed);
                        if status != "OK" {
                            progress_errors.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }

                local
            });
        }

        while let Some(joined) = join_set.join_next().await {
            if let Ok(worker_metrics) = joined {
                metrics.merge_from(worker_metrics);
            }
        }
    }

    progress_done.store(true, Ordering::Relaxed);
    let _ = progress_task.await;
    print_progress_snapshot(run_start, &progress_count, &progress_errors, config);

    let run_elapsed = run_start.elapsed();
    let end_ts = crate::polyfill::runtime::now_timestamp();

    let user_cancelled = shutdown_requested.load(Ordering::Relaxed);
    let end_reason = derive_end_reason(
        has_duration,
        config.max_duration,
        run_elapsed,
        user_cancelled,
    );

    build_report(
        start_ts,
        end_ts,
        end_reason,
        config,
        metrics,
        run_elapsed,
        source_config.as_ref(),
    )
}

async fn wait_for_rps_slot(
    config: &BenchConfigResolved,
    schedule_start: Instant,
    next_slot: &mut Instant,
) {
    let target_total_rps = target_rps_at(config, schedule_start.elapsed());
    if target_total_rps <= 0.0 {
        return;
    }

    let worker_rps = (target_total_rps / config.concurrency as f64).max(0.01);
    let interval = Duration::from_secs_f64(1.0 / worker_rps);

    let now = Instant::now();
    if now < *next_slot {
        tokio::time::sleep(*next_slot - now).await;
    }
    *next_slot = std::cmp::max(*next_slot + interval, Instant::now());
}

fn target_rps_at(config: &BenchConfigResolved, elapsed: Duration) -> f64 {
    let schedule = config.load_schedule.trim_ascii().to_ascii_lowercase();
    let fallback = config.max_rps.unwrap_or(0.0);
    let start = config.load_start.unwrap_or(fallback);

    let _no_schedule = || -> f64 {
        if config.max_rps.is_some() {
            fallback.max(0.0)
        } else {
            start.max(0.0)
        }
    };

    let rps = match schedule.as_str() {
        "step" => {
            let step = config.load_step.unwrap_or(0.0);
            let step_duration = config.load_step_duration.unwrap_or(Duration::from_secs(1));
            let mut steps = (elapsed.as_secs_f64() / step_duration.as_secs_f64()).floor();
            if let Some(max_dur) = config.load_max_duration {
                let cap = (max_dur.as_secs_f64() / step_duration.as_secs_f64()).floor();
                steps = steps.min(cap);
            }

            let mut target = start + step * steps;
            if let Some(end) = config.load_end {
                if step >= 0.0 {
                    target = target.min(end);
                } else {
                    target = target.max(end);
                }
            }
            target.max(0.0)
        }
        "line" => {
            let slope = config.load_step.unwrap_or(0.0);
            let mut t = elapsed.as_secs_f64();
            if let Some(max_dur) = config.load_max_duration {
                t = t.min(max_dur.as_secs_f64());
            }
            let mut target = start + slope * t;
            if let Some(end) = config.load_end {
                if slope >= 0.0 {
                    target = target.min(end);
                } else {
                    target = target.max(end);
                }
            }
            target.max(0.0)
        }
        "sine" => {
            let midpoint = config.load_midpoint.unwrap_or(fallback);
            let amplitude = config.load_amplitude.unwrap_or(midpoint * 0.5);
            let frequency = config.load_frequency.unwrap_or(0.1);
            let t = elapsed.as_secs_f64();
            let target = midpoint + amplitude * (frequency * t).sin();
            target.max(0.0)
        }
        "spike" => {
            let baseline = start;
            let target = config.load_spike_target.unwrap_or(fallback);
            let spike_after = config.load_spike_after.unwrap_or(30.0);
            let spike_dur = config.load_spike_duration.unwrap_or(10.0);
            let t = elapsed.as_secs_f64();
            if t >= spike_after && t < spike_after + spike_dur {
                target.max(0.0)
            } else {
                baseline.max(0.0)
            }
        }
        "custom" => {
            let t = elapsed.as_secs_f64();
            config
                .load_profile
                .as_ref()
                .map_or(start.max(0.0), |profile| {
                    interpolate_custom_profile(profile, t)
                })
        }
        _ => {
            if config.max_rps.is_some() {
                fallback.max(0.0)
            } else {
                start.max(0.0)
            }
        }
    };

    // Ramp-up overlay: linearly scale the target load from ~0 up to the computed
    // steady-state target over the first `ramp_up` seconds. Only meaningful when a
    // target RPS exists (max_rps / load schedule); with unbounded load `rps == 0`
    // and there is nothing to ramp.
    if let Some(ramp) = config.ramp_up {
        let ramp_secs = ramp.as_secs_f64();
        let t = elapsed.as_secs_f64();
        if ramp_secs > 0.0 && t < ramp_secs {
            return (rps * (t / ramp_secs)).max(0.0);
        }
    }

    // Cool-down overlay: if elapsed exceeds duration, ramp RPS to 0
    if let (Some(dur), Some(cd)) = (config.duration, config.cool_down) {
        let dur_secs = dur.as_secs_f64();
        let cd_secs = cd.as_secs_f64();
        let t = elapsed.as_secs_f64();
        if t > dur_secs && cd_secs > 0.0 {
            let fraction = ((t - dur_secs) / cd_secs).min(1.0);
            return (rps * (1.0 - fraction)).max(0.0);
        }
    }

    rps
}

/// Load-generation execution model selected by the `mode` option.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExecModel {
    /// Each worker issues one request, awaits it, then paces to the next slot.
    /// Throughput is bounded by server latency (coordinated omission).
    Closed,
    /// Requests arrive on a fixed schedule regardless of completion; latency is
    /// measured from the *scheduled* arrival so backpressure is captured.
    Open,
}

/// Pure `mode` → execution-model dispatch. `open`/`adaptive` select the open
/// model (adaptive currently maps to open — no adaptive rate control yet);
/// everything else (`fixed`/`closed`/`closed-loop`/unknown) stays closed-loop.
fn exec_model_for(mode: &str) -> ExecModel {
    match mode.trim_ascii().to_ascii_lowercase().as_str() {
        "open" | "adaptive" => ExecModel::Open,
        _ => ExecModel::Closed,
    }
}

/// The open model needs a defined arrival rate. True when any target RPS is
/// configured (explicit cap, load_start, or a non-const load schedule).
fn has_target_rate(config: &BenchConfigResolved) -> bool {
    config.max_rps.is_some()
        || config.load_start.is_some()
        || !config
            .load_schedule
            .trim_ascii()
            .eq_ignore_ascii_case("const")
}

/// How long the open model runs: a wall-clock window (`-d`) or a fixed number
/// of scheduled arrivals (`-n`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunBound {
    Duration(Duration),
    Count(u64),
}

/// Idle step used when the instantaneous target rate is zero (e.g. the start of
/// a ramp or the off-phase of a spike): advance the schedule cursor without
/// emitting an arrival so a temporarily-idle schedule cannot spin.
const OPEN_IDLE_STEP: Duration = Duration::from_millis(5);

/// Lazy generator of open-model arrival offsets (relative to schedule start).
/// Successive arrivals are spaced by `1 / target_rps` sampled at the current
/// cursor, so ramp/step/sine/spike/custom schedules shape the arrival stream.
/// Pure and time-free, so scheduling behaviour is unit-testable.
struct ArrivalSchedule<F> {
    rate_at: F,
    cursor: Duration,
    bound: RunBound,
    emitted: u64,
}

impl<F: Fn(Duration) -> f64> ArrivalSchedule<F> {
    fn new(rate_at: F, bound: RunBound) -> Self {
        Self {
            rate_at,
            cursor: Duration::ZERO,
            bound,
            emitted: 0,
        }
    }
}

impl<F: Fn(Duration) -> f64> Iterator for ArrivalSchedule<F> {
    type Item = Duration;

    fn next(&mut self) -> Option<Duration> {
        loop {
            match self.bound {
                RunBound::Count(n) if self.emitted >= n => return None,
                RunBound::Duration(d) if self.cursor >= d => return None,
                _ => {}
            }

            let rate = (self.rate_at)(self.cursor);
            if rate > 0.0 {
                let arrival = self.cursor;
                self.cursor += Duration::from_secs_f64(1.0 / rate);
                self.emitted += 1;
                return Some(arrival);
            }

            self.cursor += OPEN_IDLE_STEP;
        }
    }
}

/// Coordinated-omission-correct latency: measured from the request's *intended*
/// arrival slot to completion, so any wait for an in-flight permit (backpressure
/// when the concurrency cap is saturated) is included in the sample.
fn latency_ns_from_arrival(arrival: Instant, finished: Instant) -> u64 {
    finished.saturating_duration_since(arrival).as_nanos() as u64
}

/// Pull the next data-source variable row (with reset-on-exhaustion), mirroring
/// the closed-loop source handling. Called from the scheduler thread so row
/// ordering stays deterministic.
fn next_source_vars(
    source_config: &Option<Arc<crate::bench::sources::SourceDrivenConfig>>,
) -> HashMap<String, serde_json::Value> {
    match source_config {
        Some(sdc) => match sdc.next_row_variables() {
            Ok(Some(v)) => v,
            Ok(None) => {
                if let Err(e) = sdc
                    .primary
                    .lock()
                    .map_err(|e| anyhow::anyhow!("{e}"))
                    .and_then(|mut r| r.reset())
                {
                    warn!("source reset failed: {e}");
                }
                match sdc.next_row_variables() {
                    Ok(Some(v)) => v,
                    _ => HashMap::new(),
                }
            }
            Err(_) => HashMap::new(),
        },
        None => HashMap::new(),
    }
}

/// Map a run's numeric gRPC status into the label used for the bench status
/// distribution. Reuses the canonical gRPC code→name table so codes bucket by
/// their real status (`OK`, `Unavailable`, `NotFound`, ...) instead of a flat
/// `OK`/`ERROR`. Falls back to the pass/fail outcome when the run produced no
/// gRPC status at all (e.g. a pure assertion or config failure).
fn grpc_status_label(grpc_status: Option<u32>, passed: bool) -> String {
    match grpc_status {
        Some(code) => crate::execution::TestRunner::grpc_code_name_from_numeric(code as i64)
            .map(|name| name.to_string())
            .unwrap_or_else(|| format!("CODE_{code}")),
        None => if passed { "OK" } else { "ERROR" }.to_string(),
    }
}

/// Connection-pool slot for a closed-loop worker: worker index modulo the pool
/// size, so `connections` workers cycle over `connections` distinct channels.
fn worker_connection_id(worker_index: u32, connections: u32) -> u64 {
    (worker_index % connections.max(1)) as u64
}

/// Round-robin slot for the k-th open-model request across a pool of `pool_size`
/// prebuilt runners: task k dispatches on `runners[k % pool_size]`.
fn round_robin_index(task_index: usize, pool_size: usize) -> usize {
    task_index % pool_size.max(1)
}

/// Run a single request against a *prebuilt, shared* runner. Returns the
/// outcome only — the caller computes latency from the scheduled arrival.
async fn run_request_with_runner(
    runner: &crate::execution::TestRunner,
    doc: &GctfDocument,
    vars: HashMap<String, serde_json::Value>,
) -> (String, Option<String>, String) {
    use crate::execution::TestExecutionStatus;

    let endpoint = doc.get_endpoint().unwrap_or_else(|| "unknown".to_string());
    match runner.run_test_with_variables(doc, vars).await {
        Ok(result) => {
            let passed = matches!(result.status, TestExecutionStatus::Pass);
            let status = grpc_status_label(result.grpc_status, passed);
            let error = match result.status {
                TestExecutionStatus::Pass => None,
                TestExecutionStatus::Fail(msg) => Some(msg),
            };
            (status, error, endpoint)
        }
        Err(e) => ("ERROR".to_string(), Some(e.to_string()), endpoint),
    }
}

/// Open-model (arrival-rate) executor.
///
/// Scheduling is decoupled from completion: the scheduler sleeps until each
/// arrival slot (derived from `target_rps_at`) and spawns the request as a task
/// *without* awaiting the previous one. A `Semaphore` bounds concurrent
/// in-flight requests to `concurrency`; crucially the permit is acquired
/// *inside* the spawned task, never by the scheduler, so a saturated cap applies
/// backpressure to requests but never stalls arrival scheduling. Each sample's
/// latency is taken from `latency_ns_from_arrival`, i.e. the intended slot, so
/// permit-wait time counts against latency — the coordinated-omission fix.
#[allow(clippy::too_many_arguments)]
async fn run_open_model(
    test_docs: &[(std::path::PathBuf, GctfDocument)],
    config: &BenchConfigResolved,
    bound: RunBound,
    schedule_start: Instant,
    progress_count: Arc<AtomicU64>,
    progress_errors: Arc<AtomicU64>,
    shutdown: Arc<AtomicBool>,
    source_config: Option<Arc<crate::bench::sources::SourceDrivenConfig>>,
) -> BenchMetrics {
    use crate::execution::TestRunner;

    if test_docs.is_empty() {
        return BenchMetrics::for_worker(
            0,
            config.count_errors_in_latency,
            sample_stride_from_rate(config.sample_rate),
            config.skip_first,
        );
    }

    // Prebuild `connections` runners, one per distinct client channel, and
    // round-robin spawned requests across them. Channels and descriptors are
    // globally cached (keyed by connection_id), so N distinct ids open N
    // distinct HTTP/2 channels while keeping client construction off the
    // per-request hot path.
    let timeout_seconds = config.duration.map_or(30, |d| d.as_secs()).max(1);
    let no_assert = config.no_assert || config.assert_mode == "off" || config.assert_mode == "skip";
    let runners: Vec<Arc<TestRunner>> = (0..config.connections.max(1))
        .map(|i| {
            Arc::new(
                TestRunner::new(false, timeout_seconds, no_assert, false, false, None)
                    .with_protocol(config.protocol)
                    .with_connection_id(i as u64),
            )
        })
        .collect();

    // Share docs behind Arc so per-arrival dispatch clones a pointer, not the AST.
    let docs: Vec<Arc<GctfDocument>> = test_docs
        .iter()
        .map(|(_, doc)| Arc::new(doc.clone()))
        .collect();

    let hint = match bound {
        RunBound::Count(n) => n as usize,
        RunBound::Duration(_) => 1000,
    };
    let metrics = Arc::new(tokio::sync::Mutex::new(BenchMetrics::for_worker(
        hint,
        config.count_errors_in_latency,
        sample_stride_from_rate(config.sample_rate),
        config.skip_first,
    )));

    let semaphore = Arc::new(tokio::sync::Semaphore::new(
        config.concurrency.max(1) as usize
    ));

    // Stop scheduling once the run window (or `--max-duration`) is reached.
    let deadline = match bound {
        RunBound::Duration(d) => Some(schedule_start + d),
        RunBound::Count(_) => config.max_duration.map(|d| schedule_start + d),
    };

    let cfg_for_rate = config.clone();
    let schedule =
        ArrivalSchedule::new(move |elapsed| target_rps_at(&cfg_for_rate, elapsed), bound);

    let mut tasks = JoinSet::new();

    for (doc_cursor, arrival_offset) in schedule.enumerate() {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }
        if let Some(dl) = deadline
            && Instant::now() >= dl
        {
            break;
        }

        let arrival_instant = schedule_start + arrival_offset;
        let now = Instant::now();
        if arrival_instant > now {
            tokio::time::sleep(arrival_instant - now).await;
        }
        if shutdown.load(Ordering::Relaxed) {
            break;
        }

        let doc = Arc::clone(&docs[doc_cursor % docs.len()]);
        let vars = next_source_vars(&source_config);

        let permits = Arc::clone(&semaphore);
        // Round-robin task k across the `connections` channels: k -> runners[k % N].
        let runner = Arc::clone(&runners[round_robin_index(doc_cursor, runners.len())]);
        let metrics = Arc::clone(&metrics);
        let progress_count = Arc::clone(&progress_count);
        let progress_errors = Arc::clone(&progress_errors);
        let duration_stop = config.duration_stop;

        tasks.spawn(async move {
            // Acquire the in-flight permit HERE (inside the task): if the cap is
            // saturated we queue, and because latency is measured from
            // `arrival_instant` the queuing delay is captured in the sample.
            let _permit = permits.acquire_owned().await;
            let (status, error, endpoint) = run_request_with_runner(&runner, &doc, vars).await;
            let finished_at = Instant::now();
            let lat_ns = latency_ns_from_arrival(arrival_instant, finished_at);

            let record = match deadline {
                Some(dl) => should_record_after_deadline(duration_stop, finished_at, dl),
                None => true,
            };
            if record {
                let mut m = metrics.lock().await;
                m.record(lat_ns, &status, error.as_deref(), &endpoint);
                drop(m);
                progress_count.fetch_add(1, Ordering::Relaxed);
                if status != "OK" {
                    progress_errors.fetch_add(1, Ordering::Relaxed);
                }
            }
        });

        // Reap already-finished tasks so the JoinSet doesn't accumulate handles.
        while tasks.try_join_next().is_some() {}
    }

    // Drain outstanding in-flight requests per the `duration_stop` policy.
    match config.duration_stop {
        DurationStopMode::Close => {
            // Don't wait for stragglers; requests already recorded stay counted.
            tasks.abort_all();
        }
        DurationStopMode::Wait | DurationStopMode::Ignore => {}
    }
    while tasks.join_next().await.is_some() {}

    Arc::try_unwrap(metrics)
        .map(tokio::sync::Mutex::into_inner)
        .unwrap_or_else(|_| unreachable!("all bench tasks joined; metrics Arc must be unique"))
}

fn print_progress_snapshot(
    run_start: Instant,
    progress_count: &Arc<AtomicU64>,
    progress_errors: &Arc<AtomicU64>,
    config: &BenchConfigResolved,
) {
    let count = progress_count.load(Ordering::Relaxed);
    if count == 0 {
        return;
    }
    let elapsed = run_start.elapsed().as_secs_f64();
    if elapsed <= 0.0 {
        return;
    }
    let err = progress_errors.load(Ordering::Relaxed);
    let rps = count as f64 / elapsed;
    let err_pct = (err as f64 / count as f64) * 100.0;
    let target_rps = target_rps_at(config, run_start.elapsed());
    eprintln!(
        "[bench] t={:.1}s req={} rps={:.2} target={:.2} err={:.2}%",
        elapsed, count, rps, target_rps, err_pct
    );
}

async fn execute_single_bench_iteration(
    file: &Path,
    config: &BenchConfigResolved,
) -> (u64, String, Option<String>, String) {
    let parse_result = crate::parser::parse_with_recovery(file);
    execute_single_bench_iteration_with_vars(&parse_result.document, config, HashMap::new(), 0)
        .await
}

async fn execute_single_bench_iteration_with_vars(
    doc: &GctfDocument,
    config: &BenchConfigResolved,
    source_variables: HashMap<String, serde_json::Value>,
    connection_id: u64,
) -> (u64, String, Option<String>, String) {
    use crate::execution::{TestExecutionStatus, TestRunner};

    let start = Instant::now();

    let endpoint = doc.get_endpoint().unwrap_or_else(|| "unknown".to_string());

    let timeout_seconds = config.duration.map_or(30, |d| d.as_secs()).max(1);
    let no_assert = config.no_assert || config.assert_mode == "off" || config.assert_mode == "skip";

    let runner = TestRunner::new(false, timeout_seconds, no_assert, false, false, None)
        .with_protocol(config.protocol)
        .with_connection_id(connection_id);
    match runner.run_test_with_variables(doc, source_variables).await {
        Ok(result) => {
            let latency = start.elapsed().as_nanos() as u64;
            let passed = matches!(result.status, TestExecutionStatus::Pass);
            let status = grpc_status_label(result.grpc_status, passed);
            let error = match result.status {
                TestExecutionStatus::Pass => None,
                TestExecutionStatus::Fail(msg) => Some(msg),
            };
            (latency, status, error, endpoint)
        }
        Err(e) => (
            start.elapsed().as_nanos() as u64,
            "ERROR".to_string(),
            Some(e.to_string()),
            endpoint,
        ),
    }
}

fn evaluate_thresholds(
    metrics: &BenchMetrics,
    thresholds: &HashMap<String, String>,
) -> Vec<BenchThresholdResult> {
    let mut results = Vec::new();
    for (key, expr) in thresholds {
        let (op, rhs_str) = parse_threshold_expr(expr);
        // Parse the numeric part, tolerating unit suffixes (e.g. "5%", "200ms",
        // "1.5s"). An unparsable threshold must ERROR rather than silently
        // collapsing to 0.0 (which would make e.g. "< 5%" compare against 0).
        let rhs = match parse_threshold_number(rhs_str) {
            Some(v) => v,
            None => {
                results.push(BenchThresholdResult {
                    metric: key.clone(),
                    expr: expr.clone(),
                    passed: false,
                    actual: "unknown".to_string(),
                    reason: Some(format!("invalid threshold value '{}'", rhs_str)),
                });
                continue;
            }
        };

        let actual_f64 = resolve_metric_value(metrics, key);
        if actual_f64.is_none() {
            results.push(BenchThresholdResult {
                metric: key.clone(),
                expr: expr.clone(),
                passed: false,
                actual: "unknown".to_string(),
                reason: Some(format!("unknown threshold metric '{}'", key)),
            });
            continue;
        }

        let actual_f64 = actual_f64.unwrap_or(0.0);
        let passed = match op {
            "<" => actual_f64 < rhs,
            "<=" => actual_f64 <= rhs,
            ">" => actual_f64 > rhs,
            ">=" => actual_f64 >= rhs,
            _ => false,
        };

        results.push(BenchThresholdResult {
            metric: key.clone(),
            expr: expr.clone(),
            passed,
            actual: format_metric_value(key, actual_f64),
            reason: if passed {
                None
            } else {
                Some(format!(
                    "{} {} {}",
                    format_metric_value(key, actual_f64),
                    invert_op(op),
                    rhs_str
                ))
            },
        });
    }
    results
}

fn parse_threshold_expr(expr: &str) -> (&str, &str) {
    let v = expr.trim_ascii();
    if let Some(rest) = v.strip_prefix("<=") {
        ("<=", rest.trim_ascii())
    } else if let Some(rest) = v.strip_prefix(">=") {
        (">=", rest.trim_ascii())
    } else if let Some(rest) = v.strip_prefix('<') {
        ("<", rest.trim_ascii())
    } else if let Some(rest) = v.strip_prefix('>') {
        (">", rest.trim_ascii())
    } else {
        ("", v)
    }
}

/// Parse the numeric part of a threshold right-hand side, stripping a trailing
/// unit suffix (`%`, `ms`, `us`, `ns`, `s`, `m`) and surrounding whitespace.
/// Returns `None` when the remaining text is not a valid number.
fn parse_threshold_number(rhs: &str) -> Option<f64> {
    let v = rhs.trim();
    let num = if let Some(rest) = v.strip_suffix('%') {
        rest
    } else if let Some(rest) = v.strip_suffix("ms") {
        rest
    } else if let Some(rest) = v.strip_suffix("us") {
        rest
    } else if let Some(rest) = v.strip_suffix("ns") {
        rest
    } else if let Some(rest) = v.strip_suffix('s') {
        rest
    } else if let Some(rest) = v.strip_suffix('m') {
        rest
    } else {
        v
    };
    num.trim().parse::<f64>().ok()
}

fn invert_op(op: &str) -> &str {
    match op {
        "<" => ">=",
        "<=" => ">",
        ">" => "<=",
        ">=" => "<",
        _ => "!=",
    }
}

fn resolve_metric_value(metrics: &BenchMetrics, key: &str) -> Option<f64> {
    let k = key.trim_ascii().to_ascii_lowercase();
    if k == "count" {
        return Some(metrics.count as f64);
    }
    if k == "ok" {
        return Some(metrics.ok as f64);
    }
    if k == "errors" {
        return Some(metrics.errors as f64);
    }
    if k == "average_ns" || k == "avg_ns" {
        return Some(
            metrics
                .total_ns
                .checked_div(metrics.count)
                .map(|v| v as f64)
                .unwrap_or(0.0),
        );
    }
    if k == "average_ms" || k == "avg_ms" {
        return Some(if metrics.count > 0 {
            (metrics.total_ns as f64 / metrics.count as f64) / 1_000_000.0
        } else {
            0.0
        });
    }
    if k == "fastest_ns" || k == "min_ns" {
        return Some(metrics.fastest_ns as f64);
    }
    if k == "fastest_ms" || k == "min_ms" {
        return Some(metrics.fastest_ns as f64 / 1_000_000.0);
    }
    if k == "slowest_ns" || k == "max_ns" {
        return Some(metrics.slowest_ns as f64);
    }
    if k == "slowest_ms" || k == "max_ms" {
        return Some(metrics.slowest_ns as f64 / 1_000_000.0);
    }
    if k == "total_ns" {
        return Some(metrics.total_ns as f64);
    }
    if k == "error_rate_pct" || k == "error_rate" {
        if metrics.count == 0 {
            return Some(0.0);
        }
        return Some((metrics.errors as f64 / metrics.count as f64) * 100.0);
    }
    if let Some(inner) = parse_percentile_key(&k)
        && let Ok(pct) = inner.parse::<f64>()
    {
        if k.starts_with("latency_ms.") {
            return Some(metrics.compute_percentile(pct) as f64 / 1_000_000.0);
        }
        return Some(metrics.compute_percentile(pct) as f64);
    }
    None
}

fn parse_percentile_key(key: &str) -> Option<String> {
    if let Some(inner) = key.strip_prefix("p(") {
        return inner.strip_suffix(')').map(ToString::to_string);
    }
    if let Some(inner) = key.strip_prefix("latency_ms.p(") {
        return inner.strip_suffix(')').map(ToString::to_string);
    }
    if let Some(inner) = key.strip_prefix("latency_ns.p(") {
        return inner.strip_suffix(')').map(ToString::to_string);
    }
    None
}

fn format_metric_value(key: &str, value: f64) -> String {
    let k = key.trim_ascii().to_ascii_lowercase();
    if k.contains("_ns") || k.starts_with("p(") || k.starts_with("latency_ns.p(") {
        return format_ns_value(value.max(0.0) as u64);
    }
    if k.contains("_ms") || k.starts_with("latency_ms.p(") {
        return format!("{value:.3}ms");
    }
    if k.contains("_pct") || k.contains("error_rate") {
        return format!("{value:.3}%");
    }
    format!("{value:.3}")
}

fn format_ns_value(ns: u64) -> String {
    if ns >= 1_000_000_000 {
        format!("{:.3}s", ns as f64 / 1_000_000_000.0)
    } else if ns >= 1_000_000 {
        format!("{:.3}ms", ns as f64 / 1_000_000.0)
    } else if ns >= 1_000 {
        format!("{:.3}us", ns as f64 / 1_000.0)
    } else {
        format!("{}ns", ns)
    }
}

fn should_record_after_deadline(
    mode: DurationStopMode,
    finished_at: Instant,
    deadline: Instant,
) -> bool {
    if finished_at < deadline {
        return true;
    }

    match mode {
        DurationStopMode::Close => false,
        DurationStopMode::Wait => true,
        DurationStopMode::Ignore => false,
    }
}

fn derive_end_reason(
    has_duration: bool,
    max_duration: Option<Duration>,
    run_elapsed: Duration,
    shutdown_requested: bool,
) -> &'static str {
    if shutdown_requested {
        "user_cancelled"
    } else if has_duration {
        "duration_reached"
    } else if max_duration.is_some_and(|limit| run_elapsed >= limit) {
        "max_duration_reached"
    } else {
        "requests_completed"
    }
}

fn build_report(
    start_ts: i64,
    end_ts: i64,
    end_reason: &str,
    config: &BenchConfigResolved,
    metrics: BenchMetrics,
    elapsed: Duration,
    source_config: Option<&std::sync::Arc<crate::bench::sources::SourceDrivenConfig>>,
) -> Result<BenchReport> {
    let source_for = |key: &str| {
        config
            .option_sources
            .get(key)
            .copied()
            .unwrap_or(BenchOptionSource::Default)
            .as_str()
            .to_string()
    };

    // `skip_first` warm-up trimming is applied per accumulator as samples are
    // recorded (see `BenchMetrics::record`), so the merged histogram already
    // excludes those outliers here.
    let count = metrics.count;
    let avg_ns = metrics.total_ns.checked_div(count).unwrap_or(0);

    let rps = if elapsed.as_secs_f64() > 0.0 {
        count as f64 / elapsed.as_secs_f64()
    } else {
        0.0
    };

    let latency_dist = metrics.to_percentiles(&config.latency_percentiles);
    let histogram = metrics.to_histogram(10);

    let threshold_results = evaluate_thresholds(&metrics, &config.thresholds);

    let mut options_resolved = BTreeMap::new();
    options_resolved.insert(
        "load_schedule".to_string(),
        crate::report::bench::BenchOptionValue {
            value: config.load_schedule.clone(),
            source: source_for("load_schedule"),
        },
    );
    options_resolved.insert(
        "concurrency".to_string(),
        crate::report::bench::BenchOptionValue {
            value: config.concurrency.to_string(),
            source: source_for("concurrency"),
        },
    );
    options_resolved.insert(
        "progress_interval".to_string(),
        crate::report::bench::BenchOptionValue {
            value: format!("{}s", config.progress_interval.as_secs_f64()),
            source: source_for("progress_interval"),
        },
    );
    if let Some(v) = config.load_start {
        options_resolved.insert(
            "load_start".to_string(),
            crate::report::bench::BenchOptionValue {
                value: v.to_string(),
                source: source_for("load_start"),
            },
        );
    }
    if let Some(v) = config.load_step {
        options_resolved.insert(
            "load_step".to_string(),
            crate::report::bench::BenchOptionValue {
                value: v.to_string(),
                source: source_for("load_step"),
            },
        );
    }
    if let Some(v) = config.load_end {
        options_resolved.insert(
            "load_end".to_string(),
            crate::report::bench::BenchOptionValue {
                value: v.to_string(),
                source: source_for("load_end"),
            },
        );
    }
    if let Some(v) = config.load_step_duration {
        options_resolved.insert(
            "load_step_duration".to_string(),
            crate::report::bench::BenchOptionValue {
                value: format!("{}s", v.as_secs_f64()),
                source: source_for("load_step_duration"),
            },
        );
    }
    if let Some(v) = config.load_max_duration {
        options_resolved.insert(
            "load_max_duration".to_string(),
            crate::report::bench::BenchOptionValue {
                value: format!("{}s", v.as_secs_f64()),
                source: source_for("load_max_duration"),
            },
        );
    }

    let report = BenchReport {
        schema_version: BENCH_REPORT_SCHEMA_VERSION.to_string(),
        run: BenchRunInfo {
            started_at: start_ts,
            ended_at: end_ts,
            end_reason: end_reason.to_string(),
            tool: "grpctestify".to_string(),
            tool_version: env!("CARGO_PKG_VERSION").to_string(),
        },
        options_resolved,
        summary: crate::report::bench::BenchSummary {
            count,
            ok: metrics.ok,
            errors: metrics.errors,
            total_ns: metrics.total_ns,
            average_ns: avg_ns,
            fastest_ns: metrics.fastest_ns,
            slowest_ns: metrics.slowest_ns,
            rps_observed: rps,
        },
        latency_distribution: latency_dist,
        histogram,
        grpc_status_distribution: metrics.grpc_status,
        error_distribution: metrics.error_dist,
        threshold_evaluation: threshold_results,
        details: metrics.details,
        tags: {
            // Record the execution model actually used (open falls back to
            // closed when no target rate is configured).
            let effective_model =
                if exec_model_for(&config.mode) == ExecModel::Open && has_target_rate(config) {
                    "open"
                } else {
                    "closed"
                };
            let mut tags = BTreeMap::new();
            tags.insert("exec_model".to_string(), effective_model.to_string());
            tags.insert("mode".to_string(), config.mode.clone());
            tags
        },
        sources_runtime: source_config.map(|sc| {
            let stats = sc.runtime_stats.snapshot();
            let mut source_stats = std::collections::BTreeMap::new();
            source_stats.insert(
                "global".to_string(),
                crate::report::bench::SourceRuntimeStats {
                    dimension_lookups: stats.dimension_lookups,
                    dimension_hits: stats.dimension_hits,
                    dimension_misses: stats.dimension_misses,
                    in_memory_lookups: stats.in_memory_lookups,
                    indexed_lookups: stats.indexed_lookups,
                    index_fallbacks: stats.index_fallbacks,
                },
            );
            crate::report::bench::SourcesRuntime { source_stats }
        }),
        per_endpoint: metrics
            .per_endpoint
            .into_iter()
            .map(
                |(endpoint, data)| crate::report::bench::PerEndpointSummary {
                    endpoint,
                    count: data.count,
                    errors: data.errors,
                    latency_p50: data.latency.percentile(50.0),
                    latency_p90: data.latency.percentile(90.0),
                    latency_p95: data.latency.percentile(95.0),
                    latency_p99: data.latency.percentile(99.0),
                },
            )
            .collect(),
    };

    Ok(report)
}

/// Validate BENCH section configuration from a parsed document.
/// Returns Ok(()) if the BENCH section is valid, or an error describing the issue.
pub fn validate_bench_config(doc: &crate::parser::GctfDocument) -> Result<()> {
    let bench_section = extract_bench_section(doc);
    BenchConfigResolved::from_bench_section(bench_section.as_ref())?;
    Ok(())
}

/// Main bench command handler
/// Canonicalize the `--log-format` value. `ndjson` (the value advertised in the
/// flag help) is an alias for the per-response JSON Lines format `detail-json`.
fn canonical_bench_format(fmt: &str) -> &str {
    match fmt {
        "ndjson" => "detail-json",
        other => other,
    }
}

pub async fn handle_bench(args: &BenchArgs) -> Result<()> {
    // Handle --list-profiles
    if args.list_profiles {
        crate::bench::schema::list_profiles()
            .iter()
            .for_each(|(name, keys)| {
                let desc = keys.get("description").map(|s| s.as_str()).unwrap_or("");
                eprintln!("  {:<12} {}", name, desc);
            });
        return Ok(());
    }

    // Load custom profiles from --profile-file
    if let Some(ref profile_file) = args.profile_file {
        let yaml_content = std::fs::read_to_string(profile_file)
            .with_context(|| format!("Failed to read profile file: {}", profile_file.display()))?;
        let profiles: HashMap<String, HashMap<String, String>> =
            serde_yaml_ng::from_str(&yaml_content).context("Invalid profile YAML format")?;
        // Register custom profiles into a global store for apply_profile
        for (name, mut keys) in profiles {
            // Handle extends: inherit keys from parent profile
            if let Some(parent) = keys.remove("extends") {
                let parent_keys = crate::bench::schema::apply_profile(&parent);
                if parent_keys.is_empty() {
                    anyhow::bail!("Parent profile '{}' not found for '{}'", parent, name);
                }
                for (k, v) in parent_keys {
                    keys.entry(k.to_string()).or_insert(v.to_string());
                }
            }
            // Register into BUILTIN_PROFILES via a static registry
            crate::bench::schema::register_custom_profile(&name, keys);
        }
    }

    // Direct call mode: create temp .gctf from --call / --data flags
    let synthetic_path = if let Some(endpoint) = &args.call {
        let body = args.data.as_deref().unwrap_or("{}");
        let content = format!(
            "--- ADDRESS ---\n<env:GRPCTESTIFY_ADDRESS>\n--- ENDPOINT ---\n{endpoint}\n--- REQUEST ---\n{body}\n"
        );
        let dir = std::env::temp_dir().join("grpctestify-bench");
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(format!(
            "direct-{}.gctf",
            apif_cfg_runtime::now_unix_nanos()
        ));
        std::fs::write(&path, &content)?;
        Some(path)
    } else {
        None
    };

    let mut test_paths = args.test_paths.clone();
    if let Some(ref path) = synthetic_path {
        test_paths.push(path.clone());
    }

    if test_paths.is_empty() {
        anyhow::bail!("No test paths provided. Use paths, .gctf files, or --call SERVICE/METHOD");
    }

    eprintln!("BENCH MODE - Running benchmarks...");
    eprintln!();

    // Parse first test file to extract BENCH section
    let first_file = &test_paths[0];
    if !first_file.exists() {
        anyhow::bail!("File not found: {}", first_file.display());
    }

    // Store synthetic path in first_file for cleanup later
    let _ = synthetic_path;

    let parse_result = crate::parser::parse_with_recovery(first_file);
    let doc = parse_result.document;
    let bench_section = extract_bench_section(&doc);

    // Resolve configuration
    let config = BenchConfigResolved::from_cli_and_bench(args, bench_section.as_ref())?;

    // Print configuration
    eprintln!("Configuration:");
    eprintln!("  Profile: {}", config.profile);
    eprintln!("  Mode: {}", config.mode);
    eprintln!("  Concurrency: {}", config.concurrency);
    if let Some(n) = config.requests {
        eprintln!("  Requests: {}", n);
    }
    if let Some(d) = config.duration {
        eprintln!("  Duration: {:?}", d);
    }
    if let Some(d) = config.ramp_up {
        eprintln!("  Ramp-up: {:?}", d);
    }
    if let Some(d) = config.warmup {
        eprintln!("  Warmup: {:?}", d);
    }
    if let Some(d) = config.max_duration {
        eprintln!("  Max duration: {:?}", d);
    }
    if let Some(rps) = config.max_rps {
        eprintln!("  Max RPS: {}", rps);
    }
    eprintln!("  Load schedule: {}", config.load_schedule);
    if let Some(v) = config.load_start {
        eprintln!("  Load start: {}", v);
    }
    if let Some(v) = config.load_step {
        eprintln!("  Load step: {}", v);
    }
    if let Some(v) = config.load_end {
        eprintln!("  Load end: {}", v);
    }
    if let Some(v) = config.load_step_duration {
        eprintln!("  Load step duration: {:?}", v);
    }
    if let Some(v) = config.load_max_duration {
        eprintln!("  Load max duration: {:?}", v);
    }
    eprintln!("  Connections: {}", config.connections);
    eprintln!("  Connect timeout: {:?}", config.connect_timeout);
    if let Some(k) = config.keepalive {
        eprintln!("  Keepalive: {:?}", k);
    }
    if let Some(cpus) = config.cpus {
        eprintln!("  CPUs: {}", cpus);
    }
    if let Some(name) = &config.name {
        eprintln!("  Name: {}", name);
    }
    eprintln!("  Assert mode: {}", config.assert_mode);
    eprintln!("  No assert: {}", config.no_assert);
    eprintln!("  Sample rate: {}", config.sample_rate);
    eprintln!("  Cache: {}", config.cache);
    if config.skip_first > 0 {
        eprintln!("  Skip first: {}", config.skip_first);
    }
    if config.count_errors_in_latency {
        eprintln!("  Count errors in latency: true");
    }
    eprintln!("  Duration stop: {:?}", config.duration_stop);
    if !config.latency_percentiles.is_empty() {
        eprintln!(
            "  Latency percentiles: {}",
            config.latency_percentiles.join(",")
        );
    }
    if !config.thresholds.is_empty() {
        eprintln!("  Thresholds:");
        for (metric, expr) in &config.thresholds {
            eprintln!("    {}: {}", metric, expr);
        }
    }
    eprintln!();

    let report = run_benchmark(&test_paths, &config, &args.exclude).await?;

    // Allure benchmark attachment
    if let Some(allure_dir) = &args.allure_output_dir {
        std::fs::create_dir_all(allure_dir)?;
        let bench_json = serde_json::to_string_pretty(&report)?;
        let attachment_file = allure_dir.join("benchmark-report.json");
        std::fs::write(&attachment_file, &bench_json)?;
        eprintln!(
            "Allure benchmark attachment written to: {}",
            attachment_file.display()
        );
    }

    // Custom template rendering (overrides format)
    if let Some(template_path) = &args.report_template {
        let template_str = std::fs::read_to_string(template_path)
            .with_context(|| format!("Failed to read template: {}", template_path.display()))?;
        let mut env = minijinja::Environment::new();
        env.add_template("report", &template_str)
            .context("Invalid template syntax")?;
        let tmpl = env.get_template("report").unwrap();
        let report_json = serde_json::to_value(&report)?;
        let rendered = tmpl
            .render(minijinja::Value::from_serialize(&report_json))
            .context("Template rendering failed")?;
        if let Some(output) = &args.output {
            std::fs::write(output, &rendered)?;
            eprintln!("Rendered report written to: {}", output.display());
        } else {
            println!("{}", rendered);
        }
        return Ok(());
    }

    // Output report based on format
    match canonical_bench_format(args.format.as_str()) {
        "json" => {
            let json = serde_json::to_string_pretty(&report)?;
            if let Some(output) = &args.output {
                std::fs::write(output, json)?;
                eprintln!("Benchmark report written to: {}", output.display());
            } else {
                println!("{}", json);
            }
        }
        "prometheus" => {
            let prom = report.to_prometheus_summary();
            if let Some(output) = &args.output {
                std::fs::write(output, prom)?;
                eprintln!("Prometheus metrics written to: {}", output.display());
            } else {
                println!("{}", prom);
            }
        }
        "console" => {
            let summary = report.to_summary_text(args.compact);
            println!("{}", summary);
        }
        "csv" => {
            let s = &report.summary;
            let csv = format!(
                "count,ok,errors,total_ns,average_ns,fastest_ns,slowest_ns,rps\n{},{},{},{},{},{},{},{}\n",
                s.count,
                s.ok,
                s.errors,
                s.total_ns,
                s.average_ns,
                s.fastest_ns,
                s.slowest_ns,
                s.rps_observed
            );
            if let Some(output) = &args.output {
                std::fs::write(output, csv)?;
                eprintln!("CSV report written to: {}", output.display());
            } else {
                println!("{}", csv);
            }
        }
        "html" => {
            let html = report.to_html();
            if let Some(output) = &args.output {
                std::fs::write(output, html)?;
                eprintln!("HTML report written to: {}", output.display());
            } else {
                println!("{}", html);
            }
        }
        "detail-json" => {
            // Per-response JSON Lines — one JSON object per response
            if let Some(output) = &args.output {
                let mut file = std::fs::File::create(output)?;
                for detail in &report.details {
                    let line = serde_json::to_string(detail)?;
                    writeln!(file, "{}", line)?;
                }
                eprintln!("Detail JSON written to: {}", output.display());
            } else {
                for detail in &report.details {
                    println!("{}", serde_json::to_string(detail)?);
                }
            }
        }
        _ => {
            anyhow::bail!("Unsupported format: {}", args.format);
        }
    }

    if !report.thresholds_passed() {
        anyhow::bail!("Benchmark thresholds failed");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exec_model_dispatch() {
        assert_eq!(exec_model_for("open"), ExecModel::Open);
        assert_eq!(exec_model_for("adaptive"), ExecModel::Open);
        assert_eq!(exec_model_for(" OPEN "), ExecModel::Open);
        assert_eq!(exec_model_for("closed"), ExecModel::Closed);
        assert_eq!(exec_model_for("fixed"), ExecModel::Closed);
        assert_eq!(exec_model_for("closed-loop"), ExecModel::Closed);
        assert_eq!(exec_model_for("closed_loop"), ExecModel::Closed);
        // Unknown modes stay closed-loop (the safe default).
        assert_eq!(exec_model_for("stepping"), ExecModel::Closed);
    }

    #[test]
    fn test_open_schedule_count_exactly_n() {
        // Request-count open mode schedules exactly N arrivals.
        let arrivals: Vec<Duration> =
            ArrivalSchedule::new(|_| 100.0, RunBound::Count(500)).collect();
        assert_eq!(arrivals.len(), 500);
        // Fixed 100 rps → 10ms spacing.
        assert_eq!(arrivals[0], Duration::ZERO);
        assert_eq!(arrivals[1], Duration::from_millis(10));
    }

    #[test]
    fn test_open_schedule_duration_arrival_count() {
        // Fixed rate + duration produces ≈ rate * duration arrivals.
        let rate = 50.0;
        let dur = Duration::from_secs(2);
        let arrivals: Vec<Duration> =
            ArrivalSchedule::new(|_| rate, RunBound::Duration(dur)).collect();
        let expected = (rate * dur.as_secs_f64()) as usize; // 100
        assert!(
            (arrivals.len() as i64 - expected as i64).abs() <= 1,
            "expected ≈{expected} arrivals, got {}",
            arrivals.len()
        );
    }

    #[test]
    fn test_open_schedule_ramp_from_zero_terminates() {
        // Zero-rate window at the start must idle-advance (not spin) and still
        // terminate on the duration bound once the rate turns positive.
        let arrivals: Vec<Duration> = ArrivalSchedule::new(
            |t| {
                if t < Duration::from_millis(100) {
                    0.0
                } else {
                    100.0
                }
            },
            RunBound::Duration(Duration::from_secs(1)),
        )
        .collect();
        assert!(!arrivals.is_empty());
        assert!(arrivals[0] >= Duration::from_millis(100));
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_latency_measured_from_arrival() {
        // A request whose permit acquisition is delayed still reports latency
        // ≥ the induced delay (coordinated-omission correctness).
        let arrival = Instant::now();
        let delay = Duration::from_millis(20);
        std::thread::sleep(delay);
        let finished = Instant::now();
        let lat_ns = latency_ns_from_arrival(arrival, finished);
        assert!(
            lat_ns >= delay.as_nanos() as u64,
            "latency {lat_ns}ns should be >= induced delay {}ns",
            delay.as_nanos()
        );
    }

    #[test]
    fn test_has_target_rate() {
        let mut cfg = BenchConfigResolved::default();
        assert!(!has_target_rate(&cfg)); // const schedule, no rate
        cfg.max_rps = Some(100.0);
        assert!(has_target_rate(&cfg));
        cfg.max_rps = None;
        cfg.load_start = Some(50.0);
        assert!(has_target_rate(&cfg));
        cfg.load_start = None;
        cfg.load_schedule = "step".to_string();
        assert!(has_target_rate(&cfg));
    }

    #[test]
    fn test_parse_duration_seconds() {
        let d = parse_duration("30s").unwrap();
        assert_eq!(d.as_secs(), 30);
    }

    #[test]
    fn test_parse_duration_minutes() {
        let d = parse_duration("5m").unwrap();
        assert_eq!(d.as_secs(), 300);
    }

    #[test]
    fn test_parse_duration_hours() {
        let d = parse_duration("1h").unwrap();
        assert_eq!(d.as_secs(), 3600);
    }

    #[test]
    fn test_parse_duration_milliseconds() {
        let d = parse_duration("500ms").unwrap();
        assert_eq!(d.as_millis(), 500);
    }

    #[test]
    fn test_parse_duration_invalid() {
        assert!(parse_duration("").is_err());
        assert!(parse_duration("abc").is_err());
        assert!(parse_duration("30x").is_err());
    }

    // Bug 4: "ms" must be parsed before the single-char "s" suffix.
    #[test]
    fn test_parse_duration_sec_units() {
        assert_eq!(parse_duration_sec("500ms"), Some(0.5));
        assert_eq!(parse_duration_sec("2s"), Some(2.0));
        assert_eq!(parse_duration_sec("1m"), Some(60.0));
        assert_eq!(parse_duration_sec("1h"), Some(3600.0));
        assert_eq!(parse_duration_sec("10"), Some(10.0));
    }

    // Bug 4: load_profile points with "ms" durations must not be dropped.
    #[test]
    fn test_parse_custom_profile_with_ms() {
        let points = parse_custom_profile("500ms:10, 2s:100").expect("should parse");
        assert_eq!(points.len(), 2);
        assert_eq!(points[0], (0.5, 10.0));
        assert_eq!(points[1], (2.0, 100.0));
    }

    // Feed a set of samples into the histogram distribution.
    fn metrics_with_latencies(samples: &[u64]) -> BenchMetrics {
        let mut m = BenchMetrics::default();
        for &s in samples {
            m.latency.record(s);
        }
        m
    }

    // Bug 1: near-equal latencies must not cause a divide-by-zero panic.
    #[test]
    fn test_to_histogram_zero_width_no_panic() {
        let metrics = metrics_with_latencies(&[10, 10, 11, 12, 12]);
        // (max-min)/bucket_count = (12-10)/10 = 0 in integer math -> was a panic.
        let buckets = metrics.to_histogram(10);
        assert!(!buckets.is_empty());
        let total: u64 = buckets.iter().map(|b| b.count).sum();
        assert_eq!(total, 5);
    }

    // Bug 3: `--requests` is the TOTAL budget across all docs.
    #[test]
    fn test_request_passes_honours_total_budget() {
        // 100 total requests over 3 docs -> ~33 passes (33*3 = 99 requests).
        assert_eq!(request_passes(100, 3), 33);
        // Single doc: passes == requests (unchanged behaviour).
        assert_eq!(request_passes(100, 1), 100);
        // Zero docs must not divide by zero.
        assert_eq!(request_passes(100, 0), 100);
    }

    // Bug 2: unit-suffixed thresholds must parse, not silently become 0.0.
    #[test]
    fn test_evaluate_thresholds_percent_suffix() {
        let mut metrics = BenchMetrics {
            count: 100,
            errors: 2,
            ..Default::default()
        };
        metrics.ok = 98;
        let mut thresholds = HashMap::new();
        thresholds.insert("error_rate_pct".to_string(), "< 5%".to_string());
        let results = evaluate_thresholds(&metrics, &thresholds);
        assert_eq!(results.len(), 1);
        // 2% < 5% must PASS. With the old unwrap_or(0.0), rhs was 0 -> failed.
        assert!(results[0].passed, "2% should pass a < 5% threshold");
    }

    // Bug 2: unparsable thresholds must error instead of defaulting to 0.0.
    #[test]
    fn test_evaluate_thresholds_invalid_value_errors() {
        let metrics = BenchMetrics {
            count: 100,
            ..Default::default()
        };
        let mut thresholds = HashMap::new();
        thresholds.insert("error_rate_pct".to_string(), "< abc".to_string());
        let results = evaluate_thresholds(&metrics, &thresholds);
        assert_eq!(results.len(), 1);
        assert!(!results[0].passed);
        assert!(
            results[0]
                .reason
                .as_deref()
                .is_some_and(|r| r.contains("invalid threshold value"))
        );
    }

    #[test]
    fn test_bench_config_defaults() {
        let config = BenchConfigResolved::default();
        assert_eq!(config.profile, "functional");
        assert_eq!(config.mode, "fixed");
        assert_eq!(config.concurrency, 1);
        assert_eq!(config.requests, Some(100));
        assert_eq!(config.assert_mode, "collect_all");
        assert_eq!(config.duration_stop, DurationStopMode::Wait);
        assert_eq!(config.sample_rate, 1.0);
        assert!(config.cache);
    }

    #[test]
    fn ndjson_format_is_alias_for_detail_json() {
        // `ndjson` is advertised in --log-format help; it must map to the real
        // per-response JSON Lines format `detail-json` instead of erroring.
        assert_eq!(canonical_bench_format("ndjson"), "detail-json");
        assert_eq!(canonical_bench_format("detail-json"), "detail-json");
        assert_eq!(canonical_bench_format("json"), "json");
        assert_eq!(canonical_bench_format("console"), "console");
    }

    #[test]
    fn bench_protocol_override_resolves_from_cli() {
        use crate::cli::args::{Cli, Commands};
        use clap::Parser;

        let cli = Cli::parse_from(["grpctestify", "bench", "tests/", "--protocol", "grpc-web"]);
        let Some(Commands::Bench(args)) = cli.command else {
            panic!("expected bench command");
        };
        let config = BenchConfigResolved::from_cli_and_bench(&args, None).unwrap();
        assert_eq!(config.protocol, crate::grpc::WireProtocol::GrpcWeb);

        // Default (flag omitted) resolves to grpc.
        let cli = Cli::parse_from(["grpctestify", "bench", "tests/"]);
        let Some(Commands::Bench(args)) = cli.command else {
            panic!("expected bench command");
        };
        let config = BenchConfigResolved::from_cli_and_bench(&args, None).unwrap();
        assert_eq!(config.protocol, crate::grpc::WireProtocol::Grpc);
    }

    #[test]
    fn test_bench_config_cli_override() {
        let args = BenchArgs {
            protocol: "grpc".to_string(),
            test_paths: vec![],
            profile: Some("load".to_string()),
            mode: Some("stepping".to_string()),
            concurrency: Some(10),
            requests: Some(1000),
            duration: None,
            ramp_up: Some("2s".to_string()),
            warmup: Some("1s".to_string()),
            max_duration: None,
            max_rps: Some(100.0),
            load_schedule: None,
            load_start: None,
            load_step: None,
            load_end: None,
            load_step_duration: None,
            load_max_duration: None,
            connections: Some(5),
            connect_timeout: Some("3s".to_string()),
            keepalive: Some("1s".to_string()),
            cpus: Some(2),
            name: Some("load-test".to_string()),
            assert_mode: Some("skip".to_string()),
            no_assert: true,
            sample_rate: Some(0.1),
            cache: Some(false),
            skip_first: Some(5),
            count_errors_in_latency: Some(true),
            duration_stop: Some("ignore".to_string()),
            latency_percentiles: Some("p50,p95,p99".to_string()),
            progress_interval: None,
            format: "console".to_string(),
            output: None,
            compact: false,
            tags: vec![],
            skip_tags: vec![],
            exclude: vec![],
            report_template: None,
            allure_output_dir: None,
            profile_file: None,
            call: None,
            data: None,
            list_profiles: false,
        };

        let config = BenchConfigResolved::from_cli_and_bench(&args, None).unwrap();
        assert_eq!(config.profile, "load");
        assert_eq!(config.mode, "stepping");
        assert_eq!(config.concurrency, 10);
        assert_eq!(config.requests, Some(1000));
        assert_eq!(config.ramp_up, Some(Duration::from_secs(2)));
        assert_eq!(config.warmup, Some(Duration::from_secs(1)));
        assert_eq!(config.max_rps, Some(100.0));
        assert_eq!(config.connections, 5);
        assert_eq!(config.connect_timeout, Duration::from_secs(3));
        assert_eq!(config.keepalive, Some(Duration::from_secs(1)));
        assert_eq!(config.cpus, Some(2));
        assert_eq!(config.name.as_deref(), Some("load-test"));
        assert_eq!(config.assert_mode, "skip");
        assert!(config.no_assert);
        assert_eq!(config.latency_percentiles, vec!["p50", "p95", "p99"]);
        assert_eq!(config.sample_rate, 0.1);
        assert!(!config.cache);
        assert_eq!(config.skip_first, 5);
        assert!(config.count_errors_in_latency);
        assert_eq!(config.duration_stop, DurationStopMode::Ignore);
    }

    #[test]
    fn test_bench_config_bench_section() {
        let mut bench_section = HashMap::new();
        bench_section.insert("profile".to_string(), "stress".to_string());
        bench_section.insert("concurrency".to_string(), "50".to_string());
        bench_section.insert("requests".to_string(), "5000".to_string());
        bench_section.insert("threshold.latency_ms.p95".to_string(), "< 200".to_string());

        let args = BenchArgs {
            protocol: "grpc".to_string(),
            test_paths: vec![],
            profile: None,
            mode: None,
            concurrency: None,
            requests: None,
            duration: None,
            ramp_up: None,
            warmup: None,
            max_duration: None,
            max_rps: None,
            load_schedule: None,
            load_start: None,
            load_step: None,
            load_end: None,
            load_step_duration: None,
            load_max_duration: None,
            connections: None,
            connect_timeout: None,
            keepalive: None,
            cpus: None,
            name: None,
            assert_mode: None,
            no_assert: false,
            sample_rate: None,
            cache: None,
            skip_first: None,
            count_errors_in_latency: None,
            duration_stop: None,
            latency_percentiles: None,
            progress_interval: None,
            format: "console".to_string(),
            output: None,
            compact: false,
            tags: vec![],
            skip_tags: vec![],
            exclude: vec![],
            report_template: None,
            allure_output_dir: None,
            profile_file: None,
            call: None,
            data: None,
            list_profiles: false,
        };

        let config = BenchConfigResolved::from_cli_and_bench(&args, Some(&bench_section)).unwrap();
        assert_eq!(config.profile, "stress");
        assert_eq!(config.concurrency, 50);
        assert_eq!(config.requests, None);
        assert_eq!(config.thresholds.len(), 1);
        assert_eq!(
            config.thresholds.get("latency_ms.p95"),
            Some(&"< 200".to_string())
        );
    }

    #[test]
    fn test_bench_config_cli_overrides_bench_section() {
        let mut bench_section = HashMap::new();
        bench_section.insert("profile".to_string(), "stress".to_string());
        bench_section.insert("concurrency".to_string(), "50".to_string());

        let args = BenchArgs {
            protocol: "grpc".to_string(),
            test_paths: vec![],
            profile: Some("load".to_string()),
            mode: None,
            concurrency: Some(100),
            requests: None,
            duration: None,
            ramp_up: None,
            warmup: None,
            max_duration: None,
            max_rps: None,
            load_schedule: None,
            load_start: None,
            load_step: None,
            load_end: None,
            load_step_duration: None,
            load_max_duration: None,
            connections: None,
            connect_timeout: None,
            keepalive: None,
            cpus: None,
            name: None,
            assert_mode: None,
            no_assert: false,
            sample_rate: None,
            cache: None,
            skip_first: None,
            count_errors_in_latency: None,
            duration_stop: None,
            latency_percentiles: None,
            progress_interval: None,
            format: "console".to_string(),
            output: None,
            compact: false,
            tags: vec![],
            skip_tags: vec![],
            exclude: vec![],
            report_template: None,
            allure_output_dir: None,
            profile_file: None,
            call: None,
            data: None,
            list_profiles: false,
        };

        let config = BenchConfigResolved::from_cli_and_bench(&args, Some(&bench_section)).unwrap();
        assert_eq!(config.profile, "load"); // CLI overrides BENCH section
        assert_eq!(config.concurrency, 100); // CLI overrides BENCH section
    }

    #[test]
    fn test_bench_option_sources_track_cli_bench_default() {
        let mut bench_section = HashMap::new();
        bench_section.insert("concurrency".to_string(), "20".to_string());
        bench_section.insert("load_schedule".to_string(), "step".to_string());

        let args = BenchArgs {
            protocol: "grpc".to_string(),
            test_paths: vec![],
            profile: None,
            mode: None,
            concurrency: Some(50),
            requests: None,
            duration: None,
            ramp_up: None,
            warmup: None,
            max_duration: None,
            max_rps: None,
            load_schedule: None,
            load_start: None,
            load_step: None,
            load_end: None,
            load_step_duration: None,
            load_max_duration: None,
            connections: None,
            connect_timeout: None,
            keepalive: None,
            cpus: None,
            name: None,
            assert_mode: None,
            no_assert: false,
            sample_rate: None,
            cache: None,
            skip_first: None,
            count_errors_in_latency: None,
            duration_stop: None,
            latency_percentiles: None,
            progress_interval: None,
            format: "console".to_string(),
            output: None,
            compact: false,
            tags: vec![],
            skip_tags: vec![],
            exclude: vec![],
            report_template: None,
            allure_output_dir: None,
            profile_file: None,
            call: None,
            data: None,
            list_profiles: false,
        };

        let config = BenchConfigResolved::from_cli_and_bench(&args, Some(&bench_section)).unwrap();
        assert_eq!(
            config.option_sources.get("concurrency"),
            Some(&BenchOptionSource::Cli)
        );
        assert_eq!(
            config.option_sources.get("load_schedule"),
            Some(&BenchOptionSource::BenchSection)
        );
        assert_eq!(
            config.option_sources.get("progress_interval"),
            Some(&BenchOptionSource::Default)
        );
    }

    #[test]
    fn test_bench_config_from_bench_section_tracks_sources() {
        let mut bench_section = HashMap::new();
        bench_section.insert("concurrency".to_string(), "7".to_string());
        bench_section.insert("load_schedule".to_string(), "step".to_string());
        bench_section.insert("progress_interval".to_string(), "2s".to_string());

        let config = BenchConfigResolved::from_bench_section(Some(&bench_section)).unwrap();

        assert_eq!(config.concurrency, 7);
        assert_eq!(config.load_schedule, "step");
        assert_eq!(config.progress_interval, Duration::from_secs(2));
        assert_eq!(
            config.option_sources.get("concurrency"),
            Some(&BenchOptionSource::BenchSection)
        );
        assert_eq!(
            config.option_sources.get("load_schedule"),
            Some(&BenchOptionSource::BenchSection)
        );
        assert_eq!(
            config.option_sources.get("progress_interval"),
            Some(&BenchOptionSource::BenchSection)
        );
    }

    #[test]
    fn test_duration_mode_ignores_requests() {
        let args = BenchArgs {
            protocol: "grpc".to_string(),
            test_paths: vec![],
            profile: None,
            mode: None,
            concurrency: Some(2),
            requests: Some(1000),
            duration: Some("10s".to_string()),
            ramp_up: None,
            warmup: None,
            max_duration: None,
            max_rps: None,
            load_schedule: None,
            load_start: None,
            load_step: None,
            load_end: None,
            load_step_duration: None,
            load_max_duration: None,
            connections: Some(1),
            connect_timeout: None,
            keepalive: None,
            cpus: None,
            name: None,
            assert_mode: None,
            no_assert: false,
            sample_rate: None,
            cache: None,
            skip_first: None,
            count_errors_in_latency: None,
            duration_stop: None,
            latency_percentiles: None,
            progress_interval: None,
            format: "console".to_string(),
            output: None,
            compact: false,
            tags: vec![],
            skip_tags: vec![],
            exclude: vec![],
            report_template: None,
            allure_output_dir: None,
            profile_file: None,
            call: None,
            data: None,
            list_profiles: false,
        };

        let config = BenchConfigResolved::from_cli_and_bench(&args, None).unwrap();
        assert_eq!(config.duration, Some(Duration::from_secs(10)));
        assert_eq!(config.requests, None);
    }

    #[test]
    fn test_connections_must_not_exceed_concurrency() {
        let args = BenchArgs {
            protocol: "grpc".to_string(),
            test_paths: vec![],
            profile: None,
            mode: None,
            concurrency: Some(2),
            requests: Some(100),
            duration: None,
            ramp_up: None,
            warmup: None,
            max_duration: None,
            max_rps: None,
            load_schedule: None,
            load_start: None,
            load_step: None,
            load_end: None,
            load_step_duration: None,
            load_max_duration: None,
            connections: Some(3),
            connect_timeout: None,
            keepalive: None,
            cpus: None,
            name: None,
            assert_mode: None,
            no_assert: false,
            sample_rate: None,
            cache: None,
            skip_first: None,
            count_errors_in_latency: None,
            duration_stop: None,
            latency_percentiles: None,
            progress_interval: None,
            format: "console".to_string(),
            output: None,
            compact: false,
            tags: vec![],
            skip_tags: vec![],
            exclude: vec![],
            report_template: None,
            allure_output_dir: None,
            profile_file: None,
            call: None,
            data: None,
            list_profiles: false,
        };

        assert!(BenchConfigResolved::from_cli_and_bench(&args, None).is_err());
    }

    #[test]
    fn test_duration_stop_invalid_value_fails() {
        let args = BenchArgs {
            protocol: "grpc".to_string(),
            test_paths: vec![],
            profile: None,
            mode: None,
            concurrency: Some(2),
            requests: Some(100),
            duration: None,
            ramp_up: None,
            warmup: None,
            max_duration: None,
            max_rps: None,
            load_schedule: None,
            load_start: None,
            load_step: None,
            load_end: None,
            load_step_duration: None,
            load_max_duration: None,
            connections: Some(1),
            connect_timeout: None,
            keepalive: None,
            cpus: None,
            name: None,
            assert_mode: None,
            no_assert: false,
            sample_rate: None,
            cache: None,
            skip_first: None,
            count_errors_in_latency: None,
            duration_stop: Some("bad-mode".to_string()),
            latency_percentiles: None,
            progress_interval: None,
            format: "console".to_string(),
            output: None,
            compact: false,
            tags: vec![],
            skip_tags: vec![],
            exclude: vec![],
            report_template: None,
            allure_output_dir: None,
            profile_file: None,
            call: None,
            data: None,
            list_profiles: false,
        };

        assert!(BenchConfigResolved::from_cli_and_bench(&args, None).is_err());
    }

    #[test]
    fn test_should_record_after_deadline_modes() {
        let start = Instant::now();
        let deadline = start + Duration::from_millis(1);
        let finished_after = deadline + Duration::from_millis(1);

        assert!(!should_record_after_deadline(
            DurationStopMode::Close,
            finished_after,
            deadline
        ));
        assert!(should_record_after_deadline(
            DurationStopMode::Wait,
            finished_after,
            deadline
        ));
        assert!(!should_record_after_deadline(
            DurationStopMode::Ignore,
            finished_after,
            deadline
        ));
    }

    // Relative-error bound of the log-linear histogram: any percentile must be
    // within ~1/SUB_BUCKETS of the true value.
    const HIST_TOLERANCE: f64 = 1.0 / SUB_BUCKETS as f64;

    fn assert_within_tolerance(got: u64, expected: u64, ctx: &str) {
        let rel = (got as f64 - expected as f64).abs() / (expected as f64).max(1.0);
        assert!(
            rel <= HIST_TOLERANCE,
            "{ctx}: got {got}, expected {expected}, rel error {rel:.4} > {HIST_TOLERANCE:.4}"
        );
    }

    // Percentiles are accurate over a wide, dense distribution (1..=100_000).
    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_histogram_percentile_accuracy() {
        let mut h = LatencyHistogram::default();
        for v in 1..=100_000u64 {
            h.record(v);
        }
        assert_within_tolerance(h.percentile(50.0), 50_000, "p50");
        assert_within_tolerance(h.percentile(90.0), 90_000, "p90");
        assert_within_tolerance(h.percentile(99.0), 99_000, "p99");
        assert_within_tolerance(h.percentile(99.9), 99_900, "p99.9");
        // min/max are tracked exactly.
        assert_eq!(h.min, 1);
        assert_eq!(h.max, 100_000);
        assert_eq!(h.percentile(100.0), 100_000);
    }

    // Anti-bias: a distribution whose early samples are small and late samples
    // are large. The old "keep every other sample" downsample-on-merge biased
    // the aggregate toward the last-appended (large) samples; the bounded
    // histogram weights every sample equally, so p50 stays in the low mode.
    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_histogram_not_biased_toward_late_samples() {
        let mut h = LatencyHistogram::default();
        // 90k low samples recorded first, then 10k high samples.
        for _ in 0..90_000 {
            h.record(1_000);
        }
        for _ in 0..10_000 {
            h.record(1_000_000);
        }
        // True p50 is firmly in the low mode; a late-biased estimator would
        // report a value orders of magnitude too high.
        assert_within_tolerance(h.percentile(50.0), 1_000, "p50");
        assert_within_tolerance(h.percentile(85.0), 1_000, "p85");
        // The high mode only shows up above the 90th percentile.
        assert_within_tolerance(h.percentile(99.0), 1_000_000, "p99");
    }

    // Mergeability: two per-worker histograms merged bucket-wise equal one
    // histogram fed all the samples (exactly — no loss on merge).
    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_histogram_merge_is_lossless() {
        let mut whole = LatencyHistogram::default();
        let mut a = LatencyHistogram::default();
        let mut b = LatencyHistogram::default();
        for v in 1..=50_000u64 {
            a.record(v);
            whole.record(v);
        }
        for v in 50_001..=100_000u64 {
            b.record(v);
            whole.record(v);
        }
        a.merge(&b);
        assert_eq!(a.total, whole.total);
        assert_eq!(a.min, whole.min);
        assert_eq!(a.max, whole.max);
        assert_eq!(a.buckets, whole.buckets);
        for p in [50.0, 90.0, 95.0, 99.0, 99.9] {
            assert_eq!(a.percentile(p), whole.percentile(p), "p{p}");
        }
    }

    // Memory is bounded independent of sample count: recording far more than the
    // old 100k cap never grows the fixed bucket array, and every sample counts.
    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_histogram_memory_is_bounded() {
        let mut metrics = BenchMetrics::default();
        let n = MAX_LATENCY_SAMPLES + 10;
        for i in 0..n {
            metrics.record(i as u64, "OK", None, "test");
        }
        assert_eq!(metrics.latency.buckets.len(), HIST_BUCKETS);
        assert_eq!(metrics.latency.total, n as u64);
        assert_eq!(metrics.count, n as u64);
    }

    // mean is computed exactly from the running sum/count, not the histogram.
    #[test]
    fn test_mean_is_exact() {
        let mut m = BenchMetrics::default();
        for v in [10u64, 20, 30, 40, 100] {
            m.record(v, "OK", None, "e");
        }
        // total 200 over 5 requests -> exact 40.
        assert_eq!(m.total_ns / m.count, 40);
    }

    #[test]
    fn test_derive_end_reason_variants() {
        assert_eq!(
            derive_end_reason(true, None, Duration::from_secs(5), false),
            "duration_reached"
        );
        assert_eq!(
            derive_end_reason(false, None, Duration::from_secs(5), true),
            "user_cancelled"
        );
        assert_eq!(
            derive_end_reason(
                false,
                Some(Duration::from_secs(2)),
                Duration::from_secs(3),
                false
            ),
            "max_duration_reached"
        );
        assert_eq!(
            derive_end_reason(
                false,
                Some(Duration::from_secs(5)),
                Duration::from_secs(3),
                false
            ),
            "requests_completed"
        );
    }

    #[test]
    fn test_parse_percentile_key() {
        assert_eq!(parse_percentile_key("p(95)"), Some("95".to_string()));
        assert_eq!(
            parse_percentile_key("latency_ms.p(99.9)"),
            Some("99.9".to_string())
        );
        assert_eq!(
            parse_percentile_key("latency_ns.p(99)"),
            Some("99".to_string())
        );
        assert_eq!(parse_percentile_key("p95"), None);
    }

    #[test]
    fn test_resolve_metric_error_rate_pct() {
        let mut metrics = BenchMetrics::default();
        metrics.record(1_000_000, "OK", None, "test");
        metrics.record(1_000_000, "ERROR", Some("boom"), "test");

        let value = resolve_metric_value(&metrics, "error_rate_pct").unwrap_or_default();
        assert!((value - 50.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_unknown_threshold_metric_fails_with_reason() {
        let mut metrics = BenchMetrics::default();
        metrics.record(1_000_000, "OK", None, "test");

        let mut thresholds = HashMap::new();
        thresholds.insert("unknown_metric".to_string(), "< 10".to_string());

        let results = evaluate_thresholds(&metrics, &thresholds);
        assert_eq!(results.len(), 1);
        assert!(!results[0].passed);
        assert_eq!(results[0].actual, "unknown");
        assert!(
            results[0]
                .reason
                .as_deref()
                .unwrap_or_default()
                .contains("unknown threshold metric")
        );
    }

    #[test]
    fn test_target_rps_step_schedule() {
        let cfg = BenchConfigResolved {
            load_schedule: "step".to_string(),
            load_start: Some(50.0),
            load_step: Some(10.0),
            load_end: Some(150.0),
            load_step_duration: Some(Duration::from_secs(5)),
            ..Default::default()
        };

        assert!((target_rps_at(&cfg, Duration::from_secs(0)) - 50.0).abs() < f64::EPSILON);
        assert!((target_rps_at(&cfg, Duration::from_secs(5)) - 60.0).abs() < f64::EPSILON);
        assert!((target_rps_at(&cfg, Duration::from_secs(50)) - 150.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_target_rps_line_schedule_down() {
        let cfg = BenchConfigResolved {
            load_schedule: "line".to_string(),
            load_start: Some(200.0),
            load_step: Some(-2.0),
            load_end: Some(100.0),
            ..Default::default()
        };

        assert!((target_rps_at(&cfg, Duration::from_secs(0)) - 200.0).abs() < f64::EPSILON);
        assert!((target_rps_at(&cfg, Duration::from_secs(10)) - 180.0).abs() < f64::EPSILON);
        assert!((target_rps_at(&cfg, Duration::from_secs(100)) - 100.0).abs() < f64::EPSILON);
    }

    // skip_first: hold back the first N sampled latencies (warm-up outliers)
    // from the global distribution as they are recorded.
    #[test]
    fn test_skip_first_discards_leading_samples() {
        let mut m = BenchMetrics {
            skip_first_remaining: 2,
            ..Default::default()
        };
        for v in [9999, 9998, 10, 11, 12] {
            m.record(v, "OK", None, "e");
        }
        // The two warm-up outliers never enter the global histogram.
        assert_eq!(m.latency.total, 3);
        assert_eq!(m.latency.min, 10);
        assert_eq!(m.latency.max, 12);
        assert_eq!(m.compute_percentile(100.0), 12);
        // Per-endpoint stats keep every sample (skip gates only the global one).
        assert_eq!(m.per_endpoint["e"].latency.total, 5);
    }

    #[test]
    fn test_skip_first_saturates_without_panic() {
        let mut m = BenchMetrics {
            skip_first_remaining: 10,
            ..Default::default()
        };
        m.record(1, "OK", None, "e");
        m.record(2, "OK", None, "e");
        assert_eq!(m.latency.total, 0);
        // Zero skip is a no-op.
        let mut m2 = BenchMetrics::default();
        for v in [1, 2, 3] {
            m2.record(v, "OK", None, "e");
        }
        assert_eq!(m2.latency.total, 3);
    }

    // count_errors_in_latency=false (default): error latencies are EXCLUDED from
    // the latency distribution, but still counted in throughput and overall timing.
    #[test]
    fn test_count_errors_excluded_from_latency_by_default() {
        let mut m = BenchMetrics::default();
        m.record(100, "OK", None, "e");
        m.record(9999, "ERROR", Some("boom"), "e");
        // Only the OK latency enters the distribution (value 100 -> linear bucket).
        assert_eq!(m.latency.total, 1);
        assert_eq!(m.latency.buckets[100], 1);
        assert_eq!(m.per_endpoint["e"].latency.total, 1);
        // Throughput + overall timing still see the error.
        assert_eq!(m.count, 2);
        assert_eq!(m.errors, 1);
        assert_eq!(m.slowest_ns, 9999);
    }

    // count_errors_in_latency=true: error latencies are INCLUDED in the distribution.
    #[test]
    fn test_count_errors_included_when_flag_set() {
        let mut m = BenchMetrics {
            count_errors_in_latency: true,
            ..Default::default()
        };
        m.record(100, "OK", None, "e");
        m.record(200, "ERROR", Some("boom"), "e");
        // Both latencies (< 256 -> bucket index == value) enter the distribution.
        assert_eq!(m.latency.total, 2);
        assert_eq!(m.latency.buckets[100], 1);
        assert_eq!(m.latency.buckets[200], 1);
        assert_eq!(m.per_endpoint["e"].latency.total, 2);
    }

    // sample_rate: deterministic every-Nth recording (N = round(1/rate)).
    #[test]
    fn test_sample_stride_from_rate() {
        assert_eq!(sample_stride_from_rate(1.0), 1);
        assert_eq!(sample_stride_from_rate(0.5), 2);
        assert_eq!(sample_stride_from_rate(0.25), 4);
        assert_eq!(sample_stride_from_rate(0.0), u64::MAX);
        assert_eq!(sample_stride_from_rate(2.0), 1); // clamped to record-all
    }

    #[test]
    fn test_sample_rate_records_every_nth() {
        let mut m = BenchMetrics {
            sample_stride: sample_stride_from_rate(0.5),
            ..Default::default()
        };
        for i in 0..6 {
            m.record(i, "OK", None, "e");
        }
        // stride 2 -> records requests 1,3,5 (i = 0,2,4); all 6 still counted.
        assert_eq!(m.latency.total, 3);
        assert_eq!(m.latency.buckets[0], 1);
        assert_eq!(m.latency.buckets[2], 1);
        assert_eq!(m.latency.buckets[4], 1);
        assert_eq!(m.count, 6);
    }

    #[test]
    fn test_sample_rate_full_records_all() {
        let mut m = BenchMetrics {
            sample_stride: sample_stride_from_rate(1.0),
            ..Default::default()
        };
        for i in 0..4 {
            m.record(i, "OK", None, "e");
        }
        assert_eq!(m.latency.total, 4);
    }

    // ramp_up: linearly scale the target load from ~0 to the steady-state target
    // over the first `ramp_up` seconds.
    #[test]
    fn test_ramp_up_scales_target_rps() {
        let cfg = BenchConfigResolved {
            load_schedule: "const".to_string(),
            max_rps: Some(100.0),
            ramp_up: Some(Duration::from_secs(10)),
            ..Default::default()
        };
        assert!(target_rps_at(&cfg, Duration::from_secs(0)) < 1.0);
        assert!((target_rps_at(&cfg, Duration::from_secs(5)) - 50.0).abs() < 1e-6);
        // At/after the ramp end the full target applies.
        assert!((target_rps_at(&cfg, Duration::from_secs(10)) - 100.0).abs() < 1e-6);
        assert!((target_rps_at(&cfg, Duration::from_secs(20)) - 100.0).abs() < 1e-6);
    }

    // count_errors_in_latency / skip_first are also settable via the BENCH section.
    #[test]
    fn test_bench_section_parses_skip_first_and_count_errors() {
        let mut bench_section = HashMap::new();
        bench_section.insert("skip_first".to_string(), "7".to_string());
        bench_section.insert("count_errors_in_latency".to_string(), "true".to_string());
        let config = BenchConfigResolved::from_bench_section(Some(&bench_section)).unwrap();
        assert_eq!(config.skip_first, 7);
        assert!(config.count_errors_in_latency);
    }

    // Feature 1: each closed-loop worker maps to `worker % connections`, so
    // `connections` workers cycle over exactly `connections` distinct channels.
    #[test]
    fn test_worker_connection_id_assignment() {
        // 4 workers, pool of 2 -> ids 0,1,0,1.
        let ids: Vec<u64> = (0..4).map(|w| worker_connection_id(w, 2)).collect();
        assert_eq!(ids, vec![0, 1, 0, 1]);

        // connections == concurrency -> every worker gets a distinct channel.
        let ids: Vec<u64> = (0..4).map(|w| worker_connection_id(w, 4)).collect();
        assert_eq!(ids, vec![0, 1, 2, 3]);
        let distinct: std::collections::BTreeSet<u64> = ids.iter().copied().collect();
        assert_eq!(distinct.len(), 4, "N connections -> N distinct channel ids");

        // Degenerate pool size is clamped to 1 (single shared channel).
        assert_eq!(worker_connection_id(3, 0), 0);
    }

    // Feature 1: the open-model round-robin sends task k to runners[k % N].
    #[test]
    fn test_round_robin_index_picks_k_mod_n() {
        let picks: Vec<usize> = (0..7).map(|k| round_robin_index(k, 3)).collect();
        assert_eq!(picks, vec![0, 1, 2, 0, 1, 2, 0]);
        assert_eq!(round_robin_index(5, 0), 0);
    }

    // Feature 2: the real numeric gRPC code maps to its canonical status bucket;
    // OK and errored codes land in distinct buckets.
    #[test]
    fn test_grpc_status_label_mapping() {
        assert_eq!(grpc_status_label(Some(0), true), "OK");
        assert_eq!(grpc_status_label(Some(14), false), "Unavailable");
        assert_eq!(grpc_status_label(Some(5), true), "NotFound");
        // No gRPC status observed -> fall back to the pass/fail outcome.
        assert_eq!(grpc_status_label(None, true), "OK");
        assert_eq!(grpc_status_label(None, false), "ERROR");
    }

    // Feature 2: recording real status labels buckets them separately and keeps
    // OK vs non-OK accounting correct.
    #[test]
    fn test_record_buckets_by_real_status() {
        let mut m = BenchMetrics::with_capacity(4);
        m.record(10, "OK", None, "svc/M");
        m.record(20, "OK", None, "svc/M");
        m.record(30, "Unavailable", Some("boom"), "svc/M");

        assert_eq!(m.grpc_status.get("OK"), Some(&2));
        assert_eq!(m.grpc_status.get("Unavailable"), Some(&1));
        assert_eq!(m.ok, 2);
        assert_eq!(m.errors, 1);
    }
}
