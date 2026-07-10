use std::fmt;

#[derive(Debug)]
pub enum SourceError {
    FileOpenFailed(String, std::io::Error),
    EmptyFile(String),
    DuplicateColumn(String),
    FieldCountMismatch {
        row: usize,
        fields: usize,
        expected: usize,
    },
    InvalidJson(usize, String),
    UnknownFormat(String),
    SourceNotFound(String),
    ColumnNotFound(String, String),
}

impl fmt::Display for SourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SourceError::FileOpenFailed(path, err) => {
                write!(f, "failed to open source file '{path}': {err}")
            }
            SourceError::EmptyFile(fmt) => {
                write!(f, "header required but file is empty: {fmt}")
            }
            SourceError::DuplicateColumn(col) => {
                write!(f, "duplicate column name '{col}' in header")
            }
            SourceError::FieldCountMismatch {
                row,
                fields,
                expected,
            } => {
                write!(f, "row {row} has {fields} fields, expected {expected}")
            }
            SourceError::InvalidJson(line, msg) => {
                write!(f, "invalid JSON on line {line}: {msg}")
            }
            SourceError::UnknownFormat(file) => {
                write!(f, "unknown format for file: {file}")
            }
            SourceError::SourceNotFound(name) => {
                write!(f, "source '{name}' not found")
            }
            SourceError::ColumnNotFound(col, source) => {
                write!(f, "column '{col}' not found in source '{source}'")
            }
        }
    }
}

impl std::error::Error for SourceError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_source_error_display() {
        let err = SourceError::FileOpenFailed(
            "test.csv".into(),
            std::io::Error::new(std::io::ErrorKind::NotFound, "file not found"),
        );
        assert!(err.to_string().contains("test.csv"));

        let err = SourceError::EmptyFile("empty.csv".into());
        assert!(err.to_string().contains("empty.csv"));

        let err = SourceError::DuplicateColumn("id".into());
        assert!(err.to_string().contains("id"));

        let err = SourceError::FieldCountMismatch {
            row: 5,
            fields: 2,
            expected: 3,
        };
        assert!(err.to_string().contains("5"));

        let err = SourceError::InvalidJson(10, "bad json".into());
        assert!(err.to_string().contains("bad json"));

        let err = SourceError::UnknownFormat("file.bin".into());
        assert!(err.to_string().contains("file.bin"));

        let err = SourceError::SourceNotFound("users".into());
        assert!(err.to_string().contains("users"));

        let err = SourceError::ColumnNotFound("name".into(), "users".into());
        assert!(err.to_string().contains("name"));
    }

    #[test]
    fn test_source_error_debug() {
        let err = SourceError::EmptyFile("test.csv".into());
        let s = format!("{:?}", err);
        assert!(!s.is_empty());
    }
}
