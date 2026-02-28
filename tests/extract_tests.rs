// EXTRACT section tests - JQ functions and metadata extraction

use grpctestify::execution::{ExecutionPlan, Workflow, WorkflowEvent};
use grpctestify::parser::{parse_gctf, parse_gctf_from_str};
use std::path::Path;

#[test]
fn test_extract_basic_jq_paths() {
    // Arrange
    let content = r#"
--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{"id": 123}

--- RESPONSE ---
{"id": 123, "name": "test", "value": 100}

--- EXTRACT ---
id = .id
name = .name
value = .value

--- ASSERTS ---
.id == 123
"#;

    // Act
    let doc = parse_gctf_from_str(content, "test.gctf").unwrap();
    let plan = ExecutionPlan::from_document(&doc);
    let workflow = Workflow::from_plan(&plan);
    let extracts = workflow.extractions();

    // Assert
    assert!(!extracts.is_empty(), "Expected at least 1 extract event");

    let result = workflow.validate();
    assert!(
        result.passed,
        "Workflow validation should pass: {:?}",
        result.errors
    );
}

#[test]
fn test_extract_string_functions() {
    // Arrange
    let content = r#"
--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{"name": "  Hello World  "}

--- RESPONSE ---
{"name": "  Hello World  ", "tags": "a,b,c"}

--- EXTRACT ---
upper = .name | upper
lower = .name | lower
trimmed = .name | trim
parts = .tags | split(",")
joined = .tags | split(",") | join("-")

--- ASSERTS ---
@len(.trimmed) > 0
"#;

    // Act
    let doc = parse_gctf_from_str(content, "test.gctf").unwrap();
    let plan = ExecutionPlan::from_document(&doc);
    let workflow = Workflow::from_plan(&plan);
    let extracts = workflow.extractions();

    // Assert
    assert!(!extracts.is_empty(), "Expected at least 1 extract event");

    let result = workflow.validate();
    assert!(
        result.passed,
        "Workflow validation should pass: {:?}",
        result.errors
    );
}

#[test]
fn test_extract_numeric_aggregations() {
    // Arrange
    let content = r#"
--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{"items": [{"price": 10}, {"price": 20}, {"price": 30}]}

--- RESPONSE ---
{"items": [{"price": 10}, {"price": 20}, {"price": 30}]}

--- EXTRACT ---
count = .items | length
avg = [.items[].price] | avg
min = [.items[].price] | min
max = [.items[].price] | max
sum = [.items[].price] | add

--- ASSERTS ---
.count == 3
"#;

    // Act
    let doc = parse_gctf_from_str(content, "test.gctf").unwrap();
    let plan = ExecutionPlan::from_document(&doc);
    let workflow = Workflow::from_plan(&plan);
    let extracts = workflow.extractions();

    // Assert
    assert!(!extracts.is_empty(), "Expected at least 1 extract event");

    let result = workflow.validate();
    assert!(
        result.passed,
        "Workflow validation should pass: {:?}",
        result.errors
    );
}

#[test]
fn test_extract_array_operations() {
    // Arrange
    let content = r#"
--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{"users": [{"name": "Alice", "active": true}, {"name": "Bob", "active": false}]}

--- RESPONSE ---
{"users": [{"name": "Alice", "active": true}, {"name": "Bob", "active": false}]}

--- EXTRACT ---
first = .users[0].name
names = [.users[].name]
active = [.users[] | select(.active == true)]
sorted = .users | sort_by(.name)
unique_names = [.users[].name] | unique

--- ASSERTS ---
@len(.names) == 2
"#;

    // Act
    let doc = parse_gctf_from_str(content, "test.gctf").unwrap();
    let plan = ExecutionPlan::from_document(&doc);
    let workflow = Workflow::from_plan(&plan);
    let extracts = workflow.extractions();

    // Assert
    assert!(!extracts.is_empty(), "Expected at least 1 extract event");

    let result = workflow.validate();
    assert!(
        result.passed,
        "Workflow validation should pass: {:?}",
        result.errors
    );
}

#[test]
fn test_extract_conditional() {
    // Arrange
    let content = r#"
--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{"status": 200}

--- RESPONSE ---
{"status": 200}

--- EXTRACT ---
label = if .status == 200 then "OK" elif .status == 404 then "Not Found" else "Error" end
default_name = .name // "Anonymous"
default_port = .port // 8080

--- ASSERTS ---
.label == "OK"
"#;

    // Act
    let doc = parse_gctf_from_str(content, "test.gctf").unwrap();
    let plan = ExecutionPlan::from_document(&doc);
    let workflow = Workflow::from_plan(&plan);
    let extracts = workflow.extractions();

    // Assert
    assert!(!extracts.is_empty(), "Expected at least 1 extract event");

    let result = workflow.validate();
    assert!(
        result.passed,
        "Workflow validation should pass: {:?}",
        result.errors
    );
}

