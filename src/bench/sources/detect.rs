use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceFormat {
    Csv,
    Tsv,
    Ndjson,
}

impl std::fmt::Display for SourceFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SourceFormat::Csv => write!(f, "csv"),
            SourceFormat::Tsv => write!(f, "tsv"),
            SourceFormat::Ndjson => write!(f, "ndjson"),
        }
    }
}

impl std::str::FromStr for SourceFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim_ascii().to_ascii_lowercase().as_str() {
            "csv" => Ok(SourceFormat::Csv),
            "tsv" => Ok(SourceFormat::Tsv),
            "ndjson" | "json" | "jsonl" => Ok(SourceFormat::Ndjson),
            other => Err(format!("unknown source format: {other}")),
        }
    }
}

pub fn detect_format(path: &Path) -> Result<SourceFormat, apif_source_error::SourceError> {
    let filename = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default()
        .to_ascii_lowercase();

    if filename.ends_with(".tsv") || filename.ends_with(".tab") {
        return Ok(SourceFormat::Tsv);
    }
    if filename.ends_with(".ndjson") || filename.ends_with(".jsonl") {
        return Ok(SourceFormat::Ndjson);
    }
    if filename.ends_with(".json") {
        let content = std::fs::read_to_string(path).map_err(|e| {
            apif_source_error::SourceError::FileOpenFailed(path.display().to_string(), e)
        })?;
        return Ok(detect_format_from_content(&content));
    }
    if filename.ends_with(".csv") {
        return Ok(SourceFormat::Csv);
    }

    let content = std::fs::read_to_string(path).map_err(|e| {
        apif_source_error::SourceError::FileOpenFailed(path.display().to_string(), e)
    })?;
    Ok(detect_format_from_content(&content))
}

pub fn detect_format_from_content(content: &str) -> SourceFormat {
    let first_line = content.lines().next().unwrap_or("");

    if first_line.trim_start().starts_with('{') {
        return SourceFormat::Ndjson;
    }

    if first_line.contains('\t') && !first_line.contains(',') {
        return SourceFormat::Tsv;
    }

    SourceFormat::Csv
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_csv_extension() {
        assert_eq!(
            detect_format(Path::new("data/users.csv")).unwrap(),
            SourceFormat::Csv
        );
    }

    #[test]
    fn detect_tsv_extension() {
        assert_eq!(
            detect_format(Path::new("data/data.tsv")).unwrap(),
            SourceFormat::Tsv
        );
        assert_eq!(
            detect_format(Path::new("data/data.tab")).unwrap(),
            SourceFormat::Tsv
        );
    }

    #[test]
    fn detect_ndjson_extension() {
        assert_eq!(
            detect_format(Path::new("data/logs.ndjson")).unwrap(),
            SourceFormat::Ndjson
        );
        assert_eq!(
            detect_format(Path::new("data/logs.jsonl")).unwrap(),
            SourceFormat::Ndjson
        );
    }

    #[test]
    fn detect_from_content_json() {
        assert_eq!(
            detect_format_from_content("{\"id\":1}\n{\"id\":2}\n"),
            SourceFormat::Ndjson
        );
    }

    #[test]
    fn detect_from_content_tsv() {
        assert_eq!(
            detect_format_from_content("id\tname\n1\tAlice\n"),
            SourceFormat::Tsv
        );
    }

    #[test]
    fn detect_from_content_csv_default() {
        assert_eq!(
            detect_format_from_content("id,name\n1,Alice\n"),
            SourceFormat::Csv
        );
    }

    #[test]
    fn format_from_str() {
        assert_eq!("csv".parse::<SourceFormat>(), Ok(SourceFormat::Csv));
        assert_eq!("tsv".parse::<SourceFormat>(), Ok(SourceFormat::Tsv));
        assert_eq!("ndjson".parse::<SourceFormat>(), Ok(SourceFormat::Ndjson));
        assert_eq!("json".parse::<SourceFormat>(), Ok(SourceFormat::Ndjson));
        assert_eq!("jsonl".parse::<SourceFormat>(), Ok(SourceFormat::Ndjson));
        assert!("unknown".parse::<SourceFormat>().is_err());
    }

    #[test]
    fn format_display() {
        assert_eq!(format!("{}", SourceFormat::Csv), "csv");
        assert_eq!(format!("{}", SourceFormat::Tsv), "tsv");
        assert_eq!(format!("{}", SourceFormat::Ndjson), "ndjson");
    }
}
