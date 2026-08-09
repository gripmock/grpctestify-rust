//
// The reports are parsed as schema-tolerant `serde_json::Value` so this command
// stays decoupled from the exact `BenchReport` struct. It reads the stable,
// versioned `bench_report_schema_v1` fields: `summary.{count,errors,rps_observed,
// average_ns}`, the `latency_distribution` array of `{percentile, latency_ns}`,
// and the `per_endpoint` array of `{endpoint, latency_p99}`.

use crate::cli::args::BenchCompareArgs;
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// Whether a metric is better when it goes up (throughput) or down (latency, errors).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    LowerIsBetter,
    HigherIsBetter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Pass,
    Improved,
    Regressed,
}

impl Verdict {
    fn label(self) -> &'static str {
        match self {
            Verdict::Pass => "PASS",
            Verdict::Improved => "IMPROVED",
            Verdict::Regressed => "REGRESSED",
        }
    }
}

/// Normalized metrics extracted from a bench report, in comparable units.
#[derive(Debug, Clone)]
struct Metrics {
    total: f64,
    errors: f64,
    rps: f64,
    mean_ns: f64,
    /// percentile label ("p50", "p90", ...) -> latency in ns
    percentiles: BTreeMap<String, f64>,
    /// endpoint name -> p99 latency in ns
    endpoint_p99: BTreeMap<String, f64>,
}

/// Percentage change from baseline to current: (current - baseline) / baseline * 100.
/// A baseline of zero yields 0 when unchanged and +inf when the value grew.
pub fn pct_change(baseline: f64, current: f64) -> f64 {
    if baseline == 0.0 {
        if current == 0.0 { 0.0 } else { f64::INFINITY }
    } else {
        (current - baseline) / baseline * 100.0
    }
}

/// Verdict for a metric given its direction and the max tolerated regression (percent).
/// For LowerIsBetter, a rise beyond `threshold_pct` regresses; any drop improves.
/// For HigherIsBetter, a drop beyond `threshold_pct` regresses; any rise improves.
pub fn verdict_for_metric(
    baseline: f64,
    current: f64,
    direction: Direction,
    threshold_pct: f64,
) -> Verdict {
    let change = pct_change(baseline, current);
    match direction {
        Direction::LowerIsBetter => {
            if change > threshold_pct {
                Verdict::Regressed
            } else if current < baseline {
                Verdict::Improved
            } else {
                Verdict::Pass
            }
        }
        Direction::HigherIsBetter => {
            // drop percentage is the negative of the change
            if -change > threshold_pct {
                Verdict::Regressed
            } else if current > baseline {
                Verdict::Improved
            } else {
                Verdict::Pass
            }
        }
    }
}

/// Verdict for error rate, compared in percentage points (not relative percent).
/// `max_points` is the max tolerated rise in the error-rate fraction, in points.
pub fn verdict_for_error_rate(baseline_rate: f64, current_rate: f64, max_points: f64) -> Verdict {
    let delta_points = (current_rate - baseline_rate) * 100.0;
    if delta_points > max_points {
        Verdict::Regressed
    } else if current_rate < baseline_rate {
        Verdict::Improved
    } else {
        Verdict::Pass
    }
}

/// Overall gate: pass only when no metric regressed.
pub fn overall_pass(rows: &[MetricRow]) -> bool {
    rows.iter().all(|r| r.verdict != Verdict::Regressed)
}

#[derive(Debug, Clone)]
pub struct MetricRow {
    pub name: String,
    pub baseline: f64,
    pub current: f64,
    pub abs_delta: f64,
    pub pct_delta: f64,
    pub threshold: f64,
    pub verdict: Verdict,
}

/// Thresholds controlling which deltas count as regressions.
#[derive(Debug, Clone, Copy)]
pub struct Thresholds {
    /// Max tolerated latency rise (percent) for mean and each percentile.
    pub max_latency_regression: f64,
    /// Max tolerated error-rate rise, in percentage points.
    pub max_error_rate_regression: f64,
    /// Max tolerated throughput drop (percent) before failing.
    pub min_throughput: f64,
}

fn require_f64(v: &Value, obj: &str, key: &str) -> Result<f64> {
    v.get(obj)
        .and_then(|o| o.get(key))
        .and_then(Value::as_f64)
        .with_context(|| format!("bench report missing numeric field `{obj}.{key}`"))
}

/// Canonical key for a percentile: `50.0` -> `p50`, `99.9` -> `p99.9`.
fn percentile_key(p: f64) -> String {
    format!("p{p}")
}

