use crate::SourceRow;
use crate::{SourceReader, span_of, strip_bom};
use anyhow::Result;
use apif_source_error::SourceError;
use std::io::{BufReader, Read, Seek};

type CsvRewind<R> = Box<dyn Fn(&mut csv::Reader<BufReader<R>>) -> Result<()> + Send>;

pub struct CsvReader<R> {
    reader: csv::Reader<BufReader<R>>,
    headers: std::sync::Arc<[String]>,
    _delimiter: u8,
    row_number: usize,
    finished: bool,
    rewind: Option<CsvRewind<R>>,
    record: csv::StringRecord,
    last_span: Option<(u64, u32)>,
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
        let mut headers = headers;
        if let Some(first) = headers.first_mut() {
            strip_bom(first);
        }

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
            headers: headers.into(),
            _delimiter: delimiter,
            row_number: 1,
            finished: false,
            rewind: None,
            record: csv::StringRecord::new(),
            last_span: None,
        })
    }
}

impl<R: Read + Seek + Send> CsvReader<R> {
    pub fn new_seekable(reader: BufReader<R>, delimiter: u8) -> Result<Self> {
        let mut this = Self::new(reader, delimiter)?;
        let start = this.reader.position().clone();
        this.rewind = Some(Box::new(move |rdr: &mut csv::Reader<BufReader<R>>| {
            rdr.seek(start.clone())
                .map_err(|e| anyhow::anyhow!("failed to rewind CSV reader: {e}"))
        }));
        Ok(this)
    }
}

impl<R: Read + Send> SourceReader for CsvReader<R> {
    fn last_row_span(&self) -> Option<(u64, u32)> {
        self.last_span
    }

    fn next_row(&mut self) -> Result<Option<SourceRow>> {
        if self.finished {
            return Ok(None);
        }

        let has_row = self
            .reader
            .read_record(&mut self.record)
            .map_err(|e| anyhow::anyhow!("CSV error at row {}: {e}", self.row_number + 1))?;

        if has_row {
            self.row_number += 1;
            self.last_span = span_of(self.record.position(), self.reader.position());
            let values: Vec<String> = self.record.iter().map(|f| f.to_string()).collect();

            if values.len() != self.headers.len() {
                return Err(SourceError::FieldCountMismatch {
                    row: self.row_number,
                    fields: values.len(),
                    expected: self.headers.len(),
                }
                .into());
            }

            return Ok(Some(SourceRow::with_columns(
                std::sync::Arc::clone(&self.headers),
                values,
            )));
        }

        self.finished = true;
        Ok(None)
    }

    fn headers(&self) -> &[String] {
        &self.headers
    }

    fn supports_reset(&self) -> bool {
        self.rewind.is_some()
    }

    fn reset(&mut self) -> Result<()> {
        let Some(rewind) = self.rewind.as_ref() else {
            return Ok(());
        };
        rewind(&mut self.reader)?;
        self.row_number = 1;
        self.finished = false;
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

    #[test]
    fn csv_non_seekable_reader_does_not_claim_reset() {
        let data = "id,name\n1,Alice\n";
        let reader = CsvReader::new(cursor(data), b',').unwrap();
        assert!(!reader.supports_reset());
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn csv_reset_rewinds_to_first_data_row() {
        let data = "id,name\n1,Alice\n2,Bob\n";
        let mut reader = CsvReader::new_seekable(cursor(data), b',').unwrap();
        assert!(reader.supports_reset());

        let read_all = |r: &mut CsvReader<Cursor<&str>>| {
            let mut rows = Vec::new();
            while let Some(row) = r.next_row().unwrap() {
                rows.push((row.get_or("id", ""), row.get_or("name", "")));
            }
            rows
        };

        let first_pass = read_all(&mut reader);
        assert_eq!(
            first_pass,
            vec![("1".into(), "Alice".into()), ("2".into(), "Bob".into())]
        );

        reader.reset().unwrap();

        let after = reader.next_row().unwrap().unwrap();
        assert_eq!(after.get("id"), Some("1"));
        assert_eq!(after.get("name"), Some("Alice"));

        let row2 = reader.next_row().unwrap().unwrap();
        assert_eq!(row2.get("name"), Some("Bob"));
        assert!(reader.next_row().unwrap().is_none());
    }
}
