use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::PlayState;

/// Consistent JSON error response.
#[derive(Serialize)]
pub struct ApiError {
    pub error: String,
}

/// Reject paths containing `..` to prevent directory traversal.
fn reject_traversal(path: &str) -> Result<(), (StatusCode, String)> {
    if path.contains("..") {
        return Err((StatusCode::NOT_FOUND, "File not found".to_string()));
    }
    // Also reject absolute paths and paths starting with /
    if path.starts_with('/') {
        return Err((StatusCode::NOT_FOUND, "File not found".to_string()));
    }
    Ok(())
}

fn parse_protocol(s: Option<&str>) -> crate::grpc::WireProtocol {
    s.and_then(|s| s.parse().ok()).unwrap_or_default()
}

/// Resolve a relative path across all collections dirs. Returns first match.
fn resolve_file(state: &PlayState, rel: &str) -> Option<std::path::PathBuf> {
    for dir in &state.collections_dirs {
        let fp = dir.join(rel);
        if fp.exists() {
            return Some(fp);
        }
    }
    None
}

/// The primary collections dir (first one) — used for all writes.
fn primary_dir(state: &PlayState) -> &std::path::Path {
    &state.collections_dirs[0]
}

#[derive(Deserialize)]
pub struct ReflectRequest {
    pub address: String,
    pub tls: Option<bool>,
    pub tls_insecure: Option<bool>,
    /// Optional: collection path to load PROTO config from
    pub collection_path: Option<String>,
    /// Wire protocol: "grpc" (default), "grpc-web", "connectrpc"
    pub protocol: Option<String>,
}

#[derive(Serialize)]
pub struct MethodInfo {
    pub name: String,
    pub full_name: String,
    pub input_type: String,
    pub output_type: String,
    pub client_streaming: bool,
    pub server_streaming: bool,
}

#[derive(Serialize)]
pub struct ServiceInfo {
    pub name: String,
    pub full_name: String,
    pub methods: Vec<MethodInfo>,
}

#[derive(Deserialize)]
pub struct SchemaFillRequest {
    pub address: String,
    pub endpoint: String,
    pub tls: Option<bool>,
    pub tls_insecure: Option<bool>,
    pub collection_path: Option<String>,
    /// Wire protocol: "grpc" (default), "grpc-web", "connectrpc"
    pub protocol: Option<String>,
}

#[derive(Deserialize)]
pub struct ImportGrpcurlRequest {
    /// Raw command string (shell-parsed on frontend)
    pub command: Option<String>,
    /// Pre-parsed arguments (from frontend shell parser)
    pub args: Option<Vec<String>>,
}

#[derive(Serialize)]
pub struct ImportGrpcurlResponse {
    pub endpoint: String,
    pub address: String,
    pub headers: std::collections::HashMap<String, String>,
    pub body: String,
    pub plaintext: bool,
}

#[derive(Serialize)]
pub struct CollectionItem {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
    pub tags: Vec<String>,
}

/// Structured data extracted from a .gctf file — frontend-friendly, no gctf concepts.
#[derive(Debug, Serialize)]
pub struct CollectionParsed {
    pub endpoint: String,
    pub address: String,
    pub headers: std::collections::HashMap<String, String>,
    pub bodies: Vec<String>,
    pub asserts: Vec<String>,
    pub extracts: std::collections::HashMap<String, String>,
    pub meta_name: Option<String>,
    pub meta_tags: Vec<String>,
    pub meta_owner: Option<String>,
    pub meta_summary: Option<String>,
    pub tls: std::collections::HashMap<String, String>,
    pub options: std::collections::HashMap<String, String>,
    pub bench: std::collections::HashMap<String, String>,
    pub proto: std::collections::HashMap<String, String>,
}

fn parse_collection(doc: &crate::parser::GctfDocument) -> CollectionParsed {
    use crate::parser::SectionType;

    let get_section = |t: SectionType| -> Option<String> {
        doc.sections
            .iter()
            .find(|s| s.section_type == t)
            .and_then(|s| {
                use crate::parser::SectionContent;
                match &s.content {
                    SectionContent::Single(v) => Some(v.clone()),
                    SectionContent::Json(v) => serde_json::to_string_pretty(v).ok(),
                    SectionContent::KeyValues(kv) => Some(
                        kv.iter()
                            .map(|(k, v)| format!("{}: {}", k, v))
                            .collect::<Vec<_>>()
                            .join("\n"),
                    ),
                    _ => None,
                }
            })
    };

    let get_kv = |t: SectionType| -> std::collections::HashMap<String, String> {
        doc.sections
            .iter()
            .find(|s| s.section_type == t)
            .and_then(|s| {
                use crate::parser::SectionContent;
                match &s.content {
                    SectionContent::KeyValues(kv) => Some(kv.clone()),
                    _ => None,
                }
            })
            .unwrap_or_default()
    };

    let endpoint = get_section(SectionType::Endpoint).unwrap_or_default();
    let address = get_section(SectionType::Address).unwrap_or_default();
    let headers = get_kv(SectionType::RequestHeaders);

    let bodies: Vec<String> = doc
        .sections
        .iter()
        .filter(|s| s.section_type == SectionType::Request)
        .filter_map(|s| {
            use crate::parser::SectionContent;
            match &s.content {
                SectionContent::Json(v) => serde_json::to_string_pretty(v).ok(),
                _ => None,
            }
        })
        .collect();
    let bodies = if bodies.is_empty() {
        vec!["{}".to_string()]
    } else {
        bodies
    };

    let asserts: Vec<String> = doc
        .sections
        .iter()
        .filter(|s| s.section_type == SectionType::Asserts)
        .flat_map(|s| {
            use crate::parser::SectionContent;
            match &s.content {
                SectionContent::Assertions(lines) => lines.clone(),
                _ => vec![],
            }
        })
        .collect();

    let extracts: std::collections::HashMap<String, String> = doc
        .sections
        .iter()
        .filter(|s| s.section_type == SectionType::Extract)
        .filter_map(|s| {
            use crate::parser::SectionContent;
            match &s.content {
                SectionContent::Extract(m) => Some(m.clone()),
                _ => None,
            }
        })
        .fold(std::collections::HashMap::new(), |mut acc, m| {
            acc.extend(m);
            acc
        });

    let mut meta_name = None;
    let mut meta_tags = Vec::new();
    let mut meta_owner = None;
    let mut meta_summary = None;
    if let Some(meta_section) = doc
        .sections
        .iter()
        .find(|s| s.section_type == SectionType::Meta)
    {
        use crate::parser::SectionContent;
        if let SectionContent::Meta(m) = &meta_section.content {
            meta_name = m.name.clone();
            meta_tags = m.tags.clone();
            meta_owner = m.owner.clone();
            meta_summary = m.summary.clone();
        }
    }

    CollectionParsed {
        endpoint,
        address,
        headers,
        bodies,
        asserts,
        extracts,
        meta_name,
        meta_tags,
        meta_owner,
        meta_summary,
        tls: get_kv(SectionType::Tls),
        options: get_kv(SectionType::Options),
        bench: get_kv(SectionType::Bench),
        proto: get_kv(SectionType::Proto),
    }
}

