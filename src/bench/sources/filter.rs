use source_row::SourceRow;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct FilterCondition {
    pub field: String,
    #[serde(default)]
    pub equals: Option<String>,
    #[serde(default)]
    pub contains: Option<String>,
    #[serde(default)]
    pub gte: Option<String>,
    #[serde(default)]
    pub lt: Option<String>,
    #[serde(default, rename = "in")]
    pub in_values: Option<Vec<String>>,
}

impl FilterCondition {
    pub fn matches(&self, row: &SourceRow) -> bool {
        let Some(actual) = row.get(&self.field) else {
            return false;
        };

        if let Some(expected) = &self.equals
            && actual != expected
        {
            return false;
        }

        if let Some(needle) = &self.contains
            && !actual.contains(needle)
        {
            return false;
        }

        if let Some(min) = &self.gte
            && actual < min.as_str()
        {
            return false;
        }

        if let Some(max) = &self.lt
            && actual >= max.as_str()
        {
            return false;
        }

        if let Some(values) = &self.in_values
            && !values.iter().any(|v| v == actual)
        {
            return false;
        }

        true
    }
}

pub fn matches_all(row: &SourceRow, conditions: &[FilterCondition]) -> bool {
    conditions.iter().all(|c| c.matches(row))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row() -> SourceRow {
        SourceRow::from_pairs(vec![
            ("status".into(), "active".into()),
            ("region_id".into(), "R01".into()),
            ("created_at".into(), "2024-02-15".into()),
            ("name".into(), "PVZ Alpha".into()),
        ])
    }

    #[test]
    fn equals_match() {
        let cond = FilterCondition {
            field: "status".into(),
            equals: Some("active".into()),
            contains: None,
            gte: None,
            lt: None,
            in_values: None,
        };
        assert!(cond.matches(&row()));
    }

    #[test]
    fn in_match() {
        let cond = FilterCondition {
            field: "status".into(),
            equals: None,
            contains: None,
            gte: None,
            lt: None,
            in_values: Some(vec!["inactive".into(), "active".into()]),
        };
        assert!(cond.matches(&row()));
    }

    #[test]
    fn contains_match() {
        let cond = FilterCondition {
            field: "name".into(),
            equals: None,
            contains: Some("Alpha".into()),
            gte: None,
            lt: None,
            in_values: None,
        };
        assert!(cond.matches(&row()));
    }

    #[test]
    fn range_match() {
        let cond = FilterCondition {
            field: "created_at".into(),
            equals: None,
            contains: None,
            gte: Some("2024-01-01".into()),
            lt: Some("2025-01-01".into()),
            in_values: None,
        };
        assert!(cond.matches(&row()));
    }

    #[test]
    fn matches_all_false_on_any_failure() {
        let conds = vec![
            FilterCondition {
                field: "status".into(),
                equals: Some("active".into()),
                contains: None,
                gte: None,
                lt: None,
                in_values: None,
            },
            FilterCondition {
                field: "region_id".into(),
                equals: Some("R02".into()),
                contains: None,
                gte: None,
                lt: None,
                in_values: None,
            },
        ];
        assert!(!matches_all(&row(), &conds));
    }
}
