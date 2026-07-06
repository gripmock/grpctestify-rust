use std::collections::HashMap;
use std::sync::LazyLock;

pub const BENCH_NUMERIC_KEYS: &[&str] = &[
    "concurrency",
    "requests",
    "max_rps",
    "connections",
    "cpus",
    "skip_first",
    "load_start",
    "load_step",
    "load_end",
];

pub const BENCH_DURATION_KEYS: &[&str] = &[
    "max_duration",
    "connect_timeout",
    "keepalive",
    "ramp_up",
    "warmup",
    "duration",
    "cache_ttl",
    "load_step_duration",
    "load_max_duration",
    "progress_interval",
];

pub const BENCH_BOOLEAN_KEYS: &[&str] = &["no_assert", "count_errors_in_latency"];

pub const BENCH_DIRECT_KEYS: &[&str] = &[
    "mode",
    "profile",
    "load_schedule",
    "name",
    "assert_mode",
    "duration_stop",
    "sample_rate",
    "cache",
    "latency_percentiles",
];

pub const BENCH_MODE_VALUES: &[&str] = &["fixed", "stepping", "adaptive", "closed", "open"];
pub const BENCH_LOAD_SCHEDULE_VALUES: &[&str] = &["const", "step", "line"];
pub const BENCH_DURATION_STOP_VALUES: &[&str] = &["close", "wait", "ignore"];
pub const BENCH_ASSERT_MODE_VALUES: &[&str] =
    &["full", "sampled", "off", "fail_fast", "collect_all", "skip"];
pub const BENCH_CACHE_VALUES: &[&str] = &["on", "off", "refresh", "true", "false", "1", "0"];

pub const BENCH_COMPOUND_KEYS: &[&str] = &["sources"];

pub fn supported_bench_keys() -> Vec<&'static str> {
    let mut keys: Vec<&'static str> = Vec::new();
    keys.extend_from_slice(BENCH_DIRECT_KEYS);
    keys.extend_from_slice(BENCH_NUMERIC_KEYS);
    keys.extend_from_slice(BENCH_DURATION_KEYS);
    keys.extend_from_slice(BENCH_BOOLEAN_KEYS);
    keys.extend_from_slice(BENCH_COMPOUND_KEYS);
    keys.push("thresholds.*");
    keys.sort_unstable();
    keys.dedup();
    keys
}

pub fn bench_keys_canonical_order() -> Vec<&'static str> {
    let mut keys = supported_bench_keys();
    keys.sort_by(|a, b| {
        bench_key_rank(a)
            .cmp(&bench_key_rank(b))
            .then_with(|| a.cmp(b))
    });
    keys
}

pub fn is_allowed_value(value: &str, allowed: &[&str]) -> bool {
    let normalized = value.trim().to_ascii_lowercase();
    allowed.iter().any(|v| *v == normalized)
}

pub fn allowed_values_message(allowed: &[&str]) -> String {
    allowed.join(", ")
}

pub fn bench_key_detail(key: &str) -> String {
    match key {
        "mode" => format!(
            "Runtime mode ({})",
            allowed_values_message(BENCH_MODE_VALUES)
        ),
        "profile" => "Bench profile label (e.g. smoke, stress, soak)".to_string(),
        "concurrency" => "Worker concurrency".to_string(),
        "requests" => "Total requests stop condition".to_string(),
        "duration" => "Duration stop condition (e.g. 30s)".to_string(),
        "max_duration" => "Hard duration cap in requests mode".to_string(),
        "ramp_up" => "Ramp-up duration".to_string(),
        "warmup" => "Warmup duration".to_string(),
        "max_rps" => "Max requests per second".to_string(),
        "load_schedule" => format!(
            "Load schedule ({})",
            allowed_values_message(BENCH_LOAD_SCHEDULE_VALUES)
        ),
        "load_start" => "Schedule start RPS".to_string(),
        "load_step" => "Schedule step delta RPS".to_string(),
        "load_end" => "Schedule end RPS".to_string(),
        "load_step_duration" => "Duration per schedule step".to_string(),
        "load_max_duration" => "Maximum schedule duration".to_string(),
        "progress_interval" => "Progress heartbeat interval".to_string(),
        "connections" => "Number of transport connections".to_string(),
        "connect_timeout" => "gRPC dial timeout".to_string(),
        "keepalive" => "Transport keepalive interval".to_string(),
        "cpus" => "CPU pinning hint".to_string(),
        "name" => "Run name metadata".to_string(),
        "assert_mode" => format!(
            "Assertion mode ({})",
            allowed_values_message(BENCH_ASSERT_MODE_VALUES)
        ),
        "no_assert" => "Disable assertions for transport-only benchmark".to_string(),
        "duration_stop" => format!(
            "In-flight behavior at duration deadline ({})",
            allowed_values_message(BENCH_DURATION_STOP_VALUES)
        ),
        "sample_rate" => "Sampling rate for sampled assert/details".to_string(),
        "cache" => format!(
            "Cache mode ({})",
            allowed_values_message(BENCH_CACHE_VALUES)
        ),
        "skip_first" => "Skip first N samples from stats".to_string(),
        "count_errors_in_latency" => "Include failed calls in latency stats".to_string(),
        "latency_percentiles" => "Comma-separated percentile list".to_string(),
        "sources" => "Data source definitions for bench (file, format, index)".to_string(),
        "cache_ttl" => "Cache TTL duration".to_string(),
        "thresholds.*" => "Threshold expressions map".to_string(),
        _ => "BENCH option".to_string(),
    }
}

