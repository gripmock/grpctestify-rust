use super::SourceReader;
use super::error::SourceError;
use super::row::SourceRow;
use anyhow::Result;
use std::io::{BufRead, BufReader, Read, Seek};

pub struct NdjsonReader<R> {
    reader: BufReader<R>,
    headers: Vec<String>,
    pending_first: Option<SourceRow>,
    row_number: usize,
    finished: bool,
}

impl<R: Read> NdjsonReader<R> {
    pub fn new(reader: BufReader<R>) -> Self {
        Self {
            reader,
            headers: Vec::new(),
            pending_first: None,
            row_number: 0,
            finished: false,
        }
    }

    fn discover_headers_from_first_row(&mut self) -> Result<()> {
        if !self.headers.is_empty() {
            return Ok(());
        }

        loop {
            let mut line = String::new();
            let bytes = self.reader.read_line(&mut line)?;
            if bytes == 0 {
                return Ok(());
            }

            let trimmed = line.trim_ascii();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            let value: serde_json::Value = serde_json::from_str(trimmed)
                .map_err(|e| SourceError::InvalidJson(1, e.to_string()))?;

            let obj = match &value {
                serde_json::Value::Object(m) => m,
                _ => return Err(SourceError::InvalidJson(1, "expected JSON object".into()).into()),
            };

            let mut keys: Vec<String> = obj.keys().cloned().collect();
            keys.sort();
            self.headers = keys;
            self.row_number = 1;

            let values: Vec<String> = self
                .headers
                .iter()
                .map(|k| json_value_to_string(obj.get(k)))
                .collect();
            self.pending_first = Some(SourceRow::new(&self.headers, values));
            return Ok(());
        }
    }
}

impl<R: Read + Send> SourceReader for NdjsonReader<R> {
    fn next_row(&mut self) -> Result<Option<SourceRow>> {
        if self.finished {
            return Ok(None);
        }

        if let Some(row) = self.pending_first.take() {
            return Ok(Some(row));
        }

        self.discover_headers_from_first_row()?;
        if let Some(row) = self.pending_first.take() {
            return Ok(Some(row));
        }

        loop {
            let mut line = String::new();
            let bytes = self.reader.read_line(&mut line)?;
            if bytes == 0 {
                self.finished = true;
                return Ok(None);
            }

            let trimmed = line.trim_ascii();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            self.row_number += 1;

            let value: serde_json::Value = serde_json::from_str(trimmed)
                .map_err(|e| SourceError::InvalidJson(self.row_number, e.to_string()))?;

            let values: Vec<String> = self
                .headers
                .iter()
                .map(|k| {
                    value
                        .get(k)
                        .map(|v| json_value_to_string(Some(v)))
                        .unwrap_or_default()
                })
                .collect();

            return Ok(Some(SourceRow::new(&self.headers, values)));
        }
    }

    fn headers(&self) -> &[String] {
        &self.headers
    }

    fn reset(&mut self) -> Result<()> {
        Ok(())
    }
}

impl<R: Read + Seek + Send> NdjsonReader<R> {
    pub fn reset_seekable(&mut self) -> Result<()> {
        self.reader.seek(std::io::SeekFrom::Start(0))?;
        self.headers.clear();
        self.pending_first = None;
        self.row_number = 0;
        self.finished = false;
        Ok(())
    }
}

fn json_value_to_string(v: Option<&serde_json::Value>) -> String {
    match v {
        None => String::new(),
        Some(serde_json::Value::Null) => String::new(),
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Number(n)) => n.to_string(),
        Some(serde_json::Value::Bool(b)) => b.to_string(),
        Some(other) => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn cursor(data: &str) -> BufReader<Cursor<&str>> {
        BufReader::new(Cursor::new(data))
    }

    #[test]
    fn ndjson_reads_basic_rows() {
        let data = "{\"id\":1,\"name\":\"Alice\"}\n{\"id\":2,\"name\":\"Bob\"}\n";
        let mut reader = NdjsonReader::new(cursor(data));

        let row1 = reader.next_row().unwrap().unwrap();
        assert_eq!(row1.get("id"), Some("1"));
        assert_eq!(row1.get("name"), Some("Alice"));

        let row2 = reader.next_row().unwrap().unwrap();
        assert_eq!(row2.get("name"), Some("Bob"));

        assert!(reader.next_row().unwrap().is_none());
    }

    #[test]
    fn ndjson_discover_headers_sorted() {
        let data = "{\"z\":1,\"a\":2}\n";
        let mut reader = NdjsonReader::new(cursor(data));
        let _row = reader.next_row().unwrap().unwrap();
        assert_eq!(reader.headers(), &["a", "z"]);
    }

    #[test]
    fn ndjson_skips_comments_and_blank_lines() {
        let data = "# comment\n\n{\"id\":1,\"val\":\"hello\"}\n\n{\"id\":2,\"val\":\"world\"}\n";
        let mut reader = NdjsonReader::new(cursor(data));
        let row1 = reader.next_row().unwrap().unwrap();
        assert_eq!(row1.get("val"), Some("hello"));
        let row2 = reader.next_row().unwrap().unwrap();
        assert_eq!(row2.get("val"), Some("world"));
        assert!(reader.next_row().unwrap().is_none());
    }

    #[test]
    fn ndjson_empty_returns_none() {
        let data = "";
        let mut reader = NdjsonReader::new(cursor(data));
        assert!(reader.next_row().unwrap().is_none());
    }

    #[test]
    fn ndjson_handles_null_and_bool() {
        let data = "{\"s\":null,\"b\":true,\"n\":42}\n";
        let mut reader = NdjsonReader::new(cursor(data));
        let row = reader.next_row().unwrap().unwrap();
        assert_eq!(row.get("s"), Some(""));
        assert_eq!(row.get("b"), Some("true"));
        assert_eq!(row.get("n"), Some("42"));
    }

    #[test]
    fn ndjson_invalid_json_errors() {
        let data = "{\"id\":1}\nnot json\n";
        let mut reader = NdjsonReader::new(cursor(data));
        let _row1 = reader.next_row().unwrap().unwrap();
        let result = reader.next_row();
        assert!(result.is_err());
    }

    #[test]
    fn ndjson_missing_key_defaults_empty() {
        let data = "{\"id\":1,\"name\":\"Alice\"}\n{\"id\":2}\n";
        let mut reader = NdjsonReader::new(cursor(data));
        let _row1 = reader.next_row().unwrap();
        let row2 = reader.next_row().unwrap().unwrap();
        assert_eq!(row2.get("name"), Some(""));
        assert_eq!(row2.get("id"), Some("2"));
    }
}
