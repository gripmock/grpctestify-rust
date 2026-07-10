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