#[derive(Serialize, Debug)]
pub struct CollectionResponse {
    pub content: String,
    pub path: String,
    pub parsed: CollectionParsed,
}

#[derive(Deserialize)]
pub struct ProtoUploadRequest {
    pub filename: String,
    pub content: String,
}

#[derive(Serialize)]
pub struct ProtoInfo {
    pub path: String,
    pub name: String,
    pub size: u64,
}

#[derive(Deserialize)]
pub struct SaveRequest {
    pub path: String,
    pub content: String,
}

#[derive(Deserialize)]
pub struct SaveRequestStructured {
    pub path: String,
    pub endpoint: String,
    pub address: Option<String>,
    pub headers: Option<std::collections::HashMap<String, String>>,
    pub bodies: Option<Vec<String>>,
    /// When set, preserves non-REQUEST sections from the original file
    pub original_path: Option<String>,
}

#[derive(Deserialize)]
pub struct CallRequest {
    pub endpoint: String,
    /// Single JSON object, array of objects, or null (when using bodies_raw).
    #[serde(default)]
    pub body: serde_json::Value,
    /// Raw body strings — each string is parsed as JSON on the backend.
    /// When set, overrides `body`. Preserves int64 precision.
    pub bodies_raw: Option<Vec<String>>,
    pub headers: Option<std::collections::HashMap<String, String>>,
    pub tls: Option<bool>,
    pub tls_insecure: Option<bool>,
    pub address: Option<String>,
    pub protocol: Option<String>,
    pub environment: Option<std::collections::HashMap<String, String>>,
    /// Optional: relative path to original .gctf collection.
    /// When set, the backend reads the file to get PROTO/TLS/OPTIONS sections.
    pub collection_path: Option<String>,
    /// Session ID for project history auto-save.
    pub session_id: Option<String>,
}

#[derive(Serialize)]
pub struct GrpcurlResponse {
    pub command: String,
}

#[derive(Serialize)]
pub struct CallResponse {
    pub success: bool,
    /// Always an array — one element for unary, multiple for streaming, empty for errors
    pub messages: Vec<serde_json::Value>,
    pub headers: std::collections::HashMap<String, String>,
    pub trailers: std::collections::HashMap<String, String>,
    pub error: Option<String>,
}

/// Extract tags from a .gctf file using the existing parser.
fn extract_tags(path: &std::path::Path) -> Vec<String> {
    let doc = match crate::parser::parse_gctf(path) {
        Ok(d) => d,
        Err(_) => return vec![],
    };
    for section in &doc.sections {
        if section.section_type == crate::parser::SectionType::Meta {
            use crate::parser::SectionContent;
            if let SectionContent::Meta(m) = &section.content {
                return m.tags.clone();
            }
        }
    }
    vec![]
}

/// GET /api/collections — list .gctf files + empty dirs recursively from all dirs
pub async fn list_collections(
    State(state): State<Arc<PlayState>>,
) -> Result<Json<Vec<CollectionItem>>, (StatusCode, String)> {
    let mut items = Vec::new();
    let mut seen_paths = std::collections::HashSet::new();
    let mut seen_dirs = std::collections::HashSet::new();

    for dir in &state.collections_dirs {
        if !dir.is_dir() {
            continue;
        }

        // Collect .gctf files
        for file in crate::utils::FileUtils::collect_test_files(dir, &[]) {
            let rel = file.strip_prefix(dir).unwrap_or(&file);
            let rel_str = rel.to_string_lossy().to_string();
            if seen_paths.insert(rel_str.clone()) {
                items.push(CollectionItem {
                    path: rel_str.clone(),
                    name: file
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string(),
                    is_dir: false,
                    tags: extract_tags(&file),
                });
            }
            // Track all parent directories as "seen" (they contain files)
            if let Some(parent) = rel.parent() {
                let parent_str = parent.to_string_lossy().to_string();
                if !parent_str.is_empty() {
                    seen_dirs.insert(parent_str);
                }
            }
        }

        // Find empty directories (not tracked by any file path)
        fn collect_empty_dirs(
            dir: &std::path::Path,
            base: &std::path::Path,
            seen_dirs: &mut std::collections::HashSet<String>,
            result: &mut Vec<CollectionItem>,
        ) {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        let rel = path.strip_prefix(base).unwrap_or(&path);
                        let rel_str = rel.to_string_lossy().to_string();
                        if !seen_dirs.contains(&rel_str) {
                            result.push(CollectionItem {
                                path: rel_str.clone(),
                                name: entry.file_name().to_string_lossy().to_string(),
                                is_dir: true,
                                tags: vec![],
                            });
                            seen_dirs.insert(rel_str); // prevent duplicates
                        }
                        // Recurse into subdirectories
                        collect_empty_dirs(&path, base, seen_dirs, result);
                    }
                }
            }
        }

        collect_empty_dirs(dir, dir, &mut seen_dirs, &mut items);
    }

    // Sort: dirs first, then files, alphabetical
    items.sort_by(|a, b| {
        if a.is_dir != b.is_dir {
            return if a.is_dir {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Greater
            };
        }
        a.path.cmp(&b.path)
    });

    Ok(Json(items))
}

/// GET /api/collections/*path — read a .gctf file, returns raw + parsed
pub async fn get_collection(
    State(state): State<Arc<PlayState>>,
    path: Path<String>,
) -> Result<Json<CollectionResponse>, (StatusCode, String)> {
    reject_traversal(&path.0)?;
    let file_path = resolve_file(&state, &path.0)
        .ok_or_else(|| (StatusCode::NOT_FOUND, "File not found".to_string()))?;
    if file_path.is_dir() {
        return Err((StatusCode::NOT_FOUND, "Path is a directory".to_string()));
    }
    let content = std::fs::read_to_string(&file_path)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let doc = crate::parser::parse_gctf_from_str(&content, &file_path.to_string_lossy())
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("Parse error: {}", e)))?;
    Ok(Json(CollectionResponse {
        path: file_path.to_string_lossy().to_string(),
        content,
        parsed: parse_collection(&doc),
    }))
}

