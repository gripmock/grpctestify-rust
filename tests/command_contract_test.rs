use std::process::Command;

fn get_binary() -> String {
    env!("CARGO_BIN_EXE_grpctestify").to_string()
}

#[test]
fn test_check_valid_file_json_output() {
    let binary = get_binary();
    let output = Command::new(&binary)
        .args([
            "check",
            "tests/data/gctf/valid_simple.gctf",
            "--format",
            "json",
        ])
        .output()
        .expect("Failed to execute check command");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("Invalid JSON output");

    assert!(json.get("diagnostics").is_some());
    assert!(json.get("summary").is_some());
    assert_eq!(json["summary"]["total_files"], 1);
}

#[test]
fn test_check_missing_file_json_output() {
    let binary = get_binary();
    let output = Command::new(&binary)
        .args([
            "check",
            "tests/data/gctf/nonexistent.gctf",
            "--format",
            "json",
        ])
        .output()
        .expect("Failed to execute check command");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("Invalid JSON output");

    assert!(!json["diagnostics"].as_array().unwrap().is_empty());
    assert_eq!(json["diagnostics"][0]["code"], "FILE_NOT_FOUND");
}

#[test]
fn test_inspect_valid_file_json_output() {
    let binary = get_binary();
    let output = Command::new(&binary)
        .args([
            "inspect",
            "tests/data/gctf/valid_simple.gctf",
            "--format",
            "json",
        ])
        .output()
        .expect("Failed to execute inspect command");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("Invalid JSON output");

    assert!(json.get("file").is_some());
    assert!(json.get("ast").is_some());
    assert!(json.get("diagnostics").is_some());
    assert!(json.get("inferred_rpc_mode").is_some());
}

#[test]
fn test_inssect_sections_have_required_fields() {
    let binary = get_binary();
    let output = Command::new(&binary)
        .args([
            "inspect",
            "tests/data/gctf/valid_simple.gctf",
            "--format",
            "json",
        ])
        .output()
        .expect("Failed to execute inspect command");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("Invalid JSON output");

    let sections = json["ast"]["sections"]
        .as_array()
        .expect("sections should be array");
    for section in sections {
        assert!(section.get("section_type").is_some());
        assert!(section.get("start_line").is_some());
        assert!(section.get("end_line").is_some());
        assert!(section.get("content_kind").is_some());
    }
}

#[test]
fn test_fmt_stdout_output() {
    let binary = get_binary();
    let output = Command::new(&binary)
        .args(["fmt", "tests/data/gctf/valid_simple.gctf"])
        .output()
        .expect("Failed to execute fmt command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--- ENDPOINT ---"));
}

#[test]
fn test_fmt_idempotent() {
    let binary = get_binary();

    let output1 = Command::new(&binary)
        .args(["fmt", "tests/data/gctf/valid_simple.gctf"])
        .output()
        .expect("Failed to execute fmt command");

    let output2 = Command::new(&binary)
        .args(["fmt", "tests/data/gctf/valid_simple.gctf"])
        .output()
        .expect("Failed to execute fmt command");

    let stdout1 = String::from_utf8_lossy(&output1.stdout);
    let stdout2 = String::from_utf8_lossy(&output2.stdout);

    assert_eq!(stdout1, stdout2, "Formatter should be idempotent");
}

#[test]
fn test_list_json_output() {
    let binary = get_binary();
    let output = Command::new(&binary)
        .args(["list", "tests/data/gctf", "--format", "json"])
        .output()
        .expect("Failed to execute list command");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("Invalid JSON output");

    assert!(json.get("tests").is_some());
    let tests = json["tests"].as_array().expect("tests should be array");
    assert!(!tests.is_empty());

    for test in tests {
        assert!(test.get("id").is_some());
        assert!(test.get("label").is_some());
        assert!(test.get("uri").is_some());
    }
}

#[test]
fn test_list_with_range() {
    let binary = get_binary();
    let output = Command::new(&binary)
        .args([
            "list",
            "tests/data/gctf",
            "--format",
            "json",
            "--with-range",
        ])
        .output()
        .expect("Failed to execute list command");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("Invalid JSON output");

    let tests = json["tests"].as_array().expect("tests should be array");
    for test in tests {
        if test.get("range").is_some() {
            let range = &test["range"];
            assert!(range.get("start").is_some());
            assert!(range.get("end").is_some());
        }
    }
}