/// Inverse of [`percentile_key`], for ordering shared keys numerically —
/// lexicographic order would put `p100` before `p50`.
fn percentile_of_key(key: &str) -> Option<f64> {
    key.strip_prefix('p')?.parse().ok()
}

fn extract_metrics(v: &Value) -> Result<Metrics> {
    if !v.is_object() || v.get("summary").is_none() {
        bail!("input is not a bench report (no `summary` object)");
    }

    let total = require_f64(v, "summary", "count")?;
    let errors = require_f64(v, "summary", "errors")?;
    let rps = require_f64(v, "summary", "rps_observed")?;
    let mean_ns = require_f64(v, "summary", "average_ns")?;

    let mut percentiles = BTreeMap::new();
    if let Some(arr) = v.get("latency_distribution").and_then(Value::as_array) {
        for entry in arr {
            let (Some(p), Some(ns)) = (
                entry.get("percentile").and_then(Value::as_f64),
                entry.get("latency_ns").and_then(Value::as_f64),
            ) else {
                continue;
            };
            // Keyed by the exact percentile. Rounding to an integer collapsed
            // `p99.5` and `p99.9` onto the same `p100` key — the last one
            // parsed won, and the tail percentiles a run was configured to
            // measure were never compared at all.
            percentiles.insert(percentile_key(p), ns);
        }
    }

    let mut endpoint_p99 = BTreeMap::new();
    if let Some(arr) = v.get("per_endpoint").and_then(Value::as_array) {
        for entry in arr {
            let (Some(name), Some(ns)) = (
                entry.get("endpoint").and_then(Value::as_str),
                entry.get("latency_p99").and_then(Value::as_f64),
            ) else {
                continue;
            };
            endpoint_p99.insert(name.to_string(), ns);
        }
    }

    Ok(Metrics {
        total,
        errors,
        rps,
        mean_ns,
        percentiles,
        endpoint_p99,
    })
}

fn error_rate(m: &Metrics) -> f64 {
    if m.total > 0.0 {
        m.errors / m.total
    } else {
        0.0
    }
}

/// Build the aggregate metric comparison rows (throughput, error rate, mean, percentiles).
fn compare_aggregate(base: &Metrics, cur: &Metrics, th: &Thresholds) -> Vec<MetricRow> {
    let mut rows = Vec::new();

    // Throughput (rps) — higher is better, gated by --min-throughput drop percent.
    rows.push(MetricRow {
        name: "throughput_rps".to_string(),
        baseline: base.rps,
        current: cur.rps,
        abs_delta: cur.rps - base.rps,
        pct_delta: pct_change(base.rps, cur.rps),
        threshold: th.min_throughput,
        verdict: verdict_for_metric(
            base.rps,
            cur.rps,
            Direction::HigherIsBetter,
            th.min_throughput,
        ),
    });

    // Error rate — compared in percentage points.
    let base_rate = error_rate(base);
    let cur_rate = error_rate(cur);
    rows.push(MetricRow {
        name: "error_rate".to_string(),
        baseline: base_rate,
        current: cur_rate,
        abs_delta: cur_rate - base_rate,
        pct_delta: (cur_rate - base_rate) * 100.0,
        threshold: th.max_error_rate_regression,
        verdict: verdict_for_error_rate(base_rate, cur_rate, th.max_error_rate_regression),
    });

    // Mean latency — lower is better.
    rows.push(MetricRow {
        name: "latency_mean".to_string(),
        baseline: base.mean_ns,
        current: cur.mean_ns,
        abs_delta: cur.mean_ns - base.mean_ns,
        pct_delta: pct_change(base.mean_ns, cur.mean_ns),
        threshold: th.max_latency_regression,
        verdict: verdict_for_metric(
            base.mean_ns,
            cur.mean_ns,
            Direction::LowerIsBetter,
            th.max_latency_regression,
        ),
    });

    // Percentiles — lower is better; every one present in BOTH reports, in
    // ascending order. A hardcoded `[p50, p90, p95, p99]` list silently skipped
    // whatever else `latency_percentiles` was configured to produce.
    let mut shared: Vec<(f64, &String)> = base
        .percentiles
        .keys()
        .filter(|k| cur.percentiles.contains_key(*k))
        .filter_map(|k| percentile_of_key(k).map(|p| (p, k)))
        .collect();
    shared.sort_by(|a, b| a.0.total_cmp(&b.0));

    for (_, key) in shared {
        if let (Some(&b), Some(&c)) = (base.percentiles.get(key), cur.percentiles.get(key)) {
            rows.push(MetricRow {
                name: format!("latency_{key}"),
                baseline: b,
                current: c,
                abs_delta: c - b,
                pct_delta: pct_change(b, c),
                threshold: th.max_latency_regression,
                verdict: verdict_for_metric(
                    b,
                    c,
                    Direction::LowerIsBetter,
                    th.max_latency_regression,
                ),
            });
        }
    }

    rows
}