/// POST /api/save — save raw content to a .gctf file
pub async fn save_collection(
    State(state): State<Arc<PlayState>>,
    Json(req): Json<SaveRequest>,
) -> Result<Json<()>, (StatusCode, String)> {
    reject_traversal(&req.path)?;
    let file_path = primary_dir(&state).join(&req.path);
    if let Some(parent) = file_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::fs::write(&file_path, &req.content)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    state
        .collections_mtime
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    Ok(Json(()))
}

/// POST /api/save-structured — save structured request data, backend builds .gctf
pub async fn save_collection_structured(
    State(state): State<Arc<PlayState>>,
    Json(req): Json<SaveRequestStructured>,
) -> Result<Json<()>, (StatusCode, String)> {
    reject_traversal(&req.path)?;
    let file_path = primary_dir(&state).join(&req.path);

    // Build base doc via builder, then merge preserved sections
    let mut builder = crate::parser::GctfDocumentBuilder::new().with_file_path(&req.path);

    if let Some(ref addr) = req.address
        && !addr.is_empty()
    {
        builder = builder.address(addr);
    }
    builder = builder.endpoint(&req.endpoint);

    if let Some(ref headers) = req.headers
        && !headers.is_empty()
    {
        builder = builder.request_headers(headers.clone());
    }

    if let Some(ref bodies) = req.bodies {
        for b in bodies {
            if let Ok(val) = serde_json::from_str(b) {
                builder = builder.request(val);
            }
        }
    }

    let mut doc = builder.build();

    // Preserve non-edited sections from original file
    if let Some(ref orig_path) = req.original_path {
        if orig_path.contains("..") || orig_path.starts_with('/') {
            return Err((StatusCode::NOT_FOUND, "Invalid original_path".to_string()));
        }
        let orig_file =
            resolve_file(&state, orig_path).unwrap_or_else(|| primary_dir(&state).join(orig_path));
        if orig_file.exists()
            && let Ok(orig_doc) = crate::parser::parse_gctf(&orig_file)
        {
            for s in &orig_doc.sections {
                match s.section_type {
                    crate::parser::SectionType::Request
                    | crate::parser::SectionType::Address
                    | crate::parser::SectionType::Endpoint
                    | crate::parser::SectionType::RequestHeaders => continue,
                    _ => doc.sections.push(s.clone()),
                }
            }
        }
    }

    let content = crate::parser::serialize_gctf(&doc);
    if let Some(parent) = file_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::fs::write(&file_path, &content)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    state
        .collections_mtime
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    Ok(Json(()))
}

/// POST /api/proto-upload — upload a .proto file into the collections directory
pub async fn proto_upload(
    State(state): State<Arc<PlayState>>,
    Json(req): Json<ProtoUploadRequest>,
) -> Result<Json<()>, (StatusCode, String)> {
    let filename = req.filename.trim().to_string();
    if filename.is_empty() || !filename.ends_with(".proto") {
        return Err((
            StatusCode::BAD_REQUEST,
            "Filename must end with .proto".to_string(),
        ));
    }
    if filename.contains("..") || filename.contains('/') {
        return Err((StatusCode::BAD_REQUEST, "Invalid filename".to_string()));
    }
    let file_path = primary_dir(&state).join(&filename);
    std::fs::write(&file_path, &req.content)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    state
        .collections_mtime
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    Ok(Json(()))
}

/// GET /api/proto-files — list available .proto files in all collections dirs
pub async fn proto_files(State(state): State<Arc<PlayState>>) -> Json<Vec<ProtoInfo>> {
    let mut files = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for dir in &state.collections_dirs {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                if path.extension().and_then(|e| e.to_str()) == Some("proto")
                    && seen.insert(name.clone())
                    && let Ok(meta) = path.metadata()
                {
                    files.push(ProtoInfo {
                        path: path.to_string_lossy().to_string(),
                        name,
                        size: meta.len(),
                    });
                }
            }
        }
    }
    Json(files)
}

#[derive(Serialize)]
pub struct ReflectResponse {
    pub services: Vec<ServiceInfo>,
    pub error: Option<String>,
}

/// POST /api/reflect — list services and methods via reflection
pub async fn reflect_server(
    State(state): State<Arc<PlayState>>,
    Json(req): Json<ReflectRequest>,
) -> Json<ReflectResponse> {
    let tls_config = if req.tls.unwrap_or(false) {
        Some(crate::grpc::TlsConfig {
            ca_cert_path: None,
            client_cert_path: None,
            client_key_path: None,
            server_name: None,
            insecure_skip_verify: req.tls_insecure.unwrap_or(true),
        })
    } else {
        None
    };

    // Resolve proto config from collection if provided
    let proto_config = if let Some(ref coll_path) = req.collection_path {
        if coll_path.contains("..") {
            return Json(ReflectResponse {
                services: vec![],
                error: Some("Invalid collection_path".into()),
            });
        }
        let file_path =
            resolve_file(&state, coll_path).unwrap_or_else(|| primary_dir(&state).join(coll_path));
        if file_path.exists() {
            let parse_result = crate::parser::parse_with_recovery(&file_path);
            crate::execution::runner_helpers::build_proto_config(&parse_result.document, &file_path)
        } else {
            None
        }
    } else {
        None
    };

    let config = crate::grpc::GrpcClientConfig {
        address: req.address.clone(),
        timeout_seconds: 10,
        tls_config,
        proto_config,
        metadata: None,
        target_service: None,
        compression: Default::default(),
        connection_id: 0,
        protocol: parse_protocol(req.protocol.as_deref()),
    };

    let client = match crate::grpc::GrpcClient::new(config).await {
        Ok(c) => c,
        Err(e) => {
            return Json(ReflectResponse {
                services: vec![],
                error: Some(format!("Reflection failed: {}", e)),
            });
        }
    };

    let pool = client.descriptor_pool();
    let mut services = Vec::new();

    for svc in pool.services() {
        let mut methods = Vec::new();
        for m in svc.methods() {
            methods.push(MethodInfo {
                name: m.name().to_string(),
                full_name: format!("{}/{}", svc.full_name(), m.name()),
                input_type: m.input().full_name().to_string(),
                output_type: m.output().full_name().to_string(),
                client_streaming: m.is_client_streaming(),
                server_streaming: m.is_server_streaming(),
            });
        }
        services.push(ServiceInfo {
            name: svc.name().to_string(),
            full_name: svc.full_name().to_string(),
            methods,
        });
    }

    Json(ReflectResponse {
        services,
        error: None,
    })
}