pub fn bench_aliases(_key: &str) -> &'static [&'static str] {
    &[]
}

pub fn canonical_bench_key(key: &str) -> Option<&'static str> {
    for canonical in supported_bench_keys() {
        if canonical == "thresholds.*" {
            continue;
        }
        if key == canonical {
            return Some(canonical);
        }
        if bench_aliases(canonical).contains(&key) {
            return Some(canonical);
        }
    }
    None
}

pub fn is_known_bench_key(key: &str) -> bool {
    if key == "thresholds" || key.starts_with("thresholds.") {
        return true;
    }
    canonical_bench_key(key).is_some()
}

pub fn suggest_bench_key(raw_key: &str) -> Option<&'static str> {
    let needle = raw_key.trim().to_ascii_lowercase().replace('-', "_");
    if needle.is_empty() || needle == "thresholds" || needle.starts_with("thresholds.") {
        return None;
    }

    let candidates = bench_keys_canonical_order();
    let mut best: Option<(&'static str, usize)> = None;

    for key in candidates {
        if key == "thresholds.*" {
            continue;
        }
        let key_norm = key.to_ascii_lowercase();
        let Some(score) = bounded_edit_distance(&needle, &key_norm, 3) else {
            continue;
        };
        match best {
            Some((_, best_score)) if score >= best_score => {}
            _ => best = Some((key, score)),
        }
    }

    best.map(|(k, _)| k)
}

fn bounded_edit_distance(a: &str, b: &str, max: usize) -> Option<usize> {
    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();
    if a_bytes == b_bytes {
        return Some(0);
    }
    if a_bytes.len().abs_diff(b_bytes.len()) > max {
        return None;
    }

    let mut prev: Vec<usize> = (0..=b_bytes.len()).collect();
    let mut curr = vec![0; b_bytes.len() + 1];

    for (i, &ac) in a_bytes.iter().enumerate() {
        curr[0] = i + 1;
        let mut row_min = curr[0];
        for (j, &bc) in b_bytes.iter().enumerate() {
            let cost = if ac == bc { 0 } else { 1 };
            let del = prev[j + 1] + 1;
            let ins = curr[j] + 1;
            let sub = prev[j] + cost;
            let v = del.min(ins).min(sub);
            curr[j + 1] = v;
            row_min = row_min.min(v);
        }
        if row_min > max {
            return None;
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    let dist = prev[b_bytes.len()];
    if dist <= max { Some(dist) } else { None }
}

pub fn bench_value<'a>(bench: &'a HashMap<String, String>, key: &str) -> Option<&'a String> {
    if let Some(v) = bench.get(key) {
        return Some(v);
    }
    for alias in bench_aliases(key) {
        if let Some(v) = bench.get(*alias) {
            return Some(v);
        }
    }
    None
}

pub fn bench_key_rank(key: &str) -> usize {
    let canonical_order = [
        "mode",
        "profile",
        "name",
        "concurrency",
        "requests",
        "duration",
        "max_duration",
        "ramp_up",
        "warmup",
        "max_rps",
        "load_schedule",
        "load_start",
        "load_step",
        "load_end",
        "load_step_duration",
        "load_max_duration",
        "progress_interval",
        "connections",
        "connect_timeout",
        "keepalive",
        "cpus",
        "assert_mode",
        "no_assert",
        "sample_rate",
        "duration_stop",
        "cache",
        "cache_ttl",
        "skip_first",
        "count_errors_in_latency",
        "latency_percentiles",
        "sources",
    ];

    if let Some((idx, _)) = canonical_order.iter().enumerate().find(|(_, k)| **k == key) {
        return idx;
    }
    if key.starts_with("thresholds.") || key == "thresholds" {
        return canonical_order.len();
    }
    canonical_order.len() + 1
}

