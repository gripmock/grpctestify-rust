use super::SourceReader;
use super::error::SourceError;
use super::row::SourceRow;
use anyhow::Result;
use std::io::{BufRead, BufReader, Read, Seek};

pub struct CsvReader<R> {
    reader: BufReader<R>,
    headers: Vec<String>,
    delimiter: u8,
    row_number: usize,
    finished: bool,
}

impl<R: Read> CsvReader<R> {
    pub fn new(reader: BufReader<R>, delimiter: u8) -> Result<Self> {
        let mut slf = Self {
            reader,
            headers: Vec::new(),
            delimiter,
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
            return Err(SourceError::EmptyFile("csv".into()).into());
        }
        let header_line = line.trim_end_matches(['\n', '\r']);
        self.headers = parse_csv_line(header_line, self.delimiter);

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

impl<R: Read + Send> SourceReader for CsvReader<R> {
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
            let values = parse_csv_line(trimmed, self.delimiter);

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

impl<R: Read + Seek + Send> CsvReader<R> {
    pub fn reset_seekable(&mut self) -> Result<()> {
        self.reader.seek(std::io::SeekFrom::Start(0))?;
        self.row_number = 0;
        self.finished = false;
        self.read_header()?;
        Ok(())
    }
}

fn parse_csv_line(line: &str, delimiter: u8) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let bytes = line.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        let ch = bytes[i];
        if in_quotes {
            if ch == b'"' {
                if i + 1 < bytes.len() && bytes[i + 1] == b'"' {
                    current.push('"');
                    i += 2;
                    continue;
                }
                in_quotes = false;
                i += 1;
                continue;
            }
            current.push(ch as char);
            i += 1;
        } else {
            if ch == b'"' {
                in_quotes = true;
                i += 1;
            } else if ch == delimiter {
                fields.push(current.clone());
                current.clear();
                i += 1;
            } else {
                current.push(ch as char);
                i += 1;
            }
        }
    }

    fields.push(current);
    fields
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