/// POST /api/import-grpcurl — parse grpcurl command into request fields
pub async fn import_grpcurl(
    Json(req): Json<ImportGrpcurlRequest>,
) -> Result<Json<ImportGrpcurlResponse>, (StatusCode, Json<ApiError>)> {
    // Use pre-parsed args from frontend, or fall back to splitting raw command
    let args: Vec<String> = if let Some(a) = req.args {
        a
    } else if let Some(cmd) = req.command {
        // Fallback: basic whitespace split (frontend usually sends pre-parsed args)
        cmd.split_whitespace().map(|s| s.to_string()).collect()
    } else {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                error: "Empty command".into(),
            }),
        ));
    };

    if args.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                error: "Empty command".into(),
            }),
        ));
    }
    // Skip "grpcurl" binary name if present
    let grpcurl_args = if args.first().map(|s| s.as_str()) == Some("grpcurl") {
        &args[1..]
    } else {
        &args[..]
    };

    match crate::grpc::grpcurl_invocation::ParsedGrpcurl::parse(grpcurl_args) {
        Ok(parsed) => {
            let body_str = serde_json::to_string_pretty(&parsed.request_body).unwrap_or_default();
            Ok(Json(ImportGrpcurlResponse {
                endpoint: parsed.symbol,
                address: parsed.address,
                headers: parsed.headers,
                body: body_str,
                plaintext: parsed.options.contains_key("plaintext"),
            }))
        }
        Err(e) => Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                error: e.to_string(),
            }),
        )),
    }
}

/// POST /api/schema-fill — generate JSON template from proto message descriptor
#[derive(Serialize)]
pub struct SchemaFillResponse {
    pub schema: Option<serde_json::Value>,
    pub error: Option<String>,
}

pub async fn schema_fill(
    State(state): State<Arc<PlayState>>,
    Json(req): Json<SchemaFillRequest>,
) -> Json<SchemaFillResponse> {
    let parts: Vec<&str> = req.endpoint.split('/').collect();
    if parts.len() != 2 {
        return Json(SchemaFillResponse {
            schema: None,
            error: Some("Invalid endpoint format".into()),
        });
    }
    let (full_service, method_name) = (parts[0], parts[1]);

    // Build TLS config
    let tls_config = if req.tls.unwrap_or(false) {
        Some(crate::grpc::TlsConfig {
            ca_cert_path: None,
            client_cert_path: None,
            client_key_path: None,
            server_name: None,
            insecure_skip_verify: req.tls_insecure.unwrap_or(true),
        })
    } else {
        None
    };

    // Resolve proto config from collection if provided
    let proto_config = if let Some(ref coll_path) = req.collection_path {
        if coll_path.contains("..") {
            return Json(SchemaFillResponse {
                schema: None,
                error: Some("Invalid collection_path".into()),
            });
        }
        let file_path =
            resolve_file(&state, coll_path).unwrap_or_else(|| primary_dir(&state).join(coll_path));
        if file_path.exists() {
            let parse_result = crate::parser::parse_with_recovery(&file_path);
            crate::execution::runner_helpers::build_proto_config(&parse_result.document, &file_path)
        } else {
            None
        }
    } else {
        None
    };

    // Create a temporary gRPC client to load descriptors
    let grpc_config = crate::grpc::GrpcClientConfig {
        address: req.address.clone(),
        timeout_seconds: 10,
        tls_config,
        proto_config,
        metadata: None,
        target_service: Some(full_service.to_string()),
        compression: Default::default(),
        connection_id: 0,
        protocol: parse_protocol(req.protocol.as_deref()),
    };

    let client = match crate::grpc::GrpcClient::new(grpc_config).await {
        Ok(c) => c,
        Err(e) => {
            return Json(SchemaFillResponse {
                schema: None,
                error: Some(format!("Failed to load descriptors: {}", e)),
            });
        }
    };

    let pool = client.descriptor_pool();
    let svc = match pool.get_service_by_name(full_service) {
        Some(s) => s,
        None => {
            return Json(SchemaFillResponse {
                schema: None,
                error: Some(format!("Service '{}' not found", full_service)),
            });
        }
    };
    let method = match svc.methods().find(|m| m.name() == method_name) {
        Some(m) => m,
        None => {
            return Json(SchemaFillResponse {
                schema: None,
                error: Some(format!(
                    "Method '{}' not found in '{}'",
                    method_name, full_service
                )),
            });
        }
    };

    // Generate JSON template from input message descriptor
    let input_desc = method.input();
    let template = generate_json_template(&input_desc);

    Json(SchemaFillResponse {
        schema: Some(template),
        error: None,
    })
}

