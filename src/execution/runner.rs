// Test runner
// Executes tests defined in GctfDocument

use super::super::parser::GctfDocument;
use crate::assert::{get_json_diff, AssertionEngine, JsonComparator};
use crate::grpc::{CompressionMode, GrpcClient, GrpcClientConfig, ProtoConfig, TlsConfig};
use crate::parser::ast::{SectionContent, SectionType};
use crate::report::CoverageCollector;
use crate::utils::file::FileUtils;
use anyhow::Result;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;

/// Test execution status
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestExecutionStatus {
    Pass,
    Fail(String),
}

/// Test execution result
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestExecutionResult {
    pub status: TestExecutionStatus,
    pub grpc_duration_ms: Option<u64>,
    // Optional: captured response for updating the test file
    pub captured_response: Option<crate::grpc::GrpcResponse>,
}

impl TestExecutionResult {
    pub fn pass(grpc_duration_ms: Option<u64>) -> Self {
        Self {
            status: TestExecutionStatus::Pass,
            grpc_duration_ms,
            captured_response: None,
        }
    }

    pub fn fail(message: String, grpc_duration_ms: Option<u64>) -> Self {
        Self {
            status: TestExecutionStatus::Fail(message),
            grpc_duration_ms,
            captured_response: None,
        }
    }

    pub fn with_response(mut self, response: crate::grpc::GrpcResponse) -> Self {
        self.captured_response = Some(response);
        self
    }
}

/// Test runner
pub struct TestRunner {
    dry_run: bool,
    timeout_seconds: u64,
    no_assert: bool,
    update_mode: bool,
    verbose: bool,
    assertion_engine: AssertionEngine,
    coverage_collector: Option<Arc<CoverageCollector>>,
}

impl TestRunner {
    fn full_service_name(package: &str, service: &str) -> String {
        if package.is_empty() {
            service.to_string()
        } else {
            format!("{}.{}", package, service)
        }
    }

    fn expected_values_for_response_section(section: &crate::parser::ast::Section) -> Vec<Value> {
        match &section.content {
            SectionContent::Json(v) => vec![v.clone()],
            SectionContent::JsonLines(values) => values.clone(),
            _ => Vec::new(),
        }
    }