/// Per-endpoint p99 comparison for endpoints present in both reports.
fn compare_endpoints(base: &Metrics, cur: &Metrics, th: &Thresholds) -> Vec<MetricRow> {
    let mut rows = Vec::new();
    for (name, &b) in &base.endpoint_p99 {
        if let Some(&c) = cur.endpoint_p99.get(name) {
            rows.push(MetricRow {
                name: format!("{name}/p99"),
                baseline: b,
                current: c,
                abs_delta: c - b,
                pct_delta: pct_change(b, c),
                threshold: th.max_latency_regression,
                verdict: verdict_for_metric(
                    b,
                    c,
                    Direction::LowerIsBetter,
                    th.max_latency_regression,
                ),
            });
        }
    }
    rows
}

fn is_latency(name: &str) -> bool {
    name.starts_with("latency_") || name.ends_with("/p99")
}

fn fmt_value(name: &str, v: f64) -> String {
    if is_latency(name) {
        format!("{:.3} ms", v / 1_000_000.0)
    } else if name == "error_rate" {
        format!("{:.3}%", v * 100.0)
    } else {
        format!("{v:.2}")
    }
}

fn fmt_pct(row: &MetricRow) -> String {
    if row.name == "error_rate" {
        format!("{:+.3} pts", row.pct_delta)
    } else if row.pct_delta.is_infinite() {
        "n/a".to_string()
    } else {
        format!("{:+.2}%", row.pct_delta)
    }
}

/// The verdict cell: an icon + label, colored (green improved, red regressed,
/// dim neutral pass) and right-padded to a fixed *visible* width. Padding is
/// applied to the plain text before styling so the ANSI escapes don't skew the
/// column alignment; color/TTY/NO_COLOR gating comes from `console` via the
/// `apif-report::style` helpers.
fn styled_verdict(verdict: Verdict) -> String {
    use crate::report::style::{PASS_ICON, dim_style, fail_style, pass_style};
    let (cell, style) = match verdict {
        Verdict::Improved => (format!("{PASS_ICON} IMPROVED"), pass_style()),
        Verdict::Regressed => ("✗ REGRESSED".to_string(), fail_style()),
        Verdict::Pass => ("PASS".to_string(), dim_style()),
    };
    style.apply_to(format!("{cell:>12}")).to_string()
}

fn render_console(agg: &[MetricRow], endpoints: &[MetricRow], pass: bool) -> String {
    use crate::report::style::{fail_style, header, pass_style, rule};
    use std::fmt::Write;

    // Total visible width of the "{:<20} {:>16} {:>16} {:>14} {:>12}" layout.
    const WIDTH: usize = 82;
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{}\n",
        header("Benchmark comparison (baseline → current)")
    );
    let _ = writeln!(
        out,
        "{:<20} {:>16} {:>16} {:>14} {:>12}",
        "METRIC", "BASELINE", "CURRENT", "DELTA", "VERDICT"
    );
    let _ = writeln!(out, "{}", rule('─', WIDTH));
    let render = |out: &mut String, r: &MetricRow| {
        // The verdict cell is styled+pre-padded, so it's appended after the
        // width-formatted numeric columns rather than through `{:>12}`.
        let _ = writeln!(
            out,
            "{:<20} {:>16} {:>16} {:>14} {}",
            r.name,
            fmt_value(&r.name, r.baseline),
            fmt_value(&r.name, r.current),
            fmt_pct(r),
            styled_verdict(r.verdict)
        );
    };
    for r in agg {
        render(&mut out, r);
    }
    if !endpoints.is_empty() {
        let _ = writeln!(out, "\n{}", header("Per-endpoint p99:"));
        for r in endpoints {
            render(&mut out, r);
        }
    }
    let _ = writeln!(out, "{}", rule('─', WIDTH));
    let summary = if pass {
        pass_style()
            .apply_to("PASS: no regressions beyond thresholds")
            .to_string()
    } else {
        fail_style()
            .apply_to("FAIL: one or more metrics regressed beyond thresholds")
            .to_string()
    };
    let _ = writeln!(out, "{summary}");
    out
}