/// Generate a fake value for a given field name + type.
/// Uses the `fake` crate for realistic data, with field-name heuristics.
fn fake_value(field_name: &str, kind: &prost_reflect::Kind) -> serde_json::Value {
    use fake::Fake;
    use prost_reflect::Kind;

    let n = rand::random::<u32>() % 100000;

    match kind {
        Kind::Double | Kind::Float => serde_json::json!((n as f64) / 10.0),
        Kind::Int32 | Kind::Sint32 | Kind::Sfixed32 => serde_json::json!(n as i32),
        Kind::Int64 | Kind::Sint64 | Kind::Sfixed64 => serde_json::json!((n * 100) as i64),
        Kind::Uint32 | Kind::Fixed32 => serde_json::json!(n),
        Kind::Uint64 | Kind::Fixed64 => serde_json::json!((n * 100) as u64),
        Kind::Bool => serde_json::json!(n.is_multiple_of(2)),

        Kind::String => {
            let lower = field_name.to_lowercase();
            let val: String = if lower.contains("email") || lower.contains("mail") {
                fake::faker::internet::en::FreeEmail().fake()
            } else if lower.contains("name")
                && (lower.contains("first") || lower.starts_with("first"))
            {
                fake::faker::name::en::FirstName().fake()
            } else if lower.contains("name")
                && (lower.contains("last") || lower.contains("surname"))
            {
                fake::faker::name::en::LastName().fake()
            } else if lower.contains("name") {
                fake::faker::name::en::Name().fake()
            } else if lower.contains("phone") || lower.contains("tel") {
                fake::faker::phone_number::en::PhoneNumber().fake()
            } else if lower.contains("url") || lower.contains("uri") || lower.contains("link") {
                fake::faker::internet::en::FreeEmail().fake()
            } else if lower.contains("uuid") || lower.contains("guid") {
                let u = uuid::Uuid::new_v4();
                u.to_string()
            } else if lower.contains("address") || lower.contains("street") {
                format!(
                    "{} {}",
                    fake::faker::address::en::StreetName().fake::<String>(),
                    rand::random::<u16>() % 10000 + 1
                )
            } else if lower.contains("city") {
                fake::faker::address::en::CityName().fake()
            } else if lower.contains("country") {
                fake::faker::address::en::CountryName().fake()
            } else if lower.contains("zip")
                || lower.contains("postal")
                || lower.contains("postcode")
            {
                fake::faker::address::en::PostCode().fake()
            } else if lower.contains("password") || lower.contains("secret") {
                "••••••••".to_string()
            } else if lower.contains("token") {
                format!("tok_{:x}", uuid::Uuid::new_v4().as_u128() >> 64)
            } else if lower.contains("description")
                || lower.contains("comment")
                || lower.contains("note")
                || lower.contains("bio")
            {
                fake::faker::lorem::en::Paragraph(3..6).fake()
            } else if lower.contains("sentence")
                || lower.contains("text")
                || lower.contains("content")
            {
                fake::faker::lorem::en::Sentence(3..8).fake()
            } else if lower.contains("status") {
                ["active", "inactive", "pending"][n as usize % 3].to_string()
            } else if lower.contains("type") || lower.contains("kind") || lower.contains("category")
            {
                ["standard", "premium", "basic"][n as usize % 3].to_string()
            } else if lower.contains("date")
                || lower.contains("time")
                || lower.contains("timestamp")
                || lower.contains("at")
            {
                "2024-06-15T10:30:00Z".to_string()
            } else if lower.contains("color") {
                ["#3b82f6", "#ef4444", "#22c55e", "#f59e0b"][n as usize % 4].to_string()
            } else if lower.contains("lang") || lower.contains("locale") {
                "en-US".to_string()
            } else if lower.contains("avatar")
                || lower.contains("image")
                || lower.contains("photo")
                || lower.contains("picture")
                || lower.contains("icon")
            {
                format!("https://i.pravatar.cc/150?u={}", n)
            } else if lower.contains("title")
                || lower.contains("subject")
                || lower.contains("heading")
            {
                fake::faker::lorem::en::Sentence(3..8).fake()
            } else if lower.contains("company")
                || lower.contains("organization")
                || lower.contains("org")
            {
                fake::faker::company::en::CompanyName().fake()
            } else if lower.contains("job") || lower.contains("position") {
                fake::faker::job::en::Title().fake()
            } else if lower == "first" || lower == "first_name" {
                fake::faker::name::en::FirstName().fake()
            } else if lower == "last"
                || lower == "last_name"
                || lower == "surname"
                || lower.contains("last")
            {
                fake::faker::name::en::LastName().fake()
            } else if lower.contains("username")
                || lower.contains("nick")
                || lower.contains("handle")
            {
                fake::faker::internet::en::Username().fake()
            } else {
                fake::faker::lorem::en::Word().fake()
            };
            serde_json::Value::String(val)
        }

        Kind::Bytes => serde_json::Value::String(format!("{} bytes of data", n)),

        Kind::Enum(enum_desc) => {
            let first = enum_desc.values().next();
            match first {
                Some(v) => serde_json::Value::String(v.name().to_string()),
                None => serde_json::Value::String("UNSPECIFIED".to_string()),
            }
        }

        Kind::Message(msg_desc) => {
            let full = msg_desc.full_name();
            if full == "google.protobuf.Timestamp" {
                serde_json::json!("2024-06-15T10:30:00Z")
            } else if full == "google.protobuf.Duration" {
                serde_json::json!("30s")
            } else if full == "google.protobuf.FieldMask" {
                serde_json::json!({"paths": [fake::faker::lorem::en::Word().fake::<String>()]})
            } else if full == "google.protobuf.Struct" {
                serde_json::json!({fake::faker::lorem::en::Word().fake::<String>(): "value"})
            } else if full == "google.protobuf.Value" {
                serde_json::json!(fake::faker::lorem::en::Word().fake::<String>())
            } else if full == "google.protobuf.Any" {
                serde_json::json!({"@type": "type.googleapis.com/example.Message", "field": fake::faker::lorem::en::Word().fake::<String>()})
            } else {
                generate_json_template(msg_desc)
            }
        }
    }
}

/// Recursively generate a JSON template from a protobuf message descriptor.
/// Fills in fake/realistic data based on field names.
fn generate_json_template(desc: &prost_reflect::MessageDescriptor) -> serde_json::Value {
    let mut obj = serde_json::Map::new();

    for field in desc.fields() {
        let name = field.json_name().to_string();
        let fv = fake_value(&name, &field.kind());

        if field.is_list() {
            obj.insert(name, serde_json::Value::Array(vec![fv]));
        } else if field.is_map() {
            obj.insert(name, serde_json::Value::Object(serde_json::Map::new()));
        } else {
            obj.insert(name, fv);
        }
    }

    serde_json::Value::Object(obj)
}

/// POST /api/grpcurl — generate grpcurl command from current request
pub async fn generate_grpcurl(Json(req): Json<CallRequest>) -> Json<GrpcurlResponse> {
    // Build a temporary .gctf and convert via existing machinery
    let messages: Vec<serde_json::Value> = match &req.body {
        serde_json::Value::Array(arr) => arr.clone(),
        other => vec![other.clone()],
    };

    let mut builder = crate::parser::GctfDocumentBuilder::new()
        .with_file_path("<convert>")
        .endpoint(&req.endpoint);

    for msg in &messages {
        builder = builder.request(msg.clone());
    }

    if let Some(headers) = &req.headers
        && !headers.is_empty()
    {
        builder = builder.request_headers(headers.clone());
    }

    let doc = builder.build();
    let cwd = std::env::current_dir().unwrap_or_default();
    let command = crate::commands::grpcurl::build_grpcurl_command(
        &doc,
        std::path::Path::new("<inline>"),
        &cwd,
        1,
    );

    match command {
        Ok(output) => Json(GrpcurlResponse {
            command: output.command,
        }),
        Err(e) => Json(GrpcurlResponse {
            command: format!("# error: {}", e),
        }),
    }
}

