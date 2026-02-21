use grpctestify::parser::{self};
use std::path::PathBuf;

#[test]
fn test_parse_json5_gctf() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let file_path = PathBuf::from(manifest_dir).join("tests/data/gctf/json5_support.gctf");

    let doc = parser::parse_gctf(&file_path).expect("Failed to parse gctf");

    // Validate REQUEST
    let requests = doc.get_requests();
    assert_eq!(requests.len(), 1);
    let req = &requests[0];

    // JSON5 features verification
    assert_eq!(req["name"], "world");
    assert_eq!(req["trailing_comma"], "yes");
    assert_eq!(req["unquoted_key"], 123);
    assert_eq!(req["hex_number"], 16);
    assert_eq!(req["float_number"], 1.5);
    assert_eq!(req["quoted_key"], "value");

    // Validate RESPONSE
    // Note: get_responses is not pub in ast.rs? It says pub fn get_responses.
    // Let's check via sections_by_type manually just in case
    // Actually get_responses is pub.

    let responses = doc.get_responses();
    assert_eq!(responses.len(), 1);
    assert_eq!(responses[0]["message"], "Hello world");
}
