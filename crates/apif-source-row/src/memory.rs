use crate::SourceReader;
use crate::SourceRow;
use anyhow::Result;
use apif_source_error::SourceError;
use std::collections::HashMap;

pub struct InMemorySource {
    /// Rows are stored per key so duplicate join keys are all retained; a
    /// plain `HashMap<String, SourceRow>` would collapse them (last wins) and
    /// make `lookup_all`/CROSS joins silently drop matches.
    data: HashMap<String, Vec<SourceRow>>,
    key_column: String,
    headers: Vec<String>,
    row_count: usize,
}

impl InMemorySource {
    pub fn load(reader: &mut dyn SourceReader, key_column: &str) -> Result<Self> {
        let headers = reader.headers().to_vec();
        let mut data: HashMap<String, Vec<SourceRow>> = HashMap::new();
        let mut row_count = 0;

        while let Some(row) = reader.next_row()? {
            row_count += 1;
            let key = row
                .get(key_column)
                .ok_or_else(|| {
                    SourceError::ColumnNotFound(key_column.to_string(), "<memory>".into())
                })?
                .to_string();
            data.entry(key).or_default().push(row);
        }

        Ok(Self {
            data,
            key_column: key_column.to_string(),
            headers,
            row_count,
        })
    }

    /// Returns the first row stored under `key` (insertion order).
    pub fn lookup(&self, key: &str) -> Option<&SourceRow> {
        self.data.get(key).and_then(|rows| rows.first())
    }

    /// Returns every row stored under `key`, in insertion order.
    pub fn lookup_all(&self, key: &str) -> &[SourceRow] {
        self.data.get(key).map(Vec::as_slice).unwrap_or(&[])
    }

    #[must_use]
    pub fn contains(&self, key: &str) -> bool {
        self.data.contains_key(key)
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub fn headers(&self) -> &[String] {
        &self.headers
    }

    pub fn key_column(&self) -> &str {
        &self.key_column
    }

    pub fn row_count(&self) -> usize {
        self.row_count
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &SourceRow)> {
        self.data
            .iter()
            .flat_map(|(key, rows)| rows.iter().map(move |row| (key, row)))
    }

    /// Filter the in-memory source, keeping only rows that match ALL filter conditions.
    pub fn filter(&self, conditions: &[super::filter::FilterCondition]) -> Self {
        use crate::filter::matches_all;
        let mut filtered_data: HashMap<String, Vec<SourceRow>> = HashMap::new();
        let mut row_count = 0;
        for (key, rows) in &self.data {
            for row in rows {
                if matches_all(row, conditions) {
                    filtered_data
                        .entry(key.clone())
                        .or_default()
                        .push(row.clone());
                    row_count += 1;
                }
            }
        }
        Self {
            data: filtered_data,
            key_column: self.key_column.clone(),
            headers: self.headers.clone(),
            row_count,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CsvReader;
    use std::io::{BufReader, Cursor};

    struct CsvFixtures;

    impl CsvFixtures {
        fn make_reader(data: &'static str) -> CsvReader<Cursor<&'static str>> {
            CsvReader::new(BufReader::new(Cursor::new(data)), b',').unwrap()
        }
    }

    #[test]
    fn load_and_lookup() {
        let data = "id,name,age\n1,Alice,30\n2,Bob,25\n3,Charlie,35\n";
        let mut reader = CsvFixtures::make_reader(data);
        let mem = InMemorySource::load(&mut reader, "id").unwrap();

        assert_eq!(mem.len(), 3);
        assert_eq!(mem.row_count(), 3);
        assert_eq!(mem.headers(), &["id", "name", "age"]);

        let row = mem.lookup("2").unwrap();
        assert_eq!(row.get("name"), Some("Bob"));
        assert_eq!(row.get("age"), Some("25"));
    }

    #[test]
    fn lookup_missing_returns_none() {
        let data = "id,val\n1,hello\n";
        let mut reader = CsvFixtures::make_reader(data);
        let mem = InMemorySource::load(&mut reader, "id").unwrap();
        assert!(mem.lookup("999").is_none());
    }

    #[test]
    fn contains_check() {
        let data = "id,val\n1,hello\n2,world\n";
        let mut reader = CsvFixtures::make_reader(data);
        let mem = InMemorySource::load(&mut reader, "id").unwrap();
        assert!(mem.contains("1"));
        assert!(!mem.contains("3"));
    }

    #[test]
    fn missing_key_column_errors() {
        let data = "id,val\n1,hello\n";
        let mut reader = CsvFixtures::make_reader(data);
        let result = InMemorySource::load(&mut reader, "missing_col");
        assert!(result.is_err());
    }

    /// Regression (BUG 2): rows sharing a join key must all be retained.
    /// The old `HashMap<String, SourceRow>` collapsed them (last wins), so
    /// `lookup_all` and CROSS joins silently dropped every row but the last.
    #[test]
    fn duplicate_keys_retained_via_lookup_all() {
        let data = "id,val\n1,first\n1,second\n";
        let mut reader = CsvFixtures::make_reader(data);
        let mem = InMemorySource::load(&mut reader, "id").unwrap();

        // Both rows survive rather than collapsing to the last one.
        let all = mem.lookup_all("1");
        let vals: Vec<Option<&str>> = all.iter().map(|r| r.get("val")).collect();
        assert_eq!(vals, vec![Some("first"), Some("second")]);
        assert_eq!(mem.row_count(), 2);

        // Single-row lookup returns the first match.
        assert_eq!(mem.lookup("1").unwrap().get("val"), Some("first"));

        // iter() exposes every row, not just one per key.
        let iter_count = mem.iter().filter(|(k, _)| k.as_str() == "1").count();
        assert_eq!(iter_count, 2);
    }

    #[test]
    fn empty_source() {
        let data = "id,val\n";
        let mut reader = CsvFixtures::make_reader(data);
        let mem = InMemorySource::load(&mut reader, "id").unwrap();
        assert!(mem.is_empty());
        assert_eq!(mem.row_count(), 0);
    }

    #[test]
    fn iter_all_rows() {
        let data = "id,val\n1,a\n2,b\n";
        let mut reader = CsvFixtures::make_reader(data);
        let mem = InMemorySource::load(&mut reader, "id").unwrap();
        let keys: Vec<&str> = mem.iter().map(|(k, _)| k.as_str()).collect();
        assert!(keys.contains(&"1"));
        assert!(keys.contains(&"2"));
    }
}
