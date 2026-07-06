use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const BENCH_REPORT_SCHEMA_VERSION: &str = "bench_report_schema_v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchRunInfo {
    pub started_at: i64,
    pub ended_at: i64,
    pub end_reason: String,
    pub tool: String,
    pub tool_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BenchOptionValue {
    pub value: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BenchSummary {
    pub count: u64,
    pub ok: u64,
    pub errors: u64,
    pub total_ns: u64,
    pub average_ns: u64,
    pub fastest_ns: u64,
    pub slowest_ns: u64,
    pub rps_observed: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchPercentile {
    pub percentile: f64,
    pub latency_ns: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchHistogramBucket {
    pub lower_ns: u64,
    pub upper_ns: u64,
    pub count: u64,
    pub frequency: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchThresholdResult {
    pub metric: String,
    pub expr: String,
    pub passed: bool,
    pub actual: String,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchDetail {
    pub timestamp: i64,
    pub latency_ns: u64,
    pub status: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceRuntimeStats {
    pub dimension_lookups: u64,
    pub dimension_hits: u64,
    pub dimension_misses: u64,
    pub in_memory_lookups: u64,
    pub indexed_lookups: u64,
    pub index_fallbacks: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourcesRuntime {
    pub source_stats: BTreeMap<String, SourceRuntimeStats>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchReport {
    pub schema_version: String,
    pub run: BenchRunInfo,
    pub options_resolved: BTreeMap<String, BenchOptionValue>,
    pub summary: BenchSummary,
    pub latency_distribution: Vec<BenchPercentile>,
    pub histogram: Vec<BenchHistogramBucket>,
    pub grpc_status_distribution: BTreeMap<String, u64>,
    pub error_distribution: BTreeMap<String, u64>,
    pub threshold_evaluation: Vec<BenchThresholdResult>,
    pub details: Vec<BenchDetail>,
    pub tags: BTreeMap<String, String>,
    pub sources_runtime: Option<SourcesRuntime>,
    pub per_endpoint: Vec<PerEndpointSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerEndpointSummary {
    pub endpoint: String,
    pub count: u64,
    pub errors: u64,
    pub latency_p50: u64,
    pub latency_p90: u64,
    pub latency_p95: u64,
    pub latency_p99: u64,
}

impl BenchReport {
    pub fn new(run: BenchRunInfo) -> Self {
        Self {
            schema_version: BENCH_REPORT_SCHEMA_VERSION.to_string(),
            run,
            options_resolved: BTreeMap::new(),
            summary: BenchSummary::default(),
            latency_distribution: Vec::new(),
            histogram: Vec::new(),
            grpc_status_distribution: BTreeMap::new(),
            error_distribution: BTreeMap::new(),
            threshold_evaluation: Vec::new(),
            details: Vec::new(),
            tags: BTreeMap::new(),
            sources_runtime: None,
            per_endpoint: Vec::new(),
        }
    }

    pub fn thresholds_passed(&self) -> bool {
        self.threshold_evaluation.iter().all(|t| t.passed)
    }

    pub fn histogram_details(&self, bucket_count: usize) -> Vec<HistogramDetail> {
        if self.histogram.is_empty() || bucket_count == 0 {
            return vec![];
        }

        let total = self.summary.count as f64;
        if total == 0.0 {
            return vec![];
        }

        let mut result = Vec::with_capacity(bucket_count);
        let step = (self.histogram.len() / bucket_count).max(1);
        for i in (0..self.histogram.len()).step_by(step).take(bucket_count) {
            let bucket = &self.histogram[i];
            result.push(HistogramDetail {
                bucket_index: i,
                lower_ns: bucket.lower_ns,
                upper_ns: bucket.upper_ns,
                count: bucket.count,
                cumulative_count: self.histogram[..=i].iter().map(|b| b.count).sum::<u64>(),
                frequency: bucket.frequency,
                cumulative_frequency: self.histogram[..=i]
                    .iter()
                    .map(|b| b.frequency)
                    .sum::<f64>(),
                percentile_estimate: (bucket.frequency * 100.0).min(100.0),
            });
        }
        result
    }

    pub fn latency_percentile(&self, percentile: f64) -> Option<u64> {
        if self.latency_distribution.is_empty() {
            return None;
        }
        self.latency_distribution
            .iter()
            .find(|p| (p.percentile - percentile).abs() < f64::EPSILON)
            .map(|p| p.latency_ns)
    }

    pub fn histogram_bucket_at(&self, percentile: f64) -> Option<&BenchHistogramBucket> {
        if self.histogram.is_empty() || percentile <= 0.0 || percentile > 100.0 {
            return None;
        }
        let target_cumulative = percentile / 100.0;
        let mut cumulative = 0.0;
        for bucket in &self.histogram {
            cumulative += bucket.frequency;
            if cumulative >= target_cumulative {
                return Some(bucket);
            }
        }
        self.histogram.last()
    }
}

pub struct HistogramDetail {
    pub bucket_index: usize,
    pub lower_ns: u64,
    pub upper_ns: u64,
    pub count: u64,
    pub cumulative_count: u64,
    pub frequency: f64,
    pub cumulative_frequency: f64,
    pub percentile_estimate: f64,
}

impl BenchReport {
    pub fn to_prometheus_summary(&self) -> String {
        let mut lines = Vec::new();
        lines.push("# TYPE grpctestify_bench_count gauge".to_string());
        lines.push(format!("grpctestify_bench_count {}", self.summary.count));
        lines.push("# TYPE grpctestify_bench_total_ns gauge".to_string());
        lines.push(format!(
            "grpctestify_bench_total_ns {}",
            self.summary.total_ns
        ));
        lines.push("# TYPE grpctestify_bench_average_ns gauge".to_string());
        lines.push(format!(
            "grpctestify_bench_average_ns {}",
            self.summary.average_ns
        ));
        lines.push("# TYPE grpctestify_bench_fastest_ns gauge".to_string());
        lines.push(format!(
            "grpctestify_bench_fastest_ns {}",
            self.summary.fastest_ns
        ));
        lines.push("# TYPE grpctestify_bench_slowest_ns gauge".to_string());
        lines.push(format!(
            "grpctestify_bench_slowest_ns {}",
            self.summary.slowest_ns
        ));
        lines.push("# TYPE grpctestify_bench_rps_observed gauge".to_string());
        lines.push(format!(
            "grpctestify_bench_rps_observed {}",
            self.summary.rps_observed
        ));

        lines.push("# TYPE grpctestify_bench_threshold_passed gauge".to_string());
        for t in &self.threshold_evaluation {
            let passed = if t.passed { 1 } else { 0 };
            lines.push(format!(
                "grpctestify_bench_threshold_passed{{metric=\"{}\",expr=\"{}\"}} {}",
                escape_prometheus_label(&t.metric),
                escape_prometheus_label(&t.expr),
                passed
            ));
        }

        lines.join("\n")
    }

    pub fn to_summary_text(&self, compact: bool) -> String {
        let mut out = String::new();

        out.push_str("Summary:\n");
        out.push_str(&format!("  Count:        {}\n", self.summary.count));
        out.push_str(&format!(
            "  Total:        {}\n",
            format_ns(self.summary.total_ns)
        ));
        out.push_str(&format!(
            "  Slowest:      {}\n",
            format_ns(self.summary.slowest_ns)
        ));
        out.push_str(&format!(
            "  Fastest:      {}\n",
            format_ns(self.summary.fastest_ns)
        ));
        out.push_str(&format!(
            "  Average:      {}\n",
            format_ns(self.summary.average_ns)
        ));
        out.push_str(&format!(
            "  Requests/sec: {:.2}\n",
            self.summary.rps_observed
        ));

        if !self.threshold_evaluation.is_empty() {
            let passed = self
                .threshold_evaluation
                .iter()
                .filter(|t| t.passed)
                .count();
            let total = self.threshold_evaluation.len();
            out.push_str(&format!("  Thresholds:   {}/{} passed\n", passed, total));
        }

        if compact {
            out.push_str("\nLatency distribution:\n");
            for p in &self.latency_distribution {
                out.push_str(&format!(
                    "  {:>5.2}% in {}\n",
                    p.percentile,
                    format_ns(p.latency_ns)
                ));
            }
            append_status_and_errors(&mut out, self);
            return out;
        }

        out.push_str("\nResponse time histogram:\n");
        for bucket in &self.histogram {
            let mark = if bucket.upper_ns > 0 {
                format_histogram_mark_ms(bucket.upper_ns)
            } else {
                format_histogram_mark_ms(bucket.lower_ns)
            };
            let bars = histogram_bars(bucket.frequency);
            out.push_str(&format!("  {:<12} [{:<5}] |{}\n", mark, bucket.count, bars));
        }

        out.push_str("\nLatency distribution:\n");
        for p in &self.latency_distribution {
            out.push_str(&format!(
                "  {} in {}\n",
                format_percentile(p.percentile),
                format_ns(p.latency_ns)
            ));
        }

        append_status_and_errors(&mut out, self);
        out
    }
}

fn append_status_and_errors(out: &mut String, report: &BenchReport) {
    out.push_str("\nStatus code distribution:\n");
    if report.grpc_status_distribution.is_empty() {
        out.push_str("  (none)\n");
    } else {
        let total = report.summary.count.max(1);
        for (status, count) in &report.grpc_status_distribution {
            out.push_str(&format!("  [{}]   {} responses\n", status, count));
            let _ = total;
        }
    }

    out.push_str("Error distribution:\n");
    if report.error_distribution.is_empty() {
        out.push_str("  (none)\n");
    } else {
        for (err, count) in &report.error_distribution {
            out.push_str(&format!("  [{}] {}\n", count, err));
        }
    }

    if !report.per_endpoint.is_empty() {
        out.push_str("\nPer-endpoint breakdown:\n");
        for ep in &report.per_endpoint {
            out.push_str(&format!(
                "  {}: {} req, {} err, p50={} p90={} p95={} p99={}\n",
                ep.endpoint,
                ep.count,
                ep.errors,
                format_ns(ep.latency_p50),
                format_ns(ep.latency_p90),
                format_ns(ep.latency_p95),
                format_ns(ep.latency_p99),
            ));
        }
    }
}

fn format_ns(ns: u64) -> String {
    if ns >= 1_000_000_000 {
        format!("{:.2} s", ns as f64 / 1_000_000_000.0)
    } else if ns >= 1_000_000 {
        format!("{:.2} ms", ns as f64 / 1_000_000.0)
    } else if ns >= 1_000 {
        format!("{:.2} us", ns as f64 / 1_000.0)
    } else {
        format!("{} ns", ns)
    }
}

fn histogram_bars(frequency: f64) -> String {
    let n = (frequency * 40.0).round() as usize;
    "#".repeat(n)
}

fn format_histogram_mark_ms(ns: u64) -> String {
    format!("{:.3}", ns as f64 / 1_000_000.0)
}

fn format_percentile(p: f64) -> String {
    if (p - p.round()).abs() < f64::EPSILON {
        format!("{:>3.0} %", p)
    } else {
        format!("{:>5.2} %", p)
    }
}

fn escape_prometheus_label(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_report() -> BenchReport {
        let mut report = BenchReport::new(BenchRunInfo {
            started_at: 1,
            ended_at: 2,
            end_reason: "normal".to_string(),
            tool: "grpctestify".to_string(),
            tool_version: "1.0.0".to_string(),
        });
        report.summary = BenchSummary {
            count: 100,
            ok: 99,
            errors: 1,
            total_ns: 1_000_000,
            average_ns: 10_000,
            fastest_ns: 1_000,
            slowest_ns: 100_000,
            rps_observed: 500.0,
        };
        report.threshold_evaluation.push(BenchThresholdResult {
            metric: "latency_ms.p(95)".to_string(),
            expr: "<120".to_string(),
            passed: true,
            actual: "115".to_string(),
            reason: None,
        });
        report
    }

    #[test]
    fn test_thresholds_passed_true() {
        let report = sample_report();
        assert!(report.thresholds_passed());
    }

    #[test]
    fn test_prometheus_summary_contains_core_metrics() {
        let report = sample_report();
        let text = report.to_prometheus_summary();
        assert!(text.contains("grpctestify_bench_count 100"));
        assert!(text.contains("grpctestify_bench_rps_observed 500"));
        assert!(text.contains("grpctestify_bench_threshold_passed"));
    }

    #[test]
    fn test_summary_text_contains_ghz_like_sections() {
        let mut report = sample_report();
        report.latency_distribution = vec![
            BenchPercentile {
                percentile: 50.0,
                latency_ns: 10_000_000,
            },
            BenchPercentile {
                percentile: 95.0,
                latency_ns: 20_000_000,
            },
        ];
        report.histogram = vec![BenchHistogramBucket {
            lower_ns: 0,
            upper_ns: 10_000_000,
            count: 80,
            frequency: 0.8,
        }];
        report.grpc_status_distribution.insert("OK".to_string(), 99);
        report
            .error_distribution
            .insert("rpc error: code = Internal".to_string(), 1);

        let text = report.to_summary_text(false);
        assert!(text.contains("Summary:"));
        assert!(text.contains("Response time histogram:"));
        assert!(text.contains("Latency distribution:"));
        assert!(text.contains("Status code distribution:"));
        assert!(text.contains("Error distribution:"));
    }

    #[test]
    fn test_summary_text_compact_omits_histogram() {
        let report = sample_report();
        let text = report.to_summary_text(true);
        assert!(text.contains("Summary:"));
        assert!(!text.contains("Response time histogram:"));
    }
}
