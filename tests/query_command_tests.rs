// Query command integration tests

use grpctestify::cli::args::QueryArgs;
use grpctestify::commands::handle_query;

fn test_data_path(name: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/query")
        .join(name)
}

#[test]
fn test_query_direct_file() {
    let csv_path = test_data_path("test.csv");

    let args = QueryArgs {
        files: vec![csv_path],
        query: Some("test id=1".to_string()),
        shell: false,
        indexed_by: Some("id".to_string()),
        format: "json".to_string(),
        limit: Some(10),
        offset: None,
        columns: None,
        order_by: None,
        output: None,
        no_header: false,
    };

    let result = handle_query(&args);
    assert!(result.is_ok());
}

#[test]
fn test_query_with_limit() {
    let csv_path = test_data_path("test.csv");

    let args = QueryArgs {
        files: vec![csv_path],
        query: Some("test id>=1".to_string()),
        shell: false,
        indexed_by: None,
        format: "table".to_string(),
        limit: Some(2),
        offset: None,
        columns: None,
        order_by: None,
        output: None,
        no_header: false,
    };

    let result = handle_query(&args);
    assert!(result.is_ok());
}

#[test]
fn test_query_output_to_file() {
    let csv_path = test_data_path("test.csv");
    let temp_dir = tempfile::TempDir::new().unwrap();
    let output_path = temp_dir.path().join("output.ndjson");

    let args = QueryArgs {
        files: vec![csv_path],
        query: Some("test status=active".to_string()),
        shell: false,
        indexed_by: None,
        format: "table".to_string(),
        limit: None,
        offset: None,
        columns: None,
        order_by: None,
        output: Some(output_path.clone()),
        no_header: false,
    };

    let result = handle_query(&args);
    assert!(result.is_ok());

    let content = std::fs::read_to_string(&output_path).unwrap();
    assert!(content.contains("alice"));
    assert!(content.contains("charlie"));
}

#[test]
fn test_query_ndjson_file() {
    let ndjson_path = test_data_path("test.ndjson");

    let args = QueryArgs {
        files: vec![ndjson_path],
        query: Some("test status=active".to_string()),
        shell: false,
        indexed_by: None,
        format: "json".to_string(),
        limit: None,
        offset: None,
        columns: None,
        order_by: None,
        output: None,
        no_header: false,
    };

    let result = handle_query(&args);
    assert!(result.is_ok());
}