/// POST /api/call — execute a gRPC call directly, return raw response
pub async fn execute_call(
    State(state): State<Arc<PlayState>>,
    Json(req): Json<CallRequest>,
) -> Result<Json<CallResponse>, (StatusCode, String)> {
    // Resolve endpoint
    let parts: Vec<&str> = req.endpoint.split('/').collect();
    if parts.len() != 2 {
        return Err((
            StatusCode::BAD_REQUEST,
            "Invalid endpoint format".to_string(),
        ));
    }
    let (full_service, method_name) = (parts[0].to_string(), parts[1].to_string());

    // Build messages array (bodies_raw preserves int64 precision)
    let messages: Vec<serde_json::Value> = if let Some(raw_bodies) = &req.bodies_raw {
        raw_bodies
            .iter()
            .filter_map(|s| serde_json::from_str(s).ok())
            .collect()
    } else if let serde_json::Value::Array(arr) = &req.body {
        arr.clone()
    } else if req.body.is_null() {
        vec![]
    } else {
        vec![req.body.clone()]
    };
    if messages.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "No request messages".to_string()));
    }

    // Build TLS config
    let tls_config = if req.tls.unwrap_or(false) {
        Some(crate::grpc::TlsConfig {
            ca_cert_path: None,
            client_cert_path: None,
            client_key_path: None,
            server_name: None,
            insecure_skip_verify: req.tls_insecure.unwrap_or(true),
        })
    } else {
        None
    };

    // Resolve proto config from collection if provided
    let proto_config = if let Some(ref coll_path) = req.collection_path {
        if coll_path.contains("..") {
            return Err((StatusCode::NOT_FOUND, "Invalid collection_path".to_string()));
        }
        let file_path =
            resolve_file(&state, coll_path).unwrap_or_else(|| primary_dir(&state).join(coll_path));
        if file_path.exists() {
            let parse_result = crate::parser::parse_with_recovery(&file_path);
            crate::execution::runner_helpers::build_proto_config(&parse_result.document, &file_path)
        } else {
            None
        }
    } else {
        None
    };

    let address = req.address.as_deref().unwrap_or("localhost:4770");

    // Substitute environment variables in headers
    let env_ref = req.environment.as_ref();
    let substituted_headers: Option<std::collections::HashMap<String, String>> =
        req.headers.as_ref().map(|h| {
            h.iter()
                .map(|(k, v)| {
                    let mut val = v.clone();
                    if let Some(env) = env_ref {
                        for (ek, ev) in env {
                            val = val.replace(&format!("{{{{{}}}}}", ek), ev);
                        }
                    }
                    (k.clone(), val)
                })
                .collect()
        });

    // Resolve wire protocol
    let protocol = parse_protocol(req.protocol.as_deref());

    // Create gRPC client
    let grpc_config = crate::grpc::GrpcClientConfig {
        address: address.to_string(),
        timeout_seconds: 30,
        tls_config,
        proto_config,
        metadata: substituted_headers,
        target_service: Some(full_service.clone()),
        compression: Default::default(),
        connection_id: 0,
        protocol,
    };

    let mut client = match crate::grpc::GrpcClient::new(grpc_config).await {
        Ok(c) => c,
        Err(e) => {
            return Ok(Json(CallResponse {
                success: false,
                messages: Vec::new(),
                headers: std::collections::HashMap::new(),
                trailers: std::collections::HashMap::new(),
                error: Some(e.to_string()),
            }));
        }
    };

    // Send request and collect response
    let stream = futures::stream::iter(messages);

    let (resp_headers, mut response_stream) = match client
        .call_stream(&full_service, &method_name, stream)
        .await
    {
        Ok(r) => r,
        Err(e) => {
            return Ok(Json(CallResponse {
                success: false,
                messages: Vec::new(),
                headers: std::collections::HashMap::new(),
                trailers: std::collections::HashMap::new(),
                error: Some(e.to_string()),
            }));
        }
    };

    let mut response_messages = Vec::new();
    let mut response_trailers = std::collections::HashMap::new();
    let mut response_error = None;

    use futures::StreamExt;
    while let Some(item) = response_stream.next().await {
        match item {
            Ok(crate::grpc::client::StreamItem::Message(msg)) => {
                response_messages.push(msg);
            }
            Ok(crate::grpc::client::StreamItem::Trailers(trailers)) => {
                response_trailers = trailers.clone();
                if let Some(status) = trailers.get("grpc-status")
                    && status != "0"
                {
                    let msg = trailers.get("grpc-message").cloned().unwrap_or_default();
                    response_error = Some(format!("gRPC error: code={} message={}", status, msg));
                }
            }
            Err(status) => {
                response_error = Some(format!(
                    "gRPC error: code={} message={}",
                    status.code(),
                    status.message()
                ));
            }
        }
    }

    let success = response_error.is_none();

    // Auto-save history entry if session_id is provided
    if let Some(sid) = req.session_id.clone() {
        let hist_body = req.bodies_raw.clone().unwrap_or_default();
        let hist_headers = req.headers.clone().unwrap_or_default();
        if let Ok(root) = require_project(&state) {
            let entry = serde_json::json!({
                "id": uuid::Uuid::new_v4().to_string(),
                "timestamp": std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis(),
                "endpoint": req.endpoint,
                "bodies": hist_body,
                "headers": hist_headers,
                "response": {
                    "status": if success { "ok" } else { "error" },
                    "error": response_error.clone(),
                    "messages": response_messages.clone(),
                    "headers": resp_headers.clone(),
                    "trailers": response_trailers.clone(),
                },
            });

            if let Ok(line) = serde_json::to_string(&entry)
                && let Ok(_guard) = state.history_lock.lock()
            {
                super::project::append_history_entry(&root, &sid, &line).ok();
            }
        }
    }

    Ok(Json(CallResponse {
        success,
        messages: response_messages,
        headers: resp_headers,
        trailers: response_trailers,
        error: response_error,
    }))
}

/* ── Project mode helpers ──────────────────────────── */

fn require_project(state: &PlayState) -> Result<std::path::PathBuf, (StatusCode, String)> {
    state
        .project_root
        .clone()
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Not in project mode".into()))
}

/* ── Project mode API handlers ───────────────────────────── */

#[derive(Serialize)]
pub struct ProjectInfo {
    pub active: bool,
    pub envs: Vec<String>,
    pub collections_dir: String,
    pub project_dir: Option<String>,
}

pub fn project_info_inner(state: &PlayState) -> ProjectInfo {
    let root = match state.project_root.as_ref() {
        Some(r) => r.clone(),
        None => {
            return ProjectInfo {
                active: false,
                envs: vec![],
                collections_dir: state.collections_dir.display().to_string(),
                project_dir: None,
            };
        }
    };
    let envs = root
        .is_dir()
        .then(|| super::project::list_env_files(&root).ok())
        .flatten()
        .unwrap_or_default();
    ProjectInfo {
        active: state.project_root.is_some(),
        envs,
        collections_dir: state.collections_dir.display().to_string(),
        project_dir: state.project_root.as_ref().map(|p| p.display().to_string()),
    }
}