fn row_to_json(r: &MetricRow) -> Value {
    json!({
        "name": r.name,
        "baseline": r.baseline,
        "current": r.current,
        "abs_delta": r.abs_delta,
        "pct_delta": if r.pct_delta.is_finite() { json!(r.pct_delta) } else { Value::Null },
        "threshold": r.threshold,
        "verdict": r.verdict.label().to_lowercase(),
    })
}

fn load_report(path: &std::path::Path) -> Result<Value> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read bench report `{}`", path.display()))?;
    serde_json::from_str::<Value>(&text)
        .with_context(|| format!("failed to parse `{}` as JSON", path.display()))
}

pub fn run(args: &BenchCompareArgs) -> Result<()> {
    let base_v = load_report(&args.baseline)?;
    let cur_v = load_report(&args.current)?;

    let base = extract_metrics(&base_v)
        .with_context(|| format!("baseline `{}`", args.baseline.display()))?;
    let cur =
        extract_metrics(&cur_v).with_context(|| format!("current `{}`", args.current.display()))?;

    let th = Thresholds {
        max_latency_regression: args.max_latency_regression,
        max_error_rate_regression: args.max_error_rate_regression,
        min_throughput: args.min_throughput,
    };

    let agg = compare_aggregate(&base, &cur, &th);
    let endpoints = compare_endpoints(&base, &cur, &th);

    let pass = overall_pass(&agg) && overall_pass(&endpoints);

    if args.format.eq_ignore_ascii_case("json") {
        let output = json!({
            "overall": if pass { "pass" } else { "fail" },
            "thresholds": {
                "max_latency_regression": th.max_latency_regression,
                "max_error_rate_regression": th.max_error_rate_regression,
                "min_throughput": th.min_throughput,
            },
            "metrics": agg.iter().map(row_to_json).collect::<Vec<_>>(),
            "per_endpoint": endpoints.iter().map(row_to_json).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        print!("{}", render_console(&agg, &endpoints, pass));
    }

    if !pass {
        std::process::exit(1);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metrics(rps: f64, total: f64, errors: f64, mean: f64, p99: f64) -> Metrics {
        let mut percentiles = BTreeMap::new();
        percentiles.insert("p50".to_string(), mean);
        percentiles.insert("p99".to_string(), p99);
        Metrics {
            total,
            errors,
            rps,
            mean_ns: mean,
            percentiles,
            endpoint_p99: BTreeMap::new(),
        }
    }

    fn thresholds() -> Thresholds {
        Thresholds {
            max_latency_regression: 10.0,
            max_error_rate_regression: 1.0,
            min_throughput: 5.0,
        }
    }

    #[test]
    fn render_console_shows_verdict_icons_and_styled_summary() {
        // Tests run non-TTY, so `console` emits no ANSI — assert on the plain
        // text the design-language rendering produces (icons + labels + a
        // header/rule frame + the pass/fail summary line).
        let rows = vec![
            MetricRow {
                name: "throughput_rps".to_string(),
                baseline: 100.0,
                current: 120.0,
                abs_delta: 20.0,
                pct_delta: 20.0,
                threshold: 5.0,
                verdict: Verdict::Improved,
            },
            MetricRow {
                name: "latency_p99".to_string(),
                baseline: 100.0,
                current: 150.0,
                abs_delta: 50.0,
                pct_delta: 50.0,
                threshold: 10.0,
                verdict: Verdict::Regressed,
            },
        ];
        let out = render_console(&rows, &[], false);
        assert!(out.contains("Benchmark comparison"));
        assert!(out.contains("VERDICT"));
        assert!(out.contains("✓ IMPROVED"));
        assert!(out.contains("✗ REGRESSED"));
        assert!(out.contains("FAIL: one or more metrics regressed"));
    }

    #[test]
    fn pct_change_basic_and_zero_baseline() {
        assert_eq!(pct_change(100.0, 110.0), 10.0);
        assert_eq!(pct_change(100.0, 90.0), -10.0);
        assert_eq!(pct_change(0.0, 0.0), 0.0);
        assert!(pct_change(0.0, 5.0).is_infinite());
    }

    #[test]
    fn latency_regression_beyond_threshold_fails() {
        // p99 up 20% with a 10% threshold -> regression.
        let v = verdict_for_metric(100.0, 120.0, Direction::LowerIsBetter, 10.0);
        assert_eq!(v, Verdict::Regressed);
    }

    #[test]
    fn latency_within_threshold_passes() {
        // p99 up 5% with a 10% threshold -> pass.
        let v = verdict_for_metric(100.0, 105.0, Direction::LowerIsBetter, 10.0);
        assert_eq!(v, Verdict::Pass);
    }

    #[test]
    fn latency_drop_improves() {
        let v = verdict_for_metric(100.0, 80.0, Direction::LowerIsBetter, 10.0);
        assert_eq!(v, Verdict::Improved);
    }

    #[test]
    fn throughput_drop_beyond_min_fails() {
        // rps drops 20% with a 5% tolerance -> regression.
        let v = verdict_for_metric(1000.0, 800.0, Direction::HigherIsBetter, 5.0);
        assert_eq!(v, Verdict::Regressed);
    }

    #[test]
    fn throughput_improvement_passes() {
        let v = verdict_for_metric(1000.0, 1200.0, Direction::HigherIsBetter, 5.0);
        assert_eq!(v, Verdict::Improved);
    }

    #[test]
    fn error_rate_points_regression() {
        // 0% -> 2% is 2 points, over a 1-point tolerance -> regression.
        assert_eq!(verdict_for_error_rate(0.0, 0.02, 1.0), Verdict::Regressed);
        // 0% -> 0.5% is within tolerance -> pass.
        assert_eq!(verdict_for_error_rate(0.0, 0.005, 1.0), Verdict::Pass);
        // fewer errors -> improved.
        assert_eq!(verdict_for_error_rate(0.02, 0.0, 1.0), Verdict::Improved);
    }

    #[test]
    fn aggregate_deltas_are_correct() {
        let base = metrics(1000.0, 1000.0, 0.0, 100.0, 200.0);
        let cur = metrics(950.0, 1000.0, 0.0, 110.0, 210.0);
        let rows = compare_aggregate(&base, &cur, &thresholds());
        let rps = rows.iter().find(|r| r.name == "throughput_rps").unwrap();
        assert_eq!(rps.abs_delta, -50.0);
        assert_eq!(rps.pct_delta, -5.0);
        let p99 = rows.iter().find(|r| r.name == "latency_p99").unwrap();
        assert_eq!(p99.abs_delta, 10.0);
        assert_eq!(p99.pct_delta, 5.0);
    }

    #[test]
    fn overall_pass_when_all_within_thresholds() {
        let base = metrics(1000.0, 1000.0, 0.0, 100.0, 200.0);
        let cur = metrics(980.0, 1000.0, 0.0, 105.0, 208.0); // rps -2%, latency +4/5%
        let rows = compare_aggregate(&base, &cur, &thresholds());
        assert!(overall_pass(&rows));
    }

    #[test]
    fn overall_fail_on_p99_regression() {
        let base = metrics(1000.0, 1000.0, 0.0, 100.0, 200.0);
        let cur = metrics(1000.0, 1000.0, 0.0, 100.0, 260.0); // p99 +30%
        let rows = compare_aggregate(&base, &cur, &thresholds());
        assert!(!overall_pass(&rows));
    }

    #[test]
    fn extract_metrics_reads_schema_fields() {
        let v = json!({
            "schema_version": "bench_report_schema_v1",
            "summary": {"count": 100, "errors": 2, "rps_observed": 500.0, "average_ns": 12345},
            "latency_distribution": [
                {"percentile": 50.0, "latency_ns": 10000},
                {"percentile": 99.0, "latency_ns": 90000}
            ],
            "per_endpoint": [
                {"endpoint": "svc.S/M", "latency_p99": 90000}
            ]
        });
        let m = extract_metrics(&v).unwrap();
        assert_eq!(m.total, 100.0);
        assert_eq!(m.errors, 2.0);
        assert_eq!(m.rps, 500.0);
        assert_eq!(m.percentiles.get("p50"), Some(&10000.0));
        assert_eq!(m.percentiles.get("p99"), Some(&90000.0));
        assert_eq!(m.endpoint_p99.get("svc.S/M"), Some(&90000.0));
        assert!((error_rate(&m) - 0.02).abs() < f64::EPSILON);
    }

    #[test]
    fn extract_metrics_errors_on_missing_metric() {
        // missing rps_observed -> clear error, no panic.
        let v = json!({"summary": {"count": 10, "errors": 0, "average_ns": 5}});
        let err = extract_metrics(&v).unwrap_err();
        assert!(err.to_string().contains("rps_observed"));
    }

    #[test]
    fn extract_metrics_rejects_non_report() {
        let v = json!({"hello": "world"});
        let err = extract_metrics(&v).unwrap_err();
        assert!(err.to_string().contains("not a bench report"));
    }
}