/// Built-in benchmark profiles.
/// Each profile is a set of key-value pairs that override defaults.
pub static BUILTIN_PROFILES: LazyLock<HashMap<&'static str, HashMap<&'static str, &'static str>>> =
    LazyLock::new(|| {
        let mut m: HashMap<&str, HashMap<&str, &str>> = HashMap::new();

        let mut functional = HashMap::new();
        functional.insert("description", "Quick functional check");
        functional.insert("mode", "fixed");
        functional.insert("concurrency", "1");
        functional.insert("requests", "100");
        functional.insert("duration", "30s");
        m.insert("functional", functional);

        let mut load = HashMap::new();
        load.insert("description", "Stepped load test 50→200 RPS");
        load.insert("mode", "stepping");
        load.insert("concurrency", "10");
        load.insert("duration", "60s");
        load.insert("load_schedule", "step");
        load.insert("load_start", "50");
        load.insert("load_step", "10");
        load.insert("load_end", "200");
        load.insert("load_step_duration", "10s");
        m.insert("load", load);

        let mut stress = HashMap::new();
        stress.insert("description", "Linear stress test 10→500 RPS");
        stress.insert("mode", "stepping");
        stress.insert("concurrency", "50");
        stress.insert("duration", "120s");
        stress.insert("load_schedule", "line");
        stress.insert("load_start", "10");
        stress.insert("load_step", "5");
        stress.insert("load_end", "500");
        m.insert("stress", stress);

        let mut spike = HashMap::new();
        spike.insert("description", "Spike test: 10→500→10 RPS");
        spike.insert("mode", "fixed");
        spike.insert("concurrency", "100");
        spike.insert("duration", "60s");
        spike.insert("load_schedule", "spike");
        spike.insert("load_start", "10");
        spike.insert("load_spike_target", "500");
        spike.insert("load_spike_after", "30");
        spike.insert("load_spike_duration", "10");
        m.insert("spike", spike);

        let mut soak = HashMap::new();
        soak.insert("description", "Long-duration soak at 50 RPS");
        soak.insert("mode", "fixed");
        soak.insert("concurrency", "5");
        soak.insert("duration", "3600s");
        soak.insert("load_schedule", "const");
        soak.insert("load_start", "50");
        m.insert("soak", soak);

        m
    });

/// Apply a named profile to a BENCH section config.
/// Returns the list of (key, value) pairs that the profile defines.
pub fn apply_profile(
    name: &str,
) -> Vec<(&'static str, &'static str)> {
    BUILTIN_PROFILES
        .get(name)
        .map(|profile| profile.iter().map(|(k, v)| (*k, *v)).collect())
        .unwrap_or_default()
}

/// List all available profiles with their descriptions.
pub fn list_profiles() -> Vec<(&'static str, HashMap<&'static str, &'static str>)> {
    let mut result: Vec<(&str, HashMap<&str, &str>)> = BUILTIN_PROFILES
        .iter()
        .map(|(name, keys)| (*name, keys.clone()))
        .collect();
    result.sort_by(|a, b| a.0.cmp(b.0));
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_bench_keys_contains_scheduler_and_thresholds() {
        let keys = supported_bench_keys();
        assert!(keys.contains(&"load_schedule"));
        assert!(keys.contains(&"progress_interval"));
        assert!(keys.contains(&"thresholds.*"));
    }

    #[test]
    fn bench_key_rank_orders_core_fields_before_thresholds() {
        assert!(bench_key_rank("mode") < bench_key_rank("profile"));
        assert!(bench_key_rank("profile") < bench_key_rank("concurrency"));
        assert!(bench_key_rank("concurrency") < bench_key_rank("load_schedule"));
        assert!(bench_key_rank("load_schedule") < bench_key_rank("thresholds.p(95)"));
        assert!(bench_key_rank("thresholds.p(95)") < bench_key_rank("unknown_key"));
    }

    #[test]
    fn bench_value_uses_canonical_keys_only() {
        let mut bench = HashMap::new();
        bench.insert("load_schedule".to_string(), "step".to_string());
        bench.insert("progress_interval".to_string(), "2s".to_string());

        assert_eq!(
            bench_value(&bench, "load_schedule"),
            Some(&"step".to_string())
        );
        assert_eq!(
            bench_value(&bench, "progress_interval"),
            Some(&"2s".to_string())
        );
    }

    #[test]
    fn canonical_bench_key_resolves_known_keys() {
        assert_eq!(canonical_bench_key("load_schedule"), Some("load_schedule"));
        assert_eq!(
            canonical_bench_key("progress_interval"),
            Some("progress_interval")
        );
        assert_eq!(canonical_bench_key("mode"), Some("mode"));
        assert_eq!(canonical_bench_key("unknown_key"), None);
    }

    #[test]
    fn bench_keys_canonical_order_starts_with_mode() {
        let keys = bench_keys_canonical_order();
        assert_eq!(keys.first().copied(), Some("mode"));
        assert!(keys.iter().position(|k| *k == "thresholds.*").is_some());
    }

    #[test]
    fn suggest_bench_key_for_typo() {
        assert_eq!(suggest_bench_key("load_shedule"), Some("load_schedule"));
        assert_eq!(suggest_bench_key("duraton_stop"), Some("duration_stop"));
        assert_eq!(suggest_bench_key("thresholds"), None);
    }
}
