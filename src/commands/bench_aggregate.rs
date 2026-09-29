use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use crate::cli::args::BenchAggregateArgs;

pub const BENCH_MATRIX_SCHEMA_VERSION: &str = "bench_matrix_schema_v1";

#[derive(Debug)]
struct Row {
    run: String,
    concurrency: Option<u64>,
    summary: Value,
    percentiles: Vec<(f64, u64)>,
}

fn percentiles_of(v: &Value) -> Vec<(f64, u64)> {
    let Some(arr) = v.get("latency_distribution").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut out: Vec<(f64, u64)> = arr
        .iter()
        .filter_map(|e| {
            Some((
                e.get("percentile")?.as_f64()?,
                e.get("latency_ns")?.as_u64()?,
            ))
        })
        .collect();
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out
}

fn rows_of(run: &str, report: &Value) -> Result<Vec<Row>> {
    if report.get("summary").is_none() {
        bail!("{run} is not a bench report (no `summary` object)");
    }

    if let Some(levels) = report.get("levels").and_then(Value::as_array)
        && !levels.is_empty()
    {
        return Ok(levels
            .iter()
            .map(|l| Row {
                run: run.to_string(),
                concurrency: l.get("concurrency").and_then(Value::as_u64),
                summary: l.get("summary").cloned().unwrap_or(Value::Null),
                percentiles: percentiles_of(l),
            })
            .collect());
    }

    Ok(vec![Row {
        run: run.to_string(),
        concurrency: report
            .get("options_resolved")
            .and_then(|o| o.get("concurrency"))
            .and_then(|c| c.get("value"))
            .and_then(Value::as_str)
            .and_then(|s| s.parse().ok()),
        summary: report.get("summary").cloned().unwrap_or(Value::Null),
        percentiles: percentiles_of(report),
    }])
}

fn to_json(rows: &[Row]) -> Value {
    json!({
        "schema_version": BENCH_MATRIX_SCHEMA_VERSION,
        "points": rows
            .iter()
            .map(|r| json!({
                "run": r.run,
                "concurrency": r.concurrency,
                "summary": r.summary,
                "latency_distribution": r.percentiles
                    .iter()
                    .map(|(p, ns)| json!({ "percentile": p, "latency_ns": ns }))
                    .collect::<Vec<_>>(),
            }))
            .collect::<Vec<_>>(),
    })
}

fn summary_num(summary: &Value, key: &str) -> String {
    match summary.get(key) {
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

fn csv_field(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

fn to_csv(rows: &[Row]) -> String {
    let mut percentiles: Vec<f64> = Vec::new();
    for row in rows {
        for (p, _) in &row.percentiles {
            if !percentiles.iter().any(|q| q == p) {
                percentiles.push(*p);
            }
        }
    }
    percentiles.sort_by(|a, b| a.total_cmp(b));

    let mut header = vec![
        "run".to_string(),
        "concurrency".to_string(),
        "count".to_string(),
        "ok".to_string(),
        "errors".to_string(),
        "passed".to_string(),
        "failed".to_string(),
        "rps_observed".to_string(),
        "average_ns".to_string(),
    ];
    header.extend(percentiles.iter().map(|p| format!("p{p}_ns")));

    let mut out = String::new();
    out.push_str(&header.join(","));
    out.push('\n');

    for row in rows {
        let mut cells = vec![
            csv_field(&row.run),
            row.concurrency.map(|c| c.to_string()).unwrap_or_default(),
            summary_num(&row.summary, "count"),
            summary_num(&row.summary, "ok"),
            summary_num(&row.summary, "errors"),
            summary_num(&row.summary, "passed"),
            summary_num(&row.summary, "failed"),
            summary_num(&row.summary, "rps_observed"),
            summary_num(&row.summary, "average_ns"),
        ];
        for p in &percentiles {
            cells.push(
                row.percentiles
                    .iter()
                    .find(|(q, _)| q == p)
                    .map(|(_, ns)| ns.to_string())
                    .unwrap_or_default(),
            );
        }
        out.push_str(&cells.join(","));
        out.push('\n');
    }
    out
}

pub fn run(args: &BenchAggregateArgs) -> Result<()> {
    let mut rows = Vec::new();
    for path in &args.reports {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let report: Value = serde_json::from_str(&text)
            .with_context(|| format!("{} is not valid JSON", path.display()))?;
        rows.extend(rows_of(&path.to_string_lossy(), &report)?);
    }

    let rendered = match args.format.to_ascii_lowercase().as_str() {
        "json" => serde_json::to_string_pretty(&to_json(&rows))?,
        "csv" => to_csv(&rows),
        other => bail!("unsupported format '{other}'; expected json or csv"),
    };

    match &args.output {
        Some(path) => {
            std::fs::write(path, &rendered)
                .with_context(|| format!("failed to write {}", path.display()))?;
            eprintln!("Matrix written to: {}", path.display());
        }
        None => println!("{rendered}"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report_with_levels() -> Value {
        json!({
            "summary": { "count": 30, "ok": 30, "errors": 0, "passed": 30, "failed": 0,
                         "rps_observed": 900.0, "average_ns": 1000 },
            "levels": [
                { "concurrency": 1, "summary": { "count": 10, "rps_observed": 100.0 },
                  "latency_distribution": [{ "percentile": 99.9, "latency_ns": 7 }] },
                { "concurrency": 4, "summary": { "count": 20, "rps_observed": 400.0 },
                  "latency_distribution": [{ "percentile": 50.0, "latency_ns": 3 }] }
            ]
        })
    }

    #[test]
    fn a_sweep_contributes_one_point_per_level() {
        let rows = rows_of("sweep.json", &report_with_levels()).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].concurrency, Some(1));
        assert_eq!(rows[1].concurrency, Some(4));
    }

    #[test]
    fn a_plain_report_contributes_one_point() {
        let report = json!({
            "summary": { "count": 5, "rps_observed": 50.0 },
            "options_resolved": { "concurrency": { "value": "8", "source": "cli" } },
            "latency_distribution": [{ "percentile": 50.0, "latency_ns": 2 }]
        });
        let rows = rows_of("plain.json", &report).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].concurrency, Some(8));
    }

    #[test]
    fn a_non_report_is_rejected() {
        let err = rows_of("junk.json", &json!({ "hello": "world" })).unwrap_err();
        assert!(err.to_string().contains("not a bench report"));
    }

    #[test]
    fn csv_columns_cover_the_union_of_percentiles() {
        let rows = rows_of("sweep.json", &report_with_levels()).unwrap();
        let csv = to_csv(&rows);
        let mut lines = csv.lines();
        let header = lines.next().unwrap();
        assert!(header.ends_with("p50_ns,p99.9_ns"), "got: {header}");

        let first = lines.next().unwrap();
        assert!(first.ends_with(",,7"), "got: {first}");
    }

    #[test]
    fn csv_quotes_a_run_name_containing_a_comma() {
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_field("plain"), "plain");
    }
}
