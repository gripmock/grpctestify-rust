// Inspect output tests - verify AST and document structure analysis

use grpctestify::parser::{parse_gctf, parse_gctf_with_diagnostics};
use std::path::Path;

/// Test inspect returns correct section count
#[test]
fn test_inspect_section_count() {
    let test_cases = vec![
        ("examples/basic/unary.gctf", 3), // ENDPOINT + REQUEST + RESPONSE
        ("examples/basic/with-headers.gctf", 6), // ENDPOINT + REQUEST_HEADERS + REQUEST + RESPONSE + ASSERTS + (extra)
        ("examples/assertions/response-with-asserts.gctf", 4), // ENDPOINT + REQUEST + RESPONSE + ASSERTS
    ];

    for (path, expected_sections) in test_cases {
        if !Path::new(path).exists() {
            continue;
        }

        let doc = parse_gctf(Path::new(path));
        assert!(doc.is_ok(), "Failed to parse {}", path);

        let actual_sections = doc.unwrap().sections.len();
        assert_eq!(
            actual_sections, expected_sections,
            "Expected {} sections for {}, got {}",
            expected_sections, path, actual_sections
        );
    }
}

/// Test inspect detects endpoint correctly
#[test]
fn test_inspect_endpoint_detection() {
    let path = "examples/basic/unary.gctf";

    if !Path::new(path).exists() {
        return;
    }

    let doc = parse_gctf(Path::new(path)).unwrap();
    let endpoint = doc.get_endpoint();

    assert!(endpoint.is_some(), "Expected endpoint to be detected");
    assert_eq!(
        endpoint.unwrap(),
        "helloworld.Greeter/SayHello",
        "Endpoint mismatch"
    );
}

/// Test inspect parses endpoint components
#[test]
fn test_inspect_endpoint_components() {
    let path = "examples/basic/unary.gctf";

    if !Path::new(path).exists() {
        return;
    }

    let doc = parse_gctf(Path::new(path)).unwrap();
    let components = doc.parse_endpoint();

    assert!(components.is_some(), "Expected endpoint components");

    let (package, service, method) = components.unwrap();
    assert_eq!(package, "helloworld");
    assert_eq!(service, "Greeter");
    assert_eq!(method, "SayHello");
}

/// Test inspect detects requests
#[test]
fn test_inspect_request_detection() {
    let path = "examples/basic/unary.gctf";

    if !Path::new(path).exists() {
        return;
    }

    let doc = parse_gctf(Path::new(path)).unwrap();
    let requests = doc.get_requests();

    assert_eq!(
        requests.len(),
        1,
        "Expected 1 request, got {}",
        requests.len()
    );
}

/// Test inspect detects headers
#[test]
fn test_inspect_header_detection() {
    let path = "examples/basic/with-headers.gctf";

    if !Path::new(path).exists() {
        return;
    }

    let doc = parse_gctf(Path::new(path)).unwrap();
    let headers = doc.get_request_headers();

    assert!(headers.is_some(), "Expected headers to be detected");

    let headers = headers.unwrap();
    assert_eq!(
        headers.get("Authorization"),
        Some(&"Bearer token123".to_string()),
        "Authorization header mismatch"
    );
    assert_eq!(
        headers.get("X-Request-ID"),
        Some(&"req-001".to_string()),
        "X-Request-ID header mismatch"
    );
}

/// Test inspect detects extractions
#[test]
fn test_inspect_extraction_detection() {
    let path = "examples/variables/extract-basic.gctf";

    if !Path::new(path).exists() {
        return;
    }

    let doc = parse_gctf(Path::new(path)).unwrap();

    // Find EXTRACT sections
    let extract_sections = doc.sections_by_type(grpctestify::parser::ast::SectionType::Extract);

    assert!(extract_sections.len() > 0, "Expected EXTRACT section");
}

/// Test inspect with diagnostics
#[test]
fn test_inspect_with_diagnostics() {
    let path = "examples/basic/unary.gctf";

    if !Path::new(path).exists() {
        return;
    }

    let result = parse_gctf_with_diagnostics(Path::new(path));
    assert!(result.is_ok(), "Failed to parse {}", path);

    let (doc, diagnostics) = result.unwrap();

    // Check diagnostics has expected fields
    assert!(diagnostics.bytes > 0, "Expected non-zero file size");
    assert!(diagnostics.total_lines > 0, "Expected non-zero line count");
    assert!(diagnostics.section_headers > 0, "Expected sections");

    // Use doc to avoid unused warning
    assert!(doc.sections.len() > 0, "Expected sections in document");
}

/// Test inspect detects inline options
#[test]
fn test_inspect_inline_options() {
    let path = "examples/basic/partial-match.gctf";

    if !Path::new(path).exists() {
        return;
    }

    let doc = parse_gctf(Path::new(path)).unwrap();

    // Find RESPONSE sections
    let response_sections = doc.sections_by_type(grpctestify::parser::ast::SectionType::Response);

    assert!(response_sections.len() > 0, "Expected RESPONSE section");

    // Check inline options
    let response = &response_sections[0];
    assert!(
        response.inline_options.partial,
        "Expected partial=true option"
    );
}

/// Test inspect detects tolerance option
#[test]
fn test_inspect_tolerance_option() {
    let path = "examples/basic/tolerance.gctf";

    if !Path::new(path).exists() {
        return;
    }

    let doc = parse_gctf(Path::new(path)).unwrap();

    let response_sections = doc.sections_by_type(grpctestify::parser::ast::SectionType::Response);

    assert!(response_sections.len() > 0, "Expected RESPONSE section");

    let response = &response_sections[0];
    assert!(
        response.inline_options.tolerance.is_some(),
        "Expected tolerance option"
    );
    assert_eq!(
        response.inline_options.tolerance.unwrap(),
        0.01,
        "Tolerance value mismatch"
    );
}

/// Test inspect detects redact option
#[test]
fn test_inspect_redact_option() {
    let path = "examples/advanced/redact-sensitive.gctf";

    if !Path::new(path).exists() {
        return;
    }

    let doc = parse_gctf(Path::new(path)).unwrap();

    let response_sections = doc.sections_by_type(grpctestify::parser::ast::SectionType::Response);

    assert!(response_sections.len() > 0, "Expected RESPONSE section");

    let response = &response_sections[0];
    assert!(
        !response.inline_options.redact.is_empty(),
        "Expected redact fields"
    );
    assert!(
        response
            .inline_options
            .redact
            .contains(&"password".to_string()),
        "Expected 'password' in redact fields"
    );
    assert!(
        response
            .inline_options
            .redact
            .contains(&"token".to_string()),
        "Expected 'token' in redact fields"
    );
}

/// Test inspect detects with_asserts option
#[test]
fn test_inspect_with_asserts_option() {
    let path = "examples/assertions/response-with-asserts.gctf";

    if !Path::new(path).exists() {
        return;
    }

    let doc = parse_gctf(Path::new(path)).unwrap();

    let response_sections = doc.sections_by_type(grpctestify::parser::ast::SectionType::Response);

    assert!(response_sections.len() > 0, "Expected RESPONSE section");

    let response = &response_sections[0];
    assert!(
        response.inline_options.with_asserts,
        "Expected with_asserts=true option"
    );
}