pub async fn project_info(State(state): State<Arc<PlayState>>) -> Json<ProjectInfo> {
    Json(project_info_inner(&state))
}

#[derive(Serialize, Deserialize)]
pub struct ProjectSettingsResponse {
    pub address: String,
    pub protocol: String,
    pub tls: bool,
    pub tls_insecure: bool,
    pub active_env: Option<String>,
}

pub async fn project_get_settings(
    State(state): State<Arc<PlayState>>,
) -> Result<Json<ProjectSettingsResponse>, (StatusCode, String)> {
    let root = require_project(&state)?;
    let settings = super::project::load_project_settings(&root)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(ProjectSettingsResponse {
        address: settings.address,
        protocol: settings.protocol,
        tls: settings.tls,
        tls_insecure: settings.tls_insecure,
        active_env: settings.active_env,
    }))
}

#[derive(Deserialize)]
pub struct ProjectSettingsUpdate {
    pub address: Option<String>,
    pub protocol: Option<String>,
    pub tls: Option<bool>,
    pub tls_insecure: Option<bool>,
    pub active_env: Option<String>,
}

pub async fn project_put_settings(
    State(state): State<Arc<PlayState>>,
    Json(update): Json<ProjectSettingsUpdate>,
) -> Result<Json<()>, (StatusCode, String)> {
    let root = require_project(&state)?;
    let mut settings = super::project::load_project_settings(&root)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if let Some(v) = update.address {
        settings.address = v;
    }
    if let Some(v) = update.protocol {
        settings.protocol = v;
    }
    if let Some(v) = update.tls {
        settings.tls = v;
    }
    if let Some(v) = update.tls_insecure {
        settings.tls_insecure = v;
    }
    settings.active_env = update.active_env;
    super::project::save_project_settings(&root, &settings)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(()))
}

pub async fn project_env_list(
    State(state): State<Arc<PlayState>>,
) -> Result<Json<Vec<String>>, (StatusCode, String)> {
    let root = require_project(&state)?;
    let names = super::project::list_env_files(&root)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(names))
}

pub async fn project_env_get(
    State(state): State<Arc<PlayState>>,
    Path(name): Path<String>,
) -> Result<Json<String>, (StatusCode, String)> {
    let root = require_project(&state)?;
    let content = super::project::read_dotenv(&root, &name)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                format!("Environment '{}' not found", name),
            )
        })?;
    Ok(Json(content))
}

#[derive(Deserialize)]
pub struct EnvPutBody {
    pub content: String,
}

pub async fn project_env_put(
    State(state): State<Arc<PlayState>>,
    Path(name): Path<String>,
    Json(body): Json<EnvPutBody>,
) -> Result<Json<()>, (StatusCode, String)> {
    let root = require_project(&state)?;
    super::project::write_dotenv(&root, &name, &body.content)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(()))
}

#[derive(Serialize)]
pub struct EnvLocalStatus {
    pub exists: bool,
    pub content: Option<String>,
}

#[derive(Serialize)]
pub struct EnvMergedResponse {
    pub variables: std::collections::HashMap<String, String>,
    pub has_local: bool,
    pub address: Option<String>,
}

pub async fn project_env_merged(
    State(state): State<Arc<PlayState>>,
    Path(name): Path<String>,
) -> Result<Json<EnvMergedResponse>, (StatusCode, String)> {
    let root = require_project(&state)?;
    let shared_raw = super::project::read_dotenv(&root, &name)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .unwrap_or_default();
    let local_raw = super::project::read_dotenv_local(&root, &name)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .unwrap_or_default();

    fn parse_dotenv(s: &str) -> std::collections::HashMap<String, String> {
        let mut map = std::collections::HashMap::new();
        for line in s.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(eq) = line.find('=') {
                let key = line[..eq].trim().to_string();
                let val = line[eq + 1..].trim().to_string();
                if !key.is_empty() {
                    map.insert(key, val);
                }
            }
        }
        map
    }
    let shared = parse_dotenv(&shared_raw);
    let local = parse_dotenv(&local_raw);
    let mut variables = shared;
    for (k, v) in local {
        variables.insert(k, v);
    }

    let address = variables.remove("GRPC_ADDRESS");
    Ok(Json(EnvMergedResponse {
        variables,
        has_local: !local_raw.is_empty(),
        address,
    }))
}

pub async fn project_env_local_get(
    State(state): State<Arc<PlayState>>,
    Path(name): Path<String>,
) -> Result<Json<EnvLocalStatus>, (StatusCode, String)> {
    let root = require_project(&state)?;
    let content = super::project::read_dotenv_local(&root, &name)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(EnvLocalStatus {
        exists: content.is_some(),
        content,
    }))
}

pub async fn project_env_local_put(
    State(state): State<Arc<PlayState>>,
    Path(name): Path<String>,
    Json(body): Json<EnvPutBody>,
) -> Result<Json<()>, (StatusCode, String)> {
    let root = require_project(&state)?;
    super::project::write_dotenv_local(&root, &name, &body.content)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(()))
}

pub async fn project_env_local_delete(
    State(state): State<Arc<PlayState>>,
    Path(name): Path<String>,
) -> Result<Json<()>, (StatusCode, String)> {
    let root = require_project(&state)?;
    super::project::delete_dotenv_local(&root, &name)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(()))
}

/// GET /api/project/history — read ALL session history files, return {sessions: {id: [...]}}
pub async fn project_history_get(
    State(state): State<Arc<PlayState>>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let root = require_project(&state)?;
    let sessions = super::project::list_history_sessions(&root)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let mut map = serde_json::Map::new();
    for sid in &sessions {
        if let Ok(lines) = super::project::read_history_session(&root, sid) {
            let entries: Vec<serde_json::Value> = lines
                .iter()
                .filter_map(|l| serde_json::from_str(l).ok())
                .collect();
            if !entries.is_empty() {
                map.insert(sid.clone(), serde_json::Value::Array(entries));
            }
        }
    }
    Ok(Json(serde_json::Value::Object(map)))
}

/// DELETE /api/collections/{*path} — delete a file
pub async fn delete_collection(
    State(state): State<Arc<PlayState>>,
    Path(path): Path<String>,
) -> Result<Json<()>, (StatusCode, String)> {
    reject_traversal(&path)?;
    let file_path = resolve_file(&state, &path)
        .ok_or_else(|| (StatusCode::NOT_FOUND, "File not found".to_string()))?;
    if file_path.is_dir() {
        std::fs::remove_dir_all(&file_path).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to delete directory: {}", e),
            )
        })?;
    } else {
        std::fs::remove_file(&file_path).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to delete file: {}", e),
            )
        })?;
    }
    state
        .collections_mtime
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    Ok(Json(()))
}

