use super::SourceReader;
use source_error::SourceError;
use source_row::SourceRow;
use anyhow::Result;
use std::io::{BufReader, Read, Seek};

pub struct TsvReader<R> {
    reader: csv::Reader<BufReader<R>>,
    headers: Vec<String>,
    row_number: usize,
    finished: bool,
}

impl<R: Read> TsvReader<R> {
    pub fn new(reader: BufReader<R>) -> Result<Self> {
        let mut csv_reader = csv::ReaderBuilder::new()
            .delimiter(b'\t')
            .comment(Some(b'#'))
            .has_headers(true)
            .flexible(false)
            .from_reader(reader);

        let headers = csv_reader
            .headers()
            .map_err(|e| anyhow::anyhow!("failed to read TSV header: {e}"))?
            .iter()
            .map(|h| h.to_string())
            .collect::<Vec<_>>();

        if headers.is_empty() {
            return Err(SourceError::EmptyFile("tsv".into()).into());
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
            row_number: 1,
            finished: false,
        })
    }
}

impl<R: Read + Send> SourceReader for TsvReader<R> {
    fn next_row(&mut self) -> Result<Option<SourceRow>> {
        if self.finished {
            return Ok(None);
        }

        for result in self.reader.records() {
            self.row_number += 1;
            let record = match result {
                Ok(r) => r,
                Err(e) => {
                    return Err(anyhow::anyhow!("TSV error at row {}: {e}", self.row_number));
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

impl<R: Read + Seek + Send> TsvReader<R> {
    pub fn reset_seekable(&mut self) -> Result<()> {
        Err(anyhow::anyhow!("reset_seekable not supported with csv crate reader"))
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
    fn tsv_reads_basic_rows() {
        let data = "id\tname\tage\n1\tAlice\t30\n2\tBob\t25\n";
        let mut reader = TsvReader::new(cursor(data)).unwrap();
        assert_eq!(reader.headers(), &["id", "name", "age"]);

        let row1 = reader.next_row().unwrap().unwrap();
        assert_eq!(row1.get("id"), Some("1"));
        assert_eq!(row1.get("name"), Some("Alice"));

        let row2 = reader.next_row().unwrap().unwrap();
        assert_eq!(row2.get("name"), Some("Bob"));

        assert!(reader.next_row().unwrap().is_none());
    }

    #[test]
    fn tsv_skips_comments_and_blank_lines() {
        let data = "id\tval\n# comment\n\n1\thello\n\n2\tworld\n";
        let mut reader = TsvReader::new(cursor(data)).unwrap();
        let row1 = reader.next_row().unwrap().unwrap();
        assert_eq!(row1.get("val"), Some("hello"));
        let row2 = reader.next_row().unwrap().unwrap();
        assert_eq!(row2.get("val"), Some("world"));
        assert!(reader.next_row().unwrap().is_none());
    }

    #[test]
    fn tsv_empty_file_errors() {
        let result = TsvReader::new(cursor(""));
        assert!(result.is_err());
    }

    #[test]
    fn tsv_duplicate_column_errors() {
        let data = "id\tid\tval\n";
        let result = TsvReader::new(cursor(data));
        assert!(result.is_err());
    }

    #[test]
    fn tsv_field_count_mismatch_errors() {
        let data = "id\tname\n1\tAlice\textra\n";
        let mut reader = TsvReader::new(cursor(data)).unwrap();
        let result = reader.next_row();
        assert!(result.is_err());
    }
}
