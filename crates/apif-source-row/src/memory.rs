use crate::SourceReader;
use crate::SourceRow;
use anyhow::Result;
use apif_source_error::SourceError;
use std::collections::HashMap;

pub struct InMemorySource {
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
            let key = crate::index::composite_value(key_column, |c| row.get(c).map(str::to_string))
                .ok_or_else(|| {
                    SourceError::ColumnNotFound(key_column.to_string(), "<memory>".into())
                })?;
            data.entry(key).or_default().push(row);
        }

        Ok(Self {
            data,
            key_column: key_column.to_string(),
            headers,
            row_count,
        })
    }

    pub fn lookup(&self, key: &str) -> Option<&SourceRow> {
        self.data.get(key).and_then(|rows| rows.first())
    }

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

    #[cfg_attr(miri, ignore)]
    #[test]
    #[cfg(not(miri))]
    fn in_memory_expansion_stays_near_the_budget_constant() {
        let mut data = String::from("id,name,region,city,note\n");
        for i in 0..200_000 {
            data.push_str(&format!(
                "{i},user-{i:06},region-{},city-{},note-{}\n",
                i % 8,
                i % 32,
                i % 4
            ));
        }
        let file_bytes = data.len();
        let data: &'static str = Box::leak(data.into_boxed_str());
        let mut reader = CsvFixtures::make_reader(data);
        let before = rss_bytes();
        let src = InMemorySource::load(&mut reader, "id").unwrap();
        let after = rss_bytes();
        assert_eq!(src.row_count(), 200_000);
        let multiplier = (after - before) as f64 / file_bytes as f64;
        println!(
            "file={file_bytes} rss_delta={} multiplier={multiplier:.2}",
            after - before
        );
        assert!(
            (4.0..=20.0).contains(&multiplier),
            "in-memory expansion moved to {multiplier:.2}x; DIMENSION_MEMORY_EXPANSION is {}",
            crate::driven::DIMENSION_MEMORY_EXPANSION
        );
    }

    #[cfg(not(miri))]
    fn rss_bytes() -> usize {
        let mut system = sysinfo::System::new();
        let pid = sysinfo::get_current_pid().unwrap();
        system.refresh_processes_specifics(
            sysinfo::ProcessesToUpdate::Some(&[pid]),
            true,
            sysinfo::ProcessRefreshKind::nothing().with_memory(),
        );
        system.process(pid).unwrap().memory() as usize
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

    #[test]
    fn duplicate_keys_retained_via_lookup_all() {
        let data = "id,val\n1,first\n1,second\n";
        let mut reader = CsvFixtures::make_reader(data);
        let mem = InMemorySource::load(&mut reader, "id").unwrap();

        let all = mem.lookup_all("1");
        let vals: Vec<Option<&str>> = all.iter().map(|r| r.get("val")).collect();
        assert_eq!(vals, vec![Some("first"), Some("second")]);
        assert_eq!(mem.row_count(), 2);

        assert_eq!(mem.lookup("1").unwrap().get("val"), Some("first"));

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
