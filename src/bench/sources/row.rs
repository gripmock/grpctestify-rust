use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct SourceRow {
    columns: Vec<String>,
    values: Vec<String>,
    index: HashMap<String, usize>,
}

impl SourceRow {
    pub fn new(headers: &[String], values: Vec<String>) -> Self {
        let index: HashMap<String, usize> = headers
            .iter()
            .enumerate()
            .map(|(i, h)| (h.clone(), i))
            .collect();
        Self {
            columns: headers.to_vec(),
            values,
            index,
        }
    }

    pub fn from_csv_line(line: &str) -> Self {
        let mut columns = Vec::new();
        let mut values = Vec::new();
        for part in line.split(',') {
            let part = part.trim();
            values.push(part.to_string());
            if columns.len() < values.len() {
                columns.push(format!("col_{}", columns.len()));
            }
        }
        let index: HashMap<String, usize> = columns
            .iter()
            .enumerate()
            .map(|(i, h)| (h.clone(), i))
            .collect();
        Self {
            columns,
            values,
            index,
        }
    }

    pub fn from_pairs(pairs: Vec<(String, String)>) -> Self {
        let mut columns = Vec::with_capacity(pairs.len());
        let mut values = Vec::with_capacity(pairs.len());
        let mut index = HashMap::new();
        for (i, (k, v)) in pairs.into_iter().enumerate() {
            index.insert(k.clone(), i);
            columns.push(k);
            values.push(v);
        }
        Self {
            columns,
            values,
            index,
        }
    }

    pub fn get(&self, column: &str) -> Option<&str> {
        self.index
            .get(column)
            .map(|&i| self.values.get(i).map(|s| s.as_str()))
            .flatten()
    }

    pub fn get_or(&self, column: &str, default: &str) -> String {
        self.get(column)
            .map(|s| s.to_string())
            .unwrap_or_else(|| default.to_string())
    }

    pub fn columns(&self) -> &[String] {
        &self.columns
    }

    pub fn values(&self) -> &[String] {
        &self.values
    }

    pub fn to_map(&self) -> HashMap<String, String> {
        let mut map = HashMap::with_capacity(self.columns.len());
        for (i, col) in self.columns.iter().enumerate() {
            if let Some(v) = self.values.get(i) {
                map.insert(col.clone(), v.clone());
            }
        }
        map
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_new_and_get() {
        let headers = vec!["id".into(), "name".into()];
        let row = SourceRow::new(&headers, vec!["42".into(), "Alice".into()]);
        assert_eq!(row.get("id"), Some("42"));
        assert_eq!(row.get("name"), Some("Alice"));
        assert_eq!(row.get("missing"), None);
    }

    #[test]
    fn row_from_pairs() {
        let row = SourceRow::from_pairs(vec![("x".into(), "1".into()), ("y".into(), "2".into())]);
        assert_eq!(row.get("x"), Some("1"));
        assert_eq!(row.get("y"), Some("2"));
    }

    #[test]
    fn row_get_or_default() {
        let headers = vec!["id".into()];
        let row = SourceRow::new(&headers, vec!["1".into()]);
        assert_eq!(row.get_or("id", "fallback"), "1");
        assert_eq!(row.get_or("missing", "fallback"), "fallback");
    }

    #[test]
    fn row_to_map() {
        let headers = vec!["a".into(), "b".into()];
        let row = SourceRow::new(&headers, vec!["1".into(), "2".into()]);
        let map = row.to_map();
        assert_eq!(map.get("a"), Some(&"1".to_string()));
        assert_eq!(map.get("b"), Some(&"2".to_string()));
    }

    #[test]
    fn row_len_and_empty() {
        let empty_row = SourceRow::new(&[], vec![]);
        assert!(empty_row.is_empty());
        assert_eq!(empty_row.len(), 0);

        let row = SourceRow::new(&["x".into()], vec!["1".into()]);
        assert!(!row.is_empty());
        assert_eq!(row.len(), 1);
    }
}
