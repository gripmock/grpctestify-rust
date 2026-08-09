use crate::SourceRow;
use serde::Deserialize;
use std::collections::HashSet;

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
    /// Built on first use. `optimize()` was the only thing that populated it and
    /// nothing ever called `optimize()`, so every `in` filter was a linear scan.
    #[serde(skip)]
    in_set: std::sync::OnceLock<HashSet<String>>,
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
            && compare(actual, min).is_lt()
        {
            return false;
        }

        if let Some(max) = &self.lt
            && !compare(actual, max).is_lt()
        {
            return false;
        }

        if let Some(values) = &self.in_values {
            let set = self.in_set.get_or_init(|| values.iter().cloned().collect());
            if !set.contains(actual) {
                return false;
            }
        }

        true
    }
}

/// Numeric when both sides are numbers, bytewise otherwise — so `qty gte 100`
/// excludes `99` while ISO dates still order as strings.
fn compare(actual: &str, expected: &str) -> std::cmp::Ordering {
    if let (Ok(a), Ok(b)) = (actual.trim().parse::<f64>(), expected.trim().parse::<f64>())
        && let Some(ordering) = a.partial_cmp(&b)
    {
        return ordering;
    }
    actual.cmp(expected)
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

    fn numeric_row() -> SourceRow {
        SourceRow::from_pairs(vec![
            ("qty".into(), "99".into()),
            ("price".into(), "9.5".into()),
        ])
    }

    fn condition(field: &str) -> FilterCondition {
        FilterCondition {
            field: field.into(),
            equals: None,
            contains: None,
            gte: None,
            lt: None,
            in_values: None,
            in_set: std::sync::OnceLock::new(),
        }
    }

    // `'9'` sorts after `'1'`, so a bytewise `gte "100"` kept `"99"`.
    #[test]
    fn numeric_fields_compare_as_numbers() {
        let mut cond = condition("qty");
        cond.gte = Some("100".into());
        assert!(!cond.matches(&numeric_row()), "99 is not >= 100");

        let mut cond = condition("qty");
        cond.gte = Some("99".into());
        assert!(cond.matches(&numeric_row()));

        let mut cond = condition("qty");
        cond.lt = Some("100".into());
        assert!(cond.matches(&numeric_row()), "99 is < 100");

        let mut cond = condition("price");
        cond.gte = Some("10".into());
        assert!(!cond.matches(&numeric_row()), "9.5 is not >= 10");
    }

    // ISO dates and identifiers must keep comparing as strings.
    #[test]
    fn non_numeric_fields_still_compare_bytewise() {
        let mut cond = condition("created_at");
        cond.gte = Some("2024-01-01".into());
        cond.lt = Some("2024-03-01".into());
        assert!(cond.matches(&row()));

        let mut cond = condition("created_at");
        cond.gte = Some("2024-03-01".into());
        assert!(!cond.matches(&row()));
    }

    // `optimize()` was the only thing that built the set and nothing called it.
    #[test]
    fn an_in_filter_builds_its_set_without_an_explicit_optimize_call() {
        let mut cond = condition("status");
        cond.in_values = Some(vec!["inactive".into(), "active".into()]);
        assert!(cond.matches(&row()));
        assert!(cond.in_set.get().is_some(), "the set must be memoised");
        assert!(cond.matches(&row()), "second call uses the memoised set");
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
            in_set: std::sync::OnceLock::new(),
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
            in_set: std::sync::OnceLock::new(),
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
            in_set: std::sync::OnceLock::new(),
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
            in_set: std::sync::OnceLock::new(),
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
                in_set: std::sync::OnceLock::new(),
            },
            FilterCondition {
                field: "region_id".into(),
                equals: Some("R02".into()),
                contains: None,
                gte: None,
                lt: None,
                in_values: None,
                in_set: std::sync::OnceLock::new(),
            },
        ];
        assert!(!matches_all(&row(), &conds));
    }
}