    fn grpc_code_name_from_numeric(code: i64) -> Option<&'static str> {
        match code {
            0 => Some("OK"),
            1 => Some("Cancelled"),
            2 => Some("Unknown"),
            3 => Some("InvalidArgument"),
            4 => Some("DeadlineExceeded"),
            5 => Some("NotFound"),
            6 => Some("AlreadyExists"),
            7 => Some("PermissionDenied"),
            8 => Some("ResourceExhausted"),
            9 => Some("FailedPrecondition"),
            10 => Some("Aborted"),
            11 => Some("OutOfRange"),
            12 => Some("Unimplemented"),
            13 => Some("Internal"),
            14 => Some("Unavailable"),
            15 => Some("DataLoss"),
            16 => Some("Unauthenticated"),
            _ => None,
        }
    }

    fn error_matches_expected(error_text: &str, expected: &Value) -> bool {
        if let Some(expected_msg) = expected.get("message").and_then(|v| v.as_str()) {
            if !error_text.contains(expected_msg) {
                return false;
            }
        } else if expected.is_string() {
            if let Some(s) = expected.as_str() {
                if !error_text.contains(s) {
                    return false;
                }
            }
        }

        if let Some(code) = expected.get("code").and_then(|v| v.as_i64()) {
            if let Some(code_name) = Self::grpc_code_name_from_numeric(code) {
                let status_marker = format!("status: {}", code_name);
                if !error_text.contains(&status_marker) && !error_text.contains(&format!("code: {}", code)) {
                    return false;
                }
            }
        }

        true
    }

    /// Create a new test runner
    pub fn new(
        dry_run: bool,
        timeout_seconds: u64,
        no_assert: bool,
        update_mode: bool,
        verbose: bool,
        coverage_collector: Option<Arc<CoverageCollector>>,
    ) -> Self {
        Self {
            dry_run,
            timeout_seconds,
            no_assert,
            update_mode,
            verbose,
            assertion_engine: AssertionEngine::new(),
            coverage_collector,
        }
    }

    /// Run a single test
    pub async fn run_test(&self, document: &GctfDocument) -> Result<TestExecutionResult> {
        // Extract address
        let address = match document.get_address(
            std::env::var(crate::config::ENV_GRPCTESTIFY_ADDRESS)
                .ok()
                .as_deref(),
        ) {
            Some(a) => a,
            None => {
                // Default to localhost:4770 if no address is specified anywhere
                crate::config::default_address()
            }
        };

        // Extract endpoint
        let (package, service, method) = match document.parse_endpoint() {
            Some(e) => e,
            None => {
                return Ok(TestExecutionResult::fail(
                    "Invalid or missing endpoint".to_string(),
                    None,
                ))
            }
        };

        if document.sections.is_empty() {
            return Ok(TestExecutionResult::fail(
                "No sections found".to_string(),
                None,
            ));
        }

        if self.dry_run {
            // In dry-run, show detailed preview of what will be executed
            self.print_dry_run_preview(document, &address, &package, &service, &method);
            return Ok(TestExecutionResult::pass(None));
        }

        // Configure Client
        let document_path = Path::new(&document.file_path);

        let tls_config = document.get_tls_config().map(|tls_map| TlsConfig {
            ca_cert_path: tls_map.get("ca_cert").map(|p| {
                FileUtils::resolve_relative_path(document_path, p)
                    .to_string_lossy()
                    .to_string()
            }),
            client_cert_path: tls_map.get("client_cert").map(|p| {
                FileUtils::resolve_relative_path(document_path, p)
                    .to_string_lossy()
                    .to_string()
            }),
            client_key_path: tls_map.get("client_key").map(|p| {
                FileUtils::resolve_relative_path(document_path, p)
                    .to_string_lossy()
                    .to_string()
            }),
            server_name: tls_map.get("server_name").cloned(),
            insecure_skip_verify: tls_map
                .get("insecure")
                .map(|s| s == "true")
                .unwrap_or(false),
        });

        // Check for Proto config in document
        let proto_config = if let Some(proto_map) = document.get_proto_config() {
            let files = proto_map
                .get("files")
                .map(|s| {
                    s.split(',')
                        .map(|p| {
                            FileUtils::resolve_relative_path(document_path, p.trim())
                                .to_string_lossy()
                                .to_string()
                        })
                        .collect()
                })
                .unwrap_or_default();

            let import_paths = proto_map
                .get("import_paths")
                .map(|s| {
                    s.split(',')
                        .map(|p| {
                            FileUtils::resolve_relative_path(document_path, p.trim())
                                .to_string_lossy()
                                .to_string()
                        })
                        .collect()
                })
                .unwrap_or_default();

            let descriptor = proto_map.get("descriptor").map(|p| {
                FileUtils::resolve_relative_path(document_path, p)
                    .to_string_lossy()
                    .to_string()
            });

            Some(ProtoConfig {
                files,
                import_paths,
                descriptor,
            })
        } else {
            None
        };

        let full_service = Self::full_service_name(&package, &service);

        let client_config = GrpcClientConfig {
            address,
            timeout_seconds: self.timeout_seconds,
            tls_config,
            proto_config,
            metadata: document.get_request_headers(),
            target_service: Some(full_service.clone()),
            compression: CompressionMode::from_env(),
        };

        let client = GrpcClient::new(client_config).await?;

        // Get input/output message types for field coverage tracking
        let input_message_type = client.descriptor_pool()
            .get_service_by_name(&full_service)
            .and_then(|s| s.methods().find(|m| m.name() == method))
            .map(|m| m.input().full_name().to_string());
        let output_message_type = client.descriptor_pool()
            .get_service_by_name(&full_service)
            .and_then(|s| s.methods().find(|m| m.name() == method))
            .map(|m| m.output().full_name().to_string());

        // Setup Streaming
        let (tx, rx) = mpsc::channel::<Value>(100);
        let request_stream = ReceiverStream::new(rx);
        let mut tx = Some(tx);

        // Coverage: Register pool and record call
        if let Some(collector) = &self.coverage_collector {
            collector.register_pool(client.descriptor_pool());
            collector.record_call(&full_service, &method);
        }

        let start_time = std::time::Instant::now();

        // Start the gRPC call in background so unary/server-streaming methods can wait
        // for the first request message without deadlocking this task.
        let full_service_clone = full_service.clone();
        let method_clone = method.clone();
        let mut client_for_call = client;
        let mut call_handle = Some(tokio::spawn(async move {
            client_for_call
                .call_stream(&full_service_clone, &method_clone, request_stream)
                .await
        }));

        let mut response_stream = None;

        let mut variables: HashMap<String, Value> = HashMap::new();
        let mut last_message: Option<Value> = None;
        let mut captured_headers: HashMap<String, String> = HashMap::new();
        let mut captured_trailers: HashMap<String, String> = HashMap::new();
        let mut failure_reasons: Vec<String> = Vec::new();

        // Iterator for sections
        // We iterate by index to allow lookahead
        let sections = &document.sections;

        let has_request_sections = sections
            .iter()
            .any(|s| s.section_type == SectionType::Request);

        // Legacy behavior: if no REQUEST section is provided, send an empty
        // JSON object as a single request message for unary/server-stream calls.
        if !has_request_sections {
            if let Some(tx_ref) = tx.as_mut() {
                if let Err(e) = tx_ref.send(Value::Object(serde_json::Map::new())).await {
                    failure_reasons.push(format!("Failed to send implicit empty request: {}", e));
                }
            }
            drop(tx.take());
        }

        let mut skip_next_section = false;

        // Capture full response for update mode
        let mut captured_response = if self.update_mode {
            Some(crate::grpc::GrpcResponse::new())
        } else {
            None
        };

        for (i, section) in sections.iter().enumerate() {
            if skip_next_section {
                skip_next_section = false;
                continue;
            }

            match section.section_type {
                SectionType::Request => {
                    let request_value = match &section.content {
                        SectionContent::Json(req_json) => {
                            let mut req = req_json.clone();
                            self.substitute_variables(&mut req, &variables);
                            req
                        }
                        SectionContent::Empty => Value::Object(serde_json::Map::new()),
                        _ => continue,
                    };

                    // Coverage: record request fields
                    if let (Some(collector), Some(msg_type)) = (&self.coverage_collector, &input_message_type) {
                        collector.record_fields_from_json(msg_type, &request_value);
                    }

                    tracing::debug!(
                        "Sending Request:\n{}",
                        serde_json::to_string_pretty(&request_value)
                            .unwrap_or_else(|_| request_value.to_string())
                    );

                    if self.verbose {
                        println!("🔍 Sending request: '{}'",
                            serde_json::to_string_pretty(&request_value)
                                .unwrap_or_else(|_| request_value.to_string())
                        );
                    }

                    let Some(tx_ref) = tx.as_mut() else {
                        failure_reasons.push(format!(
                            "Failed to send request at line {}: request stream already closed",
                            section.start_line
                        ));
                        break;
                    };

                    if let Err(e) = tx_ref.send(request_value).await {
                        failure_reasons.push(format!(
                            "Failed to send request at line {}: {}",
                            section.start_line, e
                        ));
                        break;
                    }
                }
                SectionType::Response => {
                    if sections[i + 1..]
                        .iter()
                        .all(|s| s.section_type != SectionType::Request)
                    {
                        drop(tx.take());
                    }

                    if response_stream.is_none() {
                        if let Some(handle) = call_handle.take() {
                            match handle.await {
                                Ok(Ok((h, stream))) => {
                                    captured_headers = h.clone();
                                    if let Some(resp) = &mut captured_response {
                                        resp.headers = h;
                                    }
                                    response_stream = Some(stream);
                                }
                                Ok(Err(e)) => {
                                    failure_reasons
                                        .push(format!("Failed to start gRPC stream: {}", e));
                                    break;
                                }
                                Err(e) => {
                                    failure_reasons.push(format!(
                                        "Failed to join gRPC stream startup task: {}",
                                        e
                                    ));
                                    break;
                                }
                            }
                        }
                    }

                    let mut received_messages_for_section: Vec<Value> = Vec::new();
                    let expected_values = Self::expected_values_for_response_section(section);

                    for expected_template in expected_values {
                        match response_stream.as_mut().unwrap().next().await {
                            Some(Ok(item)) => {
                                match item {
                                    crate::grpc::client::StreamItem::Message(msg) => {
                                        last_message = Some(msg.clone());
                                        received_messages_for_section.push(msg.clone());
                                        if let Some(resp) = &mut captured_response {
                                            resp.messages.push(msg.clone());
                                        }

                                        tracing::debug!(
                                            "Received Response:\n{}",
                                            serde_json::to_string_pretty(&msg)
                                                .unwrap_or_else(|_| msg.to_string())
                                        );

                                        if self.no_assert {
                                            println!("--- RESPONSE (Raw) ---");
                                            println!(
                                                "{}",
                                                serde_json::to_string_pretty(&msg)
                                                    .unwrap_or_else(|_| msg.to_string())
                                            );
                                        } else if self.verbose {
                                            println!("🔍 gRPC response received: '{}'",
                                                serde_json::to_string_pretty(&msg)
                                                    .unwrap_or_else(|_| msg.to_string())
                                            );
                                        }

                                        if !self.no_assert {
                                            let mut expected = expected_template.clone();
                                            self.substitute_variables(&mut expected, &variables);

                                            // Coverage: record expected response fields
                                            if let (Some(collector), Some(msg_type)) = (&self.coverage_collector, &output_message_type) {
                                                collector.record_fields_from_json(msg_type, &expected);
                                            }

                                            let diffs = JsonComparator::compare(
                                                &msg,
                                                &expected,
                                                &section.inline_options,
                                            );

                                            if !diffs.is_empty() {
                                                failure_reasons.push(format!(
                                                    "Response mismatch at line {}:",
                                                    section.start_line
                                                ));
                                                for diff in diffs {
                                                    match diff {
                                                        crate::assert::AssertionResult::Fail {
                                                            message,
                                                            expected,
                                                            actual,
                                                        } => {
                                                            let mut msg = format!("  - {}", message);
                                                            if let (Some(exp), Some(act)) =
                                                                (expected, actual)
                                                            {
                                                                msg.push_str(&format!("\n      Expected: {}\n      Actual:   {}", exp, act));
                                                            }
                                                            failure_reasons.push(msg);
                                                        }
                                                        crate::assert::AssertionResult::Error(m) => {
                                                            failure_reasons
                                                                .push(format!("  - Error: {}", m))
                                                        }
                                                        _ => {}
                                                    }
                                                }
                                                failure_reasons.push(get_json_diff(&expected, &msg));
                                            }
                                        }
                                    }
                                    crate::grpc::client::StreamItem::Trailers(t) => {
                                        captured_trailers.extend(t.clone());
                                        if let Some(resp) = &mut captured_response {
                                            resp.trailers.extend(t);
                                        }
                                        if !self.no_assert {
                                            failure_reasons.push(format!(
                                                "Expected message for RESPONSE section at line {}, but received Trailers (End of Stream)",
                                                section.start_line
                                            ));
                                        }
                                        break;
                                    }
                                }
                            }
                            Some(Err(status)) => {
                                if let Some(resp) = &mut captured_response {
                                    resp.error = Some(status.message().to_string());
                                }
                                if !self.no_assert {
                                    failure_reasons.push(format!(
                                        "Expected message for RESPONSE section at line {}, but received Error: {}",
                                        section.start_line,
                                        status.message()
                                    ));
                                } else {
                                    println!("--- RESPONSE (Error) ---");
                                    println!("{}", status.message());
                                }
                                break;
                            }
                            None => {
                                if !self.no_assert {
                                    failure_reasons.push(format!(
                                        "Expected message for RESPONSE section at line {}, but stream ended",
                                        section.start_line
                                    ));
                                }
                                break;
                            }
                        }
                    }

                    if section.inline_options.with_asserts {
                        if let Some(next_section) = sections.get(i + 1) {
                            if next_section.section_type == SectionType::Asserts {
                                if !self.no_assert {
                                    if let SectionContent::Assertions(lines) = &next_section.content {
                                        for msg in &received_messages_for_section {
                                            self.run_assertions(
                                                lines,
                                                msg,
                                                &captured_headers,
                                                &captured_trailers,
                                                &mut failure_reasons,
                                                format!(
                                                    "(attached to RESPONSE at line {})",
                                                    section.start_line
                                                ),
                                            );
                                        }
                                    }
                                }
                                skip_next_section = true;
                            } else if !self.no_assert {
                                failure_reasons.push(format!(
                                    "RESPONSE at line {} has 'with_asserts' but is not followed by ASSERTS",
                                    section.start_line
                                ));
                            }
                        }
                    }
                }
                SectionType::Asserts => {
                    if sections[i + 1..]
                        .iter()
                        .all(|s| s.section_type != SectionType::Request)
                    {
                        drop(tx.take());
                    }

                    if response_stream.is_none() {
                        if let Some(handle) = call_handle.take() {
                            match handle.await {
                                Ok(Ok((h, stream))) => {
                                    captured_headers = h.clone();
                                    if let Some(resp) = &mut captured_response {
                                        resp.headers = h;
                                    }
                                    response_stream = Some(stream);
                                }
                                Ok(Err(e)) => {
                                    failure_reasons
                                        .push(format!("Failed to start gRPC stream: {}", e));
                                    break;
                                }
                                Err(e) => {
                                    failure_reasons.push(format!(
                                        "Failed to join gRPC stream startup task: {}",
                                        e
                                    ));
                                    break;
                                }
                            }
                        }
                    }

                    // Standalone Asserts - consumes a new message
                    match response_stream.as_mut().unwrap().next().await {
                        Some(Ok(crate::grpc::client::StreamItem::Message(msg))) => {
                            last_message = Some(msg.clone());

                            tracing::debug!(
                                "Received Response (for Asserts):\n{}",
                                serde_json::to_string_pretty(&msg)
                                    .unwrap_or_else(|_| msg.to_string())
                            );

                            if self.no_assert {
                                println!("--- RESPONSE (Raw) ---");
                                println!(
                                    "{}",
                                    serde_json::to_string_pretty(&msg)
                                        .unwrap_or_else(|_| msg.to_string())
                                );
                            }

                            if !self.no_assert {
                                if let SectionContent::Assertions(lines) = &section.content {
                                    self.run_assertions(
                                        lines,
                                        &msg,
                                        &captured_headers,
                                        &captured_trailers,
                                        &mut failure_reasons,
                                        format!("at line {}", section.start_line),
                                    );
                                }
                            }
                        }
                        Some(Ok(crate::grpc::client::StreamItem::Trailers(t))) => {
                            captured_trailers.extend(t);
                            if !self.no_assert {
                                failure_reasons.push(format!(
                                    "Expected message for ASSERTS section at line {}, but received Trailers",
                                    section.start_line
                                ));
                            }
                        }
                        Some(Err(status)) => {
                            if !self.no_assert {
                                failure_reasons.push(format!(
                                     "Expected message for ASSERTS section at line {}, but received Error: {}",
                                     section.start_line, status.message()
                                 ));
                            } else {
                                println!("--- RESPONSE (Error) ---");
                                println!("{}", status.message());
                            }
                        }
                        None => {
                            if !self.no_assert {
                                failure_reasons.push(format!(
                                    "Expected message for ASSERTS section at line {}, but stream ended",
                                    section.start_line
                                ));
                            }
                        }
                    }
                }

                SectionType::Extract => {
                    if let Some(msg) = &last_message {
                        if let SectionContent::Extract(extractions) = &section.content {
                            for (key, query) in extractions {
                                match self.assertion_engine.query(query, msg) {
                                    Ok(results) => {
                                        if let Some(val) = results.first() {
                                            variables.insert(key.clone(), val.clone());
                                        } else {
                                            failure_reasons.push(format!(
                                                 "Extraction failed at line {}: Query '{}' returned no results",
                                                 section.start_line, query
                                             ));
                                        }
                                    }
                                    Err(e) => {
                                        failure_reasons.push(format!(
                                            "Extraction error at line {}: {}",
                                            section.start_line, e
                                        ));
                                    }
                                }
                            }
                        }
                    } else {
                        failure_reasons.push(format!(
                            "EXTRACT at line {} requires a previous response message",
                            section.start_line
                        ));
                    }
                }
                SectionType::Error => {
                    if sections[i + 1..]
                        .iter()
                        .all(|s| s.section_type != SectionType::Request)
                    {
                        drop(tx.take());
                    }

                    if response_stream.is_none() {
                        if let Some(handle) = call_handle.take() {
                            match handle.await {
                                Ok(Ok((h, stream))) => {
                                    captured_headers = h.clone();
                                    if let Some(resp) = &mut captured_response {
                                        resp.headers = h;
                                    }
                                    response_stream = Some(stream);
                                }
                                Ok(Err(e)) => {
                                    // If ERROR section is expected, startup failures from unary/client-streaming
                                    // calls may represent the expected application error.
                                    if !self.no_assert {
                                        if let SectionContent::Json(expected_json) = &section.content {
                                            let mut expected = expected_json.clone();
                                            self.substitute_variables(&mut expected, &variables);

                                            // Try to extract tonic Status from anyhow::Error
                                            let got = if let Some(status) = e.downcast_ref::<tonic::Status>() {
                                                let status_name = Self::grpc_code_name_from_numeric(status.code() as i64)
                                                    .unwrap_or("Unknown");
                                                format!("status: {}, message: \"{}\"", status_name, status.message())
                                            } else {
                                                // Fallback to error string representation
                                                e.to_string()
                                            };

                                            if self.verbose {
                                                println!("🔍 gRPC error received: '{}'", got);
                                            }

                                            if !Self::error_matches_expected(&got, &expected) {
                                                failure_reasons.push(format!(
                                                    "Error mismatch at line {}: expected {}, got '{}'",
                                                    section.start_line, expected, got
                                                ));
                                            }
                                        }
                                    } else {
                                        println!("--- RESPONSE (Error) ---");
                                        println!("{}", e);
                                    }
                                    // Error has been consumed at startup stage; continue with next sections.
                                    continue;
                                }
                                Err(e) => {
                                    failure_reasons.push(format!(
                                        "Failed to join gRPC stream startup task: {}",
                                        e
                                    ));
                                    break;
                                }
                            }
                        }
                    }

                    // Expect an error from the stream
                    match response_stream.as_mut().unwrap().next().await {
                        Some(Err(status)) => {
                            let err_msg = status.message();

                            if self.no_assert {
                                println!("--- RESPONSE (Error) ---");
                                println!("{}", err_msg);
                            } else if self.verbose {
                                println!("🔍 gRPC error received: '{}'", err_msg);
                            }

                            if !self.no_assert {
                                if let SectionContent::Json(expected_json) = &section.content {
                                    let mut expected = expected_json.clone();
                                    self.substitute_variables(&mut expected, &variables);

                                    if !Self::error_matches_expected(err_msg, &expected) {
                                        failure_reasons.push(format!(
                                            "Error mismatch at line {}: expected {}, got '{}'",
                                            section.start_line, expected, err_msg
                                        ));
                                    }
                                }

                                // Handle with_asserts for Error
                                if section.inline_options.with_asserts {
                                    if let Some(next_section) = sections.get(i + 1) {
                                        if next_section.section_type == SectionType::Asserts {
                                            if let SectionContent::Assertions(lines) =
                                                &next_section.content
                                            {
                                                let error_value =
                                                    Value::String(err_msg.to_string());
                                                self.run_assertions(
                                                    lines,
                                                    &error_value,
                                                    &captured_headers,
                                                    &captured_trailers,
                                                    &mut failure_reasons,
                                                    format!(
                                                        "(attached to ERROR at line {})",
                                                        section.start_line
                                                    ),
                                                );
                                            }
                                            skip_next_section = true;
                                        }
                                    }
                                }
                            } else {
                                // In no_assert mode, we still need to skip the attached ASSERTS section if present
                                if section.inline_options.with_asserts {
                                    if let Some(next_section) = sections.get(i + 1) {
                                        if next_section.section_type == SectionType::Asserts {
                                            skip_next_section = true;
                                        }
                                    }
                                }
                            }
                        }
                        Some(Ok(msg_item)) => {
                            if !self.no_assert {
                                failure_reasons.push(format!(
                                    "Expected ERROR at line {}, but received success message or trailers",
                                    section.start_line
                                ));
                            } else {
                                // If we got a message instead of error in no_assert mode, print it
                                if let crate::grpc::client::StreamItem::Message(msg) = msg_item {
                                    println!("--- RESPONSE (Raw) ---");
                                    println!(
                                        "{}",
                                        serde_json::to_string_pretty(&msg)
                                            .unwrap_or_else(|_| msg.to_string())
                                    );
                                }
                            }
                        }
                        None => {
                            if !self.no_assert {
                                failure_reasons.push(format!(
                                    "Expected ERROR at line {}, but stream ended successfully",
                                    section.start_line
                                ));
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        // Ensure we close the request stream
        drop(tx.take());

        // If in update mode, capture any remaining responses
        if let Some(resp) = &mut captured_response {
            if response_stream.is_none() {
                if let Some(handle) = call_handle.take() {
                    match handle.await {
                        Ok(Ok((h, stream))) => {
                            resp.headers = h;
                            response_stream = Some(stream);
                        }
                        Ok(Err(_)) | Err(_) => {
                            response_stream = None;
                        }
                    }
                }
            }

            loop {
                let next_item = if let Some(stream) = response_stream.as_mut() {
                    stream.next().await
                } else {
                    None
                };

                let Some(item_res) = next_item else {
                    break;
                };

                match item_res {
                    Ok(crate::grpc::client::StreamItem::Message(msg)) => {
                        resp.messages.push(msg);
                    }
                    Ok(crate::grpc::client::StreamItem::Trailers(t)) => {
                        resp.trailers.extend(t);
                    }
                    Err(status) => {
                        resp.error = Some(status.message().to_string());
                    }
                }
            }
        }

        let grpc_duration = start_time.elapsed().as_millis() as u64;

        if !failure_reasons.is_empty() {
            // Even if failed, we might want to return captured response?
            // Usually snapshot update only happens if user asks for it.
            // If update_mode is true, we should probably ignore failures?
            if self.update_mode {
                // In update mode, failures (mismatches) are expected because we are updating!
                // But validation errors (like invalid JSON) might still be relevant.
                // Let's assume update mode implies "I want to overwrite whatever happens".
                if let Some(resp) = captured_response {
                    return Ok(TestExecutionResult::pass(Some(grpc_duration)).with_response(resp));
                }
            }

            return Ok(TestExecutionResult::fail(
                format!("Validation failed:\n  - {}", failure_reasons.join("\n  - ")),
                Some(grpc_duration),
            ));
        }

        let mut result = TestExecutionResult::pass(Some(grpc_duration));
        if let Some(resp) = captured_response {
            result = result.with_response(resp);
        }
        Ok(result)
    }

    /// Validates a collected response against the document (for testing purposes)
    pub fn validate_response(
        &self,
        document: &GctfDocument,
        response: &crate::grpc::GrpcResponse,
        _timeout_ms: u64,
    ) -> TestExecutionResult {
        let mut failure_reasons: Vec<String> = Vec::new();
        let mut variables: HashMap<String, Value> = HashMap::new(); // Empty for tests

        let mut message_iter = response.messages.iter();
        let sections = &document.sections;
        let mut skip_next_section = false;
        let mut last_message: Option<Value> = None;

        for (i, section) in sections.iter().enumerate() {
            if skip_next_section {
                skip_next_section = false;
                continue;
            }

            match section.section_type {
                SectionType::Response => {
                    let expected_values = Self::expected_values_for_response_section(section);
                    let mut received_messages_for_section: Vec<Value> = Vec::new();

                    for expected_template in expected_values {
                        if let Some(msg) = message_iter.next() {
                            last_message = Some(msg.clone());
                            received_messages_for_section.push(msg.clone());

                            if !self.no_assert {
                                let mut expected = expected_template.clone();
                                self.substitute_variables(&mut expected, &variables);

                                let diffs = JsonComparator::compare(
                                    msg,
                                    &expected,
                                    &section.inline_options,
                                );

                                if !diffs.is_empty() {
                                    failure_reasons.push(format!(
                                        "Response mismatch at line {}:",
                                        section.start_line
                                    ));
                                    for diff in diffs {
                                        match diff {
                                            crate::assert::AssertionResult::Fail {
                                                message,
                                                expected,
                                                actual,
                                            } => {
                                                let mut msg = format!("  - {}", message);
                                                if let (Some(exp), Some(act)) = (expected, actual) {
                                                    msg.push_str(&format!(
                                                        "\n      Expected: {}\n      Actual:   {}",
                                                        exp, act
                                                    ));
                                                }
                                                failure_reasons.push(msg);
                                            }
                                            crate::assert::AssertionResult::Error(m) => {
                                                failure_reasons.push(format!("  - Error: {}", m))
                                            }
                                            _ => {}
                                        }
                                    }

                                    failure_reasons.push(get_json_diff(&expected, msg));
                                }
                            }
                        } else if !self.no_assert {
                            failure_reasons.push(format!(
                                "Expected message for RESPONSE section at line {}, but no more messages received",
                                section.start_line
                            ));
                            break;
                        }
                    }

                    if section.inline_options.with_asserts {
                        if let Some(next_section) = sections.get(i + 1) {
                            if next_section.section_type == SectionType::Asserts {
                                if !self.no_assert {
                                    if let SectionContent::Assertions(lines) =
                                        &next_section.content
                                    {
                                        for msg in &received_messages_for_section {
                                            self.run_assertions(
                                                lines,
                                                msg,
                                                &response.headers,
                                                &response.trailers,
                                                &mut failure_reasons,
                                                format!(
                                                    "(attached to RESPONSE at line {})",
                                                    section.start_line
                                                ),
                                            );
                                        }
                                    }
                                }
                                skip_next_section = true;
                            }
                        }
                    }
                }
                SectionType::Asserts => {
                    // Standalone Asserts - consumes a new message
                    if let Some(msg) = message_iter.next() {
                        last_message = Some(msg.clone());
                        if !self.no_assert {
                            if let SectionContent::Assertions(lines) = &section.content {
                                self.run_assertions(
                                    lines,
                                    msg,
                                    &response.headers,
                                    &response.trailers,
                                    &mut failure_reasons,
                                    format!("at line {}", section.start_line),
                                );
                            }
                        }
                    } else if !self.no_assert {
                        failure_reasons.push(format!(
                            "Expected message for ASSERTS section at line {}, but no more messages received",
                            section.start_line
                        ));
                    }
                }
                SectionType::Extract => {
                    if let Some(msg) = &last_message {
                        if let SectionContent::Extract(extractions) = &section.content {
                            for (key, query) in extractions {
                                match self.assertion_engine.query(query, msg) {
                                    Ok(results) => {
                                        if let Some(val) = results.first() {
                                            variables.insert(key.clone(), val.clone());
                                        } else {
                                            failure_reasons.push(format!(
                                                 "Extraction failed at line {}: Query '{}' returned no results",
                                                 section.start_line, query
                                             ));
                                        }
                                    }
                                    Err(e) => {
                                        failure_reasons.push(format!(
                                            "Extraction error at line {}: {}",
                                            section.start_line, e
                                        ));
                                    }
                                }
                            }
                        }
                    } else {
                        failure_reasons.push(format!(
                            "EXTRACT at line {} requires a previous response message",
                            section.start_line
                        ));
                    }
                }
                _ => {}
            }
        }

        if !failure_reasons.is_empty() {
            TestExecutionResult::fail(
                format!("Validation failed:\n  - {}", failure_reasons.join("\n  - ")),
                None,
            )
        } else {
            TestExecutionResult::pass(None)
        }
    }

    fn substitute_variables(&self, value: &mut Value, variables: &HashMap<String, Value>) {
        match value {
            Value::String(s) => {
                // Check for exact match "{{ var }}" to preserve type
                if s.starts_with("{{") && s.ends_with("}}") {
                    let inner = s[2..s.len() - 2].trim();
                    // check if inner has more {{ }} which implies complex string
                    if !inner.contains("{{") {
                        if let Some(val) = variables.get(inner) {
                            *value = val.clone();
                            return;
                        }
                    }
                }

                // String interpolation "prefix {{ var }} suffix"
                let mut result = s.clone();
                let mut changed = false;
                for (key, val) in variables {
                    let pattern = format!("{{{{ {} }}}}", key);
                    if result.contains(&pattern) {
                        if let Value::String(s_val) = val {
                            result = result.replace(&pattern, s_val);
                            changed = true;
                        } else {
                            // Fallback to JSON string representation
                            result = result.replace(&pattern, &val.to_string());
                            changed = true;
                        }
                    }
                }
                if changed {
                    *value = Value::String(result);
                }
            }
            Value::Array(arr) => {
                for v in arr {
                    self.substitute_variables(v, variables);
                }
            }
            Value::Object(obj) => {
                for v in obj.values_mut() {
                    self.substitute_variables(v, variables);
                }
            }
            _ => {}
        }
    }

    fn run_assertions(
        &self,
        lines: &[String],
        target_value: &Value,
        headers: &HashMap<String, String>,
        trailers: &HashMap<String, String>,
        failure_reasons: &mut Vec<String>,
        context: String,
    ) {
        let results =
            self.assertion_engine
                .evaluate_all(lines, target_value, Some(headers), Some(trailers));

        if self.assertion_engine.has_failures(&results) {
            for fail in self.assertion_engine.get_failures(&results) {
                match fail {
                    crate::assert::AssertionResult::Fail {
                        message,
                        expected,
                        actual,
                    } => {
                        failure_reasons.push(format!("Assertion failed {}: {}", context, message));
                        if let (Some(exp), Some(act)) = (expected, actual) {
                            failure_reasons
                                .push(format!("    Expected: {}\n    Actual:   {}", exp, act));
                        }
                    }
                    crate::assert::AssertionResult::Error(msg) => {
                        failure_reasons.push(format!("Assertion error {}: {}", context, msg));
                    }
                    _ => {}
                }
            }
        }
    }

    /// Print dry-run preview of test execution
    fn print_dry_run_preview(
        &self,
        document: &GctfDocument,
        address: &str,
        package: &str,
        service: &str,
        method: &str,
    ) {
        println!();
        println!("🔍 Dry-Run Preview: {}", document.file_path);
        println!("═══════════════════════════════════════════════════════════════");
        println!();
        println!("📍 Target:");
        println!("   Address: {}", address);
        let full_service = Self::full_service_name(package, service);
        println!("   Endpoint: {} / {}", full_service, method);
        println!();

        // Display headers first
        let mut has_headers = false;
        for section in &document.sections {
            if section.section_type == SectionType::RequestHeaders {
                if !has_headers {
                    println!();
                    println!("📋 Request Headers:");
                    has_headers = true;
                }
                if let SectionContent::KeyValues(headers) = &section.content {
                    for (key, value) in headers {
                        println!("   {}: {}", key, value);
                    }
                }
            }
        }

        // Group requests and responses to show flow
        let mut has_request = false;
        let mut has_asserts = false;
        let mut has_error = false;

        for section in &document.sections {
            match section.section_type {
                SectionType::Address => {}
                SectionType::Endpoint => {}
                SectionType::RequestHeaders => {}
                SectionType::Options => {}
                SectionType::Tls => {}
                SectionType::Proto => {}
                SectionType::Request => {
                    if !has_request {
                        println!();
                        println!("📤 Request/Response Flow:");
                        has_request = true;
                    }
                    if let SectionContent::Json(json) = &section.content {
                        let json_str =
                            serde_json::to_string_pretty(json).unwrap_or_else(|_| json.to_string());
                        println!("   ➤ REQUEST:");
                        println!("     {}", json_str.replace('\n', "\n     "));
                    }
                }
                SectionType::Response => {
                    let with_asserts = if section.inline_options.with_asserts {
                        " (with_asserts)"
                    } else {
                        ""
                    };
                    match &section.content {
                        SectionContent::Json(json) => {
                            let json_str = serde_json::to_string_pretty(json)
                                .unwrap_or_else(|_| json.to_string());
                            println!(
                                "   ↤ RESPONSE (Line {}):{}",
                                section.start_line, with_asserts
                            );
                            println!("     {}", json_str.replace('\n', "\n     "));
                        }
                        SectionContent::JsonLines(values) => {
                            println!(
                                "   ↤ RESPONSE (Line {}, {} messages):{}",
                                section.start_line,
                                values.len(),
                                with_asserts
                            );
                            for value in values {
                                let json_str = serde_json::to_string_pretty(value)
                                    .unwrap_or_else(|_| value.to_string());
                                println!("     {}", json_str.replace('\n', "\n     "));
                            }
                        }
                        _ => {}
                    }
                }
                SectionType::Asserts => {
                    if !has_asserts {
                        println!();
                        println!("✓ Assertions:");
                        has_asserts = true;
                    }
                    if let SectionContent::Assertions(lines) = &section.content {
                        for line in lines {
                            println!("   . {}", line);
                        }
                    }
                }
                SectionType::Error => {
                    if !has_error {
                        println!();
                        println!("❌ Expected Error:");
                        has_error = true;
                    }
                    if let SectionContent::Json(json) = &section.content {
                        let json_str =
                            serde_json::to_string_pretty(json).unwrap_or_else(|_| json.to_string());
                        println!("   {}", json_str);
                    }
                }
                SectionType::Extract => {
                    println!();
                    println!("💾 Variables to Extract:");
                    if let SectionContent::Extract(extractions) = &section.content {
                        for (key, query) in extractions {
                            println!("   {} -> {}", key, query);
                        }
                    }
                }
            }
        }

        // Show TLS config if present
        if let Some(tls_config) = document.get_tls_config() {
            println!();
            println!("🔒 TLS Configuration:");
            if tls_config.contains_key("ca_cert") {
                println!("   CA Cert: {}", tls_config.get("ca_cert").unwrap());
            }
            if tls_config.contains_key("cert") {
                println!("   Client Cert: {}", tls_config.get("cert").unwrap());
            }
            if tls_config.contains_key("key") {
                println!("   Client Key: {}", tls_config.get("key").unwrap());
            }
            if tls_config.get("insecure") == Some(&"true".to_string()) {
                println!("   Insecure Skip Verify: true");
            }
        }

        // Show PROTO config if present
        if let Some(proto_config) = document.get_proto_config() {
            println!();
            println!("📄 Proto Configuration:");
            if proto_config.contains_key("descriptor") {
                println!("   Descriptor: {}", proto_config.get("descriptor").unwrap());
            }
            if proto_config.contains_key("files") {
                println!("   Proto Files: {}", proto_config.get("files").unwrap());
            }
        }

        println!();
        println!("═══════════════════════════════════════════════════════════════");
        println!();
    }
}