/* ── Directory and file operations ──────────────────── */

/// POST /api/dir/{*path} — create a directory (mkdir -p)
pub async fn create_directory(
    State(state): State<Arc<PlayState>>,
    Path(path): Path<String>,
) -> Result<Json<()>, (StatusCode, String)> {
    reject_traversal(&path)?;
    let dir_path = primary_dir(&state).join(&path);
    std::fs::create_dir_all(&dir_path).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to create directory: {}", e),
        )
    })?;
    state
        .collections_mtime
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    Ok(Json(()))
}

#[derive(Deserialize)]
pub struct MoveRequest {
    pub from: String,
    pub to: String,
}

/// POST /api/move — move/rename a file or directory
pub async fn move_item(
    State(state): State<Arc<PlayState>>,
    Json(req): Json<MoveRequest>,
) -> Result<Json<()>, (StatusCode, String)> {
    reject_traversal(&req.from)?;
    reject_traversal(&req.to)?;
    let src =
        resolve_file(&state, &req.from).unwrap_or_else(|| primary_dir(&state).join(&req.from));
    let dst = primary_dir(&state).join(&req.to);

    if !src.exists() {
        return Err((
            StatusCode::NOT_FOUND,
            format!("Source not found: {}", req.from),
        ));
    }
    if dst.exists() {
        return Err((
            StatusCode::CONFLICT,
            format!("Destination already exists: {}", req.to),
        ));
    }

    // Ensure parent directory of destination exists
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to create parent: {}", e),
            )
        })?;
    }

    std::fs::rename(&src, &dst).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to move: {}", e),
        )
    })?;
    state
        .collections_mtime
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    Ok(Json(()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(not(miri))]
    use std::path::PathBuf;
    use std::sync::atomic::AtomicU64;

    #[test]
    fn test_reject_traversal_valid() {
        assert!(reject_traversal("foo.gctf").is_ok());
        assert!(reject_traversal("dir/foo.gctf").is_ok());
        assert!(reject_traversal("a/b/c.gctf").is_ok());
    }

    #[test]
    fn test_reject_traversal_invalid() {
        assert!(reject_traversal("../foo.gctf").is_err());
        assert!(reject_traversal("dir/../../foo.gctf").is_err());
        assert!(reject_traversal("/etc/passwd").is_err());
    }

    #[cfg(not(miri))]
    #[test]
    fn test_resolve_file_nonexistent() {
        let state = PlayState {
            collections_dir: PathBuf::from("/tmp/nonexistent_XXXX"),
            collections_dirs: vec![PathBuf::from("/tmp/nonexistent_XXXX")],
            project_root: None,
            project_settings: None,
            history_lock: std::sync::Mutex::new(()),
            collections_mtime: Arc::new(AtomicU64::new(0)),
        };
        assert!(resolve_file(&state, "foo.gctf").is_none());
    }

    #[cfg(not(miri))]
    #[test]
    fn test_get_collection_returns_404_for_directory() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("emptydir");
        std::fs::create_dir(&sub).unwrap();

        let state = Arc::new(PlayState {
            collections_dir: dir.path().to_path_buf(),
            collections_dirs: vec![dir.path().to_path_buf()],
            project_root: None,
            project_settings: None,
            history_lock: std::sync::Mutex::new(()),
            collections_mtime: Arc::new(AtomicU64::new(0)),
        });

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(get_collection(State(state), Path("emptydir".to_string())));
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::NOT_FOUND);
    }

    #[cfg(not(miri))]
    #[test]
    fn test_get_collection_ok_for_gctf_file() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.gctf");
        std::fs::write(
            &file_path,
            "--- ENDPOINT ---\n\ngrpc://localhost:5000\n\n--- REQUEST ---\n{}\n",
        )
        .unwrap();

        let state = Arc::new(PlayState {
            collections_dir: dir.path().to_path_buf(),
            collections_dirs: vec![dir.path().to_path_buf()],
            project_root: None,
            project_settings: None,
            history_lock: std::sync::Mutex::new(()),
            collections_mtime: Arc::new(AtomicU64::new(0)),
        });

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(get_collection(State(state), Path("test.gctf".to_string())));
        assert!(result.is_ok());
        let resp = result.unwrap();
        assert!(resp.path.contains("test.gctf"));
    }

    #[cfg(not(miri))]
    #[test]
    fn test_list_collections_includes_empty_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("emptydir");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(
            dir.path().join("test.gctf"),
            "--- ENDPOINT ---\n--- REQUEST ---\n{}\n",
        )
        .unwrap();

        let state = Arc::new(PlayState {
            collections_dir: dir.path().to_path_buf(),
            collections_dirs: vec![dir.path().to_path_buf()],
            project_root: None,
            project_settings: None,
            history_lock: std::sync::Mutex::new(()),
            collections_mtime: Arc::new(AtomicU64::new(0)),
        });

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(list_collections(State(state)));
        assert!(result.is_ok());
        let items = result.unwrap();

        let dir_item = items.iter().find(|i| i.path == "emptydir");
        assert!(dir_item.is_some(), "empty dir must be listed");
        assert!(dir_item.unwrap().is_dir, "empty dir must have is_dir: true");

        let file_item = items.iter().find(|i| i.path == "test.gctf");
        assert!(file_item.is_some(), "gctf file must be listed");
        assert!(!file_item.unwrap().is_dir, "file must have is_dir: false");
    }

    #[cfg(not(miri))]
    #[test]
    fn test_list_collections_empty_dir_with_gitkeep() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("projects");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join(".gitkeep"), "").unwrap();

        let state = Arc::new(PlayState {
            collections_dir: dir.path().to_path_buf(),
            collections_dirs: vec![dir.path().to_path_buf()],
            project_root: None,
            project_settings: None,
            history_lock: std::sync::Mutex::new(()),
            collections_mtime: Arc::new(AtomicU64::new(0)),
        });

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(list_collections(State(state)));
        assert!(result.is_ok());
        let items = result.unwrap();

        let dir_item = items.iter().find(|i| i.path == "projects");
        assert!(
            dir_item.is_some(),
            "projects dir must be listed even with .gitkeep"
        );
        assert!(dir_item.unwrap().is_dir, "projects must have is_dir: true");
    }
}