#[test]
fn test_extract_datetime() {
    // Arrange
    let content = r#"
--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{"created_at": "2024-01-15T10:30:00Z"}

--- RESPONSE ---
{"created_at": "2024-01-15T10:30:00Z"}

--- EXTRACT ---
date_only = .created_at | split("T")[0]
time_only = .created_at | split("T")[1] | split("Z")[0]

--- ASSERTS ---
@len(.date_only) > 0
"#;

    // Act
    let doc = parse_gctf_from_str(content, "test.gctf").unwrap();
    let plan = ExecutionPlan::from_document(&doc);
    let workflow = Workflow::from_plan(&plan);
    let extracts = workflow.extractions();

    // Assert
    assert!(!extracts.is_empty(), "Expected at least 1 extract event");

    let result = workflow.validate();
    assert!(
        result.passed,
        "Workflow validation should pass: {:?}",
        result.errors
    );
}

#[test]
fn test_extract_json5_syntax() {
    // Arrange
    let content = r#"
--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{
  id: 123,
  name: "test",
}
"#;

    // Act
    let result = parse_gctf_from_str(content, "test.gctf");

    // Assert
    assert!(result.is_ok(), "JSON5 syntax should be supported");
}

#[test]
fn test_extract_workflow_events() {
    // Arrange
    let content = r#"
--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{"id": 123}

--- RESPONSE ---
{"id": 123, "token": "abc123"}

--- EXTRACT ---
id = .id
token = .token

--- ASSERTS ---
.id == 123
"#;

    // Act
    let doc = parse_gctf_from_str(content, "test.gctf").unwrap();
    let plan = ExecutionPlan::from_document(&doc);
    let workflow = Workflow::from_plan(&plan);

    // Assert
    let has_extract = workflow
        .events
        .iter()
        .any(|e| matches!(e, WorkflowEvent::Extract { .. }));
    assert!(has_extract, "Workflow should have Extract event");

    let has_extracted = workflow
        .events
        .iter()
        .any(|e| matches!(e, WorkflowEvent::Extracted { .. }));
    assert!(has_extracted, "Workflow should have Extracted event");
}

#[test]
fn test_extract_chained_operations() {
    // Arrange
    let content = r#"
--- ENDPOINT ---
shop.OrderService/GetOrder

--- REQUEST ---
{"order_id": 123}

--- RESPONSE ---
{
  "items": [
    {"name": "Item 1", "price": 10.00, "qty": 2},
    {"name": "Item 2", "price": 25.00, "qty": 1}
  ],
  "tax_rate": 0.08
}

--- EXTRACT ---
item_count = .items | length
subtotal = [.items[].price * .items[].qty] | add
tax_amount = $subtotal * .tax_rate
total = $subtotal + $tax_amount
expensive = [.items[] | select(.price > 15) | .name]

--- ASSERTS ---
.item_count == 2
"#;

    // Act
    let doc = parse_gctf_from_str(content, "test.gctf").unwrap();
    let plan = ExecutionPlan::from_document(&doc);
    let workflow = Workflow::from_plan(&plan);
    let extracts = workflow.extractions();

    // Assert
    assert!(!extracts.is_empty(), "Expected at least 1 extract event");

    let result = workflow.validate();
    assert!(
        result.passed,
        "Workflow validation should pass: {:?}",
        result.errors
    );
}

#[test]
fn test_extract_from_example_file() {
    // Arrange
    let path = "examples/advanced/extract-jq-functions.gctf";

    // Act
    if !Path::new(path).exists() {
        return;
    }

    let doc = parse_gctf(Path::new(path)).unwrap();
    let plan = ExecutionPlan::from_document(&doc);
    let workflow = Workflow::from_plan(&plan);
    let extracts = workflow.extractions();

    // Assert
    assert!(!extracts.is_empty(), "Expected extract events from example");

    let result = workflow.validate();
    assert!(
        result.passed,
        "Example workflow validation should pass: {:?}",
        result.errors
    );
}

#[test]
fn test_extract_variable_in_asserts() {
    // Arrange
    let content = r#"
--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{"expected": 100}

--- RESPONSE ---
{"value": 100}

--- EXTRACT ---
expected_value = .value

--- ASSERTS ---
.value == {{ expected_value }}
"#;

    // Act
    let doc = parse_gctf_from_str(content, "test.gctf").unwrap();
    let plan = ExecutionPlan::from_document(&doc);
    let workflow = Workflow::from_plan(&plan);

    // Assert
    assert_eq!(plan.extractions.len(), 1, "Expected 1 extraction");

    let result = workflow.validate();
    assert!(
        result.passed,
        "Workflow validation should pass: {:?}",
        result.errors
    );
}
