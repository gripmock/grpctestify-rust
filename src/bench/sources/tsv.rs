use super::SourceReader;
use super::error::SourceError;
use super::row::SourceRow;
use anyhow::Result;
use std::io::{BufRead, BufReader, Read, Seek};

pub struct TsvReader<R> {
    reader: BufReader<R>,
    headers: Vec<String>,
    row_number: usize,
    finished: bool,
}

impl<R: Read> TsvReader<R> {
    pub fn new(reader: BufReader<R>) -> Result<Self> {
        let mut slf = Self {
            reader,
            headers: Vec::new(),
            row_number: 0,
            finished: false,
        };
        slf.read_header()?;
        Ok(slf)
    }

    fn read_header(&mut self) -> Result<()> {
        let mut line = String::new();
        let bytes = self.reader.read_line(&mut line)?;
        if bytes == 0 {
            return Err(SourceError::EmptyFile("tsv".into()).into());
        }
        let header_line = line.trim_end_matches(['\n', '\r']);
        self.headers = header_line
            .split('\t')
            .map(|s| s.trim().to_string())
            .collect();

        let mut seen = std::collections::HashSet::new();
        for col in &self.headers {
            if !seen.insert(col.as_str()) {
                return Err(SourceError::DuplicateColumn(col.clone()).into());
            }
        }

        self.row_number = 1;
        Ok(())
    }
}

impl<R: Read + Send> SourceReader for TsvReader<R> {
    fn next_row(&mut self) -> Result<Option<SourceRow>> {
        if self.finished {
            return Ok(None);
        }

        let mut line = String::new();
        loop {
            line.clear();
            let bytes = self.reader.read_line(&mut line)?;
            if bytes == 0 {
                self.finished = true;
                return Ok(None);
            }

            let trimmed = line.trim_end_matches(['\n', '\r']);
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            self.row_number += 1;
            let values: Vec<String> = trimmed.split('\t').map(|s| s.trim().to_string()).collect();

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
        self.reader.seek(std::io::SeekFrom::Start(0))?;
        self.row_number = 0;
        self.finished = false;
        self.read_header()?;
        Ok(())
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
