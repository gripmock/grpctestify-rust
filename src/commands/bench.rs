#![allow(clippy::unwrap_used, clippy::expect_used)] // audited safe

use crate::bench::schema::bench_value;
use crate::cli::args::BenchArgs;
use crate::parser::ast::{GctfDocument, SectionContent, SectionType};
use crate::report::bench::{
    BENCH_REPORT_SCHEMA_VERSION, BenchHistogramBucket, BenchPercentile, BenchReport, BenchRunInfo,
    BenchThresholdResult,
};
use crate::utils::FileUtils;
use anyhow::{Context, Result};
use std::borrow::Cow;
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

/// Parse a BENCH numeric value tolerating digit separators (`1_000_000`),
/// falling back to `default` on a non-numeric value. Gives BENCH numeric keys
/// the same digit-separator support JSON/YAML/ASSERTS numbers already have
/// (gctf-parser-hardening §4.3) and that the `load_*` f64 keys already do via
/// their own `.replace('_', "")`; the `unwrap_or(default)` fallback semantics
/// are unchanged for genuinely-invalid values.
fn parse_bench_num<T: std::str::FromStr>(value: &str, default: T) -> T {
    value.replace('_', "").parse().unwrap_or(default)
}

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
    /// Second schedule axis: how the worker count changes across the run.
    /// `const` measures one level; `step`/`line` measure a series of levels,
    /// each in full, so a sweep no longer needs an external loop.
    pub concurrency_schedule: String,
    pub concurrency_start: Option<u32>,
    pub concurrency_end: Option<u32>,
    pub concurrency_step: Option<u32>,
    pub concurrency_step_duration: Option<Duration>,
    pub load_midpoint: Option<f64>,
    pub load_amplitude: Option<f64>,
    pub load_frequency: Option<f64>,
    pub load_spike_target: Option<f64>,
    pub load_spike_after: Option<f64>,
    pub load_spike_duration: Option<f64>,
    pub load_profile: Option<Vec<(f64, f64)>>,
    pub connections: u32,
    pub connect_timeout: Duration,
    pub request_timeout: Option<Duration>,
    pub collect_details: bool,
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
    /// Set by `--calibrate`: every request goes here instead of the document's
    /// address, and assertions are off because the target answers with defaults.
    pub calibration_address: Option<String>,
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
            concurrency_schedule: "const".to_string(),
            concurrency_start: None,
            concurrency_end: None,
            concurrency_step: None,
            concurrency_step_duration: None,
            load_midpoint: None,
            load_amplitude: None,
            load_frequency: None,
            load_spike_target: None,
            load_spike_after: None,
            load_spike_duration: None,
            load_profile: None,
            connections: 1,
            connect_timeout: Duration::from_secs(10),
            request_timeout: None,
            collect_details: false,
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
            calibration_address: None,
            option_sources: {
                let mut s = HashMap::new();
                for key in [
                    "concurrency",
                    "connections",
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
        rest.replace('_', "")
            .parse::<f64>()
            .ok()
            .map(|v| v * 3600.0)
    } else if let Some(rest) = s.strip_suffix("ms") {
        rest.replace('_', "")
            .parse::<f64>()
            .ok()
            .map(|v| v / 1000.0)
    } else if let Some(rest) = s.strip_suffix('s') {
        rest.replace('_', "").parse::<f64>().ok()
    } else if let Some(rest) = s.strip_suffix('m') {
        rest.replace('_', "").parse::<f64>().ok().map(|v| v * 60.0)
    } else {
        s.replace('_', "").parse::<f64>().ok()
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
    /// Apply every `BENCH` section key onto `self`.
    ///
    /// Both constructors need this and used to carry a line-for-line copy of
    /// it, so every new key had to be added twice.
    fn apply_bench_section(&mut self, bench: &crate::parser::OrderedStringMap) -> Result<()> {
        // Record the source of every key the section actually writes. Only
        // eight keys used to be tracked, so `apply_profile_defaults` saw
        // `duration`/`mode`/`requests` as unset and overwrote values the user
        // had written explicitly.
        for key in bench.keys() {
            self.option_sources
                .insert(key.clone(), BenchOptionSource::BenchSection);
        }

        if let Some(mode) = bench.get("mode") {
            self.mode = mode.clone();
        }
        if let Some(p) = bench.get("profile") {
            self.profile = p.clone();
        }
        if let Some(c) = bench.get("concurrency") {
            self.concurrency = parse_bench_num(c, 1);
            self.option_sources
                .insert("concurrency".to_string(), BenchOptionSource::BenchSection);
        }
        if let Some(n) = bench.get("requests") {
            self.requests = Some(parse_bench_num(n, 100));
        }
        if let Some(d) = bench.get("duration") {
            self.duration = Some(parse_duration(d)?);
        }
        if let Some(d) = bench_value(bench, "ramp_up") {
            self.ramp_up = Some(parse_duration(d)?);
        }
        if let Some(d) = bench.get("warmup") {
            self.warmup = Some(parse_duration(d)?);
        }
        if let Some(v) = bench.get("warmup_mode") {
            self.warmup_mode = v.clone();
        }
        if let Some(d) = bench.get("cool_down") {
            self.cool_down = Some(parse_duration(d)?);
        }
        if let Some(d) = bench_value(bench, "max_duration") {
            self.max_duration = Some(parse_duration(d)?);
        }
        if let Some(rps) = bench_value(bench, "max_rps") {
            self.max_rps = Some(parse_bench_num(rps, 0.0));
        }
        if let Some(v) = bench_value(bench, "load_schedule") {
            self.load_schedule = v.clone();
            self.option_sources
                .insert("load_schedule".to_string(), BenchOptionSource::BenchSection);
        }
        if let Some(v) = bench.get("load_profile") {
            self.load_profile = parse_custom_profile(v);
        }
        if let Some(v) = bench.get("load_midpoint") {
            self.load_midpoint = v.replace('_', "").parse::<f64>().ok();
        }
        if let Some(v) = bench.get("load_amplitude") {
            self.load_amplitude = v.replace('_', "").parse::<f64>().ok();
        }
        if let Some(v) = bench.get("load_frequency") {
            self.load_frequency = v.replace('_', "").parse::<f64>().ok();
        }
        if let Some(v) = bench.get("load_spike_target") {
            self.load_spike_target = v.replace('_', "").parse::<f64>().ok();
        }
        if let Some(v) = bench.get("load_spike_after") {
            self.load_spike_after = v.replace('_', "").parse::<f64>().ok();
        }
        if let Some(v) = bench.get("load_spike_duration") {
            self.load_spike_duration = v.replace('_', "").parse::<f64>().ok();
        }
        if let Some(v) = bench_value(bench, "load_start") {
            self.load_start = v.replace('_', "").parse::<f64>().ok();
            self.option_sources
                .insert("load_start".to_string(), BenchOptionSource::BenchSection);
        }
        if let Some(v) = bench_value(bench, "load_step") {
            self.load_step = v.replace('_', "").parse::<f64>().ok();
            self.option_sources
                .insert("load_step".to_string(), BenchOptionSource::BenchSection);
        }
        if let Some(v) = bench_value(bench, "load_end") {
            self.load_end = v.replace('_', "").parse::<f64>().ok();
            self.option_sources
                .insert("load_end".to_string(), BenchOptionSource::BenchSection);
        }
        if let Some(v) = bench_value(bench, "load_step_duration") {
            self.load_step_duration = Some(parse_duration(v)?);
            self.option_sources.insert(
                "load_step_duration".to_string(),
                BenchOptionSource::BenchSection,
            );
        }
        if let Some(v) = bench_value(bench, "concurrency_schedule") {
            self.concurrency_schedule = v.trim().to_ascii_lowercase();
        }
        if let Some(v) = bench_value(bench, "concurrency_start") {
            self.concurrency_start = Some(parse_bench_num(v, 1));
        }
        if let Some(v) = bench_value(bench, "concurrency_end") {
            self.concurrency_end = Some(parse_bench_num(v, 1));
        }
        if let Some(v) = bench_value(bench, "concurrency_step") {
            self.concurrency_step = Some(parse_bench_num(v, 1));
        }
        if let Some(v) = bench_value(bench, "concurrency_step_duration") {
            self.concurrency_step_duration = Some(parse_duration(v)?);
        }
        if let Some(v) = bench_value(bench, "load_max_duration") {
            self.load_max_duration = Some(parse_duration(v)?);
            self.option_sources.insert(
                "load_max_duration".to_string(),
                BenchOptionSource::BenchSection,
            );
        }
        if let Some(v) = bench.get("connections") {
            self.connections = parse_bench_num(v, 1);
            self.option_sources
                .insert("connections".to_string(), BenchOptionSource::BenchSection);
        }
        if let Some(v) = bench_value(bench, "request_timeout") {
            self.request_timeout = Some(parse_duration(v)?);
        }
        if let Some(v) = bench_value(bench, "connect_timeout") {
            self.connect_timeout = parse_duration(v)?;
        }
        if let Some(v) = bench.get("keepalive") {
            self.keepalive = Some(parse_duration(v)?);
        }
        if let Some(v) = bench.get("cpus") {
            self.cpus = Some(parse_bench_num(v, 1));
        }
        if let Some(v) = bench.get("name") {
            self.name = Some(v.clone());
        }
        if let Some(am) = bench_value(bench, "assert_mode") {
            self.assert_mode = am.clone();
        }
        if let Some(v) = bench_value(bench, "no_assert") {
            self.no_assert = v == "true" || v == "1";
        }
        if let Some(v) = bench_value(bench, "duration_stop") {
            self.duration_stop = DurationStopMode::parse(v)?;
        }
        if let Some(sr) = bench_value(bench, "sample_rate") {
            self.sample_rate = parse_sample_rate(sr)?;
        }
        if let Some(v) = bench_value(bench, "skip_first") {
            self.skip_first = parse_bench_num(v, 0);
        }
        if let Some(v) = bench_value(bench, "count_errors_in_latency") {
            self.count_errors_in_latency = v == "true" || v == "1";
        }
        if let Some(v) = bench_value(bench, "latency_percentiles") {
            self.latency_percentiles = parse_latency_percentiles(v);
        }
        if let Some(v) = bench_value(bench, "progress_interval") {
            self.progress_interval = parse_duration(v)?;
            self.option_sources.insert(
                "progress_interval".to_string(),
                BenchOptionSource::BenchSection,
            );
        }
        if let Some(cache) = bench.get("cache") {
            self.cache = cache == "true" || cache == "1";
        }
        if let Some(ttl) = bench.get("cache_ttl") {
            self.cache_ttl = Some(parse_duration(ttl)?);
        }

        for (key, value) in bench {
            if let Some(metric) = key.strip_prefix("thresholds.") {
                self.thresholds.insert(metric.to_string(), value.clone());
            }
        }

        if let Some(sources_yaml) = bench.get("sources") {
            self.sources = serde_yaml_ng::from_str::<Vec<crate::bench::sources::SourceDefinition>>(
                sources_yaml,
            )
            .map_err(|e| {
                anyhow::anyhow!(
                    "BENCH.sources is not a valid list of source definitions: {e}. \
                     A malformed block used to be ignored, leaving every \
                     `{{{{source.column}}}}` placeholder unsubstituted."
                )
            })?;
        }
        Ok(())
    }

    pub fn from_bench_section(
        bench_section: Option<&crate::parser::OrderedStringMap>,
    ) -> Result<Self> {
        let mut config = Self::default();
        if let Some(bench) = bench_section {
            config.apply_bench_section(bench)?;
        }
        if config.option_sources.get("connections") == Some(&BenchOptionSource::Default) {
            config.connections = default_connections(config.concurrency);
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
        bench_section: Option<&crate::parser::OrderedStringMap>,
    ) -> Result<Self> {
        let defaults = Self::default();
        let mut config = defaults;

        if let Some(bench) = bench_section {
            config.apply_bench_section(bench)?;
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
        // `duration_source` rather than `duration`: a profile preset must not
        // overwrite a duration the user passed on the command line, and it can
        // only tell by the recorded source.
        cli_config_field!(duration_source, config, cli, duration, "duration");
        cli_config_field!(duration_source, config, cli, ramp_up, "ramp_up");
        cli_config_field!(duration_source, config, cli, warmup, "warmup");
        cli_config_field!(duration_source, config, cli, max_duration, "max_duration");
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
        if let Some(v) = &cli.concurrency_schedule {
            config.concurrency_schedule = v.trim().to_ascii_lowercase();
            config
                .option_sources
                .insert("concurrency_schedule".to_string(), BenchOptionSource::Cli);
        }
        cli_config_field!(
            option_direct,
            config,
            cli,
            concurrency_start,
            "concurrency_start"
        );
        cli_config_field!(
            option_direct,
            config,
            cli,
            concurrency_end,
            "concurrency_end"
        );
        cli_config_field!(
            option_direct,
            config,
            cli,
            concurrency_step,
            "concurrency_step"
        );
        cli_config_field!(
            duration_source,
            config,
            cli,
            concurrency_step_duration,
            "concurrency_step_duration"
        );
        cli_config_field!(direct, config, cli, connections, "connections");
        if let Some(v) = &cli.request_timeout {
            config.request_timeout = Some(parse_duration(v)?);
        }

        // Only `detail-json` reads `details`; filling it elsewhere is pure hot-path cost.
        config.collect_details = canonical_bench_format(&cli.format) == "detail-json";
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
            if !sr.is_finite() || !(0.0..=1.0).contains(&sr) {
                anyhow::bail!("--sample-rate must be between 0 and 1, got {sr}");
            }
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

        // Profile presets fill in what neither the CLI nor the BENCH section
        // set, so they must be applied *after* the CLI overlay: `config.profile`
        // only carries a `--profile` value from this point on, and every key
        // either layer set is already marked non-`Default` in `option_sources`.
        // Applying it earlier meant `--profile stress` printed the profile name
        // and ran the defaults.
        let profile_name = config.profile.clone();
        apply_profile_defaults(&mut config, &profile_name);

        if config.option_sources.get("connections") == Some(&BenchOptionSource::Default) {
            config.connections = default_connections(config.concurrency);
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
    if !num.is_finite() || num < 0.0 {
        anyhow::bail!("duration must not be negative: {}", s);
    }

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

const MAX_DEFAULT_CONNECTIONS: u32 = 8;

/// Every stream on a connection contends for h2's per-connection mutex, so the
/// pool grows with the worker count; measured to flatten past eight.
fn default_connections(concurrency: u32) -> u32 {
    let parallelism = std::thread::available_parallelism()
        .map(std::num::NonZeroUsize::get)
        .unwrap_or(1);
    let parallelism = u32::try_from(parallelism).unwrap_or(MAX_DEFAULT_CONNECTIONS);
    concurrency
        .max(1)
        .min(parallelism)
        .clamp(1, MAX_DEFAULT_CONNECTIONS)
}

/// `None` where the platform will not account for our CPU, rather than a zero
/// that reads like "free".
struct ClientCpuSampler {
    system: sysinfo::System,
    pid: sysinfo::Pid,
    start_ms: u64,
}

impl ClientCpuSampler {
    fn start() -> Option<Self> {
        let pid = sysinfo::get_current_pid().ok()?;
        let system = sysinfo::System::new();
        let mut sampler = Self {
            system,
            pid,
            start_ms: 0,
        };
        sampler.start_ms = sampler.read_ms()?;
        Some(sampler)
    }

    fn read_ms(&mut self) -> Option<u64> {
        self.system.refresh_processes_specifics(
            sysinfo::ProcessesToUpdate::Some(&[self.pid]),
            true,
            sysinfo::ProcessRefreshKind::nothing().with_cpu(),
        );
        Some(self.system.process(self.pid)?.accumulated_cpu_time())
    }

    /// CPU seconds burned since `start`.
    fn elapsed_cpu_seconds(&mut self) -> Option<f64> {
        let now = self.read_ms()?;
        Some(now.saturating_sub(self.start_ms) as f64 / 1000.0)
    }
}

/// Above this share of the host's cores, the throughput a run reports is more
/// likely the generator's ceiling than the target's. k6 prescribes leaving at
/// least 20 % of the CPU idle for exactly this reason.
const GENERATOR_BUSY_FRACTION: f64 = 0.8;

/// Fold the client's own cost into a reportable block. Pure, so the thresholds
/// are testable without running a benchmark.
fn client_cost(
    cpu_seconds: f64,
    wall_seconds: f64,
    requests: u64,
    rps: f64,
    host_cores: usize,
) -> crate::report::bench::ClientCost {
    let host_cores = host_cores.max(1);
    let cores_used = if wall_seconds > 0.0 {
        cpu_seconds / wall_seconds
    } else {
        0.0
    };
    let mut limits = Vec::new();
    if cores_used >= host_cores as f64 * GENERATOR_BUSY_FRACTION {
        limits.push(format!(
            "client used {:.2} of {} cores — the measured throughput is likely this generator's ceiling, not the target's",
            cores_used, host_cores
        ));
    }
    crate::report::bench::ClientCost {
        cpu_seconds,
        cpu_us_per_request: if requests > 0 {
            cpu_seconds * 1e6 / requests as f64
        } else {
            0.0
        },
        rps_per_core: if cores_used > 0.0 {
            rps / cores_used
        } else {
            0.0
        },
        cores_used,
        host_cores,
        generator_limited: !limits.is_empty(),
        limits,
    }
}

/// The concurrency levels a run measures, in execution order. `step` and `line`
/// differ only in their default step, mirroring ghz: `step` moves in whole
/// blocks, `line` walks one worker at a time.
fn concurrency_levels(config: &BenchConfigResolved) -> Result<Vec<u32>> {
    let schedule = config.concurrency_schedule.trim();
    if schedule.is_empty() || schedule == "const" {
        return Ok(vec![config.concurrency]);
    }
    if schedule != "step" && schedule != "line" {
        anyhow::bail!(
            "unknown concurrency_schedule '{schedule}'; expected one of: const, step, line"
        );
    }

    let start = config.concurrency_start.unwrap_or(config.concurrency);
    let end = config.concurrency_end.unwrap_or(config.concurrency);
    if start == 0 || end == 0 {
        anyhow::bail!("concurrency levels must be greater than 0");
    }

    let default_step = if schedule == "line" { 1 } else { 0 };
    let step = config.concurrency_step.unwrap_or(default_step);
    let step = if step == 0 {
        end.abs_diff(start).max(1)
    } else {
        step
    };

    let mut levels = Vec::new();
    let mut current = start;
    loop {
        levels.push(current);
        if current == end {
            break;
        }
        let next = if start <= end {
            current.saturating_add(step).min(end)
        } else {
            current.saturating_sub(step).max(end)
        };
        if next == current {
            break;
        }
        current = next;
        if levels.len() > MAX_CONCURRENCY_LEVELS {
            anyhow::bail!(
                "concurrency schedule produces more than {MAX_CONCURRENCY_LEVELS} levels; \
                 raise concurrency_step"
            );
        }
    }
    Ok(levels)
}

/// Guards a typo (`concurrency_step: 1` from 1 to 100000) from becoming a run
/// that never ends.
const MAX_CONCURRENCY_LEVELS: usize = 64;

/// The single `BENCH` section governing a run. Two files configuring the same
/// run differently is an error: silently adopting the first-sorted one produced
/// benchmarks that ignored the configuration the user wrote.
fn resolve_bench_section(
    test_paths: &[std::path::PathBuf],
    exclude: &[String],
) -> Result<Option<crate::parser::OrderedStringMap>> {
    let mut files = Vec::new();
    for path in test_paths {
        if path.is_dir() {
            files.extend(FileUtils::collect_test_files(path, exclude));
        } else if path.is_file() {
            files.push(path.clone());
        }
    }

    let mut found: Vec<(std::path::PathBuf, crate::parser::OrderedStringMap)> = Vec::new();
    for file in files {
        let parsed = crate::parser::parse_with_recovery(&file);
        if let Some(section) = extract_bench_section(&parsed.document) {
            found.push((file, section));
        }
    }

    let Some((first_file, first)) = found.first().cloned() else {
        return Ok(None);
    };
    if let Some((other_file, _)) = found.iter().skip(1).find(|(_, s)| *s != first) {
        anyhow::bail!(
            "conflicting BENCH sections: {} and {} configure the same run differently. \
             A run has one benchmark configuration — keep one BENCH section, or bench \
             the files separately.",
            first_file.display(),
            other_file.display()
        );
    }
    Ok(Some(first))
}

/// Extract BENCH section content from document
fn extract_bench_section(doc: &GctfDocument) -> Option<crate::parser::OrderedStringMap> {
    for section in &doc.sections {
        if section.section_type == SectionType::Bench
            && let SectionContent::KeyValues(kv) = &section.content
        {
            return Some(kv.clone());
        }
    }
    None
}

/// Apply profile defaults to config for keys the CLI and BENCH section left unset.
fn apply_profile_defaults(config: &mut BenchConfigResolved, profile_name: &str) {
    let explicit = |config: &BenchConfigResolved, key: &str| {
        config
            .option_sources
            .get(key)
            .is_some_and(|s| *s != BenchOptionSource::Default)
    };
    // `requests` and `duration` are mutually exclusive stop conditions, and a
    // `duration` wins over a `requests` downstream. A preset must therefore not
    // inject one when the user already chose the other, or `--profile load -n
    // 1000` would silently discard the request budget.
    let chose_requests = explicit(config, "requests");
    let chose_duration = explicit(config, "duration");

    for (key, value) in crate::bench::schema::apply_profile_dynamic(profile_name) {
        if (key == "duration" && chose_requests && !chose_duration)
            || (key == "requests" && chose_duration && !chose_requests)
        {
            continue;
        }
        // Only apply if not explicitly set via BENCH section or CLI
        let is_explicit = explicit(config, &key);
        if is_explicit {
            continue;
        }
        match key.as_str() {
            "mode" => config.mode = value,
            "concurrency" => {
                if let Ok(v) = value.replace('_', "").parse::<u32>() {
                    config.concurrency = v;
                }
            }
            "requests" => {
                if let Ok(v) = value.replace('_', "").parse::<u64>() {
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
                if let Ok(v) = value.replace('_', "").parse::<f64>() {
                    config.load_start = Some(v);
                }
            }
            "load_step" => {
                if let Ok(v) = value.replace('_', "").parse::<f64>() {
                    config.load_step = Some(v);
                }
            }
            "load_end" => {
                if let Ok(v) = value.replace('_', "").parse::<f64>() {
                    config.load_end = Some(v);
                }
            }
            "load_step_duration" => {
                if let Ok(d) = parse_duration(&value) {
                    config.load_step_duration = Some(d);
                }
            }
            "load_spike_target" => {
                if let Ok(v) = value.replace('_', "").parse::<f64>() {
                    config.load_spike_target = Some(v);
                }
            }
            "load_spike_after" => {
                if let Ok(v) = value.replace('_', "").parse::<f64>() {
                    config.load_spike_after = Some(v);
                }
            }
            "load_spike_duration" => {
                if let Ok(v) = value.replace('_', "").parse::<f64>() {
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
#[derive(Default, Debug, Clone)]
struct BenchMetrics {
    count: u64,
    ok: u64,
    errors: u64,
    /// Requests whose document verdict was a pass, and those whose was not.
    /// Distinct from `ok`/`errors`, which follow the gRPC status.
    passed: u64,
    failed: u64,
    total_ns: u64,
    fastest_ns: u64,
    slowest_ns: u64,
    grpc_status: BTreeMap<String, u64>,
    error_dist: BTreeMap<String, u64>,
    latency: LatencyHistogram,
    per_endpoint: BTreeMap<String, PerEndpointData>,
    details: Vec<crate::report::bench::BenchDetail>,
    collect_details: bool,
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
/// `sample_rate` must be a finite number in `[0, 1]`. It used to be
/// `parse().unwrap_or(1.0)`, so a typo silently meant "sample everything".
fn parse_sample_rate(raw: &str) -> Result<f64> {
    let v: f64 = raw
        .trim()
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid sample_rate '{raw}': expected a number in [0, 1]"))?;
    if !v.is_finite() || !(0.0..=1.0).contains(&v) {
        anyhow::bail!("sample_rate must be between 0 and 1, got '{raw}'");
    }
    Ok(v)
}

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
        collect_details: bool,
    ) -> Self {
        let mut m = Self::with_capacity(hint);
        m.count_errors_in_latency = count_errors_in_latency;
        m.sample_stride = sample_stride;
        m.skip_first_remaining = skip_first;
        m.collect_details = collect_details;
        m
    }
}

#[derive(Default, Debug, Clone)]
struct PerEndpointData {
    count: u64,
    errors: u64,
    latency: LatencyHistogram,
}

impl BenchMetrics {
    /// `passed` is the document's own verdict (did its RESPONSE/ERROR/ASSERTS
    /// hold?), which is not the same thing as the transport status: a document
    /// asserting `--- ERROR partial --- {}` passes while returning `NotFound`.
    /// `ok`/`errors` stay transport-level for report compatibility;
    /// `passed`/`failed` carry the verdict, and latency follows the verdict so
    /// a negative-path benchmark still produces percentiles.
    fn record(
        &mut self,
        latency_ns: u64,
        status: &str,
        error: Option<&str>,
        endpoint: &str,
        passed: bool,
    ) {
        self.count += 1;
        let is_ok = status == "OK" || status.is_empty();
        if is_ok {
            self.ok += 1;
        } else {
            self.errors += 1;
        }
        if passed {
            self.passed += 1;
        } else {
            self.failed += 1;
        }

        // Borrowed lookup: `entry()` needs an owned key, so it allocated per request.
        let status_key = if status.is_empty() { "OK" } else { status };
        bump(&mut self.grpc_status, status_key);

        if let Some(err) = error {
            bump(&mut self.error_dist, categorize_error(err));
        }

        // Decide whether this request contributes a *latency sample* (percentiles
        // and histogram). Governed by `sample_rate` (deterministic every-Nth
        // sampling) and `count_errors_in_latency` (exclude error responses when
        // false). Throughput and overall-timing counters below are unaffected.
        self.sample_counter += 1;
        let sampled = if self.sample_stride == u64::MAX {
            // `sample_rate: 0` means record nothing. Without this the modulo
            // below is true for the very first request, and that lone sample
            // became min == max == p50 == p99 for the whole run.
            false
        } else {
            self.sample_stride <= 1 || (self.sample_counter % self.sample_stride == 1)
        };
        let contributes = sampled && (passed || self.count_errors_in_latency);

        // Per-endpoint tracking
        let ep = match self.per_endpoint.get_mut(endpoint) {
            Some(ep) => ep,
            None => self.per_endpoint.entry(endpoint.to_string()).or_default(),
        };
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
        if self.collect_details && self.details.len() < MAX_LATENCY_SAMPLES {
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
                && let Ok(pct) = stripped.trim_ascii().replace('_', "").parse::<f64>()
            {
                result.push(BenchPercentile {
                    percentile: pct,
                    latency_ns: self.latency.percentile(pct),
                });
            }
        }
        // total_cmp, not partial_cmp().unwrap(): a config percentile can be NaN.
        result.sort_by(|a, b| a.percentile.total_cmp(&b.percentile));
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
        self.passed += other.passed;
        self.failed += other.failed;
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

/// Increment a counter keyed by a borrowed label, allocating the key only when
/// the label has not been seen before.
fn bump(counters: &mut BTreeMap<String, u64>, key: &str) {
    if let Some(counter) = counters.get_mut(key) {
        *counter += 1;
    } else {
        counters.insert(key.to_string(), 1);
    }
}

/// Case-insensitive substring test without the `to_lowercase()` allocation.
fn contains_fold(haystack: &str, needle: &str) -> bool {
    let (h, n) = (haystack.as_bytes(), needle.as_bytes());

    !n.is_empty() && n.len() <= h.len() && h.windows(n.len()).any(|w| w.eq_ignore_ascii_case(n))
}

fn categorize_error(message: &str) -> &'static str {
    if contains_fold(message, "assert") {
        "assert_failure"
    } else if contains_fold(message, "timeout") || contains_fold(message, "deadline") {
        "timeout"
    } else if contains_fold(message, "connection")
        || contains_fold(message, "refused")
        || contains_fold(message, "reset")
    {
        "connection_error"
    } else if contains_fold(message, "unavailable") {
        "unavailable"
    } else if contains_fold(message, "invalid") || contains_fold(message, "malformed") {
        "invalid_input"
    } else {
        "other"
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
/// - `mode`: only the closed-loop execution model is implemented. Any other
///   mode (e.g. `open`/`adaptive`) is accepted but ignored.
fn warn_ineffective_options(config: &BenchConfigResolved) {
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
/// Measure every concurrency level the schedule describes, folded into one
/// report. The merged top-level metrics come from merging the per-level
/// histograms, so aggregate percentiles are exact rather than averaged.
async fn run_benchmark(
    test_paths: &[std::path::PathBuf],
    config: &BenchConfigResolved,
    exclude: &[String],
    tags: &[String],
    skip_tags: &[String],
) -> Result<BenchReport> {
    let levels = concurrency_levels(config)?;
    if levels.len() == 1 {
        let (report, _) = run_benchmark_level(test_paths, config, exclude, tags, skip_tags).await?;
        return Ok(report);
    }

    eprintln!(
        "Concurrency sweep ({}): {} level(s) — {}",
        config.concurrency_schedule,
        levels.len(),
        levels
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    );

    let start_ts = crate::polyfill::runtime::now_timestamp();
    let mut merged: Option<BenchMetrics> = None;
    let mut total_elapsed = Duration::ZERO;
    let mut level_summaries = Vec::new();
    let mut last_report: Option<BenchReport> = None;

    for level in levels {
        let mut level_config = config.clone();
        level_config.concurrency = level;
        // `connections` may not exceed `concurrency`, and a sweep walks below it.
        level_config.connections = level_config.connections.min(level).max(1);
        if let Some(per_level) = config.concurrency_step_duration {
            level_config.duration = Some(per_level);
            level_config.requests = None;
        }

        eprintln!("[bench] concurrency level {level}");
        let (report, metrics) =
            run_benchmark_level(test_paths, &level_config, exclude, tags, skip_tags).await?;

        total_elapsed += Duration::from_nanos(report.summary.total_ns / u64::from(level).max(1));
        level_summaries.push(crate::report::bench::BenchLevelSummary {
            concurrency: level,
            summary: report.summary.clone(),
            latency_distribution: report.latency_distribution.clone(),
            grpc_status_distribution: report.grpc_status_distribution.clone(),
        });
        merged = Some(match merged {
            Some(mut acc) => {
                acc.merge_from(metrics);
                acc
            }
            None => metrics,
        });
        last_report = Some(report);
    }

    let merged = merged.unwrap_or_default();
    let last = last_report.expect("a sweep always runs at least one level");
    let mut report = build_report(
        RunWindow {
            start_ts,
            end_ts: crate::polyfill::runtime::now_timestamp(),
            end_reason: &last.run.end_reason,
            elapsed: total_elapsed,
            // Each level already accounted for its own client cost; summing them
            // here would double-count the sampler's own overlap.
            client_cpu_seconds: None,
        },
        config,
        merged,
        None,
    )?;
    report.levels = level_summaries;
    Ok(report)
}

async fn run_benchmark_level(
    test_paths: &[std::path::PathBuf],
    config: &BenchConfigResolved,
    exclude: &[String],
    tags: &[String],
    skip_tags: &[String],
) -> Result<(BenchReport, BenchMetrics)> {
    let start_ts = crate::polyfill::runtime::now_timestamp();
    // Started before any setup so warm-up and index building count against the
    // client too — they are real cost even though they are not per-request.
    let mut client_cpu = ClientCpuSampler::start();

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

    // Pre-parse all test files for performance (avoid re-parsing on every
    // iteration). Behind `Arc` so spawning a worker clones pointers, not the
    // whole AST once per worker.
    let mut test_docs: Vec<(std::path::PathBuf, Arc<crate::parser::GctfDocument>)> = test_files
        .iter()
        .map(|f| {
            let result = crate::parser::parse_with_recovery(f);
            (f.clone(), Arc::new(result.document))
        })
        .collect();

    // `--tags`/`--skip-tags` were accepted and then never read, so `bench`
    // silently ran everything the paths matched.
    if !tags.is_empty() || !skip_tags.is_empty() {
        let before = test_docs.len();
        test_docs.retain(|(_, doc)| {
            let file_tags = crate::commands::run::extract_test_meta(doc.as_ref()).tags;
            crate::commands::run::tags_match(&file_tags, tags, skip_tags)
        });
        info!(
            "Bench: {} of {before} test file(s) matched the tag filter",
            test_docs.len()
        );
        if test_docs.is_empty() {
            anyhow::bail!("no test files matched the requested tags");
        }
    }
    // The same validation `run` applies. `bench` used to parse and go: a
    // document with no verification section was accepted and then never had its
    // response read, so the run reported requests *sent* rather than completed
    // and left one abandoned call per request behind.
    let mut invalid = Vec::new();
    for (file, doc) in &test_docs {
        if let Err(e) = crate::parser::validate_document_chain(doc) {
            invalid.push(format!("{}: {e}", file.display()));
        }
    }
    if !invalid.is_empty() {
        anyhow::bail!("invalid test document(s):\n  {}", invalid.join("\n  "));
    }

    let test_files: Vec<std::path::PathBuf> = test_docs.iter().map(|(f, _)| f.clone()).collect();

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
            // Fatal: continuing runs every request against unsubstituted placeholders.
            Err(e) => {
                return Err(e.context(
                    "the BENCH section declares data sources that could not be prepared",
                ));
            }
        }
    } else {
        None
    };

    eprintln!("Starting benchmark...");
    let run_start = Instant::now();
    let shared_config = Arc::new(config.clone());
    let progress_task = {
        let count = Arc::clone(&progress_count);
        let errors = Arc::clone(&progress_errors);
        let done = Arc::clone(&progress_done);
        let cfg = Arc::clone(&shared_config);
        tokio::spawn(async move {
            // Poll the stop flag far more often than the reporting interval.
            // Sleeping a whole interval and only then checking meant the run
            // kept the process alive until the next tick after the workers had
            // finished, which both wasted that time and, because `run_elapsed`
            // is taken afterwards, deflated the reported throughput.
            const POLL: Duration = Duration::from_millis(50);

            let mut waited = Duration::ZERO;

            loop {
                tokio::time::sleep(POLL).await;

                if done.load(Ordering::Relaxed) {
                    break;
                }

                waited += POLL;
                if waited >= cfg.progress_interval {
                    waited = Duration::ZERO;
                    print_progress_snapshot(run_start, &count, &errors, &cfg);
                }
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
            let cfg = Arc::clone(&shared_config);
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
                    cfg.collect_details,
                );
                let runner = Arc::new(worker_runner(&cfg, connection_id));
                let endpoints: Vec<Arc<str>> = docs
                    .iter()
                    .map(|(_, d)| {
                        Arc::from(d.get_endpoint().unwrap_or_else(|| "unknown".to_string()))
                    })
                    .collect();
                let prepared: Vec<_> = docs.iter().map(|(_, d)| runner.prepare(d)).collect();
                let mut next_slot = Instant::now();
                let deadline = Instant::now() + dur;
                while Instant::now() < deadline && !shutdown.load(Ordering::Relaxed) {
                    for (((_file, gctf_doc), endpoint), prep) in
                        docs.iter().zip(&endpoints).zip(&prepared)
                    {
                        if Instant::now() >= deadline || shutdown.load(Ordering::Relaxed) {
                            break;
                        }
                        wait_for_rps_slot(&cfg, schedule_start, &mut next_slot).await;

                        let vars = match &sc {
                            Some(sdc) => match sdc.next_row_variables() {
                                Ok(Some(v)) => v,
                                Ok(None) => {
                                    if let Err(e) = sdc.rewind() {
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

                        let (lat_ns, status, error, passed) =
                            execute_bench_iteration_with_runner(&runner, gctf_doc, prep, vars)
                                .await;
                        let finished_at = Instant::now();
                        if should_record_after_deadline(cfg.duration_stop, finished_at, deadline) {
                            local.record(lat_ns, &status, error.as_deref(), endpoint, passed);
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
        // Integer division: a budget smaller than the document count rounds to
        // zero passes, which used to issue no requests at all, write an empty
        // report and exit 0.
        if total_passes == 0 {
            anyhow::bail!(
                "requests ({}) is below the number of selected documents ({}) — \
                 `--requests` is the total budget across all of them, so this \
                 would issue no requests at all. Raise it to at least {}, or \
                 select fewer files.",
                total_requests,
                test_docs.len(),
                test_docs.len()
            );
        }
        let passes_per_worker = total_passes / config.concurrency as u64;
        let max_deadline = config.max_duration.map(|d| Instant::now() + d);
        let schedule_start = run_start;

        for worker_id in 0..config.concurrency {
            let docs = test_docs.clone();
            let cfg = Arc::clone(&shared_config);
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
                    cfg.collect_details,
                );
                let runner = Arc::new(worker_runner(&cfg, connection_id));
                let endpoints: Vec<Arc<str>> = docs
                    .iter()
                    .map(|(_, d)| {
                        Arc::from(d.get_endpoint().unwrap_or_else(|| "unknown".to_string()))
                    })
                    .collect();
                let prepared: Vec<_> = docs.iter().map(|(_, d)| runner.prepare(d)).collect();
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

                    for (((_file, gctf_doc), endpoint), prep) in
                        docs.iter().zip(&endpoints).zip(&prepared)
                    {
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
                                    if let Err(e) = sdc.rewind() {
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

                        let (lat_ns, status, error, passed) =
                            execute_bench_iteration_with_runner(&runner, gctf_doc, prep, vars)
                                .await;
                        local.record(lat_ns, &status, error.as_deref(), endpoint, passed);
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

    // Take the measurement window before winding the progress task down, so
    // that shutdown never counts as time the benchmark spent serving requests.
    let run_elapsed = run_start.elapsed();

    progress_done.store(true, Ordering::Relaxed);
    let _ = progress_task.await;
    print_progress_snapshot(run_start, &progress_count, &progress_errors, config);
    let end_ts = crate::polyfill::runtime::now_timestamp();

    let user_cancelled = shutdown_requested.load(Ordering::Relaxed);
    let end_reason = derive_end_reason(
        has_duration,
        config.max_duration,
        run_elapsed,
        user_cancelled,
    );

    let client_cpu_seconds = client_cpu
        .as_mut()
        .and_then(ClientCpuSampler::elapsed_cpu_seconds);

    let merged_metrics = metrics.clone();
    let report = build_report(
        RunWindow {
            start_ts,
            end_ts,
            end_reason,
            elapsed: run_elapsed,
            client_cpu_seconds,
        },
        config,
        metrics,
        source_config.as_ref(),
    )?;
    Ok((report, merged_metrics))
}

async fn wait_for_rps_slot(
    config: &BenchConfigResolved,
    schedule_start: Instant,
    next_slot: &mut Instant,
) {
    if !has_target_rate(config) {
        return;
    }
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
    let schedule = config.load_schedule.trim_ascii();
    let schedule = SCHEDULE_NAMES
        .iter()
        .find(|name| schedule.eq_ignore_ascii_case(name))
        .copied()
        .unwrap_or(schedule);
    let fallback = config.max_rps.unwrap_or(0.0);
    let start = config.load_start.unwrap_or(fallback);

    let _no_schedule = || -> f64 {
        if config.max_rps.is_some() {
            fallback.max(0.0)
        } else {
            start.max(0.0)
        }
    };

    let rps = match schedule {
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
const SCHEDULE_NAMES: &[&str] = &["const", "step", "line", "sine", "spike", "custom"];

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
fn grpc_status_label(grpc_status: Option<u32>, passed: bool) -> Cow<'static, str> {
    match grpc_status {
        Some(code) => crate::execution::TestRunner::grpc_code_name_from_numeric(code as i64)
            .map_or_else(|| Cow::Owned(format!("CODE_{code}")), Cow::Borrowed),
        None => Cow::Borrowed(if passed { "OK" } else { "ERROR" }),
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
    test_docs: &[(std::path::PathBuf, Arc<GctfDocument>)],
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
            config.collect_details,
        );
    }

    // Prebuild `connections` runners, one per distinct client channel, and
    // round-robin spawned requests across them. Channels and descriptors are
    // globally cached (keyed by connection_id), so N distinct ids open N
    // distinct HTTP/2 channels while keeping client construction off the
    // per-request hot path.
    let runners: Vec<Arc<TestRunner>> = (0..config.connections.max(1))
        .map(|i| Arc::new(worker_runner(config, i as u64)))
        .collect();

    // Per-arrival dispatch clones a pointer, not the AST.
    let docs: Vec<Arc<GctfDocument>> = test_docs.iter().map(|(_, doc)| Arc::clone(doc)).collect();
    let endpoints: Vec<Arc<str>> = docs
        .iter()
        .map(|d| Arc::from(d.get_endpoint().unwrap_or_else(|| "unknown".to_string())))
        .collect();
    let prepared: Vec<Arc<crate::execution::runner::PreparedChain>> = docs
        .iter()
        .map(|d| Arc::new(runners[0].prepare(d)))
        .collect();

    let hint = match bound {
        RunBound::Count(n) => n as usize,
        RunBound::Duration(_) => 1000,
    };
    // Each arrival is its own task, so there is no per-worker accumulator to
    // merge; the tasks hand their outcome back and this driver is the only
    // writer. A shared `Mutex<BenchMetrics>` here meant one contended async
    // lock acquisition per request.
    let mut metrics = BenchMetrics::for_worker(
        hint,
        config.count_errors_in_latency,
        sample_stride_from_rate(config.sample_rate),
        config.skip_first,
        config.collect_details,
    );

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

        let slot = doc_cursor % docs.len();
        let doc = Arc::clone(&docs[slot]);
        let endpoint = Arc::clone(&endpoints[slot]);
        let prep = Arc::clone(&prepared[slot]);
        let vars = next_source_vars(&source_config);

        let permits = Arc::clone(&semaphore);
        // Round-robin task k across the `connections` channels: k -> runners[k % N].
        let runner = Arc::clone(&runners[round_robin_index(doc_cursor, runners.len())]);
        let progress_count = Arc::clone(&progress_count);
        let progress_errors = Arc::clone(&progress_errors);
        let duration_stop = config.duration_stop;

        tasks.spawn(Box::pin(async move {
            // Acquire the in-flight permit HERE (inside the task): if the cap is
            // saturated we queue, and because latency is measured from
            // `arrival_instant` the queuing delay is captured in the sample.
            let _permit = permits.acquire_owned().await;
            let outcome = run_request_with_runner(&runner, &doc, &prep, vars).await;
            let finished_at = Instant::now();
            let lat_ns = latency_ns_from_arrival(arrival_instant, finished_at);

            let record = match deadline {
                Some(dl) => should_record_after_deadline(duration_stop, finished_at, dl),
                None => true,
            };
            if record {
                progress_count.fetch_add(1, Ordering::Relaxed);
                if outcome.status != "OK" {
                    progress_errors.fetch_add(1, Ordering::Relaxed);
                }
            }
            record.then_some(ArrivalOutcome {
                lat_ns,
                status: outcome.status,
                error: outcome.error,
                endpoint,
                passed: outcome.passed,
            })
        }));

        // Reap already-finished tasks so the JoinSet doesn't accumulate handles.
        while let Some(joined) = tasks.try_join_next() {
            record_arrival(&mut metrics, joined.ok().flatten());
        }
    }

    // Drain outstanding in-flight requests per the `duration_stop` policy.
    match config.duration_stop {
        DurationStopMode::Close => {
            // Don't wait for stragglers; requests already recorded stay counted.
            tasks.abort_all();
        }
        DurationStopMode::Wait | DurationStopMode::Ignore => {}
    }
    while let Some(joined) = tasks.join_next().await {
        record_arrival(&mut metrics, joined.ok().flatten());
    }

    metrics
}

/// What one open-model arrival produced. Returned from the task rather than
/// recorded inside it, so the driver stays the single writer of `BenchMetrics`.
struct ArrivalOutcome {
    lat_ns: u64,
    status: Cow<'static, str>,
    error: Option<String>,
    endpoint: Arc<str>,
    passed: bool,
}

fn record_arrival(metrics: &mut BenchMetrics, outcome: Option<ArrivalOutcome>) {
    if let Some(o) = outcome {
        metrics.record(
            o.lat_ns,
            &o.status,
            o.error.as_deref(),
            &o.endpoint,
            o.passed,
        );
    }
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
) -> (u64, Cow<'static, str>, Option<String>, String, bool) {
    let parse_result = crate::parser::parse_with_recovery(file);
    execute_single_bench_iteration_with_vars(&parse_result.document, config, HashMap::new(), 0)
        .await
}

/// The closed loop times the RPC, the open model the intended arrival slot.
struct RequestOutcome {
    call_duration_ns: Option<u64>,
    status: Cow<'static, str>,
    error: Option<String>,
    passed: bool,
}

async fn run_request_with_runner(
    runner: &crate::execution::TestRunner,
    doc: &GctfDocument,
    prepared: &crate::execution::runner::PreparedChain,
    vars: HashMap<String, serde_json::Value>,
) -> RequestOutcome {
    use crate::execution::TestExecutionStatus;

    match runner.run_test_prepared(doc, vars, prepared).await {
        Ok(result) => {
            let passed = matches!(result.status, TestExecutionStatus::Pass);
            RequestOutcome {
                call_duration_ns: result.call_duration_ns,
                status: grpc_status_label(result.grpc_status, passed),
                error: match result.status {
                    TestExecutionStatus::Pass => None,
                    TestExecutionStatus::Fail(msg) => Some(msg),
                },
                passed,
            }
        }
        Err(e) => RequestOutcome {
            call_duration_ns: None,
            status: Cow::Borrowed("ERROR"),
            error: Some(e.to_string()),
            passed: false,
        },
    }
}

/// One closed-loop request: latency is the gRPC call, not the wall clock around
/// it, which would also cover variable substitution and two `Value` deep clones.
async fn execute_bench_iteration_with_runner(
    runner: &crate::execution::TestRunner,
    doc: &GctfDocument,
    prepared: &crate::execution::runner::PreparedChain,
    source_variables: HashMap<String, serde_json::Value>,
) -> (u64, Cow<'static, str>, Option<String>, bool) {
    let start = Instant::now();
    let outcome = run_request_with_runner(runner, doc, prepared, source_variables).await;
    let latency = outcome
        .call_duration_ns
        .unwrap_or_else(|| start.elapsed().as_nanos() as u64);
    (latency, outcome.status, outcome.error, outcome.passed)
}

/// Build the runner a closed-loop worker reuses for every request it issues.
fn worker_runner(config: &BenchConfigResolved, connection_id: u64) -> crate::execution::TestRunner {
    let no_assert = config.no_assert
        || config.assert_mode == "off"
        || config.assert_mode == "skip"
        || config.calibration_address.is_some();
    let runner = crate::execution::TestRunner::new(
        false,
        request_timeout_seconds(config),
        no_assert,
        false,
        false,
        None,
    )
    .with_protocol(config.protocol)
    .with_connection_id(connection_id);
    match &config.calibration_address {
        Some(address) => runner.with_address_override(address.clone()),
        None => runner,
    }
}

async fn execute_single_bench_iteration_with_vars(
    doc: &GctfDocument,
    config: &BenchConfigResolved,
    source_variables: HashMap<String, serde_json::Value>,
    connection_id: u64,
) -> (u64, Cow<'static, str>, Option<String>, String, bool) {
    let endpoint = doc.get_endpoint().unwrap_or_else(|| "unknown".to_string());
    let runner = worker_runner(config, connection_id);
    let prepared = runner.prepare(doc);
    let (lat, status, error, passed) =
        execute_bench_iteration_with_runner(&runner, doc, &prepared, source_variables).await;
    (lat, status, error, endpoint, passed)
}

fn evaluate_thresholds(
    metrics: &BenchMetrics,
    rps_observed: f64,
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

        let actual_f64 = resolve_metric_value(metrics, rps_observed, key);
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
    num.trim().replace('_', "").parse::<f64>().ok()
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

fn resolve_metric_value(metrics: &BenchMetrics, rps_observed: f64, key: &str) -> Option<f64> {
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
    if k == "passed" {
        return Some(metrics.passed as f64);
    }
    if k == "failed" {
        return Some(metrics.failed as f64);
    }
    // Throughput was unreachable before: this function only ever saw the
    // counters, never the run's wall time, so `bench-ergonomics` promised a
    // gate that could not exist.
    if k == "rps" || k == "rps_observed" || k == "throughput" {
        return Some(rps_observed);
    }
    if k == "pass_rate_pct" || k == "pass_rate" {
        if metrics.count == 0 {
            return Some(0.0);
        }
        return Some((metrics.passed as f64 / metrics.count as f64) * 100.0);
    }
    if k == "fail_rate_pct" || k == "fail_rate" {
        if metrics.count == 0 {
            return Some(0.0);
        }
        return Some((metrics.failed as f64 / metrics.count as f64) * 100.0);
    }
    if k == "error_rate_pct" || k == "error_rate" {
        if metrics.count == 0 {
            return Some(0.0);
        }
        return Some((metrics.errors as f64 / metrics.count as f64) * 100.0);
    }
    if let Some(inner) = parse_percentile_key(&k)
        && let Ok(pct) = inner.replace('_', "").parse::<f64>()
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

/// When a run started and stopped, and what it cost us to produce.
struct RunWindow<'a> {
    start_ts: i64,
    end_ts: i64,
    end_reason: &'a str,
    elapsed: Duration,
    client_cpu_seconds: Option<f64>,
}

fn build_report(
    window: RunWindow<'_>,
    config: &BenchConfigResolved,
    metrics: BenchMetrics,
    source_config: Option<&std::sync::Arc<crate::bench::sources::SourceDrivenConfig>>,
) -> Result<BenchReport> {
    let RunWindow {
        start_ts,
        end_ts,
        end_reason,
        elapsed,
        client_cpu_seconds,
    } = window;
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

    let threshold_results = evaluate_thresholds(&metrics, rps, &config.thresholds);

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
            passed: metrics.passed,
            failed: metrics.failed,
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
        client_cost: client_cpu_seconds.map(|cpu| {
            client_cost(
                cpu,
                elapsed.as_secs_f64(),
                count,
                rps,
                std::thread::available_parallelism()
                    .map(std::num::NonZeroUsize::get)
                    .unwrap_or(1),
            )
        }),
        // Filled in by the sweep driver; a single-level run leaves it empty.
        levels: Vec::new(),
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

/// Canonicalize the `--log-format` value. `ndjson` (the value advertised in the
/// flag help) is an alias for the per-response JSON Lines format `detail-json`.
fn canonical_bench_format(fmt: &str) -> &str {
    match fmt {
        "ndjson" => "detail-json",
        other => other,
    }
}

/// Per-request deadline in whole seconds; falls back to the run duration.
fn request_timeout_seconds(config: &BenchConfigResolved) -> u64 {
    config
        .request_timeout
        .or(config.duration)
        .map_or(30, |d| d.as_secs())
        .max(1)
}

/// Run produced no usable measurement: requests were issued and not one of them
/// satisfied its document. Keyed on the verdict rather than the transport
/// status, so a document that asserts an expected error no longer needs a
/// special case here — those runs pass their assertions and count as measured.
fn measured_nothing(count: u64, passed: u64) -> bool {
    count > 0 && passed == 0
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
    let (_synthetic_dir, synthetic_path) = if let Some(endpoint) = &args.call {
        let body = args.data.as_deref().unwrap_or("{}");
        // No ADDRESS section — `$GRPCTESTIFY_ADDRESS` fallback (same as every
        // other command) applies when it's absent. `<env:GRPCTESTIFY_ADDRESS>`
        // is not a real placeholder syntax anything resolves; it's a
        // display-only label used elsewhere (`ExecutionPlan::from_document`'s
        // `ConnectionInfo.source`, for `inspect`/`explain` output) — writing
        // it here as a literal `ADDRESS` value made every direct-call bench
        // fail immediately with an invalid-URI error, 100% of the time.
        // `RESPONSE partial {}` matches any reply, and its presence is what makes
        // the runner await one: without a verification section the run would
        // report requests sent rather than completed.
        let content = format!(
            "--- ENDPOINT ---\n{endpoint}\n--- REQUEST ---\n{body}\n--- RESPONSE partial ---\n{{}}\n"
        );
        // `--data` routinely carries credentials; this used to land in a fixed
        // world-readable path in the shared temp dir and was never deleted.
        let dir = tempfile::Builder::new()
            .prefix("grpctestify-bench-")
            .tempdir()?;
        let path = dir.path().join("direct.gctf");
        std::fs::write(&path, &content)?;
        (Some(dir), Some(path))
    } else {
        (None, None)
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

    let first_file = &test_paths[0];
    if !first_file.exists() {
        anyhow::bail!("File not found: {}", first_file.display());
    }

    // Store synthetic path in first_file for cleanup later
    let _ = synthetic_path;

    // A run has one benchmark configuration. It used to be taken from
    // `test_paths[0]` alone, so pointing `bench` at a directory silently
    // adopted whichever file sorted first and ignored every other `BENCH`
    // block. Collect them all and refuse to guess when they disagree.
    let bench_section = resolve_bench_section(&test_paths, &args.exclude)?;

    // Resolve configuration
    let mut config = BenchConfigResolved::from_cli_and_bench(args, bench_section.as_ref())?;

    // Kept alive for the whole run; dropping it stops the target.
    let _calibration = if args.calibrate {
        let target = crate::bench::calibrate::CalibrationTarget::spawn().await?;
        eprintln!(
            "Calibrating against the built-in no-op target ({})",
            target.address()
        );
        config.calibration_address = Some(target.address().to_string());
        Some(target)
    } else {
        None
    };

    // Print configuration
    eprintln!("Configuration:");
    eprintln!("  Profile: {}", config.profile);
    eprintln!("  Mode: {}", config.mode);
    eprintln!("  Concurrency: {}", config.concurrency);
    if let Some(n) = config.requests {
        eprintln!("  Requests: {}", n);
    }
    if let Some(d) = config.duration {
        eprintln!(
            "  Duration: {}",
            crate::report::style::format_duration_ms(d.as_millis() as u64)
        );
    }
    if let Some(d) = config.ramp_up {
        eprintln!(
            "  Ramp-up: {}",
            crate::report::style::format_duration_ms(d.as_millis() as u64)
        );
    }
    if let Some(d) = config.warmup {
        eprintln!(
            "  Warmup: {}",
            crate::report::style::format_duration_ms(d.as_millis() as u64)
        );
    }
    if let Some(d) = config.max_duration {
        eprintln!(
            "  Max duration: {}",
            crate::report::style::format_duration_ms(d.as_millis() as u64)
        );
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
        eprintln!(
            "  Load step duration: {}",
            crate::report::style::format_duration_ms(v.as_millis() as u64)
        );
    }
    if let Some(v) = config.load_max_duration {
        eprintln!(
            "  Load max duration: {}",
            crate::report::style::format_duration_ms(v.as_millis() as u64)
        );
    }
    eprintln!("  Connections: {}", config.connections);
    eprintln!(
        "  Connect timeout: {}",
        crate::report::style::format_duration_ms(config.connect_timeout.as_millis() as u64)
    );
    if let Some(k) = config.keepalive {
        eprintln!(
            "  Keepalive: {}",
            crate::report::style::format_duration_ms(k.as_millis() as u64)
        );
    }
    eprintln!(
        "  Worker threads: {}",
        config
            .cpus
            .map_or_else(|| "auto".to_string(), |c| c.to_string())
    );
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

    let report = run_benchmark(
        &test_paths,
        &config,
        &args.exclude,
        &args.tags,
        &args.skip_tags,
    )
    .await?;

    // Allure output: the raw report as a standalone file, plus the shared
    // allure-results contract so a benchmark run shows up in the same Allure
    // dashboard as `run`'s test results.
    if let Some(allure_dir) = &args.allure_output_dir {
        use crate::report::Reporter;
        use crate::state::{TestResult, TestResults};

        std::fs::create_dir_all(allure_dir)?;

        // Keep the full, un-lossy report as a standalone file (there is no
        // generic file-attachment API on the Allure reporter — it only
        // attaches per-test request/response exchanges — so the raw data
        // lives alongside the results contract rather than inside it).
        let bench_json = serde_json::to_string_pretty(&report)?;
        std::fs::write(allure_dir.join("benchmark-report.json"), &bench_json)?;

        // One synthetic TestResult per benchmarked endpoint (or a single
        // "benchmark" result when the report has no per-endpoint breakdown),
        // driven through the standard on_test_end×N + on_suite_end sequence.
        // A synthetic latency stands in for the (untracked) per-endpoint
        // duration; pass/fail follows the same rule as the run's exit code
        // (any errors, or a failed threshold, is a fail).
        fn bench_result(
            name: &str,
            count: u64,
            errors: u64,
            dur_ns: u64,
            thresholds_ok: bool,
        ) -> TestResult {
            let dur_ms = dur_ns / 1_000_000;
            if count > 0 && errors == 0 && thresholds_ok {
                TestResult::pass(name.to_string(), dur_ms, Some(dur_ms))
            } else if !thresholds_ok {
                TestResult::fail(
                    name.to_string(),
                    format!("{errors}/{count} errors; one or more thresholds failed"),
                    dur_ms,
                    Some(dur_ms),
                )
            } else {
                TestResult::fail(
                    name.to_string(),
                    format!("{errors}/{count} request(s) failed"),
                    dur_ms,
                    Some(dur_ms),
                )
            }
        }

        let thresholds_ok = report.thresholds_passed();
        let entries: Vec<(String, u64, u64, u64)> = if report.per_endpoint.is_empty() {
            vec![(
                "benchmark".to_string(),
                report.summary.count,
                report.summary.errors,
                report.summary.total_ns,
            )]
        } else {
            report
                .per_endpoint
                .iter()
                .map(|ep| (ep.endpoint.clone(), ep.count, ep.errors, ep.latency_p50))
                .collect()
        };

        let reporter = crate::report::AllureReporter::new(allure_dir.clone());
        let mut results = TestResults::new();
        for (name, count, errors, dur_ns) in &entries {
            let tr = bench_result(name, *count, *errors, *dur_ns, thresholds_ok);
            reporter.on_test_end(name, &tr);
            results.add(tr);
        }
        reporter.on_suite_end(&results)?;

        eprintln!(
            "Allure benchmark results written to: {}",
            allure_dir.display()
        );
    }

    // Custom template rendering (overrides format)
    let rendered_from_template = if let Some(template_path) = &args.report_template {
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
        true
    } else {
        false
    };

    // Output report based on format
    if !rendered_from_template {
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
                if let Some(output) = &args.output {
                    std::fs::write(output, &summary)?;
                    eprintln!("Console report written to: {}", output.display());
                } else {
                    println!("{}", summary);
                }
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
    }

    // Zero successes out of one-or-more attempted requests means the run
    // measured nothing — almost certainly a broken target/config (wrong
    // endpoint, server down, every call hitting the same error), not a real
    // result. This must fail even with no `BENCH.thresholds` configured:
    // `thresholds_passed()` is vacuously `true` when there are no
    // thresholds to check, which previously let a 100%-error run exit 0.
    if measured_nothing(report.summary.count, report.summary.passed) {
        anyhow::bail!(
            "Benchmark measured nothing — all {} request(s) failed (target likely misconfigured or unreachable)",
            report.summary.count
        );
    }

    if !report.thresholds_passed() {
        anyhow::bail!("Benchmark thresholds failed");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host_cores() -> u32 {
        std::thread::available_parallelism()
            .map(std::num::NonZeroUsize::get)
            .unwrap_or(1) as u32
    }

    // The pool never exceeds the workers that use it, whatever the host has —
    // Miri reports a single core, so nothing here may assume more.
    #[test]
    fn the_default_pool_never_exceeds_the_worker_count() {
        assert_eq!(default_connections(0), 1);
        assert_eq!(default_connections(1), 1);
        assert_eq!(default_connections(2), 2.min(host_cores()));
    }

    // Splitting the pool removes h2's per-connection mutex contention, but the
    // measured curve flattens at eight and the count can never exceed the
    // workers using it.
    #[test]
    fn the_default_pool_stays_within_its_bounds() {
        let parallelism = std::thread::available_parallelism()
            .map(std::num::NonZeroUsize::get)
            .unwrap_or(1) as u32;
        for concurrency in [0, 1, 2, 7, 99, 101, 1_000, u32::MAX] {
            let n = default_connections(concurrency);
            assert!(n >= 1);
            assert!(n <= concurrency.max(1), "{n} > concurrency {concurrency}");
            assert!(n <= MAX_DEFAULT_CONNECTIONS);
            assert!(n <= parallelism.max(1), "{n} > {parallelism} cores");
        }
        assert_eq!(
            default_connections(u32::MAX),
            MAX_DEFAULT_CONNECTIONS.min(parallelism.max(1))
        );
    }

    #[test]
    fn an_explicit_connection_count_is_never_overridden() {
        let mut bench = crate::parser::OrderedStringMap::new();
        bench.insert("concurrency".to_string(), "300".to_string());
        bench.insert("connections".to_string(), "1".to_string());
        let config = BenchConfigResolved::from_bench_section(Some(&bench)).unwrap();
        assert_eq!(config.connections, 1);
        assert_eq!(
            config.option_sources.get("connections"),
            Some(&BenchOptionSource::BenchSection)
        );
    }

    #[test]
    fn an_unset_connection_count_follows_concurrency() {
        let mut bench = crate::parser::OrderedStringMap::new();
        bench.insert("concurrency".to_string(), "300".to_string());
        let config = BenchConfigResolved::from_bench_section(Some(&bench)).unwrap();
        // Not a fixed count: the derivation is bounded by the host's cores.
        assert_eq!(config.connections, default_connections(300));
        assert_eq!(
            config.option_sources.get("connections"),
            Some(&BenchOptionSource::Default)
        );
    }

    #[test]
    fn client_cost_reports_per_request_cpu_and_cores() {
        let cost = client_cost(4.0, 2.0, 40_000, 20_000.0, 8);
        assert_eq!(cost.cores_used, 2.0);
        assert_eq!(cost.cpu_us_per_request, 100.0);
        assert_eq!(cost.rps_per_core, 10_000.0);
        assert!(!cost.generator_limited);
        assert!(cost.limits.is_empty());
    }

    #[test]
    fn client_cost_flags_a_saturated_generator() {
        let cost = client_cost(14.0, 2.0, 1_000, 500.0, 8);
        assert!(cost.generator_limited);
        assert_eq!(cost.limits.len(), 1);
        assert!(cost.limits[0].contains("of 8 cores"));
    }

    // A run that finishes instantly must not divide by zero and must not claim
    // an infinite rps/core.
    #[test]
    fn client_cost_survives_a_zero_length_run() {
        let cost = client_cost(0.0, 0.0, 0, 0.0, 0);
        assert_eq!(cost.cores_used, 0.0);
        assert_eq!(cost.cpu_us_per_request, 0.0);
        assert_eq!(cost.rps_per_core, 0.0);
        assert_eq!(cost.host_cores, 1);
        assert!(!cost.generator_limited);
    }

    // Exactly at the 80 % line the run is already suspect.
    #[test]
    fn client_cost_flags_at_the_busy_boundary() {
        let cost = client_cost(3.2, 1.0, 100, 100.0, 4);
        assert!(cost.generator_limited);
        let below = client_cost(3.1, 1.0, 100, 100.0, 4);
        assert!(!below.generator_limited);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn client_cpu_sampler_accounts_for_burned_cpu() {
        let Some(mut sampler) = ClientCpuSampler::start() else {
            return;
        };

        // Linux accounts CPU in 10 ms ticks, and on a runner executing hundreds
        // of tests at once this thread can spend most of a wall-clock second
        // descheduled. Burn work until a tick lands rather than assuming one
        // will inside a fixed window.
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut cpu = 0.0;
        let mut spin: u64 = 0;
        while std::time::Instant::now() < deadline {
            for _ in 0..2_000_000u64 {
                spin = spin.wrapping_mul(31).wrapping_add(7);
            }
            cpu = sampler
                .elapsed_cpu_seconds()
                .expect("a live process must report its own CPU");
            if cpu > 0.0 {
                break;
            }
        }
        std::hint::black_box(spin);

        assert!(cpu >= 0.0, "CPU time must never go backwards, got {cpu}");
        assert!(
            sampler.elapsed_cpu_seconds().unwrap_or(cpu) >= cpu,
            "accumulated CPU must be monotonic"
        );
        if cpu == 0.0 {
            eprintln!(
                "note: this platform reported no CPU for a busy loop; only monotonicity checked"
            );
        }
    }

    /// `BenchArgs` with everything unset — the shape of `grpctestify bench x.gctf`
    /// with no flags. Tests set only the fields they exercise.
    fn base_args() -> BenchArgs {
        BenchArgs {
            calibrate: false,
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
            concurrency_schedule: None,
            concurrency_start: None,
            concurrency_end: None,
            concurrency_step: None,
            concurrency_step_duration: None,
            connections: None,
            connect_timeout: None,
            request_timeout: None,
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
        }
    }

    #[test]
    fn details_are_collected_only_for_the_per_response_format() {
        let mut m = BenchMetrics::for_worker(0, false, 1, 0, false);
        m.record(1, "OK", None, "svc/method", true);
        assert!(m.details.is_empty());

        let mut m = BenchMetrics::for_worker(0, false, 1, 0, true);
        m.record(1, "OK", None, "svc/method", true);
        assert_eq!(m.details.len(), 1);
    }

    #[test]
    fn request_timeout_defaults_to_the_run_duration() {
        let config = BenchConfigResolved {
            duration: Some(Duration::from_secs(10)),
            ..Default::default()
        };
        assert_eq!(request_timeout_seconds(&config), 10);
    }

    #[test]
    fn request_timeout_overrides_the_run_duration() {
        let config = BenchConfigResolved {
            duration: Some(Duration::from_secs(10)),
            request_timeout: Some(Duration::from_secs(120)),
            ..Default::default()
        };
        assert_eq!(request_timeout_seconds(&config), 120);
    }

    #[test]
    fn request_timeout_never_reaches_zero() {
        let config = BenchConfigResolved {
            request_timeout: Some(Duration::from_millis(1)),
            ..Default::default()
        };
        assert_eq!(request_timeout_seconds(&config), 1);
    }

    #[test]
    fn measured_nothing_flags_a_fully_failed_run() {
        assert!(measured_nothing(100, 0));
    }

    #[test]
    fn measured_nothing_allows_a_run_that_expects_an_error() {
        // Every request returned a non-OK status, but each one satisfied its
        // ERROR section, so the run measured something.
        assert!(!measured_nothing(100, 100));
    }

    #[test]
    fn measured_nothing_allows_partial_success() {
        assert!(!measured_nothing(100, 1));
    }

    #[test]
    fn measured_nothing_allows_an_empty_run() {
        assert!(!measured_nothing(0, 0));
    }

    #[test]
    fn parse_bench_num_accepts_digit_separators() {
        assert_eq!(parse_bench_num::<usize>("1_000", 1), 1000);
        assert_eq!(parse_bench_num::<u64>("1_000_000", 100), 1_000_000);
        assert_eq!(parse_bench_num::<f64>("10_000", 0.0), 10_000.0);
        // Plain values still parse; genuinely-invalid ones fall back.
        assert_eq!(parse_bench_num::<usize>("42", 1), 42);
        assert_eq!(parse_bench_num::<usize>("nope", 7), 7);
    }

    // Regression: BENCH.sources: is nested YAML, not a flat key. The generic
    // key-value tokenizer used to split every raw line on its own (even
    // indented continuation lines), so `sources` parsed to an empty string
    // and the actual list items landed under bogus keys like `"- name"`.
    #[test]
    fn bench_sources_survives_real_parsing() {
        let src = "--- BENCH ---\nmode: fixed\nsources:\n  - name: users\n    file: data/users.csv\n  - name: orders\n    file: data/orders.csv\n\n--- ENDPOINT ---\npkg.Svc/Method\n\n--- REQUEST ---\n{}\n\n--- RESPONSE ---\n{}\n";
        let doc = crate::parser::parse_gctf_from_str(src, "test.gctf").unwrap();
        let bench = extract_bench_section(&doc).expect("BENCH section");
        let config = BenchConfigResolved::from_bench_section(Some(&bench)).unwrap();
        assert_eq!(config.sources.len(), 2, "sources: {:?}", config.sources);
        assert_eq!(config.sources[0].name, Some("users".to_string()));
        assert_eq!(config.sources[1].name, Some("orders".to_string()));
    }

    #[test]
    fn exec_model_dispatch() {
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
    fn open_schedule_count_exactly_n() {
        // Request-count open mode schedules exactly N arrivals.
        let arrivals: Vec<Duration> =
            ArrivalSchedule::new(|_| 100.0, RunBound::Count(500)).collect();
        assert_eq!(arrivals.len(), 500);
        // Fixed 100 rps → 10ms spacing.
        assert_eq!(arrivals[0], Duration::ZERO);
        assert_eq!(arrivals[1], Duration::from_millis(10));
    }

    #[test]
    fn open_schedule_duration_arrival_count() {
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
    fn open_schedule_ramp_from_zero_terminates() {
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
    fn latency_measured_from_arrival() {
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
    fn parse_duration_seconds() {
        let d = parse_duration("30s").unwrap();
        assert_eq!(d.as_secs(), 30);
    }

    #[test]
    fn parse_duration_minutes() {
        let d = parse_duration("5m").unwrap();
        assert_eq!(d.as_secs(), 300);
    }

    #[test]
    fn parse_duration_hours() {
        let d = parse_duration("1h").unwrap();
        assert_eq!(d.as_secs(), 3600);
    }

    #[test]
    fn parse_duration_milliseconds() {
        let d = parse_duration("500ms").unwrap();
        assert_eq!(d.as_millis(), 500);
    }

    #[test]
    fn parse_duration_invalid() {
        assert!(parse_duration("").is_err());
        assert!(parse_duration("abc").is_err());
        assert!(parse_duration("30x").is_err());
    }

    // Bug 4: "ms" must be parsed before the single-char "s" suffix.
    #[test]
    fn parse_duration_sec_units() {
        assert_eq!(parse_duration_sec("500ms"), Some(0.5));
        assert_eq!(parse_duration_sec("2s"), Some(2.0));
        assert_eq!(parse_duration_sec("1m"), Some(60.0));
        assert_eq!(parse_duration_sec("1h"), Some(3600.0));
        assert_eq!(parse_duration_sec("10"), Some(10.0));
    }

    // Bug 4: load_profile points with "ms" durations must not be dropped.
    #[test]
    fn parse_custom_profile_with_ms() {
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
    fn to_histogram_zero_width_no_panic() {
        let metrics = metrics_with_latencies(&[10, 10, 11, 12, 12]);
        // (max-min)/bucket_count = (12-10)/10 = 0 in integer math -> was a panic.
        let buckets = metrics.to_histogram(10);
        assert!(!buckets.is_empty());
        let total: u64 = buckets.iter().map(|b| b.count).sum();
        assert_eq!(total, 5);
    }

    // A `NaN` percentile (config `"nan".parse::<f64>()` succeeds) must not
    // panic the sort — `partial_cmp().unwrap()` used to; `total_cmp` handles it.
    #[test]
    fn to_percentiles_with_nan_spec_does_not_panic() {
        let metrics = metrics_with_latencies(&[10, 20, 30, 40, 50]);
        let requested = vec!["pnan".to_string(), "p50".to_string(), "p99".to_string()];
        let result = metrics.to_percentiles(&requested);
        // Does not panic; the well-formed percentiles are still present.
        assert!(result.iter().any(|p| p.percentile == 50.0));
        assert!(result.iter().any(|p| p.percentile == 99.0));
    }

    // Bug 3: `--requests` is the TOTAL budget across all docs.
    #[test]
    fn request_passes_honours_total_budget() {
        // 100 total requests over 3 docs -> ~33 passes (33*3 = 99 requests).
        assert_eq!(request_passes(100, 3), 33);
        // Single doc: passes == requests (unchanged behaviour).
        assert_eq!(request_passes(100, 1), 100);
        // Zero docs must not divide by zero.
        assert_eq!(request_passes(100, 0), 100);
    }

    // Bug 2: unit-suffixed thresholds must parse, not silently become 0.0.
    #[test]
    fn evaluate_thresholds_percent_suffix() {
        let mut metrics = BenchMetrics {
            count: 100,
            errors: 2,
            ..Default::default()
        };
        metrics.ok = 98;
        let mut thresholds = HashMap::new();
        thresholds.insert("error_rate_pct".to_string(), "< 5%".to_string());
        let results = evaluate_thresholds(&metrics, 0.0, &thresholds);
        assert_eq!(results.len(), 1);
        // 2% < 5% must PASS. With the old unwrap_or(0.0), rhs was 0 -> failed.
        assert!(results[0].passed, "2% should pass a < 5% threshold");
    }

    #[test]
    fn concurrency_levels_default_to_a_single_level() {
        let config = BenchConfigResolved {
            concurrency: 8,
            ..Default::default()
        };
        assert_eq!(concurrency_levels(&config).unwrap(), vec![8]);
    }

    #[test]
    fn concurrency_levels_step_up_and_down() {
        let up = BenchConfigResolved {
            concurrency_schedule: "step".to_string(),
            concurrency_start: Some(1),
            concurrency_end: Some(9),
            concurrency_step: Some(4),
            ..Default::default()
        };
        assert_eq!(concurrency_levels(&up).unwrap(), vec![1, 5, 9]);

        // A ramp-down is expressed by an end below the start, not a negative
        // step — the level count is a `u32`.
        let down = BenchConfigResolved {
            concurrency_schedule: "step".to_string(),
            concurrency_start: Some(10),
            concurrency_end: Some(4),
            concurrency_step: Some(3),
            ..Default::default()
        };
        assert_eq!(concurrency_levels(&down).unwrap(), vec![10, 7, 4]);
    }

    #[test]
    fn concurrency_levels_clamp_the_last_step_to_the_end() {
        // 1 -> 5 -> 9 -> 10, not 1 -> 5 -> 9 -> 13.
        let config = BenchConfigResolved {
            concurrency_schedule: "line".to_string(),
            concurrency_start: Some(1),
            concurrency_end: Some(10),
            concurrency_step: Some(4),
            ..Default::default()
        };
        assert_eq!(concurrency_levels(&config).unwrap(), vec![1, 5, 9, 10]);
    }

    #[test]
    fn concurrency_levels_reject_an_unbounded_sweep() {
        let config = BenchConfigResolved {
            concurrency_schedule: "line".to_string(),
            concurrency_start: Some(1),
            concurrency_end: Some(100_000),
            ..Default::default()
        };
        let err = concurrency_levels(&config).unwrap_err().to_string();
        assert!(err.contains("levels"), "got: {err}");
    }

    #[test]
    fn concurrency_levels_reject_an_unknown_schedule() {
        let config = BenchConfigResolved {
            concurrency_schedule: "sine".to_string(),
            ..Default::default()
        };
        let err = concurrency_levels(&config).unwrap_err().to_string();
        assert!(err.contains("const, step, line"), "got: {err}");
    }

    // Throughput gating was promised by the spec but structurally impossible:
    // the evaluator only ever received the counters, never the run's duration.
    #[test]
    fn evaluate_thresholds_gates_on_throughput() {
        let metrics = BenchMetrics {
            count: 100,
            ..Default::default()
        };
        let mut thresholds = HashMap::new();
        thresholds.insert("rps".to_string(), "> 1000".to_string());

        let met = evaluate_thresholds(&metrics, 2_500.0, &thresholds);
        assert!(met[0].passed, "2500 rps should clear a > 1000 threshold");

        let missed = evaluate_thresholds(&metrics, 250.0, &thresholds);
        assert!(!missed[0].passed, "250 rps should fail a > 1000 threshold");
    }

    // The verdict counters are gateable too, so a negative-path benchmark can
    // require that every request behaved as its document asserted.
    #[test]
    fn evaluate_thresholds_gates_on_the_verdict() {
        let mut metrics = BenchMetrics {
            count: 100,
            ..Default::default()
        };
        metrics.passed = 97;
        metrics.failed = 3;

        let mut thresholds = HashMap::new();
        thresholds.insert("pass_rate_pct".to_string(), ">= 99".to_string());
        assert!(!evaluate_thresholds(&metrics, 0.0, &thresholds)[0].passed);

        let mut thresholds = HashMap::new();
        thresholds.insert("failed".to_string(), "< 5".to_string());
        assert!(evaluate_thresholds(&metrics, 0.0, &thresholds)[0].passed);
    }

    // Bug 2: unparsable thresholds must error instead of defaulting to 0.0.
    #[test]
    fn evaluate_thresholds_invalid_value_errors() {
        let metrics = BenchMetrics {
            count: 100,
            ..Default::default()
        };
        let mut thresholds = HashMap::new();
        thresholds.insert("error_rate_pct".to_string(), "< abc".to_string());
        let results = evaluate_thresholds(&metrics, 0.0, &thresholds);
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
    fn bench_config_defaults() {
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
    fn bench_config_cli_override() {
        let args = BenchArgs {
            calibrate: false,
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
            concurrency_schedule: None,
            concurrency_start: None,
            concurrency_end: None,
            concurrency_step: None,
            concurrency_step_duration: None,
            connections: Some(5),
            connect_timeout: Some("3s".to_string()),
            request_timeout: None,
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
    fn bench_config_bench_section() {
        let mut bench_section = crate::parser::OrderedStringMap::new();
        bench_section.insert("profile".to_string(), "stress".to_string());
        bench_section.insert("concurrency".to_string(), "50".to_string());
        bench_section.insert("requests".to_string(), "5000".to_string());
        bench_section.insert("thresholds.latency_ms.p95".to_string(), "< 200".to_string());

        let args = BenchArgs {
            calibrate: false,
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
            concurrency_schedule: None,
            concurrency_start: None,
            concurrency_end: None,
            concurrency_step: None,
            concurrency_step_duration: None,
            connections: None,
            connect_timeout: None,
            request_timeout: None,
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
        // `BENCH section > --profile preset`: the `stress` preset carries
        // `duration: 120s`, but this section chose a request budget, so the
        // preset must not inject the competing stop condition and silently
        // discard it. This previously asserted `None`, encoding that bug.
        assert_eq!(config.requests, Some(5000));
        assert_eq!(config.duration, None);
        assert_eq!(config.thresholds.len(), 1);
        assert_eq!(
            config.thresholds.get("latency_ms.p95"),
            Some(&"< 200".to_string())
        );
    }

    #[test]
    fn bench_config_cli_overrides_bench_section() {
        let mut bench_section = crate::parser::OrderedStringMap::new();
        bench_section.insert("profile".to_string(), "stress".to_string());
        bench_section.insert("concurrency".to_string(), "50".to_string());

        let args = BenchArgs {
            calibrate: false,
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
            concurrency_schedule: None,
            concurrency_start: None,
            concurrency_end: None,
            concurrency_step: None,
            concurrency_step_duration: None,
            connections: None,
            connect_timeout: None,
            request_timeout: None,
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
    fn bench_option_sources_track_cli_bench_default() {
        let mut bench_section = crate::parser::OrderedStringMap::new();
        bench_section.insert("concurrency".to_string(), "20".to_string());
        bench_section.insert("load_schedule".to_string(), "step".to_string());

        let args = BenchArgs {
            calibrate: false,
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
            concurrency_schedule: None,
            concurrency_start: None,
            concurrency_end: None,
            concurrency_step: None,
            concurrency_step_duration: None,
            connections: None,
            connect_timeout: None,
            request_timeout: None,
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
    fn bench_config_from_bench_section_tracks_sources() {
        let mut bench_section = crate::parser::OrderedStringMap::new();
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
    fn duration_mode_ignores_requests() {
        let args = BenchArgs {
            calibrate: false,
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
            concurrency_schedule: None,
            concurrency_start: None,
            concurrency_end: None,
            concurrency_step: None,
            concurrency_step_duration: None,
            connections: Some(1),
            connect_timeout: None,
            request_timeout: None,
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

    // Regression: `apply_profile_defaults` ran before `cli.profile` was
    // assigned, so `config.profile` was still the `functional` default and the
    // `!= "functional"` guard skipped it entirely. `--profile stress` printed
    // "Profile: stress" in the banner and then ran the built-in defaults.
    #[test]
    fn cli_profile_applies_its_preset() {
        let mut args = base_args();
        args.profile = Some("stress".to_string());

        let config = BenchConfigResolved::from_cli_and_bench(&args, None).unwrap();

        assert_eq!(config.profile, "stress");
        assert_eq!(config.mode, "stepping");
        assert_eq!(config.concurrency, 50);
        assert_eq!(config.duration, Some(Duration::from_secs(120)));
        assert_eq!(config.load_schedule, "line");
        assert_eq!(config.load_start, Some(10.0));
        assert_eq!(config.load_end, Some(500.0));
    }

    // A CLI flag still beats the preset it sits next to.
    #[test]
    fn cli_flag_overrides_its_profile_preset() {
        let mut args = base_args();
        args.profile = Some("stress".to_string());
        args.concurrency = Some(3);
        args.connections = Some(1);

        let config = BenchConfigResolved::from_cli_and_bench(&args, None).unwrap();

        assert_eq!(config.concurrency, 3);
        assert_eq!(
            config.mode, "stepping",
            "unset keys still come from the preset"
        );
    }

    #[test]
    fn connections_must_not_exceed_concurrency() {
        let args = BenchArgs {
            calibrate: false,
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
            concurrency_schedule: None,
            concurrency_start: None,
            concurrency_end: None,
            concurrency_step: None,
            concurrency_step_duration: None,
            connections: Some(3),
            connect_timeout: None,
            request_timeout: None,
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
    fn duration_stop_invalid_value_fails() {
        let args = BenchArgs {
            calibrate: false,
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
            concurrency_schedule: None,
            concurrency_start: None,
            concurrency_end: None,
            concurrency_step: None,
            concurrency_step_duration: None,
            connections: Some(1),
            connect_timeout: None,
            request_timeout: None,
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
    fn should_record_after_deadline_modes() {
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
    fn histogram_percentile_accuracy() {
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
    fn histogram_not_biased_toward_late_samples() {
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
    fn histogram_merge_is_lossless() {
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
    fn histogram_memory_is_bounded() {
        let mut metrics = BenchMetrics::default();
        let n = MAX_LATENCY_SAMPLES + 10;
        for i in 0..n {
            metrics.record(i as u64, "OK", None, "test", true);
        }
        assert_eq!(metrics.latency.buckets.len(), HIST_BUCKETS);
        assert_eq!(metrics.latency.total, n as u64);
        assert_eq!(metrics.count, n as u64);
    }

    // mean is computed exactly from the running sum/count, not the histogram.
    #[test]
    fn mean_is_exact() {
        let mut m = BenchMetrics::default();
        for v in [10u64, 20, 30, 40, 100] {
            m.record(v, "OK", None, "e", true);
        }
        // total 200 over 5 requests -> exact 40.
        assert_eq!(m.total_ns / m.count, 40);
    }

    #[test]
    fn derive_end_reason_variants() {
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
    fn resolve_metric_error_rate_pct() {
        let mut metrics = BenchMetrics::default();
        metrics.record(1_000_000, "OK", None, "test", true);
        metrics.record(1_000_000, "ERROR", Some("boom"), "test", false);

        let value = resolve_metric_value(&metrics, 0.0, "error_rate_pct").unwrap_or_default();
        assert!((value - 50.0).abs() < f64::EPSILON);
    }

    #[test]
    fn unknown_threshold_metric_fails_with_reason() {
        let mut metrics = BenchMetrics::default();
        metrics.record(1_000_000, "OK", None, "test", true);

        let mut thresholds = HashMap::new();
        thresholds.insert("unknown_metric".to_string(), "< 10".to_string());

        let results = evaluate_thresholds(&metrics, 0.0, &thresholds);
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
    fn target_rps_step_schedule() {
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
    fn target_rps_line_schedule_down() {
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
    fn skip_first_discards_leading_samples() {
        let mut m = BenchMetrics {
            skip_first_remaining: 2,
            ..Default::default()
        };
        for v in [9999, 9998, 10, 11, 12] {
            m.record(v, "OK", None, "e", true);
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
    fn skip_first_saturates_without_panic() {
        let mut m = BenchMetrics {
            skip_first_remaining: 10,
            ..Default::default()
        };
        m.record(1, "OK", None, "e", true);
        m.record(2, "OK", None, "e", true);
        assert_eq!(m.latency.total, 0);
        // Zero skip is a no-op.
        let mut m2 = BenchMetrics::default();
        for v in [1, 2, 3] {
            m2.record(v, "OK", None, "e", true);
        }
        assert_eq!(m2.latency.total, 3);
    }

    // count_errors_in_latency=false (default): error latencies are EXCLUDED from
    // the latency distribution, but still counted in throughput and overall timing.
    #[test]
    fn count_errors_excluded_from_latency_by_default() {
        let mut m = BenchMetrics::default();
        m.record(100, "OK", None, "e", true);
        m.record(9999, "ERROR", Some("boom"), "e", false);
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
    fn count_errors_included_when_flag_set() {
        let mut m = BenchMetrics {
            count_errors_in_latency: true,
            ..Default::default()
        };
        m.record(100, "OK", None, "e", true);
        m.record(200, "ERROR", Some("boom"), "e", false);
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
    fn sample_rate_records_every_nth() {
        let mut m = BenchMetrics {
            sample_stride: sample_stride_from_rate(0.5),
            ..Default::default()
        };
        for i in 0..6 {
            m.record(i, "OK", None, "e", true);
        }
        // stride 2 -> records requests 1,3,5 (i = 0,2,4); all 6 still counted.
        assert_eq!(m.latency.total, 3);
        assert_eq!(m.latency.buckets[0], 1);
        assert_eq!(m.latency.buckets[2], 1);
        assert_eq!(m.latency.buckets[4], 1);
        assert_eq!(m.count, 6);
    }

    #[test]
    fn sample_rate_full_records_all() {
        let mut m = BenchMetrics {
            sample_stride: sample_stride_from_rate(1.0),
            ..Default::default()
        };
        for i in 0..4 {
            m.record(i, "OK", None, "e", true);
        }
        assert_eq!(m.latency.total, 4);
    }

    // ramp_up: linearly scale the target load from ~0 to the steady-state target
    // over the first `ramp_up` seconds.
    #[test]
    fn ramp_up_scales_target_rps() {
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
    fn bench_section_parses_skip_first_and_count_errors() {
        let mut bench_section = crate::parser::OrderedStringMap::new();
        bench_section.insert("skip_first".to_string(), "7".to_string());
        bench_section.insert("count_errors_in_latency".to_string(), "true".to_string());
        let config = BenchConfigResolved::from_bench_section(Some(&bench_section)).unwrap();
        assert_eq!(config.skip_first, 7);
        assert!(config.count_errors_in_latency);
    }

    // Feature 1: each closed-loop worker maps to `worker % connections`, so
    // `connections` workers cycle over exactly `connections` distinct channels.
    #[test]
    fn worker_connection_id_assignment() {
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
    fn round_robin_index_picks_k_mod_n() {
        let picks: Vec<usize> = (0..7).map(|k| round_robin_index(k, 3)).collect();
        assert_eq!(picks, vec![0, 1, 2, 0, 1, 2, 0]);
        assert_eq!(round_robin_index(5, 0), 0);
    }

    // Feature 2: the real numeric gRPC code maps to its canonical status bucket;
    // OK and errored codes land in distinct buckets.
    #[test]
    fn grpc_status_label_mapping() {
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
    fn record_buckets_by_real_status() {
        let mut m = BenchMetrics::with_capacity(4);
        m.record(10, "OK", None, "svc/M", true);
        m.record(20, "OK", None, "svc/M", true);
        m.record(30, "Unavailable", Some("boom"), "svc/M", false);

        assert_eq!(m.grpc_status.get("OK"), Some(&2));
        assert_eq!(m.grpc_status.get("Unavailable"), Some(&1));
        assert_eq!(m.ok, 2);
        assert_eq!(m.errors, 1);
    }
}
