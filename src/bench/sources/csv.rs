use super::SourceReader;
use anyhow::Result;
use source_error::SourceError;
use source_row::SourceRow;
use std::io::{BufReader, Read, Seek};

pub struct CsvReader<R> {
    reader: csv::Reader<BufReader<R>>,
    headers: Vec<String>,
    _delimiter: u8,
    row_number: usize,
    finished: bool,
}

impl<R: Read> CsvReader<R> {
    pub fn new(reader: BufReader<R>, delimiter: u8) -> Result<Self> {
        let mut csv_reader = csv::ReaderBuilder::new()
            .delimiter(delimiter)
            .comment(Some(b'#'))
            .has_headers(true)
            .flexible(false)
            .from_reader(reader);

        let headers = csv_reader
            .headers()
            .map_err(|e| anyhow::anyhow!("failed to read CSV header: {e}"))?
            .iter()
            .map(|h| h.to_string())
            .collect::<Vec<_>>();

        if headers.is_empty() {
            return Err(SourceError::EmptyFile("csv".into()).into());
        }

        let mut seen = std::collections::HashSet::new();
        for col in &headers {
            if !seen.insert(col.as_str()) {
                return Err(SourceError::DuplicateColumn(col.clone()).into());
            }
        }

        Ok(Self {
            reader: csv_reader,
            headers,
            _delimiter: delimiter,
            row_number: 1,
            finished: false,
        })
    }
}

impl<R: Read + Send> SourceReader for CsvReader<R> {
    fn next_row(&mut self) -> Result<Option<SourceRow>> {
        if self.finished {
            return Ok(None);
        }

        if let Some(result) = self.reader.records().next() {
            self.row_number += 1;
            let record = match result {
                Ok(r) => r,
                Err(e) => {
                    return Err(anyhow::anyhow!("CSV error at row {}: {e}", self.row_number));
                }
            };

            let values: Vec<String> = record.iter().map(|f| f.to_string()).collect();

            if values.len() != self.headers.len() {
                return Err(SourceError::FieldCountMismatch {
                    row: self.row_number,
                    fields: values.len(),
                    expected: self.headers.len(),
                }
                .into());
            }

            return Ok(Some(SourceRow::new(&self.headers, values)));
        }

        self.finished = true;
        Ok(None)
    }

    fn headers(&self) -> &[String] {
        &self.headers
    }

    fn reset(&mut self) -> Result<()> {
        Ok(())
    }
}

impl<R: Read + Seek + Send> CsvReader<R> {
    pub fn reset_seekable(&mut self) -> Result<()> {
        // csv::Reader doesn't expose the inner writer,
        // so we need to replace it entirely by seeking the raw reader
        Err(anyhow::anyhow!(
            "reset_seekable not supported with csv crate reader"
        ))
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
    fn csv_reads_basic_rows() {
        let data = "id,name,age\n1,Alice,30\n2,Bob,25\n";
        let mut reader = CsvReader::new(cursor(data), b',').unwrap();
        assert_eq!(reader.headers(), &["id", "name", "age"]);

        let row1 = reader.next_row().unwrap().unwrap();
        assert_eq!(row1.get("id"), Some("1"));
        assert_eq!(row1.get("name"), Some("Alice"));
        assert_eq!(row1.get("age"), Some("30"));

        let row2 = reader.next_row().unwrap().unwrap();
        assert_eq!(row2.get("name"), Some("Bob"));

        assert!(reader.next_row().unwrap().is_none());
    }

    #[test]
    fn csv_handles_quoted_fields() {
        let data = "id,name\n1,\"Smith, John\"\n2,\"O\"\"Brien\"\n";
        let mut reader = CsvReader::new(cursor(data), b',').unwrap();

        let row1 = reader.next_row().unwrap().unwrap();
        assert_eq!(row1.get("name"), Some("Smith, John"));

        let row2 = reader.next_row().unwrap().unwrap();
        assert_eq!(row2.get("name"), Some("O\"Brien"));
    }

    #[test]
    fn csv_skips_comments_and_blank_lines() {
        let data = "id,val\n# comment\n\n1,hello\n\n2,world\n";
        let mut reader = CsvReader::new(cursor(data), b',').unwrap();
        let row1 = reader.next_row().unwrap().unwrap();
        assert_eq!(row1.get("val"), Some("hello"));
        let row2 = reader.next_row().unwrap().unwrap();
        assert_eq!(row2.get("val"), Some("world"));
        assert!(reader.next_row().unwrap().is_none());
    }

    #[test]
    fn csv_empty_file_errors() {
        let data = "";
        let result = CsvReader::new(cursor(data), b',');
        assert!(result.is_err());
    }

    #[test]
    fn csv_duplicate_column_errors() {
        let data = "id,id,value\n1,2,3\n";
        let result = CsvReader::new(cursor(data), b',');
        assert!(result.is_err());
    }

    #[test]
    fn csv_field_count_mismatch_errors() {
        let data = "id,name\n1,Alice,extra\n";
        let mut reader = CsvReader::new(cursor(data), b',').unwrap();
        let result = reader.next_row();
        assert!(result.is_err());
    }

    #[test]
    fn csv_custom_delimiter() {
        let data = "id;name;age\n1;Alice;30\n";
        let mut reader = CsvReader::new(cursor(data), b';').unwrap();
        let row = reader.next_row().unwrap().unwrap();
        assert_eq!(row.get("name"), Some("Alice"));
    }
}
