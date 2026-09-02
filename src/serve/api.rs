use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::{PlayState, ShareState};
use std::collections::HashMap;

#[derive(Serialize)]
pub struct ApiError {
    pub error: String,
}

pub(super) fn reject_traversal(path: &str) -> Result<(), (StatusCode, String)> {
    let not_found = || (StatusCode::NOT_FOUND, "File not found".to_string());
    if path.starts_with('/') || path.starts_with('\\') {
        return Err(not_found());
    }
    if path.as_bytes().get(1) == Some(&b':') {
        return Err(not_found());
    }
    if path.split(['/', '\\']).any(|component| component == "..") {
        return Err(not_found());
    }
    Ok(())
}

pub(super) fn require_gctf(path: &str) -> Result<(), (StatusCode, String)> {
    let lower = path.to_ascii_lowercase();
    if !lower.ends_with(".gctf") && !lower.ends_with(".httf") && !lower.ends_with(".apif") {
        return Err((
            StatusCode::BAD_REQUEST,
            "A test file is a .gctf, a .httf or a .apif".to_string(),
        ));
    }
    Ok(())
}

const SCHEMA_EXTS: &[&str] = &[".proto", ".pb", ".bin", ".desc", ".protoset"];
const DATA_EXTS: &[&str] = &[".csv", ".json"];
const DOC_EXTS: &[&str] = &[".md", ".txt"];

fn is_collection_file(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    require_gctf(path).is_ok()
        || SCHEMA_EXTS
            .iter()
            .chain(DATA_EXTS)
            .chain(DOC_EXTS)
            .any(|ext| lower.ends_with(ext))
}

fn is_dot_named(name: &str) -> bool {
    name.starts_with('.')
}

fn has_hidden_component(path: &str) -> bool {
    path.split(['/', '\\'])
        .any(|component| component.starts_with('.'))
}

fn stranger_in(dir: &std::path::Path) -> Option<std::path::PathBuf> {
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                return Some(path);
            };
            let name = entry.file_name().to_string_lossy().to_string();
            if kind.is_symlink() {
                return Some(path);
            }
            if kind.is_dir() {
                if is_dot_named(&name) {
                    return Some(path);
                }
                stack.push(path);
            } else if !is_dot_named(&name) && !is_collection_file(&name) {
                return Some(path);
            }
        }
    }
    None
}

fn require_collection_item(
    rel: &str,
    target: &std::path::Path,
) -> Result<(), (StatusCode, String)> {
    let refused = |why: String| (StatusCode::BAD_REQUEST, why);
    if has_hidden_component(rel) || is_generated(std::path::Path::new(rel)) {
        return Err(refused(format!(
            "`{rel}` is not managed by the workbench — hidden and generated paths stay as they are"
        )));
    }
    let meta = std::fs::symlink_metadata(target)
        .map_err(|_| (StatusCode::NOT_FOUND, "File not found".to_string()))?;
    if meta.file_type().is_symlink() {
        return Err(refused(format!(
            "`{rel}` is a link, which the workbench leaves alone"
        )));
    }
    if meta.is_dir() {
        if let Some(stranger) = stranger_in(target) {
            let inside = stranger
                .strip_prefix(target)
                .unwrap_or(&stranger)
                .to_string_lossy()
                .replace('\\', "/");
            return Err(refused(format!(
                "`{rel}` holds `{inside}`, which the workbench does not manage — a folder it removes holds test, schema, data and note files"
            )));
        }
        return Ok(());
    }
    if !is_collection_file(rel) {
        return Err(refused(format!(
            "`{rel}` is not a test, schema, data or note file — the workbench only manages those"
        )));
    }
    Ok(())
}

fn validate_env_name(name: &str) -> Result<(), (StatusCode, String)> {
    if name.is_empty() || name.contains(['/', '\\', ':']) || name.contains("..") {
        return Err((
            StatusCode::BAD_REQUEST,
            format!(
                "`{name}` cannot name an environment — it becomes `.env.{{name}}` in the project, so no `/`, `\\`, `:` or `..`"
            ),
        ));
    }
    Ok(())
}

fn parse_protocol(s: Option<&str>) -> crate::grpc::WireProtocol {
    s.and_then(|s| s.parse().ok()).unwrap_or_default()
}

pub(super) fn lexically_normal(path: &std::path::Path) -> std::path::PathBuf {
    let mut out = std::path::PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !out.pop() {
                    out.push(component.as_os_str());
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn tls_file(
    root: &std::path::Path,
    beside: &std::path::Path,
    given: &Option<String>,
) -> Result<Option<String>, String> {
    let Some(given) = given.as_deref().map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    if std::path::Path::new(given).is_absolute() {
        return Ok(Some(given.to_string()));
    }
    let landed = lexically_normal(&beside.join(given));
    if !landed.starts_with(lexically_normal(root)) {
        return Err(format!(
            "`{given}` lands outside the project — a TLS file is read from beside the test file, so it stays in the project, or is given as an absolute path"
        ));
    }
    Ok(Some(landed.to_string_lossy().to_string()))
}

fn tls_config_from_request(
    root: &std::path::Path,
    beside: &std::path::Path,
    tls: Option<bool>,
    tls_ca: &Option<String>,
    tls_cert: &Option<String>,
    tls_key: &Option<String>,
    tls_insecure: Option<bool>,
) -> Result<Option<crate::grpc::TlsConfig>, String> {
    if !tls.unwrap_or(false) {
        return Ok(None);
    }
    Ok(Some(crate::commands::tls_config_from_flags(
        tls_file(root, beside, tls_ca)?,
        tls_file(root, beside, tls_cert)?,
        tls_file(root, beside, tls_key)?,
        tls_insecure.unwrap_or(false),
    )))
}

fn beside_collection(state: &PlayState, collection_path: Option<&str>) -> std::path::PathBuf {
    let root = reports_base(state);
    collection_path
        .filter(|p| reject_traversal(p).is_ok())
        .map(|p| {
            resolve_file(state, p)
                .unwrap_or_else(|| primary_dir(state).join(p))
                .parent()
                .map(|d| d.to_path_buf())
                .unwrap_or_else(|| root.to_path_buf())
        })
        .unwrap_or_else(|| root.to_path_buf())
}

pub(super) fn resolve_file(state: &PlayState, rel: &str) -> Option<std::path::PathBuf> {
    for dir in &state.collections_dirs {
        let fp = dir.join(rel);
        if fp.exists()
            && let (Ok(canon), Ok(dir_canon)) = (fp.canonicalize(), dir.canonicalize())
            && canon.starts_with(&dir_canon)
        {
            return Some(fp);
        }
    }
    None
}

pub(super) fn primary_dir(state: &PlayState) -> &std::path::Path {
    &state.collections_dirs[0]
}

pub(super) fn reports_base(state: &PlayState) -> &std::path::Path {
    state
        .project_root
        .as_deref()
        .and_then(|dot| dot.parent())
        .unwrap_or_else(|| primary_dir(state))
}

fn resolve_write_path(
    dir: &std::path::Path,
    rel: &str,
) -> Result<std::path::PathBuf, (StatusCode, String)> {
    let invalid = || (StatusCode::BAD_REQUEST, "Invalid path".to_string());
    reject_traversal(rel).map_err(|_| invalid())?;
    if has_hidden_component(rel) || is_generated(std::path::Path::new(rel)) {
        return Err((
            StatusCode::BAD_REQUEST,
            format!(
                "`{rel}` is not a place the workbench writes — it would make something it could never remove or rename"
            ),
        ));
    }
    let target = dir.join(rel);
    if std::fs::symlink_metadata(&target).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(invalid());
    }
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).ok();
        let (Ok(parent_canon), Ok(dir_canon)) = (parent.canonicalize(), dir.canonicalize()) else {
            return Err(invalid());
        };
        if !parent_canon.starts_with(&dir_canon) {
            return Err(invalid());
        }
    }
    Ok(target)
}

#[derive(Deserialize, Default)]
pub struct ReflectRequest {
    pub address: String,
    pub tls: Option<bool>,
    pub tls_insecure: Option<bool>,
    pub tls_ca: Option<String>,
    pub tls_cert: Option<String>,
    pub tls_key: Option<String>,
    pub collection_path: Option<String>,
    pub protocol: Option<String>,
    pub timeout_seconds: Option<u64>,
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
    pub tls_ca: Option<String>,
    pub tls_cert: Option<String>,
    pub tls_key: Option<String>,
    pub collection_path: Option<String>,
    pub protocol: Option<String>,
    pub timeout_seconds: Option<u64>,
}

#[derive(Deserialize)]
pub struct ImportGrpcurlRequest {
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
}

#[derive(Serialize)]
pub struct ImportGrpcurlResponse {
    pub endpoint: String,
    pub address: String,
    pub headers: std::collections::HashMap<String, String>,
    pub body: String,
    pub plaintext: bool,
    pub tls: std::collections::HashMap<String, String>,
    pub proto: std::collections::HashMap<String, String>,
    pub options: std::collections::HashMap<String, String>,
}

#[derive(Serialize)]
pub struct CollectionItem {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mtime_ms: Option<u64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SectionAttribute {
    pub section: String,
    pub index: usize,
    pub name: String,
    pub value: String,
}

#[derive(Debug, Serialize)]
pub struct CollectionParsed {
    pub endpoint: String,
    #[serde(default)]
    pub parallel: bool,
    pub address: String,
    pub headers: crate::parser::OrderedStringMap,
    pub bodies: Vec<String>,
    #[serde(
        default,
        skip_serializing_if = "crate::parser::OrderedStringMap::is_empty"
    )]
    pub sections_as_written: crate::parser::OrderedStringMap,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bodies_as_written: Vec<String>,
    pub asserts: Vec<String>,
    pub extracts: crate::parser::OrderedStringMap,
    #[serde(
        default,
        skip_serializing_if = "crate::parser::OrderedStringMap::is_empty"
    )]
    pub extract_types: crate::parser::OrderedStringMap,
    pub meta_name: Option<String>,
    pub meta_tags: Vec<String>,
    pub meta_owner: Option<String>,
    pub meta_summary: Option<String>,
    pub meta_links: Vec<String>,
    pub tls: crate::parser::OrderedStringMap,
    pub options: crate::parser::OrderedStringMap,
    pub bench: crate::parser::OrderedStringMap,
    pub proto: crate::parser::OrderedStringMap,
    pub dataset: Vec<serde_json::Value>,
    pub attributes: Vec<SectionAttribute>,
    pub bodies_stream: bool,
    pub expect_responses: Vec<ExpectMessage>,
    pub expect_error: Option<ExpectMessage>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExpectMessage {
    pub body: String,
    #[serde(default)]
    pub partial: bool,
    #[serde(default)]
    pub unordered_arrays: bool,
    #[serde(default)]
    pub with_asserts: bool,
    #[serde(default)]
    pub tolerance: Option<f64>,
    #[serde(default)]
    pub redact: Vec<String>,
}

fn expect_message(section: &crate::parser::Section) -> ExpectMessage {
    use crate::parser::SectionContent;
    let body = match &section.content {
        SectionContent::Json(v) => serde_json::to_string_pretty(v).unwrap_or_default(),
        SectionContent::JsonLines(vals) => vals
            .iter()
            .filter_map(|v| serde_json::to_string(v).ok())
            .collect::<Vec<_>>()
            .join("\n"),
        SectionContent::Single(v) => v.clone(),
        _ => section.raw_content.trim().to_string(),
    };
    let o = &section.inline_options;
    ExpectMessage {
        body,
        partial: o.partial,
        unordered_arrays: o.unordered_arrays,
        with_asserts: o.with_asserts,
        tolerance: o.tolerance,
        redact: o.redact.clone(),
    }
}

fn inline_options_of(m: &ExpectMessage) -> crate::parser::InlineOptions {
    crate::parser::InlineOptions {
        partial: m.partial,
        unordered_arrays: m.unordered_arrays,
        with_asserts: m.with_asserts,
        tolerance: m.tolerance,
        redact: m.redact.clone(),
        parallel: false,
        extra: Default::default(),
    }
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

    let get_kv = |t: SectionType| -> crate::parser::OrderedStringMap {
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
        .flat_map(|s| {
            use crate::parser::SectionContent;
            match &s.content {
                SectionContent::Json(v) => {
                    serde_json::to_string_pretty(v).ok().into_iter().collect()
                }
                SectionContent::Single(text) => vec![text.clone()],
                SectionContent::JsonLines(values) => values
                    .iter()
                    .filter_map(|v| serde_json::to_string_pretty(v).ok())
                    .collect(),
                _ => Vec::new(),
            }
        })
        .collect();
    let bodies = if bodies.is_empty() && doc.transport() == crate::parser::ast::Transport::Grpc {
        vec!["{}".to_string()]
    } else {
        bodies
    };
    let request_sections: Vec<&crate::parser::Section> = doc
        .sections
        .iter()
        .filter(|s| s.section_type == SectionType::Request)
        .collect();
    let bodies_as_written: Vec<String> = if request_sections.len() == bodies.len() {
        request_sections
            .iter()
            .zip(bodies.iter())
            .map(|(section, shown)| {
                let raw = section.raw_content.trim();
                if raw == shown.trim() || raw.is_empty() {
                    String::new()
                } else {
                    raw.to_string()
                }
            })
            .collect()
    } else {
        Vec::new()
    };
    let bodies_as_written = if bodies_as_written.iter().all(String::is_empty) {
        Vec::new()
    } else {
        bodies_as_written
    };
    let bodies_stream = matches!(
        request_sections.as_slice(),
        [only] if matches!(only.content, crate::parser::SectionContent::JsonLines(_))
    );

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

    let extracts: crate::parser::OrderedStringMap = doc
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
        .fold(crate::parser::OrderedStringMap::new(), |mut acc, m| {
            acc.extend(m);
            acc
        });

    let extract_types: crate::parser::OrderedStringMap = doc
        .sections
        .iter()
        .filter(|s| s.section_type == SectionType::Extract)
        .flat_map(|s| s.raw_content.lines())
        .filter_map(crate::parser::gctf_tokenizer::tokenize_extract_line_full)
        .filter_map(|(name, kind, _)| kind.map(|kind| (name, kind)))
        .collect();

    let mut meta_name = None;
    let mut meta_tags = Vec::new();
    let mut meta_owner = None;
    let mut meta_summary = None;
    let mut meta_links = Vec::new();
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
            meta_links = m.links.clone();
        }
    }

    let dataset = doc
        .sections
        .iter()
        .find_map(|s| match &s.content {
            crate::parser::SectionContent::Rows(rows) => Some(rows.clone()),
            _ => None,
        })
        .unwrap_or_default();

    let mut seen: HashMap<&'static str, usize> = HashMap::new();
    let attributes = doc
        .sections
        .iter()
        .flat_map(|s| {
            let section = s.section_type.as_str();
            let index = seen.entry(section).or_insert(0);
            let at = *index;
            *index += 1;
            s.attributes.iter().map(move |a| SectionAttribute {
                section: section.to_string(),
                index: at,
                name: a.name.clone(),
                value: a.value.clone(),
            })
        })
        .collect();

    let expect_responses: Vec<ExpectMessage> = doc
        .sections
        .iter()
        .filter(|s| s.section_type == SectionType::Response)
        .map(expect_message)
        .collect();

    let expect_error = doc
        .sections
        .iter()
        .find(|s| s.section_type == SectionType::Error)
        .map(expect_message);

    let mut sections_as_written = crate::parser::OrderedStringMap::new();
    for (section_type, shown) in [
        (SectionType::Asserts, asserts.join("\n")),
        (
            SectionType::Extract,
            extracts
                .iter()
                .map(|(k, v)| format!("{k} = {v}"))
                .collect::<Vec<_>>()
                .join("\n"),
        ),
    ] {
        let Some(section) = doc.sections.iter().find(|s| s.section_type == section_type) else {
            continue;
        };
        let raw = section.raw_content.trim();
        if !raw.is_empty() && raw != shown.trim() {
            sections_as_written.insert(section_type.as_str().to_string(), raw.to_string());
        }
    }

    CollectionParsed {
        endpoint,
        parallel: doc.runs_in_parallel(),
        address,
        headers,
        bodies,
        sections_as_written,
        bodies_as_written,
        asserts,
        extracts,
        extract_types,
        meta_name,
        meta_tags,
        meta_owner,
        meta_summary,
        meta_links,
        tls: get_kv(SectionType::Tls),
        options: get_kv(SectionType::Options),
        bench: get_kv(SectionType::Bench),
        proto: get_kv(SectionType::Proto),
        dataset,
        attributes,
        bodies_stream,
        expect_responses,
        expect_error,
    }
}

#[derive(Debug, Serialize)]
pub struct DocumentSummary {
    pub index: usize,
    pub endpoint: String,
    #[serde(default)]
    pub parallel: bool,
    pub kind: String,
    pub address: String,
    pub address_source: String,
    pub headers: crate::parser::OrderedStringMap,
    pub bodies: Vec<String>,
    pub asserts: Vec<String>,
    pub extracts: crate::parser::OrderedStringMap,
    #[serde(
        default,
        skip_serializing_if = "crate::parser::OrderedStringMap::is_empty"
    )]
    pub extract_types: crate::parser::OrderedStringMap,
    pub options: crate::parser::OrderedStringMap,
    pub tls: crate::parser::OrderedStringMap,
    pub proto: crate::parser::OrderedStringMap,
    pub produces: Vec<String>,
    pub consumes: Vec<String>,
    pub start_line: usize,
    pub end_line: usize,
}

fn document_kind(doc: &crate::parser::GctfDocument) -> &'static str {
    use crate::parser::SectionType;
    let count = |t: SectionType| doc.sections.iter().filter(|s| s.section_type == t).count();
    match (
        count(SectionType::Request) > 1,
        count(SectionType::Response) > 1,
    ) {
        (true, true) => "bidi",
        (true, false) => "client",
        (false, true) => "server",
        (false, false) => "unary",
    }
}

fn referenced_variables(parsed: &CollectionParsed) -> Vec<String> {
    let mut out = Vec::new();
    let mut push = |name: &str| {
        let name = name.to_string();
        if !out.contains(&name) {
            out.push(name);
        }
    };

    let mut haystack: Vec<&str> = vec![parsed.endpoint.as_str(), parsed.address.as_str()];
    haystack.extend(parsed.bodies.iter().map(String::as_str));
    haystack.extend(parsed.asserts.iter().map(String::as_str));
    haystack.extend(parsed.headers.iter().map(|(_, v)| v.as_str()));

    for text in haystack {
        let bytes = text.as_bytes();
        let mut i = 0;
        while i + 1 < bytes.len() {
            if bytes[i] == b'{'
                && bytes[i + 1] == b'{'
                && let Some(end) = text[i + 2..].find("}}")
            {
                push(text[i + 2..i + 2 + end].trim());
                i += end + 4;
                continue;
            }
            if bytes[i] == b'$' {
                let rest = &text[i + 1..];
                let len = rest
                    .find(|c: char| !c.is_alphanumeric() && c != '_')
                    .unwrap_or(rest.len());
                if len > 0 {
                    push(&rest[..len]);
                }
                i += len + 1;
                continue;
            }
            i += 1;
        }
    }
    out
}

fn placeholder_names(parsed: &CollectionParsed) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut haystack: Vec<&str> = vec![parsed.endpoint.as_str(), parsed.address.as_str()];
    haystack.extend(parsed.bodies.iter().map(String::as_str));
    haystack.extend(parsed.asserts.iter().map(String::as_str));
    haystack.extend(parsed.headers.iter().map(|(_, v)| v.as_str()));

    for text in haystack {
        let mut rest = text;
        while let Some(open) = rest.find("{{") {
            let after = &rest[open + 2..];
            let Some(close) = after.find("}}") else { break };
            let name = after[..close].trim().to_string();
            if !name.is_empty() && !out.contains(&name) {
                out.push(name);
            }
            rest = &after[close + 2..];
        }
    }
    out
}

fn summarize_chain(doc: &crate::parser::GctfDocument) -> Vec<DocumentSummary> {
    doc.iter_chain()
        .enumerate()
        .map(|(index, d)| {
            let parsed = parse_collection(d);
            let declares_address = !parsed.address.is_empty();
            let produces: Vec<String> = parsed.extracts.iter().map(|(k, _)| k.clone()).collect();
            let consumes = referenced_variables(&parsed)
                .into_iter()
                .filter(|v| !produces.contains(v))
                .collect();

            DocumentSummary {
                index,
                endpoint: parsed.endpoint,
                parallel: parsed.parallel,
                kind: document_kind(d).to_string(),
                address: parsed.address,
                address_source: if declares_address {
                    "section".to_string()
                } else {
                    "inherited".to_string()
                },
                headers: parsed.headers,
                bodies: parsed.bodies,
                asserts: parsed.asserts,
                extracts: parsed.extracts,
                extract_types: parsed.extract_types,
                options: parsed.options,
                tls: parsed.tls,
                proto: parsed.proto,
                produces,
                consumes,
                start_line: d.sections.iter().map(|s| s.start_line).min().unwrap_or(0),
                end_line: d.sections.iter().map(|s| s.end_line).max().unwrap_or(0),
            }
        })
        .collect()
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct FileVersion {
    pub mtime_ms: u64,
    pub hash: String,
}

fn hash_content(content: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    let digest = hasher.finalize();
    let mut out = String::with_capacity(7 + digest.len() * 2);
    out.push_str("sha256:");
    for b in digest {
        out.push_str(&format!("{:02x}", b));
    }
    out
}

fn file_version(path: &std::path::Path, content: &str) -> FileVersion {
    let mtime_ms = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    FileVersion {
        mtime_ms,
        hash: hash_content(content),
    }
}

#[derive(Deserialize)]
pub struct VersionsRequest {
    pub paths: Vec<String>,
}

pub async fn file_versions(
    State(state): State<Arc<PlayState>>,
    Json(req): Json<VersionsRequest>,
) -> Json<std::collections::HashMap<String, Option<FileVersion>>> {
    let mut out = std::collections::HashMap::with_capacity(req.paths.len());
    for rel in req.paths.iter().take(MAX_VERSION_PATHS) {
        if reject_traversal(rel).is_err() {
            continue;
        }
        let version = resolve_file(&state, rel).and_then(|path| {
            std::fs::read_to_string(&path)
                .ok()
                .map(|c| file_version(&path, &c))
        });
        out.insert(rel.clone(), version);
    }
    Json(out)
}

const MAX_VERSION_PATHS: usize = 64;

fn check_version(
    path: &std::path::Path,
    expected: Option<&FileVersion>,
) -> Result<(), (StatusCode, String)> {
    let Some(expected) = expected else {
        return Ok(());
    };
    let Ok(current) = std::fs::read_to_string(path) else {
        return Ok(());
    };
    let actual = file_version(path, &current);
    if actual.hash == expected.hash {
        return Ok(());
    }
    let body = serde_json::json!({
        "error": "stale version",
        "version": actual,
        "content": current,
    });
    Err((StatusCode::CONFLICT, body.to_string()))
}

#[derive(Serialize, Debug)]
pub struct CollectionResponse {
    pub content: String,
    pub path: String,
    pub parsed: CollectionParsed,
    pub documents: Vec<DocumentSummary>,
    pub version: FileVersion,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parse_error: Option<String>,
}

#[derive(Deserialize)]
pub struct ProtoUploadRequest {
    pub filename: String,
    pub content: String,
    #[serde(default)]
    pub encoding: Option<String>,
}

#[derive(Serialize)]
pub struct ProtoInfo {
    pub path: String,
    pub name: String,
    pub size: u64,
    pub kind: &'static str,
}

#[derive(Deserialize)]
pub struct SaveRequest {
    pub path: String,
    pub content: String,
    #[serde(default)]
    pub original_path: Option<String>,
    pub version: Option<FileVersion>,
    #[serde(default)]
    pub create_only: bool,
}

#[derive(Deserialize, Default)]
pub struct SaveRequestStructured {
    pub path: String,
    pub endpoint: String,
    pub address: Option<String>,
    pub headers: Option<std::collections::HashMap<String, String>>,
    pub bodies: Option<Vec<String>>,
    #[serde(default)]
    pub bodies_stream: bool,
    pub options: Option<Vec<(String, String)>>,
    pub asserts: Option<Vec<String>>,
    pub extract: Option<Vec<(String, String)>>,
    pub meta: Option<crate::parser::FileMeta>,
    pub tls: Option<Vec<(String, String)>>,
    pub proto: Option<Vec<(String, String)>>,
    pub bench: Option<Vec<(String, String)>>,
    pub dataset: Option<Vec<serde_json::Value>>,
    pub expect: Option<ExpectSave>,
    pub original_path: Option<String>,
    #[serde(default)]
    pub document_index: usize,
    #[serde(default)]
    pub parallel: bool,
    pub fmt: Option<bool>,
    pub version: Option<FileVersion>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ExpectSave {
    #[serde(default)]
    pub responses: Vec<ExpectMessage>,
    #[serde(default)]
    pub error: Option<ExpectMessage>,
}

#[derive(Deserialize, Default)]
pub struct CallRequest {
    pub endpoint: String,
    #[serde(default)]
    pub body: serde_json::Value,
    pub bodies_raw: Option<Vec<String>>,
    pub headers: Option<std::collections::HashMap<String, String>>,
    pub tls: Option<bool>,
    pub tls_insecure: Option<bool>,
    pub tls_ca: Option<String>,
    pub tls_cert: Option<String>,
    pub tls_key: Option<String>,
    pub address: Option<String>,
    pub protocol: Option<String>,
    pub collection_path: Option<String>,
    pub session_id: Option<String>,
    pub timeout_seconds: Option<u64>,
    #[serde(default)]
    pub dataset_row: Option<usize>,
    #[serde(default)]
    pub document_index: usize,
}

#[derive(Serialize)]
pub struct GrpcurlResponse {
    pub command: String,
}

#[derive(Serialize)]
pub struct CallResponse {
    pub success: bool,
    pub messages: Vec<serde_json::Value>,
    pub message_offsets_ms: Vec<u64>,
    pub grpc_status: Option<u32>,
    pub headers: std::collections::HashMap<String, String>,
    pub trailers: std::collections::HashMap<String, String>,
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shape: Option<String>,
    pub messages_total: usize,
    pub messages_truncated: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub messages_raw: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub extracted: Vec<(String, String)>,
}

const MAX_SHOWN_MESSAGES: usize = 200;
const MAX_SHOWN_BYTES: usize = 1024 * 1024;

fn cap_messages(messages: Vec<serde_json::Value>) -> (Vec<serde_json::Value>, usize, bool) {
    let total = messages.len();
    let mut kept = Vec::with_capacity(messages.len().min(MAX_SHOWN_MESSAGES));
    let mut bytes = 0usize;
    for message in messages {
        if kept.len() >= MAX_SHOWN_MESSAGES || bytes >= MAX_SHOWN_BYTES {
            break;
        }
        bytes += serde_json::to_string(&message)
            .map(|s| s.len())
            .unwrap_or(0);
        kept.push(message);
    }
    let truncated = kept.len() < total;
    (kept, total, truncated)
}

fn shape_name(client_streaming: bool, server_streaming: bool) -> &'static str {
    match (client_streaming, server_streaming) {
        (true, true) => "duplex",
        (true, false) => "client",
        (false, true) => "server",
        (false, false) => "unary",
    }
}

fn file_mtime_ms(path: &std::path::Path) -> Option<u64> {
    let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok()?;
    let since = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(since.as_millis() as u64)
}

fn extract_tags(path: &std::path::Path) -> Vec<String> {
    let doc = crate::parser::parse_with_recovery(path).document;
    crate::commands::run::extract_test_meta(&doc).tags
}

pub async fn list_collections(
    State(state): State<Arc<PlayState>>,
) -> Result<Json<Vec<CollectionItem>>, (StatusCode, String)> {
    let dirs = state.collections_dirs.clone();
    let items = tokio::task::spawn_blocking(move || {
        let mut items = Vec::new();
        let mut seen_paths = std::collections::HashSet::new();
        let mut seen_dirs = std::collections::HashSet::new();

        for dir in &dirs {
            if !dir.is_dir() {
                continue;
            }

            for file in crate::utils::FileUtils::collect_test_files(dir, &[]) {
                let rel = file.strip_prefix(dir).unwrap_or(&file);
                if is_generated(rel) {
                    continue;
                }
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
                        mtime_ms: file_mtime_ms(&file),
                    });
                }
                if let Some(parent) = rel.parent() {
                    let parent_str = parent.to_string_lossy().to_string();
                    if !parent_str.is_empty() {
                        seen_dirs.insert(parent_str);
                    }
                }
            }

            collect_empty_dirs(dir, dir, &mut seen_dirs, &mut items);
        }

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

        items
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(items))
}

pub(super) const GENERATED_DIRS: &[&str] = &["grpctestify-reports", ".grpctestify"];

fn is_generated(rel: &std::path::Path) -> bool {
    rel.components()
        .next()
        .map(|c| GENERATED_DIRS.contains(&c.as_os_str().to_string_lossy().as_ref()))
        .unwrap_or(false)
}

fn collect_empty_dirs(
    dir: &std::path::Path,
    base: &std::path::Path,
    seen_dirs: &mut std::collections::HashSet<String>,
    result: &mut Vec<CollectionItem>,
) {
    let walker = ignore::WalkBuilder::new(dir)
        .git_global(true)
        .git_ignore(true)
        .git_exclude(true)
        .build();
    for entry in walker.flatten() {
        let path = entry.path();
        if !path.is_dir() || path == base {
            continue;
        }
        let rel = path.strip_prefix(base).unwrap_or(path);
        if is_generated(rel) {
            continue;
        }
        let rel_str = rel.to_string_lossy().to_string();
        if seen_dirs.insert(rel_str.clone()) {
            result.push(CollectionItem {
                path: rel_str,
                name: entry.file_name().to_string_lossy().to_string(),
                is_dir: true,
                tags: vec![],
                mtime_ms: None,
            });
        }
    }
}

pub async fn get_collection(
    State(state): State<Arc<PlayState>>,
    path: Path<String>,
) -> Result<Json<CollectionResponse>, (StatusCode, String)> {
    reject_traversal(&path.0)?;
    require_gctf(&path.0)?;
    let state = state.clone();
    let path_str = path.0.clone();
    let result = tokio::task::spawn_blocking(move || {
        let file_path = resolve_file(&state, &path_str)
            .ok_or_else(|| (StatusCode::NOT_FOUND, "File not found".to_string()))?;
        if file_path.is_dir() {
            return Err((StatusCode::NOT_FOUND, "Path is a directory".to_string()));
        }
        let content = std::fs::read_to_string(&file_path)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        let strict = crate::parser::parse_gctf_from_str(&content, &file_path.to_string_lossy());
        let (doc, parse_error) = match strict {
            Ok(doc) => (doc, None),
            Err(e) => (
                crate::parser::parse_content_with_recovery(&content, &file_path.to_string_lossy())
                    .document,
                Some(e.to_string()),
            ),
        };
        let version = file_version(&file_path, &content);
        Ok(CollectionResponse {
            path: path_str,
            parsed: parse_collection(&doc),
            documents: summarize_chain(&doc),
            content,
            version,
            parse_error,
        })
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))??;
    Ok(Json(result))
}

pub async fn save_collection(
    State(state): State<Arc<PlayState>>,
    Json(req): Json<SaveRequest>,
) -> Result<Json<FileVersion>, (StatusCode, String)> {
    reject_traversal(&req.path)?;
    require_gctf(&req.path)?;
    let state = state.clone();
    let path_str = req.path.clone();
    let content = req.content.clone();
    let expected = req.version.clone();
    let create_only = req.create_only;
    let original = req.original_path.clone();
    let version = tokio::task::spawn_blocking(move || {
        let file_path = resolve_write_path(primary_dir(&state), &path_str)?;
        if create_only && file_path.exists() {
            return Err((StatusCode::CONFLICT, format!("{path_str} is already here")));
        }
        check_version(&file_path, expected.as_ref())?;
        let content = match original.as_deref() {
            Some(orig) => respell_paths(&content, &parent_of(orig), &parent_of(&path_str)).0,
            None => content,
        };
        std::fs::write(&file_path, &content)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        state
            .collections_mtime
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok::<FileVersion, (StatusCode, String)>(file_version(&file_path, &content))
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))??;
    Ok(Json(version))
}

#[derive(Deserialize)]
pub struct CheckRequest {
    #[serde(default)]
    pub paths: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct CheckedFile {
    pub path: String,
    pub errors: usize,
    pub warnings: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first: Option<String>,
}

#[derive(Serialize)]
pub struct CheckResponse {
    pub files: Vec<CheckedFile>,
    pub checked: usize,
    pub truncated: bool,
}

const CHECK_LIMIT: usize = 500;

pub async fn check_files(
    State(state): State<Arc<PlayState>>,
    Json(req): Json<CheckRequest>,
) -> Result<Json<CheckResponse>, (StatusCode, String)> {
    for path in &req.paths {
        if reject_traversal(path).is_err() {
            return Err((StatusCode::NOT_FOUND, format!("Invalid path: {path}")));
        }
    }

    let mut wanted: Vec<(String, std::path::PathBuf)> = Vec::new();
    if req.paths.is_empty() {
        for dir in &state.collections_dirs {
            for file in crate::utils::FileUtils::collect_test_files(dir, &[]) {
                let rel = file
                    .strip_prefix(dir)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| file.to_string_lossy().to_string());
                wanted.push((rel, file));
            }
        }
    } else {
        for path in &req.paths {
            if let Some(file) = resolve_file(&state, path) {
                wanted.push((path.clone(), file));
            }
        }
    }

    let truncated = wanted.len() > CHECK_LIMIT;
    wanted.truncate(CHECK_LIMIT);
    let checked = wanted.len();

    let files = tokio::task::spawn_blocking(move || {
        let mut out: Vec<CheckedFile> = Vec::new();
        for (rel, file) in wanted {
            let Ok(content) = std::fs::read_to_string(&file) else {
                continue;
            };
            let diagnostics = crate::lsp::handlers::collect_all_diagnostics_in(
                &content,
                &rel,
                crate::lsp::handlers::Voice::Check,
            );
            let mut errors = 0;
            let mut warnings = 0;
            let mut first: Option<String> = None;
            let mut worst: Option<String> = None;
            for diagnostic in &diagnostics {
                match diagnostic.severity {
                    Some(tower_lsp::lsp_types::DiagnosticSeverity::ERROR) => {
                        errors += 1;
                        if worst.is_none() {
                            worst = Some(diagnostic.message.clone());
                        }
                    }
                    Some(tower_lsp::lsp_types::DiagnosticSeverity::WARNING) => warnings += 1,
                    _ => continue,
                }
                if first.is_none() {
                    first = Some(diagnostic.message.clone());
                }
            }
            let first = worst.or(first);
            if errors > 0 || warnings > 0 {
                out.push(CheckedFile {
                    path: rel,
                    errors,
                    warnings,
                    first,
                });
            }
        }
        out
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(CheckResponse {
        files,
        checked,
        truncated,
    }))
}

#[derive(Deserialize)]
pub struct DiagnosticsRequest {
    pub content: String,
    pub file_name: Option<String>,
    #[serde(default)]
    pub voice: Option<String>,
}

#[derive(Deserialize)]
pub struct TargetHealthRequest {
    pub address: String,
}

#[derive(Serialize)]
pub struct TargetHealth {
    pub reachable: bool,
    pub ms: u64,
    pub detail: Option<String>,
    pub dialled: String,
}

pub(super) fn probe_target(address: &str) -> Option<String> {
    let value = address.trim();
    if value.is_empty() {
        return None;
    }
    let (scheme, rest) = match value.split_once("://") {
        Some((scheme, rest)) => (Some(scheme.to_ascii_lowercase()), rest),
        None => (None, value),
    };
    let rest = rest.split('/').next().unwrap_or(rest);
    if rest.is_empty() {
        return None;
    }
    if rest.contains(':') && !rest.ends_with(':') {
        return Some(rest.to_string());
    }
    let host = rest.trim_end_matches(':');
    let port = match scheme.as_deref() {
        Some("https") => 443,
        Some("http") => 80,
        _ => return None,
    };
    Some(format!("{host}:{port}"))
}

pub async fn target_health(Json(req): Json<TargetHealthRequest>) -> Json<TargetHealth> {
    let Some(dialled) = probe_target(&req.address) else {
        return Json(TargetHealth {
            reachable: false,
            ms: 0,
            detail: Some("no host and port to try".to_string()),
            dialled: String::new(),
        });
    };
    let started = std::time::Instant::now();
    let outcome = tokio::time::timeout(
        std::time::Duration::from_millis(1200),
        tokio::net::TcpStream::connect(&dialled),
    )
    .await;
    let ms = started.elapsed().as_millis() as u64;
    let (reachable, detail) = match outcome {
        Ok(Ok(_)) => (true, None),
        Ok(Err(e)) => (false, Some(e.to_string())),
        Err(_) => (false, Some("no answer within 1.2 s".to_string())),
    };
    Json(TargetHealth {
        reachable,
        ms,
        detail,
        dialled,
    })
}

pub async fn get_diagnostics(
    State(state): State<Arc<PlayState>>,
    Json(req): Json<DiagnosticsRequest>,
) -> Json<Vec<tower_lsp::lsp_types::Diagnostic>> {
    let named = req.file_name.as_deref().unwrap_or("playground.gctf");
    let resolved = resolve_file(&state, named).map(|path| path.to_string_lossy().to_string());
    let file_name = resolved.as_deref().unwrap_or(named);
    let voice = match req.voice.as_deref() {
        Some("check") => crate::lsp::handlers::Voice::Check,
        _ => crate::lsp::handlers::Voice::Editor,
    };
    Json(crate::lsp::handlers::collect_all_diagnostics_in(
        &req.content,
        file_name,
        voice,
    ))
}

fn authored_here(section: &crate::parser::SectionType, req: &SaveRequestStructured) -> bool {
    use crate::parser::SectionType as T;
    *section == T::Endpoint
        || (req.bodies.is_some() && *section == T::Request)
        || (req.address.is_some() && *section == T::Address)
        || (req.headers.is_some() && *section == T::RequestHeaders)
        || (req.options.is_some() && *section == T::Options)
        || (req.asserts.is_some() && *section == T::Asserts)
        || (req.extract.is_some() && *section == T::Extract)
        || (req.meta.is_some() && *section == T::Meta)
        || (req.tls.is_some() && *section == T::Tls)
        || (req.proto.is_some() && *section == T::Proto)
        || (req.bench.is_some() && *section == T::Bench)
        || (req.dataset.is_some() && *section == T::Dataset)
        || (req.expect.is_some() && matches!(*section, T::Response | T::Error))
}

#[derive(Deserialize)]
pub struct ChainRequest {
    pub path: String,
    pub op: String,
    #[serde(default)]
    pub index: usize,
    pub version: Option<FileVersion>,
}

pub(crate) fn chain_documents(
    head: &crate::parser::GctfDocument,
) -> Vec<crate::parser::GctfDocument> {
    let mut chain = Vec::new();
    let mut current = Some(head);
    while let Some(doc) = current {
        let mut one = doc.clone();
        one.next_document = None;
        chain.push(one);
        current = doc.next_document.as_deref();
    }
    chain
}

pub(crate) fn link_documents(
    mut chain: Vec<crate::parser::GctfDocument>,
) -> Option<crate::parser::GctfDocument> {
    let mut head = chain.pop()?;
    while let Some(mut previous) = chain.pop() {
        previous.next_document = Some(Box::new(head));
        head = previous;
    }
    Some(head)
}

pub(crate) fn blank_step(path: &str, endpoint: &str) -> crate::parser::GctfDocument {
    let builder = crate::parser::GctfDocumentBuilder::new()
        .with_file_path(path)
        .endpoint(endpoint);

    if step_is_http(endpoint) {
        builder
            .asserts(vec!["@status() == 200".to_string()])
            .build()
    } else {
        builder.request(serde_json::json!({})).build()
    }
}

fn step_is_http(endpoint: &str) -> bool {
    let mut parts = endpoint.split_whitespace();
    let (Some(method), Some(_), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    crate::parser::ast::is_http_method(method)
}

pub(crate) fn apply_chain_op(
    head: &crate::parser::GctfDocument,
    path: &str,
    op: &str,
    index: usize,
) -> Result<crate::parser::GctfDocument, String> {
    let mut chain = chain_documents(head);
    match op {
        "append" => {
            let previous = chain
                .last()
                .and_then(|d| d.get_endpoint())
                .unwrap_or_default();
            chain.push(blank_step(path, &previous));
        }
        "delete" => {
            if chain.len() <= 1 {
                return Err("a file has at least one step".to_string());
            }
            if index >= chain.len() {
                return Err(format!(
                    "no step {} in a chain of {}",
                    index + 1,
                    chain.len()
                ));
            }
            chain.remove(index);
        }
        other => return Err(format!("unknown chain operation: {other}")),
    }
    link_documents(chain).ok_or_else(|| "a chain has at least one document".to_string())
}

pub async fn chain_edit(
    State(state): State<Arc<PlayState>>,
    Json(req): Json<ChainRequest>,
) -> Result<Json<FileVersion>, (StatusCode, String)> {
    require_gctf(&req.path)?;
    let _guard = state.write_lock.lock().await;
    let file = resolve_file(&state, &req.path)
        .ok_or((StatusCode::NOT_FOUND, "No such file".to_string()))?;
    check_version(&file, req.version.as_ref())?;

    let doc =
        crate::parser::parse_gctf(&file).map_err(|e| (StatusCode::BAD_REQUEST, format!("{e}")))?;
    let edited = apply_chain_op(&doc, &req.path, &req.op, req.index)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;

    let content = crate::parser::serialize_gctf_as_written(&edited);
    let steps_expected = chain_documents(&edited).len();
    match crate::parser::parse_gctf_from_str(&content, &req.path) {
        Ok(again) if chain_documents(&again).len() == steps_expected => {}
        _ => {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                "the edited chain would not read back — nothing was written".to_string(),
            ));
        }
    }
    let path = resolve_write_path(primary_dir(&state), &req.path)?;
    std::fs::write(&path, &content)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    state
        .collections_mtime
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    Ok(Json(file_version(&path, &content)))
}

fn stitch_into_chain(
    original: &crate::parser::GctfDocument,
    mut edited: crate::parser::GctfDocument,
    index: usize,
    req: &SaveRequestStructured,
) -> crate::parser::GctfDocument {
    let mut chain: Vec<crate::parser::GctfDocument> = Vec::new();
    let mut current = Some(original);
    while let Some(doc) = current {
        let mut one = doc.clone();
        one.next_document = None;
        chain.push(one);
        current = doc.next_document.as_deref();
    }

    let index = index.min(chain.len().saturating_sub(1));
    if let Some(target) = chain.get(index) {
        use crate::parser::SectionType as T;
        let mut seen: HashMap<&'static str, usize> = HashMap::new();
        for section in &mut edited.sections {
            let key = section.section_type.as_str();
            let at = *seen.entry(key).or_insert(0);
            seen.insert(key, at + 1);
            let Some(origin) = target
                .sections
                .iter()
                .filter(|s| s.section_type == section.section_type)
                .nth(at)
            else {
                continue;
            };
            if origin.content == section.content && !origin.raw_content.trim().is_empty() {
                *section = origin.clone();
                continue;
            }
            if section.attributes.is_empty() && !origin.attributes.is_empty() {
                section.attributes = origin.attributes.clone();
            }
        }
        let mut authored: HashMap<&'static str, std::collections::VecDeque<_>> = HashMap::new();
        for section in edited.sections.drain(..) {
            authored
                .entry(section.section_type.as_str())
                .or_default()
                .push_back(section);
        }

        let mut ordered: Vec<_> = Vec::new();
        for original in &target.sections {
            if authored_here(&original.section_type, req) {
                if let Some(next) = authored
                    .get_mut(original.section_type.as_str())
                    .and_then(std::collections::VecDeque::pop_front)
                {
                    ordered.push(next);
                }
                continue;
            }
            ordered.push(original.clone());
        }

        let (mut preamble, mut rest): (Vec<_>, Vec<_>) =
            authored.into_values().flatten().partition(|s| {
                matches!(
                    s.section_type,
                    T::Meta
                        | T::Bench
                        | T::Dataset
                        | T::Address
                        | T::Tls
                        | T::Proto
                        | T::Options
                        | T::RequestHeaders
                )
            });
        preamble.append(&mut ordered);
        preamble.append(&mut rest);
        edited.sections = preamble;
    }
    edited.next_document = None;
    if index < chain.len() {
        chain[index] = edited;
    } else {
        chain.push(edited);
    }

    link_documents(chain).unwrap_or_else(|| blank_step(&req.path, ""))
}

fn render_structured(state: &Arc<PlayState>, req: &SaveRequestStructured) -> String {
    let mut builder = crate::parser::GctfDocumentBuilder::new().with_file_path(&req.path);

    if let Some(ref addr) = req.address
        && !addr.is_empty()
    {
        builder = builder.address(addr);
    }
    builder = builder.endpoint_parallel(&req.endpoint, req.parallel);

    if let Some(ref headers) = req.headers
        && !headers.is_empty()
    {
        builder = builder.request_headers(headers.clone());
    }

    if let Some(ref bodies) = req.bodies {
        let empty = bodies.iter().all(|b| {
            b.trim().is_empty()
                || serde_json::from_str::<serde_json::Value>(b)
                    .is_ok_and(|v| v.as_object().is_some_and(|o| o.is_empty()))
        });
        let http = crate::parser::ast::Family::of(&req.path).allows_http();
        let http_without_body = http && empty;
        let stream: Option<Vec<serde_json::Value>> = if req.bodies_stream && bodies.len() > 1 {
            bodies
                .iter()
                .map(|b| serde_json::from_str::<serde_json::Value>(b).ok())
                .collect()
        } else {
            None
        };
        if let Some(values) = stream {
            builder = builder.request_stream(values);
        } else if !http_without_body {
            for b in bodies {
                match serde_json::from_str(b) {
                    Ok(val) => builder = builder.request(val),
                    Err(_) if http && !b.trim().is_empty() => {
                        builder = builder.request_text(b.clone());
                    }
                    Err(_) => {}
                }
            }
        }
    }

    if let Some(ref options) = req.options
        && !options.is_empty()
    {
        builder = builder.options(options.clone());
    }

    if let Some(ref expect) = req.expect {
        use crate::parser::{SectionContent, SectionType as T};
        for message in &expect.responses {
            let body = message.body.trim();
            let content = if body.is_empty() {
                SectionContent::Empty
            } else {
                match serde_json::from_str::<serde_json::Value>(body) {
                    Ok(v) => SectionContent::Json(v),
                    Err(_) => SectionContent::Single(body.to_string()),
                }
            };
            builder = builder.expectation(T::Response, content, inline_options_of(message));
        }
        if let Some(ref error) = expect.error {
            let body = error.body.trim();
            if !body.is_empty() {
                let content = match serde_json::from_str::<serde_json::Value>(body) {
                    Ok(v) => SectionContent::Json(v),
                    Err(_) => SectionContent::Single(body.to_string()),
                };
                builder = builder.expectation(T::Error, content, inline_options_of(error));
            }
        }
    }

    if let Some(ref asserts) = req.asserts
        && !asserts.is_empty()
    {
        builder = builder.asserts(asserts.clone());
    }

    if let Some(ref extract) = req.extract
        && !extract.is_empty()
    {
        builder = builder.extract(extract.clone());
    }

    if let Some(ref meta) = req.meta
        && !meta.is_empty()
    {
        builder = builder.meta(meta.clone());
    }

    if let Some(ref tls) = req.tls
        && !tls.is_empty()
    {
        builder = builder.tls(tls.clone());
    }

    if let Some(ref proto) = req.proto
        && !proto.is_empty()
    {
        builder = builder.proto(proto.clone());
    }

    if let Some(ref bench) = req.bench
        && !bench.is_empty()
    {
        builder = builder.bench(bench.clone());
    }

    if let Some(ref dataset) = req.dataset
        && !dataset.is_empty()
    {
        builder = builder.dataset(dataset.clone());
    }

    let mut doc = builder.build();

    if let Some(ref orig_path) = req.original_path
        && reject_traversal(orig_path).is_ok()
    {
        let orig_file =
            resolve_file(state, orig_path).unwrap_or_else(|| primary_dir(state).join(orig_path));
        if orig_file.exists()
            && let Ok(orig_doc) = crate::parser::parse_gctf(&orig_file)
        {
            doc = stitch_into_chain(&orig_doc, doc, req.document_index, req);
        }
    }

    let content = crate::parser::serialize_gctf_as_written(&doc);
    if req.fmt.unwrap_or(false) {
        let name = std::path::Path::new(&req.path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("playground.gctf");
        if let Ok(formatted) = crate::commands::fmt::format_gctf_content(&content, name) {
            return formatted;
        }
    }
    content
}

#[derive(Serialize)]
pub struct StructuredPreview {
    pub content: String,
    pub current: Option<String>,
    pub version: Option<FileVersion>,
}

fn check_bodies(req: &SaveRequestStructured) -> Result<(), (StatusCode, String)> {
    if crate::parser::ast::Family::of(&req.path).allows_http() {
        return Ok(());
    }
    for (i, body) in req.bodies.iter().flatten().enumerate() {
        if body.trim().is_empty() {
            continue;
        }
        if let Err(e) = serde_json::from_str::<serde_json::Value>(body) {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("message #{} is not valid JSON: {e}", i + 1),
            ));
        }
    }
    Ok(())
}

#[derive(Deserialize)]
pub struct BenchCompareRequest {
    pub baseline: serde_json::Value,
    pub current: serde_json::Value,
}

pub async fn bench_compare(
    Json(req): Json<BenchCompareRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    use crate::commands::bench_compare as cmp;

    let bad = |what: &str, e: anyhow::Error| (StatusCode::BAD_REQUEST, format!("{what}: {e}"));
    let base = cmp::extract_metrics(&req.baseline).map_err(|e| bad("baseline", e))?;
    let current = cmp::extract_metrics(&req.current).map_err(|e| bad("current", e))?;

    let thresholds = cmp::Thresholds {
        max_latency_regression: 10.0,
        max_error_rate_regression: 1.0,
        min_throughput: 5.0,
    };

    let aggregate = cmp::compare_aggregate(&base, &current, &thresholds);
    let endpoints = cmp::compare_endpoints(&base, &current, &thresholds);
    let pass = cmp::overall_pass(&aggregate) && cmp::overall_pass(&endpoints);

    Ok(Json(serde_json::json!({
        "overall": if pass { "pass" } else { "fail" },
        "metrics": aggregate.iter().map(cmp::row_to_json).collect::<Vec<_>>(),
        "per_endpoint": endpoints.iter().map(cmp::row_to_json).collect::<Vec<_>>(),
    })))
}

#[derive(Serialize)]
pub struct ChangedResponse {
    pub available: bool,
    pub since: String,
    pub paths: Vec<String>,
}

#[derive(Deserialize, Default)]
pub struct ChangedQuery {
    pub since: Option<String>,
}

pub async fn changed_collections(
    State(state): State<Arc<PlayState>>,
    axum::extract::Query(query): axum::extract::Query<ChangedQuery>,
) -> Result<Json<ChangedResponse>, (StatusCode, String)> {
    let since = query
        .since
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "HEAD".to_string());
    let base = primary_dir(&state);

    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    for entry in ignore::WalkBuilder::new(base).build().flatten() {
        let path = entry.path();
        if path.is_file() && path.extension().is_some_and(|e| e == "gctf" || e == "httf") {
            candidates.push(path.to_path_buf());
        }
    }

    let changed = match crate::only_changed::changed_files(base, &since, &candidates) {
        Ok(set) => set,
        Err(_) => {
            return Ok(Json(ChangedResponse {
                available: false,
                since,
                paths: vec![],
            }));
        }
    };

    let mut paths: Vec<String> = changed
        .iter()
        .filter_map(|p| p.strip_prefix(base).ok())
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    paths.sort();

    Ok(Json(ChangedResponse {
        available: true,
        since,
        paths,
    }))
}

pub async fn preview_collection_structured(
    State(state): State<Arc<PlayState>>,
    Json(req): Json<SaveRequestStructured>,
) -> Result<Json<StructuredPreview>, (StatusCode, String)> {
    reject_traversal(&req.path)?;
    require_gctf(&req.path)?;
    check_bodies(&req)?;
    refuse_unreadable_original(&state, &req)?;
    let file_path = resolve_write_path(primary_dir(&state), &req.path)?;
    let content = render_structured(&state, &req);
    let current = std::fs::read_to_string(&file_path).ok();
    let version = current.as_ref().map(|c| file_version(&file_path, c));
    Ok(Json(StructuredPreview {
        content,
        current,
        version,
    }))
}

fn refuse_unreadable_original(
    state: &Arc<PlayState>,
    req: &SaveRequestStructured,
) -> Result<(), (StatusCode, String)> {
    let Some(orig_path) = req.original_path.as_ref() else {
        return Ok(());
    };
    if reject_traversal(orig_path).is_err() {
        return Ok(());
    }
    let orig_file =
        resolve_file(state, orig_path).unwrap_or_else(|| primary_dir(state).join(orig_path));
    if !orig_file.exists() {
        return Ok(());
    }
    let Ok(content) = std::fs::read_to_string(&orig_file) else {
        return Ok(());
    };
    match crate::parser::parse_gctf_from_str(&content, &orig_file.to_string_lossy()) {
        Ok(_) => Ok(()),
        Err(e) => Err((
            StatusCode::BAD_REQUEST,
            format!(
                "{orig_path} has a section the parser cannot read ({e}) — edit it as text; \
                 saving it from the forms would write the file without that section"
            ),
        )),
    }
}

pub async fn save_collection_structured(
    State(state): State<Arc<PlayState>>,
    Json(req): Json<SaveRequestStructured>,
) -> Result<Json<FileVersion>, (StatusCode, String)> {
    reject_traversal(&req.path)?;
    require_gctf(&req.path)?;
    if let Some(ref orig) = req.original_path {
        reject_traversal(orig)
            .map_err(|_| (StatusCode::NOT_FOUND, "Invalid original_path".to_string()))?;
    }
    check_bodies(&req)?;
    refuse_unreadable_original(&state, &req)?;
    let file_path = resolve_write_path(primary_dir(&state), &req.path)?;
    check_version(&file_path, req.version.as_ref())?;

    let content = render_structured(&state, &req);
    let content = match req.original_path.as_deref() {
        Some(orig) => respell_paths(&content, &parent_of(orig), &parent_of(&req.path)).0,
        None => content,
    };
    std::fs::write(&file_path, &content)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    state
        .collections_mtime
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    Ok(Json(file_version(&file_path, &content)))
}

#[derive(Deserialize)]
pub struct FmtRequest {
    pub content: String,
    pub file_name: Option<String>,
}

#[derive(Serialize)]
pub struct FmtResponse {
    pub formatted: String,
    pub changed: bool,
}

pub async fn format_content(Json(req): Json<FmtRequest>) -> Json<FmtResponse> {
    let file_name = req.file_name.as_deref().unwrap_or("playground.gctf");
    match crate::commands::fmt::format_gctf_content(&req.content, file_name) {
        Ok(formatted) => Json(FmtResponse {
            changed: formatted != req.content,
            formatted,
        }),
        Err(_) => Json(FmtResponse {
            formatted: req.content,
            changed: false,
        }),
    }
}

pub(crate) fn decode_base64(text: &str) -> Option<Vec<u8>> {
    fn sextet(c: u8) -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => u32::from(c - b'A'),
            b'a'..=b'z' => u32::from(c - b'a') + 26,
            b'0'..=b'9' => u32::from(c - b'0') + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        })
    }

    let body: Vec<u8> = text
        .bytes()
        .filter(|b| !b.is_ascii_whitespace())
        .take_while(|b| *b != b'=')
        .collect();
    if text
        .bytes()
        .filter(|b| !b.is_ascii_whitespace())
        .skip(body.len())
        .any(|b| b != b'=')
    {
        return None;
    }
    if body.len() % 4 == 1 {
        return None;
    }

    let mut out = Vec::with_capacity(body.len() * 3 / 4);
    for chunk in body.chunks(4) {
        let mut acc = 0u32;
        for (i, byte) in chunk.iter().enumerate() {
            acc |= sextet(*byte)? << (18 - 6 * i);
        }
        let bytes = chunk.len() - 1;
        for i in 0..bytes {
            out.push(((acc >> (16 - 8 * i)) & 0xff) as u8);
        }
    }
    Some(out)
}

pub(crate) fn proto_kind(filename: &str) -> Option<&'static str> {
    let lower = filename.to_ascii_lowercase();
    if lower.ends_with(".proto") {
        return Some("proto");
    }
    if [".pb", ".bin", ".desc", ".protoset"]
        .iter()
        .any(|ext| lower.ends_with(ext))
    {
        return Some("descriptor");
    }
    None
}

pub async fn proto_upload(
    State(state): State<Arc<PlayState>>,
    Json(req): Json<ProtoUploadRequest>,
) -> Result<Json<()>, (StatusCode, String)> {
    let filename = req.filename.trim().to_string();
    if filename.is_empty() || proto_kind(&filename).is_none() {
        return Err((
            StatusCode::BAD_REQUEST,
            "Filename must end with .proto, .pb, .bin, .desc or .protoset".to_string(),
        ));
    }
    if filename.contains(['/', '\\']) || reject_traversal(&filename).is_err() {
        return Err((StatusCode::BAD_REQUEST, "Invalid filename".to_string()));
    }
    let bytes = if req.encoding.as_deref() == Some("base64") {
        decode_base64(req.content.trim())
            .ok_or((StatusCode::BAD_REQUEST, "Not base64".to_string()))?
    } else {
        req.content.clone().into_bytes()
    };
    let file_path = resolve_write_path(primary_dir(&state), &filename)?;
    std::fs::write(&file_path, &bytes)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    state
        .collections_mtime
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    Ok(Json(()))
}

pub async fn proto_files(State(state): State<Arc<PlayState>>) -> Json<Vec<ProtoInfo>> {
    const MAX_DEPTH: usize = 4;

    fn walk(
        dir: &std::path::Path,
        root: &std::path::Path,
        depth: usize,
        seen: &mut std::collections::HashSet<String>,
        files: &mut Vec<ProtoInfo>,
    ) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            if name.starts_with('.') || name == "grpctestify-reports" || name == "node_modules" {
                continue;
            }
            if path.is_dir() {
                if depth < MAX_DEPTH {
                    walk(&path, root, depth + 1, seen, files);
                }
                continue;
            }
            let Some(kind) = proto_kind(&name) else {
                continue;
            };
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            if seen.insert(rel.clone())
                && let Ok(meta) = path.metadata()
            {
                files.push(ProtoInfo {
                    kind,
                    path: rel,
                    name,
                    size: meta.len(),
                });
            }
        }
    }

    let mut files = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for dir in &state.collections_dirs {
        walk(dir, dir, 0, &mut seen, &mut files);
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Json(files)
}

#[derive(Serialize)]
pub struct DataFileInfo {
    pub path: String,
    pub name: String,
    pub size: u64,
    pub format: &'static str,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub columns: Vec<String>,
}

const COLUMNS_READ_LIMIT: u64 = 1_000_000;

fn source_columns(path: &std::path::Path, size: u64) -> Vec<String> {
    if size > COLUMNS_READ_LIMIT {
        return Vec::new();
    }
    let Ok(rows) = crate::commands::run::collect_data_rows(path, None) else {
        return Vec::new();
    };
    let Some(first) = rows.first() else {
        return Vec::new();
    };
    let mut names: Vec<String> = first.keys().cloned().collect();
    names.sort();
    names
}

fn data_format(name: &str) -> Option<&'static str> {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".csv") {
        Some("csv")
    } else if lower.ends_with(".tsv") {
        Some("tsv")
    } else if lower.ends_with(".ndjson") || lower.ends_with(".jsonl") {
        Some("ndjson")
    } else {
        None
    }
}

fn sends_one_stream(client_streaming: Option<bool>, messages: usize) -> bool {
    match client_streaming {
        Some(known) => known,
        None => messages > 1,
    }
}

#[derive(Serialize)]
pub struct BenchProfileInfo {
    pub name: String,
    pub description: String,
    pub keys: Vec<(String, String)>,
}

pub async fn bench_profiles() -> Json<Vec<BenchProfileInfo>> {
    let mut out: Vec<BenchProfileInfo> = crate::bench::schema::list_profiles()
        .into_iter()
        .map(|(name, keys)| {
            let description = keys.get("description").cloned().unwrap_or_default();
            let mut pairs: Vec<(String, String)> = keys
                .into_iter()
                .filter(|(k, _)| k != "description")
                .collect();
            pairs.sort_by(|a, b| a.0.cmp(&b.0));
            BenchProfileInfo {
                name,
                description,
                keys: pairs,
            }
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Json(out)
}

pub async fn data_files(State(state): State<Arc<PlayState>>) -> Json<Vec<DataFileInfo>> {
    const MAX_DEPTH: usize = 4;

    fn walk(
        dir: &std::path::Path,
        root: &std::path::Path,
        depth: usize,
        seen: &mut std::collections::HashSet<String>,
        files: &mut Vec<DataFileInfo>,
    ) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            if name.starts_with('.') || name == "grpctestify-reports" || name == "node_modules" {
                continue;
            }
            if path.is_dir() {
                if depth < MAX_DEPTH {
                    walk(&path, root, depth + 1, seen, files);
                }
                continue;
            }
            let Some(format) = data_format(&name) else {
                continue;
            };
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            if seen.insert(rel.clone())
                && let Ok(meta) = path.metadata()
            {
                let size = meta.len();
                files.push(DataFileInfo {
                    path: rel,
                    name,
                    size,
                    format,
                    columns: source_columns(&path, size),
                });
            }
        }
    }

    let mut files = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for dir in &state.collections_dirs {
        walk(dir, dir, 0, &mut seen, &mut files);
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Json(files)
}

#[derive(Serialize)]
pub struct ReflectResponse {
    pub services: Vec<ServiceInfo>,
    pub error: Option<String>,
}

fn no_schema_within(wait: u64) -> String {
    format!(
        "No schema within {wait}s — the target accepted the connection and did not answer. The wait is set beside the address."
    )
}

fn ran_out_of_time(error: &str) -> bool {
    let error = error.to_ascii_lowercase();
    error.contains("timeout expired")
        || error.contains("deadline exceeded")
        || error.contains("timed out")
}

pub async fn reflect_server(
    State(state): State<Arc<PlayState>>,
    Json(req): Json<ReflectRequest>,
) -> Json<ReflectResponse> {
    super::jobs::forget_target_schema().await;

    let tls_config = match tls_config_from_request(
        reports_base(&state),
        &beside_collection(&state, req.collection_path.as_deref()),
        req.tls,
        &req.tls_ca,
        &req.tls_cert,
        &req.tls_key,
        req.tls_insecure,
    ) {
        Ok(c) => c,
        Err(e) => {
            return Json(ReflectResponse {
                services: vec![],
                error: Some(e),
            });
        }
    };

    let proto_config = if let Some(ref coll_path) = req.collection_path {
        if reject_traversal(coll_path).is_err() {
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

    let wait = dial_timeout(req.timeout_seconds);
    let config = crate::grpc::GrpcClientConfig {
        address: req.address.clone(),
        timeout_seconds: wait,
        tls_config,
        proto_config,
        metadata: None,
        target_service: None,
        compression: Default::default(),
        connection_id: 0,
        protocol: parse_protocol(req.protocol.as_deref()),
        version: env!("CARGO_PKG_VERSION").to_string(),
    };

    let client = match tokio::time::timeout(
        std::time::Duration::from_secs(wait),
        crate::grpc::GrpcClient::new(config),
    )
    .await
    {
        Ok(Ok(c)) => c,
        Ok(Err(e)) => {
            let reported = e.to_string();
            return Json(ReflectResponse {
                services: vec![],
                error: Some(if ran_out_of_time(&reported) {
                    no_schema_within(wait)
                } else {
                    format!("Reflection failed: {}", reported)
                }),
            });
        }
        Err(_) => {
            return Json(ReflectResponse {
                services: vec![],
                error: Some(no_schema_within(wait)),
            });
        }
    };

    let pool = client.descriptor_pool();

    if pool.services().next().is_none()
        && matches!(
            parse_protocol(req.protocol.as_deref()),
            crate::grpc::WireProtocol::GrpcWeb | crate::grpc::WireProtocol::ConnectRpc
        )
    {
        return Json(ReflectResponse {
            services: vec![],
            error: Some(
                "grpc-web and ConnectRPC have no reflection — name a descriptor in the file's PROTO section, or ask the gRPC port"
                    .to_string(),
            ),
        });
    }

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

pub(super) fn split_command_line(line: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;

    for ch in line.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }
        match ch {
            '\\' if !in_single => escaped = true,
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            ' ' | '\t' | '\n' if !in_single && !in_double => {
                if !current.is_empty() {
                    args.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }
    if !current.is_empty() {
        args.push(current);
    }
    args
}

pub async fn import_grpcurl(
    Json(req): Json<ImportGrpcurlRequest>,
) -> Result<Json<ImportGrpcurlResponse>, (StatusCode, Json<ApiError>)> {
    let args: Vec<String> = if let Some(a) = req.args {
        a
    } else if let Some(cmd) = req.command {
        split_command_line(&cmd)
    } else {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                error: "There is nothing to read — paste a `grpcurl` command".into(),
            }),
        ));
    };

    if args.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                error: "There is nothing to read — paste a `grpcurl` command".into(),
            }),
        ));
    }
    if args
        .first()
        .is_some_and(|first| first.rsplit('/').next().unwrap_or(first) == "curl")
    {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                error: "That is a curl command, not a grpcurl one — the workbench imports curl on its own"
                    .into(),
            }),
        ));
    }

    let names_grpcurl = args.first().is_some_and(|first| {
        let name = first.rsplit('/').next().unwrap_or(first);
        name == "grpcurl" || name == "grpcurl.exe"
    });
    let grpcurl_args = if names_grpcurl { &args[1..] } else { &args[..] };

    match crate::grpc::grpcurl_invocation::ParsedGrpcurl::parse(grpcurl_args) {
        Ok(parsed) => {
            let body_str = serde_json::to_string_pretty(&parsed.request_body).unwrap_or_default();
            Ok(Json(ImportGrpcurlResponse {
                endpoint: parsed.symbol,
                address: parsed.address,
                headers: parsed.headers,
                body: body_str,
                plaintext: parsed.options.contains_key("plaintext"),
                tls: parsed.tls,
                proto: parsed.proto,
                options: parsed.options,
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

#[derive(Serialize)]
pub struct SchemaFillResponse {
    pub schema: Option<serde_json::Value>,
    pub error: Option<String>,
}

async fn resolve_endpoint_descriptors(
    state: &PlayState,
    req: &SchemaFillRequest,
) -> Result<
    (
        prost_reflect::ServiceDescriptor,
        prost_reflect::MethodDescriptor,
    ),
    String,
> {
    let (full_service, method_name) = req
        .endpoint
        .split_once('/')
        .ok_or_else(|| "Invalid endpoint format".to_string())?;

    let tls_config = tls_config_from_request(
        reports_base(state),
        &beside_collection(state, req.collection_path.as_deref()),
        req.tls,
        &req.tls_ca,
        &req.tls_cert,
        &req.tls_key,
        req.tls_insecure,
    )?;

    let proto_config = if let Some(ref coll_path) = req.collection_path {
        if reject_traversal(coll_path).is_err() {
            return Err("Invalid collection_path".to_string());
        }
        let file_path =
            resolve_file(state, coll_path).unwrap_or_else(|| primary_dir(state).join(coll_path));
        if file_path.exists() {
            let parse_result = crate::parser::parse_with_recovery(&file_path);
            crate::execution::runner_helpers::build_proto_config(&parse_result.document, &file_path)
        } else {
            None
        }
    } else {
        None
    };

    let wait = dial_timeout(req.timeout_seconds);
    let grpc_config = crate::grpc::GrpcClientConfig {
        address: req.address.clone(),
        timeout_seconds: wait,
        tls_config,
        proto_config,
        metadata: None,
        target_service: Some(full_service.to_string()),
        compression: Default::default(),
        connection_id: 0,
        protocol: parse_protocol(req.protocol.as_deref()),
        version: env!("CARGO_PKG_VERSION").to_string(),
    };

    let client = tokio::time::timeout(
        std::time::Duration::from_secs(wait),
        crate::grpc::GrpcClient::new(grpc_config),
    )
    .await
    .map_err(|_| no_schema_within(wait))?
    .map_err(|e| {
        let reported = e.to_string();
        if ran_out_of_time(&reported) {
            no_schema_within(wait)
        } else {
            format!("Failed to load descriptors: {}", reported)
        }
    })?;

    let svc = client
        .descriptor_pool()
        .get_service_by_name(full_service)
        .ok_or_else(|| format!("Service '{}' not found", full_service))?;
    let method = svc
        .methods()
        .find(|m| m.name() == method_name)
        .ok_or_else(|| format!("Method '{}' not found in '{}'", method_name, full_service))?;

    Ok((svc, method))
}

#[derive(Serialize)]
pub struct ScaffoldResponse {
    pub content: Option<String>,
    pub error: Option<String>,
}

pub async fn scaffold(
    State(state): State<Arc<PlayState>>,
    Json(req): Json<SchemaFillRequest>,
) -> Json<ScaffoldResponse> {
    if split_http_endpoint(&req.endpoint).is_some() {
        return Json(ScaffoldResponse {
            content: None,
            error: Some(
                "A scaffold is written from a gRPC schema — an HTTP request has none. Save this request instead."
                    .to_string(),
            ),
        });
    }

    let method = match resolve_endpoint_descriptors(&state, &req).await {
        Ok((_, method)) => method,
        Err(e) => {
            return Json(ScaffoldResponse {
                content: None,
                error: Some(e),
            });
        }
    };

    let proto_ref = req
        .collection_path
        .as_deref()
        .and_then(|path| proto_ref_of(&state, path));

    Json(ScaffoldResponse {
        content: Some(crate::commands::scaffold::render_scaffold(
            &req.endpoint,
            &req.address,
            req.protocol.as_deref().unwrap_or("grpc"),
            proto_ref.as_ref(),
            &method,
        )),
        error: None,
    })
}

fn proto_ref_of(state: &PlayState, path: &str) -> Option<crate::commands::scaffold::ProtoRef> {
    if reject_traversal(path).is_err() {
        return None;
    }
    let file = resolve_file(state, path)?;
    let parsed = parse_collection(&crate::parser::parse_with_recovery(&file).document);
    let csv = |key: &str| -> Vec<String> {
        parsed
            .proto
            .get(key)
            .map(|v| {
                v.split(',')
                    .map(|p| p.trim().to_string())
                    .filter(|p| !p.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    };
    let files = csv("files");
    let import_paths = csv("import_paths");
    let descriptor = parsed.proto.get("descriptor").cloned();
    if files.is_empty() && import_paths.is_empty() && descriptor.is_none() {
        return None;
    }
    Some(crate::commands::scaffold::ProtoRef {
        files,
        import_paths,
        descriptor,
    })
}

pub async fn schema_fill(
    State(state): State<Arc<PlayState>>,
    Json(req): Json<SchemaFillRequest>,
) -> Json<SchemaFillResponse> {
    match resolve_endpoint_descriptors(&state, &req).await {
        Ok((_, method)) => Json(SchemaFillResponse {
            schema: Some(generate_json_template(&method.input())),
            error: None,
        }),
        Err(e) => Json(SchemaFillResponse {
            schema: None,
            error: Some(e),
        }),
    }
}

#[derive(Serialize)]
pub struct ProtoSourceResponse {
    pub source: Option<String>,
    pub error: Option<String>,
}

pub async fn proto_source(
    State(state): State<Arc<PlayState>>,
    Json(req): Json<SchemaFillRequest>,
) -> Json<ProtoSourceResponse> {
    match resolve_endpoint_descriptors(&state, &req).await {
        Ok((svc, _)) => Json(ProtoSourceResponse {
            source: Some(render_service_schema(&svc)),
            error: None,
        }),
        Err(e) => Json(ProtoSourceResponse {
            source: None,
            error: Some(e),
        }),
    }
}

fn render_service_schema(svc: &prost_reflect::ServiceDescriptor) -> String {
    let mut out = format!("service {} {{\n", svc.full_name());
    for m in svc.methods() {
        let stream_in = if m.is_client_streaming() {
            "stream "
        } else {
            ""
        };
        let stream_out = if m.is_server_streaming() {
            "stream "
        } else {
            ""
        };
        out.push_str(&format!(
            "  rpc {}({}{}) returns ({}{});\n",
            m.name(),
            stream_in,
            m.input().name(),
            stream_out,
            m.output().name()
        ));
    }
    out.push_str("}\n");

    let mut messages: Vec<prost_reflect::MessageDescriptor> = Vec::new();
    let mut enums: Vec<prost_reflect::EnumDescriptor> = Vec::new();
    for m in svc.methods() {
        collect_types(&m.input(), &mut messages, &mut enums);
        collect_types(&m.output(), &mut messages, &mut enums);
    }

    for message in &messages {
        out.push('\n');
        out.push_str(&render_message(message));
    }
    for enumeration in &enums {
        out.push('\n');
        out.push_str(&render_enum(enumeration));
    }
    out
}

fn collect_types(
    desc: &prost_reflect::MessageDescriptor,
    messages: &mut Vec<prost_reflect::MessageDescriptor>,
    enums: &mut Vec<prost_reflect::EnumDescriptor>,
) {
    const LIMIT: usize = 120;
    if desc.is_map_entry() || messages.len() >= LIMIT {
        return;
    }
    if messages.iter().any(|m| m.full_name() == desc.full_name()) {
        return;
    }
    messages.push(desc.clone());
    for field in desc.fields() {
        match field.kind() {
            prost_reflect::Kind::Message(m) => {
                if m.is_map_entry() {
                    for inner in [m.map_entry_key_field(), m.map_entry_value_field()] {
                        match inner.kind() {
                            prost_reflect::Kind::Message(m) => collect_types(&m, messages, enums),
                            prost_reflect::Kind::Enum(e) => push_enum(e, enums),
                            _ => {}
                        }
                    }
                } else {
                    collect_types(&m, messages, enums);
                }
            }
            prost_reflect::Kind::Enum(e) => push_enum(e, enums),
            _ => {}
        }
    }
}

fn push_enum(e: prost_reflect::EnumDescriptor, enums: &mut Vec<prost_reflect::EnumDescriptor>) {
    if !enums.iter().any(|x| x.full_name() == e.full_name()) {
        enums.push(e);
    }
}

fn render_message(desc: &prost_reflect::MessageDescriptor) -> String {
    use std::fmt::Write as _;
    let mut out = format!("message {} {{\n", desc.name());
    for field in desc.fields() {
        if field.containing_oneof().is_some_and(|o| !o.is_synthetic()) {
            continue;
        }
        let _ = writeln!(out, "  {}", field_line(&field));
    }
    for oneof in desc.oneofs().filter(|o| !o.is_synthetic()) {
        let _ = writeln!(out, "  oneof {} {{", oneof.name());
        for field in oneof.fields() {
            let _ = writeln!(out, "    {}", field_line(&field));
        }
        out.push_str("  }\n");
    }
    out.push_str("}\n");
    out
}

fn field_line(field: &prost_reflect::FieldDescriptor) -> String {
    let label = if field.is_list() { "repeated " } else { "" };
    format!(
        "{label}{} {} = {};",
        crate::commands::reflect::field_type_label(field),
        field.name(),
        field.number()
    )
}

fn render_enum(desc: &prost_reflect::EnumDescriptor) -> String {
    use std::fmt::Write as _;
    let mut out = format!("enum {} {{\n", desc.name());
    for value in desc.values() {
        let _ = writeln!(out, "  {} = {};", value.name(), value.number());
    }
    out.push_str("}\n");
    out
}

pub use crate::grpc::template::generate_json_template;
#[cfg(test)]
use crate::grpc::template::{fake_value, well_known_sample};

#[derive(Serialize)]
pub struct CallCommandResponse {
    pub command: String,
}

fn command_path(state: &PlayState, file: &std::path::Path) -> String {
    let base = state
        .project_root
        .clone()
        .map(|root| {
            if root.file_name().is_some_and(|name| name == ".grpctestify") {
                root.parent()
                    .map(std::path::Path::to_path_buf)
                    .unwrap_or(root)
            } else {
                root
            }
        })
        .or_else(|| std::env::current_dir().ok());
    match base.and_then(|base| {
        file.strip_prefix(base)
            .ok()
            .map(std::path::Path::to_path_buf)
    }) {
        Some(relative) => relative.to_string_lossy().replace('\\', "/"),
        None => file.to_string_lossy().to_string(),
    }
}

pub async fn generate_call_command(
    State(state): State<Arc<PlayState>>,
    Json(req): Json<CallRequest>,
) -> Json<CallCommandResponse> {
    let body = match &req.body {
        serde_json::Value::Array(arr) => arr.first().cloned(),
        serde_json::Value::Null => None,
        other => Some(other.clone()),
    };
    let body = body.map(|b| serde_json::to_string(&b).unwrap_or_default());
    let tls = req.tls.unwrap_or(false);

    if let Some(path) = req
        .collection_path
        .as_deref()
        .map(str::trim)
        .filter(|path| !path.is_empty())
    {
        let named = resolve_file(&state, path)
            .map(|file| command_path(&state, &file))
            .unwrap_or_else(|| path.to_string());
        return Json(CallCommandResponse {
            command: crate::commands::call_line::grpctestify_call_file(
                &named,
                req.document_index + 1,
            ),
        });
    }

    Json(CallCommandResponse {
        command: crate::commands::call_line::grpctestify_call(
            crate::commands::call_line::CallSpec {
                endpoint: &req.endpoint,
                address: req.address.as_deref(),
                protocol: req.protocol.as_deref(),
                body: body.as_deref(),
                headers: &{
                    let mut headers: Vec<(String, String)> = req
                        .headers
                        .clone()
                        .unwrap_or_default()
                        .into_iter()
                        .collect();
                    headers.sort();
                    headers
                },
                tls: crate::commands::call_line::TlsPaths {
                    ca: req.tls_ca.as_deref(),
                    cert: req.tls_cert.as_deref(),
                    key: req.tls_key.as_deref(),
                },
                insecure: tls && req.tls_insecure.unwrap_or(false),
                plaintext: !tls,
                max_time: req.timeout_seconds,
                ..Default::default()
            },
        ),
    })
}

pub async fn generate_grpcurl(
    State(state): State<Arc<PlayState>>,
    Json(req): Json<CallRequest>,
) -> Json<GrpcurlResponse> {
    let messages: Vec<serde_json::Value> = match &req.body {
        serde_json::Value::Array(arr) => arr.clone(),
        other => vec![other.clone()],
    };

    let mut builder = crate::parser::GctfDocumentBuilder::new()
        .with_file_path("<convert>")
        .endpoint(&req.endpoint);

    if let Some(address) = &req.address {
        builder = builder.address(address);
    }

    if req.tls.unwrap_or(false) {
        let mut tls: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        if let Some(ca) = &req.tls_ca {
            tls.insert("ca_cert".to_string(), ca.clone());
        }
        if let Some(cert) = &req.tls_cert {
            tls.insert("client_cert".to_string(), cert.clone());
        }
        if let Some(key) = &req.tls_key {
            tls.insert("client_key".to_string(), key.clone());
        }
        if req.tls_insecure.unwrap_or(true) {
            tls.insert("insecure".to_string(), "true".to_string());
        }
        builder = builder.tls(tls);
    }

    for msg in &messages {
        builder = builder.request(msg.clone());
    }

    if let Some(headers) = &req.headers
        && !headers.is_empty()
    {
        builder = builder.request_headers(headers.clone());
    }

    let from_file = req
        .collection_path
        .as_deref()
        .filter(|path| reject_traversal(path).is_ok())
        .and_then(|path| resolve_file(&state, path))
        .filter(|path| path.exists());
    if let Some(file) = &from_file {
        let parsed = crate::parser::parse_with_recovery(file);
        if let Some(proto) = parsed
            .document
            .first_section(crate::parser::SectionType::Proto)
            && let crate::parser::SectionContent::KeyValues(pairs) = &proto.content
        {
            builder = builder.proto(pairs.iter().map(|(k, v)| (k.clone(), v.clone())));
        }
    }

    let doc = builder.build();
    let cwd = std::env::current_dir().unwrap_or_default();
    let anchor = from_file
        .clone()
        .unwrap_or_else(|| std::path::PathBuf::from("<inline>"));
    let command = crate::commands::grpcurl::build_grpcurl_command(&doc, &anchor, &cwd, 1);

    match command {
        Ok(output) => Json(GrpcurlResponse {
            command: output.command,
        }),
        Err(e) => Json(GrpcurlResponse {
            command: format!("# error: {}", e),
        }),
    }
}

pub(super) fn names_unknown_symbol(message: &str) -> bool {
    let m = message.to_ascii_lowercase();
    (m.contains("not found") || m.contains("unimplemented") || m.contains("unknown"))
        && (m.contains("service") || m.contains("method") || m.contains("symbol"))
}

fn rebased_offsets(call_base: u64, elapsed: u64, offsets: &[u64], messages: usize) -> Vec<u64> {
    if offsets.len() != messages {
        return vec![elapsed; messages];
    }
    offsets.iter().map(|o| call_base + o).collect()
}

fn messages_of(req: &CallRequest) -> Result<Vec<serde_json::Value>, (StatusCode, String)> {
    let Some(raw_bodies) = &req.bodies_raw else {
        return Ok(match &req.body {
            serde_json::Value::Array(arr) => arr.clone(),
            serde_json::Value::Null => vec![],
            other => vec![other.clone()],
        });
    };
    let mut parsed = Vec::with_capacity(raw_bodies.len());
    for (i, body) in raw_bodies.iter().enumerate() {
        if body.trim().is_empty() {
            continue;
        }
        match serde_json::from_str(body) {
            Ok(value) => parsed.push(value),
            Err(e) => {
                return Err((
                    StatusCode::BAD_REQUEST,
                    format!("message #{} is not valid JSON: {e}", i + 1),
                ));
            }
        }
    }
    Ok(parsed)
}

fn protocol_for_call(
    from_file: Option<crate::grpc::WireProtocol>,
    requested: Option<&str>,
) -> crate::grpc::WireProtocol {
    from_file.unwrap_or_else(|| parse_protocol(requested))
}

fn tls_for_call(
    from_file: Option<crate::grpc::TlsConfig>,
    from_client: Option<crate::grpc::TlsConfig>,
) -> Option<crate::grpc::TlsConfig> {
    from_file.or(from_client)
}

fn call_refused(why: String) -> CallResponse {
    CallResponse {
        success: false,
        messages: vec![],
        message_offsets_ms: vec![],
        grpc_status: None,
        headers: HashMap::new(),
        trailers: HashMap::new(),
        error: Some(why),
        shape: None,
        messages_total: 0,
        messages_truncated: false,
        messages_raw: vec![],
        extracted: Vec::new(),
    }
}

#[derive(Deserialize)]
pub struct DocsRequest {
    #[serde(default)]
    pub paths: Vec<String>,
    #[serde(default)]
    pub job_id: Option<String>,
}

#[derive(Serialize)]
pub struct DocsPage {
    pub name: String,
    pub markdown: String,
}

pub async fn docs(
    State(state): State<Arc<PlayState>>,
    Json(req): Json<DocsRequest>,
) -> Result<Json<Vec<DocsPage>>, (StatusCode, String)> {
    let mut paths: Vec<std::path::PathBuf> = Vec::new();
    for path in &req.paths {
        if reject_traversal(path).is_err() {
            return Err((StatusCode::NOT_FOUND, format!("Invalid path: {path}")));
        }
        paths.push(resolve_file(&state, path).unwrap_or_else(|| primary_dir(&state).join(path)));
    }
    if paths.is_empty() {
        paths.push(primary_dir(&state).to_path_buf());
    }

    let coverage = req
        .job_id
        .as_deref()
        .and_then(|id| state.jobs.get(id))
        .and_then(|job| job.coverage())
        .and_then(|value| {
            serde_json::from_value::<apif_utils::coverage::CoverageReport>(value).ok()
        });

    let pages = tokio::task::spawn_blocking(move || {
        crate::commands::docs::render_pages(&paths, coverage.as_ref())
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(
        pages
            .into_iter()
            .map(|p| DocsPage {
                name: p.name,
                markdown: p.markdown,
            })
            .collect(),
    ))
}

pub fn timeout_for_file(doc: &crate::parser::GctfDocument) -> Option<u64> {
    doc.sections
        .iter()
        .filter_map(|s| s.get_timeout())
        .next()
        .or_else(|| {
            doc.get_options()
                .as_ref()
                .and_then(|o| o.get("timeout"))
                .and_then(|v| v.trim().parse::<u64>().ok())
                .filter(|v| *v > 0)
        })
}

pub fn compression_for_call(
    doc: &crate::parser::GctfDocument,
    env_default: apif_grpc_transport::CompressionMode,
) -> Result<apif_grpc_transport::CompressionMode, String> {
    let options = doc.get_options().unwrap_or_default();
    crate::execution::runner_helpers::resolve_compression(doc, &options, env_default)
}

pub const MAX_DIAL_TIMEOUT_SECS: u64 = 300;
const DEFAULT_CALL_TIMEOUT_SECS: u64 = 30;
const DEFAULT_DIAL_TIMEOUT_SECS: u64 = 10;

pub fn timeout_for_call(file: Option<u64>, requested: Option<u64>) -> u64 {
    file.filter(|v| *v > 0)
        .or(requested.filter(|v| *v > 0))
        .unwrap_or(DEFAULT_CALL_TIMEOUT_SECS)
}

pub fn dial_timeout(requested: Option<u64>) -> u64 {
    requested
        .filter(|v| *v > 0)
        .unwrap_or(DEFAULT_DIAL_TIMEOUT_SECS)
        .min(MAX_DIAL_TIMEOUT_SECS)
}

fn step_of_file(
    file: &crate::parser::ast::GctfDocument,
    index: usize,
) -> (&crate::parser::ast::GctfDocument, Option<String>) {
    let doc = file.iter_chain().nth(index).unwrap_or(file);
    let address = crate::execution::runner::chain_addresses(file)
        .get(index)
        .cloned()
        .flatten()
        .or_else(|| doc.get_address(None));
    (doc, address)
}

fn follow_redirects_for_file(doc: &crate::parser::ast::GctfDocument) -> bool {
    doc.get_options()
        .as_ref()
        .and_then(|o| o.get(apif_http_transport::FOLLOW_REDIRECTS_OPTION))
        .and_then(|v| apif_http_transport::parse_follow_redirects(v))
        .unwrap_or(false)
}

async fn http_file_connection(
    state: &Arc<PlayState>,
    path: &str,
    step_index: usize,
) -> Result<(Option<String>, Option<u64>, bool), (StatusCode, String)> {
    if reject_traversal(path).is_err() {
        return Err((StatusCode::NOT_FOUND, "Invalid collection_path".to_string()));
    }
    let state = state.clone();
    let path = path.to_string();
    tokio::task::spawn_blocking(move || {
        let file_path =
            resolve_file(&state, &path).unwrap_or_else(|| primary_dir(&state).join(&path));
        if !file_path.exists() {
            return (None, None, false);
        }
        let parse_result = crate::parser::parse_with_recovery(&file_path);
        let (doc, address) = step_of_file(&parse_result.document, step_index);
        (
            address.filter(|a| !a.trim().is_empty()),
            timeout_for_file(doc),
            follow_redirects_for_file(doc),
        )
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

async fn execute_http_call(
    state: Arc<PlayState>,
    req: CallRequest,
) -> Result<Json<CallResponse>, (StatusCode, String)> {
    let Some((method, path)) = split_http_endpoint(&req.endpoint) else {
        return Err((
            StatusCode::BAD_REQUEST,
            "ENDPOINT must be a method and a path, like `POST /v1/users`".to_string(),
        ));
    };

    let (file_address, file_timeout, follow_redirects) = match req.collection_path.as_deref() {
        Some(path) => http_file_connection(&state, path, req.document_index).await?,
        None => (None, None, false),
    };

    let address = file_address
        .or_else(|| req.address.clone())
        .filter(|a| !a.trim().is_empty())
        .map(|a| {
            crate::execution::runner_helpers::interpolate_variables(
                &a,
                &project_call_variables(&state),
            )
            .unwrap_or(a)
        });
    let mut vars = project_call_variables(&state);
    vars.extend(dataset_variables(&state, &req));
    let path =
        crate::execution::runner_helpers::interpolate_variables(&path, &vars).unwrap_or(path);
    let url = apif_http_transport::url_for(address.as_deref(), &path);
    let headers: std::collections::HashMap<String, String> = req
        .headers
        .clone()
        .unwrap_or_default()
        .into_iter()
        .map(|(k, v)| {
            let v = crate::execution::runner_helpers::interpolate_variables(&v, &vars).unwrap_or(v);
            (k, v)
        })
        .collect();
    let body = req
        .bodies_raw
        .as_ref()
        .and_then(|bodies| bodies.first().cloned())
        .filter(|b| !b.trim().is_empty())
        .map(|b| crate::execution::runner_helpers::interpolate_variables(&b, &vars).unwrap_or(b));

    let timeout =
        std::time::Duration::from_secs(timeout_for_call(file_timeout, req.timeout_seconds));
    let started = std::time::Instant::now();
    let answer = apif_http_transport::send(apif_http_transport::HttpCall {
        method: method.clone(),
        url: url.clone(),
        headers: headers
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
        body: body.clone(),
        timeout,
        follow_redirects,
    })
    .await;

    let response = match answer {
        Ok(answer) => CallResponse {
            success: true,
            messages: vec![answer.body.clone()],
            message_offsets_ms: vec![answer.duration_ms],
            grpc_status: Some(answer.status as u32),
            headers: answer.headers.clone(),
            trailers: Default::default(),
            error: None,
            shape: Some("unary".to_string()),
            messages_total: 1,
            messages_truncated: false,
            messages_raw: vec![answer.raw_body.clone()],
            extracted: extracted_by_call(&state, &req, Some(&answer.body)),
        },
        Err(message) => CallResponse {
            success: false,
            messages: vec![],
            message_offsets_ms: vec![],
            grpc_status: None,
            headers: Default::default(),
            trailers: Default::default(),
            error: Some(message),
            shape: Some("unary".to_string()),
            messages_total: 0,
            messages_truncated: false,
            messages_raw: vec![],
            extracted: Vec::new(),
        },
    };

    let took_ms = started.elapsed().as_millis() as u64;
    if let Some(sid) = req.session_id.clone()
        && let Ok(root) = require_project(&state)
    {
        let entry = serde_json::json!({
            "id": uuid::Uuid::new_v4().to_string(),
            "timestamp": apif_cfg_runtime::now_unix_millis(),
            "endpoint": req.endpoint,
            "collection_path": req.collection_path.clone(),
            "dataset_row": req.dataset_row,
            "bodies": recorded_bodies(&req),
            "headers": super::project::redact_secrets(&headers),
            "connection": {
                "address": address.clone().unwrap_or_default(),
                "tls": url.starts_with("https://"),
            },
            "response": {
                "status": if response.success { "ok" } else { "error" },
                "status_code": response.grpc_status,
                "duration_ms": took_ms,
                "error": response.error.clone(),
                "messages": response.messages.clone(),
                "headers": super::project::redact_secrets(&response.headers),
                "trailers": serde_json::Map::new(),
            },
        });
        if let Ok(line) = serde_json::to_string(&entry) {
            let _guard = state.history_lock.lock().await;
            let root = root.to_path_buf();
            tokio::task::spawn_blocking(move || {
                super::project::append_history_entry(&root, &sid, &line).ok();
            })
            .await
            .ok();
        }
    }

    Ok(Json(response))
}

fn recorded_bodies(req: &CallRequest) -> Vec<String> {
    req.bodies_raw.clone().unwrap_or_default()
}

fn dataset_variables(
    state: &PlayState,
    req: &CallRequest,
) -> std::collections::HashMap<String, serde_json::Value> {
    let (Some(path), Some(row)) = (req.collection_path.as_deref(), req.dataset_row) else {
        return std::collections::HashMap::new();
    };
    resolve_file(state, path)
        .and_then(|file| super::jobs::dataset_rows(&file))
        .and_then(|rows| rows.into_iter().nth(row))
        .unwrap_or_default()
}

fn extracted_by_call(
    state: &PlayState,
    req: &CallRequest,
    message: Option<&serde_json::Value>,
) -> Vec<(String, String)> {
    const MAX_VALUE_BYTES: usize = 4 * 1024;
    use crate::parser::ast::{SectionContent, SectionType};

    let (Some(path), Some(message)) = (req.collection_path.as_deref(), message) else {
        return Vec::new();
    };
    let Some(file) = resolve_file(state, path) else {
        return Vec::new();
    };
    if !file.exists() {
        return Vec::new();
    }
    let parsed = crate::parser::parse_with_recovery(&file);
    let (doc, _) = step_of_file(&parsed.document, req.document_index);
    let engine = crate::assert::AssertionEngine::new();
    let mut out: Vec<(String, String)> = Vec::new();
    for section in doc.sections_by_type(SectionType::Extract) {
        let SectionContent::Extract(bindings) = &section.content else {
            continue;
        };
        for (name, query) in bindings.iter() {
            if out.iter().any(|(seen, _)| seen == name) {
                continue;
            }
            let Ok(results) = engine.query(query, message) else {
                continue;
            };
            let Some(value) = results.first() else {
                continue;
            };
            let rendered = match value {
                serde_json::Value::String(text) => text.clone(),
                other => other.to_string(),
            };
            if rendered.len() > MAX_VALUE_BYTES {
                continue;
            }
            out.push((name.clone(), rendered));
        }
    }
    out
}

fn split_http_endpoint(endpoint: &str) -> Option<(String, String)> {
    let trimmed = endpoint.trim();
    let (method, path) = trimmed.split_once(' ')?;
    let method = method.trim();
    let path = path.trim();
    if method.is_empty() || path.is_empty() {
        return None;
    }
    Some((method.to_ascii_uppercase(), path.to_string()))
}

pub async fn execute_call(
    State(state): State<Arc<PlayState>>,
    Json(req): Json<CallRequest>,
) -> Result<Json<CallResponse>, (StatusCode, String)> {
    let http = match req.collection_path.as_deref() {
        Some(path) if path.to_ascii_lowercase().ends_with(".httf") => true,
        Some(path) if path.to_ascii_lowercase().ends_with(".gctf") => false,
        _ => split_http_endpoint(&req.endpoint).is_some(),
    };
    if http {
        return execute_http_call(state, req).await;
    }

    let parts: Vec<&str> = req.endpoint.split('/').collect();
    if parts.len() != 2 {
        return Err((
            StatusCode::BAD_REQUEST,
            format!(
                "`{}` is not a gRPC endpoint — a service and a method, like `package.Service/Method`",
                req.endpoint.trim()
            ),
        ));
    }
    let (full_service, method_name) = (parts[0].to_string(), parts[1].to_string());

    let mut messages = messages_of(&req)?;
    {
        let mut vars = project_call_variables(&state);
        vars.extend(dataset_variables(&state, &req));
        if !vars.is_empty() {
            for message in &mut messages {
                crate::execution::runner_helpers::substitute_variables(message, &vars);
            }
        }
    }
    if messages.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "There is no message to send — a call carries one even when it is `{}`".to_string(),
        ));
    }

    type FileConnection = (
        Option<crate::grpc::ProtoConfig>,
        Option<String>,
        Option<crate::grpc::TlsConfig>,
        Option<crate::grpc::WireProtocol>,
        Option<u64>,
        apif_grpc_transport::CompressionMode,
    );
    let (proto_config, file_address, file_tls, file_protocol, file_timeout, compression) =
        if let Some(ref coll_path) = req.collection_path {
            if reject_traversal(coll_path).is_err() {
                return Err((StatusCode::NOT_FOUND, "Invalid collection_path".to_string()));
            }
            let state = state.clone();
            let path = coll_path.clone();
            let step_index = req.document_index;
            let result = tokio::task::spawn_blocking(
                move || -> Result<FileConnection, (StatusCode, String)> {
                    let file_path = resolve_file(&state, &path)
                        .unwrap_or_else(|| primary_dir(&state).join(&path));
                    if file_path.exists() {
                        let parse_result = crate::parser::parse_with_recovery(&file_path);
                        let (doc, address) = step_of_file(&parse_result.document, step_index);
                        let options = doc.get_options();
                        Ok((
                            crate::execution::runner_helpers::build_proto_config(doc, &file_path),
                            address,
                            crate::execution::runner_helpers::build_tls_config(doc, &file_path),
                            options
                                .as_ref()
                                .and_then(|o| o.get("protocol").cloned())
                                .and_then(|p| p.parse::<crate::grpc::WireProtocol>().ok()),
                            timeout_for_file(doc),
                            compression_for_call(doc, crate::config::compression_from_env())
                                .map_err(|e| (StatusCode::BAD_REQUEST, e))?,
                        ))
                    } else {
                        Ok((
                            None,
                            None,
                            None,
                            None,
                            None,
                            crate::config::compression_from_env(),
                        ))
                    }
                },
            )
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            result?
        } else {
            (
                None,
                None,
                None,
                None,
                None,
                crate::config::compression_from_env(),
            )
        };

    let protocol = protocol_for_call(file_protocol, req.protocol.as_deref());
    let from_client = if file_tls.is_some() {
        None
    } else {
        match tls_config_from_request(
            reports_base(&state),
            &beside_collection(&state, req.collection_path.as_deref()),
            req.tls,
            &req.tls_ca,
            &req.tls_cert,
            &req.tls_key,
            req.tls_insecure,
        ) {
            Ok(built) => built,
            Err(why) => return Ok(Json(call_refused(why))),
        }
    };
    let tls_config = tls_for_call(file_tls, from_client);
    let over_tls = tls_config.is_some();

    let address = file_address
        .or_else(|| req.address.clone().filter(|a| !a.trim().is_empty()))
        .or_else(|| {
            std::env::var(crate::config::ENV_GRPCTESTIFY_ADDRESS)
                .ok()
                .filter(|a| !a.trim().is_empty())
        })
        .unwrap_or_else(|| crate::grpc::default_address_for(protocol).to_string());
    let address = crate::execution::runner_helpers::interpolate_variables(
        &address,
        &project_call_variables(&state),
    )
    .unwrap_or(address);

    let call_variables = project_call_variables(&state);
    let substituted_headers = req.headers.clone().map(|headers| {
        headers
            .into_iter()
            .map(|(k, v)| {
                let v =
                    crate::execution::runner_helpers::interpolate_variables(&v, &call_variables)
                        .unwrap_or(v);
                (k, v)
            })
            .collect()
    });

    let grpc_config = crate::grpc::GrpcClientConfig {
        address: address.to_string(),
        timeout_seconds: timeout_for_call(file_timeout, req.timeout_seconds),
        tls_config,
        proto_config,
        metadata: substituted_headers,
        target_service: Some(full_service.clone()),
        compression,
        connection_id: 0,
        protocol,
        version: env!("CARGO_PKG_VERSION").to_string(),
    };

    let dial_deadline = std::time::Duration::from_secs(grpc_config.timeout_seconds);
    let dialled = tokio::time::timeout(dial_deadline, async {
        match crate::grpc::TransportRef::new(&grpc_config).await {
            Err(e) if names_unknown_symbol(&e.to_string()) => {
                super::jobs::forget_target_schema().await;
                crate::grpc::TransportRef::new(&grpc_config).await
            }
            other => other,
        }
    })
    .await
    .unwrap_or_else(|_| {
        Err(anyhow::anyhow!(
            "No answer within {}s — the wait is set beside the address, and a file's OPTIONS timeout wins over it",
            grpc_config.timeout_seconds
        ))
    });

    let mut transport = match dialled {
        Ok(t) => t,
        Err(e) => {
            return Ok(Json(CallResponse {
                success: false,
                messages: vec![],
                message_offsets_ms: vec![],
                grpc_status: None,
                headers: HashMap::new(),
                trailers: HashMap::new(),
                error: Some(e.to_string()),
                shape: None,
                messages_total: 0,
                messages_truncated: false,
                messages_raw: vec![],
                extracted: Vec::new(),
            }));
        }
    };

    let mut response_messages = Vec::new();
    let mut response_offsets_ms: Vec<u64> = Vec::new();
    let mut grpc_status: Option<u32> = None;
    let mut resp_headers = HashMap::new();
    let mut response_trailers = HashMap::new();
    let mut response_error = None;

    let info = transport.method_info(&full_service, &method_name);
    let shape = info
        .as_ref()
        .map(|m| shape_name(m.client_streaming, m.server_streaming).to_string());
    let streams_from_client =
        sends_one_stream(info.as_ref().map(|m| m.client_streaming), messages.len());

    let call_started = std::time::Instant::now();
    let calls: Vec<Vec<serde_json::Value>> = if streams_from_client {
        vec![messages]
    } else {
        messages.into_iter().map(|m| vec![m]).collect()
    };

    for bodies in calls {
        let call_base = call_started.elapsed().as_millis() as u64;
        let result = if streams_from_client {
            transport
                .execute_streaming(&grpc_config, &full_service, &method_name, bodies, None)
                .await
        } else {
            let body = bodies.into_iter().next().unwrap_or(serde_json::Value::Null);
            transport
                .execute(&grpc_config, &full_service, &method_name, body, None)
                .await
        };
        let elapsed = call_started.elapsed().as_millis() as u64;
        response_offsets_ms.extend(rebased_offsets(
            call_base,
            elapsed,
            &result.message_offsets_ms,
            result.messages.len(),
        ));
        response_messages.extend(result.messages);
        response_trailers.extend(result.trailers);
        if resp_headers.is_empty() {
            resp_headers = result.headers;
        }
        if let Some(e) = result.error {
            grpc_status = Some(e.code());
            response_error = Some(e.to_string());
            break;
        }
    }

    let success = response_error.is_none();
    if grpc_status.is_none() {
        grpc_status = response_trailers
            .get("grpc-status")
            .and_then(|s| s.parse::<u32>().ok())
            .or(if success { Some(0) } else { None });
    }

    if let Some(sid) = req.session_id.clone() {
        let hist_body = recorded_bodies(&req);
        let hist_headers = req.headers.clone().unwrap_or_default();
        if let Ok(root) = require_project(&state) {
            let entry = serde_json::json!({
                "id": uuid::Uuid::new_v4().to_string(),
                "timestamp": apif_cfg_runtime::now_unix_millis(),
                "endpoint": req.endpoint,
                "collection_path": req.collection_path.clone(),
                "dataset_row": req.dataset_row,
                "bodies": hist_body,
                "headers": super::project::redact_secrets(&hist_headers),
                "connection": {
                    "address": address.clone(),
                    "protocol": match protocol {
                        crate::grpc::WireProtocol::GrpcWeb => "grpc-web",
                        crate::grpc::WireProtocol::ConnectRpc => "connectrpc",
                        _ => "grpc",
                    },
                    "tls": over_tls,
                },
                "response": {
                    "status": if success { "ok" } else { "error" },
                    "status_code": grpc_status,
                    "duration_ms": call_started.elapsed().as_millis() as u64,
                    "error": response_error.clone(),
                    "shape": shape.clone(),
                    "messages": response_messages.clone(),
                    "headers": super::project::redact_secrets(&resp_headers),
                    "trailers": super::project::redact_secrets(&response_trailers),
                },
            });

            if let Ok(line) = serde_json::to_string(&entry) {
                let _guard = state.history_lock.lock().await;
                let root = root.to_path_buf();
                let sid = sid.clone();
                tokio::task::spawn_blocking(move || {
                    super::project::append_history_entry(&root, &sid, &line).ok();
                })
                .await
                .ok();
            }
        }
    }

    let extracted = if success {
        extracted_by_call(&state, &req, response_messages.last())
    } else {
        Vec::new()
    };

    let (response_messages, messages_total, messages_truncated) = cap_messages(response_messages);
    response_offsets_ms.truncate(response_messages.len());

    let messages_raw = response_messages
        .iter()
        .map(|m| serde_json::to_string(m).unwrap_or_default())
        .collect();

    Ok(Json(CallResponse {
        success,
        grpc_status,
        message_offsets_ms: response_offsets_ms,
        messages: response_messages,
        headers: resp_headers,
        trailers: response_trailers,
        error: response_error,
        shape,
        messages_total,
        messages_truncated,
        messages_raw,
        extracted,
    }))
}

#[derive(Deserialize)]
pub struct RunTestRequest {
    pub collection_path: String,
    pub session_id: Option<String>,
    pub timeout_seconds: Option<u64>,
    pub data: Option<String>,
}

#[derive(Serialize)]
pub struct RunAssertionResult {
    pub line: usize,
    pub expression: String,
    pub passed: bool,
    pub elapsed_ms: u64,
    pub message: Option<String>,
    pub expected: Option<String>,
    pub actual: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

#[derive(Serialize, Default)]
pub struct RunTestResponse {
    pub success: bool,
    pub error: Option<String>,
    pub grpc_status: Option<u32>,
    pub call_duration_ms: Option<u64>,
    pub assertions: Vec<RunAssertionResult>,
    pub documents: Vec<u64>,
    pub response_messages: Vec<serde_json::Value>,
    pub headers: HashMap<String, String>,
    pub trailers: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rows_total: Option<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extracted: Vec<(String, String)>,
}

fn project_call_variables(
    state: &PlayState,
) -> std::collections::HashMap<String, serde_json::Value> {
    state
        .project_root
        .as_deref()
        .and_then(|root| root.parent())
        .map(super::project::project_variables)
        .unwrap_or_default()
        .into_iter()
        .map(|(k, v)| (k, serde_json::Value::String(v)))
        .collect()
}

pub(crate) fn first_parse_error(
    diagnostics: &apif_diagnostics::DiagnosticCollection,
) -> Option<String> {
    diagnostics
        .diagnostics
        .iter()
        .find(|d| d.severity == apif_diagnostics::DiagnosticSeverity::Error)
        .map(|d| d.message.clone())
}

pub async fn execute_test(
    State(state): State<Arc<PlayState>>,
    Json(req): Json<RunTestRequest>,
) -> Result<Json<RunTestResponse>, (StatusCode, String)> {
    reject_traversal(&req.collection_path)?;
    let file_path = resolve_file(&state, &req.collection_path)
        .ok_or((StatusCode::NOT_FOUND, "File not found".to_string()))?;

    let recovered =
        tokio::task::spawn_blocking(move || crate::parser::parse_with_recovery(&file_path))
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if let Some(fault) = first_parse_error(&recovered.diagnostics) {
        return Ok(Json(RunTestResponse {
            success: false,
            error: Some(format!("Parse error: {fault}")),
            ..Default::default()
        }));
    }
    let document = recovered.document;

    if let Err(e) = crate::parser::validate_document_chain(&document) {
        return Ok(Json(RunTestResponse {
            success: false,
            error: Some(format!("Validation error: {e}")),
            ..Default::default()
        }));
    }

    let env = state
        .project_root
        .as_deref()
        .and_then(|root| root.parent())
        .map(super::project::project_variables)
        .unwrap_or_default();
    let runner = crate::execution::runner::TestRunner::new(false, 30, false, false, false, None)
        .with_capture_exchange(true);
    let runner = match super::project::address_of(&env) {
        Some(address) => runner.with_env_address(address),
        None => runner,
    };
    let vars: std::collections::HashMap<String, serde_json::Value> = env
        .into_iter()
        .map(|(k, v)| (k, serde_json::Value::String(v)))
        .collect();
    let (max_retries, retry_delay) = super::jobs::retry_plan(&document);

    let own_rows = resolve_file(&state, &req.collection_path)
        .and_then(|path| super::jobs::dataset_rows(&path))
        .filter(|rows| !rows.is_empty());
    let rows = match req.data.as_deref().map(str::trim).filter(|d| !d.is_empty()) {
        Some(source) => {
            if own_rows.is_some() {
                return Err((
                    StatusCode::BAD_REQUEST,
                    format!(
                        "{} has a DATASET section, which is its own row source — a data source cannot be combined with it",
                        req.collection_path
                    ),
                ));
            }
            reject_traversal(source)?;
            let path = resolve_file(&state, source).ok_or((
                StatusCode::NOT_FOUND,
                format!("Data source not found: {source}"),
            ))?;
            let read = tokio::task::spawn_blocking(move || {
                crate::commands::run::collect_data_rows(&path, None)
            })
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
            .map_err(|e| (StatusCode::BAD_REQUEST, format!("{source}: {e}")))?;
            if read.is_empty() {
                return Err((
                    StatusCode::BAD_REQUEST,
                    format!("{source} produced no rows — there is nothing to run"),
                ));
            }
            Some(read)
        }
        None => own_rows,
    };
    let rows_total = rows.as_ref().map(Vec::len);

    let mut exec = None;
    let mut row_index = None;
    for (index, row) in rows
        .unwrap_or_else(|| vec![std::collections::HashMap::new()])
        .into_iter()
        .enumerate()
    {
        let mut case_vars = vars.clone();
        case_vars.extend(row);
        let result = super::jobs::run_with_retries(
            &runner,
            &document,
            Some(case_vars),
            max_retries,
            retry_delay,
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        let failed = matches!(
            result.status,
            crate::execution::runner::TestExecutionStatus::Fail(_)
        );
        exec = Some(result);
        row_index = rows_total.map(|_| index);
        if failed {
            break;
        }
    }
    let exec = exec.ok_or((
        StatusCode::INTERNAL_SERVER_ERROR,
        "The run produced no result".to_string(),
    ))?;

    let (success, error) = match exec.status {
        crate::execution::runner::TestExecutionStatus::Pass => (true, None),
        crate::execution::runner::TestExecutionStatus::Fail(msg) => (false, Some(msg)),
    };

    let assertions: Vec<RunAssertionResult> = exec
        .assertions
        .into_iter()
        .map(|a| RunAssertionResult {
            line: a.line,
            expression: a.expression,
            passed: a.passed,
            elapsed_ms: a.elapsed_ms,
            message: a.message,
            expected: a.expected,
            actual: a.actual,
            endpoint: a.endpoint,
            hint: a.hint,
        })
        .collect();

    let dialled = exec.dialled_address.clone();
    let (response_messages, headers, trailers) = match exec.captured_response {
        Some(r) => (r.messages, r.headers, r.trailers),
        None => (vec![], HashMap::new(), HashMap::new()),
    };

    let run_status = headers
        .get(apif_http_transport::STATUS_HEADER)
        .and_then(|s| s.parse::<u32>().ok())
        .or_else(|| {
            trailers
                .get("grpc-status")
                .and_then(|s| s.parse::<u32>().ok())
        })
        .or(if success { Some(0) } else { None });

    if let Some(sid) = req.session_id.clone()
        && let Ok(root) = require_project(&state)
    {
        let entry = serde_json::json!({
            "id": uuid::Uuid::new_v4().to_string(),
            "timestamp": apif_cfg_runtime::now_unix_millis(),
            "kind": "run",
            "collection_path": req.collection_path,
            "connection": { "address": dialled.clone().unwrap_or_default() },
            "response": {
                "status": if success { "ok" } else { "error" },
                "status_code": run_status,
                "duration_ms": exec.call_duration_ms,
                "error": error.clone(),
                "assertions_passed": assertions.iter().filter(|a| a.passed).count(),
                "assertions_total": assertions.len(),
                "messages": response_messages.clone(),
                "headers": super::project::redact_secrets(&headers),
                "trailers": super::project::redact_secrets(&trailers),
            },
        });

        if let Ok(line) = serde_json::to_string(&entry) {
            let _guard = state.history_lock.lock().await;
            let root = root.to_path_buf();
            tokio::task::spawn_blocking(move || {
                super::project::append_history_entry(&root, &sid, &line).ok();
            })
            .await
            .ok();
        }
    }

    Ok(Json(RunTestResponse {
        success,
        error,
        grpc_status: run_status,
        call_duration_ms: exec.call_duration_ms,
        assertions,
        documents: exec.document_durations_ms.clone(),
        response_messages,
        headers,
        trailers,
        address: dialled,
        row: row_index,
        rows_total,
        extracted: exec.extracted.clone(),
    }))
}

#[derive(Deserialize)]
pub struct ShareRequest {
    pub endpoint: String,
    pub headers: Option<std::collections::HashMap<String, String>>,
    pub bodies: Vec<String>,
    pub address: Option<String>,
    pub protocol: Option<String>,
    pub tls: Option<bool>,
    pub tls_insecure: Option<bool>,
    pub ttl_days: Option<u64>,
    #[serde(default)]
    pub include_secrets: bool,
    #[serde(default)]
    pub omitted: Vec<String>,
}

#[derive(Serialize)]
pub struct ShareResponse {
    pub id: String,
    pub url: String,
    pub expires_at: i64,
}

pub async fn create_share(
    State(state): State<Arc<PlayState>>,
    Json(req): Json<ShareRequest>,
) -> Result<Json<ShareResponse>, (StatusCode, String)> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = apif_cfg_runtime::now_unix_millis() as i64;
    let ttl_days = req.ttl_days.unwrap_or(7).min(30);
    let expires_at = now + (ttl_days as i64) * 24 * 60 * 60 * 1000;

    let headers = req.headers.unwrap_or_default();
    let mut redacted: Vec<String> = if req.include_secrets {
        Vec::new()
    } else {
        headers
            .keys()
            .filter(|k| super::project::is_secret_header(k))
            .cloned()
            .collect()
    };
    redacted.extend(req.omitted.iter().cloned());
    redacted.sort();
    redacted.dedup();
    let shared_headers: std::collections::HashMap<String, String> = headers
        .into_iter()
        .filter(|(k, _)| req.include_secrets || !super::project::is_secret_header(k))
        .collect();

    let share = ShareState {
        id: id.clone(),
        endpoint: req.endpoint,
        headers: shared_headers,
        bodies: req.bodies,
        address: req.address,
        protocol: req.protocol,
        tls: req.tls,
        tls_insecure: req.tls_insecure,
        created_at: now,
        expires_at,
        access_count: 0,
        redacted,
    };

    let json = serde_json::to_string(&share)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let shares_dir = state.shares_dir.clone();
    let id2 = id.clone();
    tokio::task::spawn_blocking(move || {
        let written = super::project::write_share(&shares_dir, &id2, &json);
        let _ = super::project::cleanup_expired_shares(&shares_dir);
        written
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(ShareResponse {
        id: id.clone(),
        url: format!("/s/{}", id),
        expires_at,
    }))
}

pub async fn get_share(
    State(state): State<Arc<PlayState>>,
    Path(id): Path<String>,
) -> Result<Json<ShareState>, (StatusCode, String)> {
    let shares_dir = state.shares_dir.clone();
    let id2 = id.clone();
    let json = tokio::task::spawn_blocking(move || super::project::read_share(&shares_dir, &id2))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Share not found".to_string()))?;

    let mut share: ShareState = serde_json::from_str(&json).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Corrupt share".to_string(),
        )
    })?;

    let now = apif_cfg_runtime::now_unix_millis() as i64;
    if share.expires_at < now {
        let shares_dir = state.shares_dir.clone();
        let id2 = id.clone();
        tokio::task::spawn_blocking(move || {
            super::project::delete_share(&shares_dir, &id2).ok();
        })
        .await
        .ok();
        return Err((StatusCode::GONE, "Share has expired".to_string()));
    }

    share.access_count += 1;
    if let Ok(json) = serde_json::to_string(&share) {
        let shares_dir = state.shares_dir.clone();
        let id2 = id.clone();
        tokio::task::spawn_blocking(move || {
            let _ = super::project::write_share(&shares_dir, &id2, &json);
        })
        .await
        .ok();
    }

    Ok(Json(share))
}

fn require_project(state: &PlayState) -> Result<std::path::PathBuf, (StatusCode, String)> {
    state
        .project_root
        .clone()
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Not in project mode".into()))
}

#[derive(Serialize)]
pub struct ProjectInfo {
    pub active: bool,
    pub envs: Vec<String>,
    pub collections_dir: String,
    pub project_dir: Option<String>,
    pub project_dir_abs: Option<String>,
}

pub(super) fn shown_path(base: &std::path::Path, path: &std::path::Path) -> String {
    match path.strip_prefix(base) {
        Ok(rel) if !rel.as_os_str().is_empty() => rel.to_string_lossy().replace('\\', "/"),
        _ => path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| ".".to_string()),
    }
}

pub fn project_info_inner(state: &PlayState) -> ProjectInfo {
    let base = reports_base(state);
    let root = match state.project_root.as_ref() {
        Some(r) => r.clone(),
        None => {
            return ProjectInfo {
                active: false,
                envs: vec![],
                collections_dir: shown_path(base, &state.collections_dir),
                project_dir: None,
                project_dir_abs: None,
            };
        }
    };
    let envs = root
        .is_dir()
        .then(|| super::project::list_env_files(&root).ok())
        .flatten()
        .unwrap_or_default();
    ProjectInfo {
        active: true,
        envs,
        collections_dir: shown_path(base, &state.collections_dir),
        project_dir: Some(shown_path(base, &root)),
        project_dir_abs: std::fs::canonicalize(&root)
            .ok()
            .map(|p| p.display().to_string()),
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

#[derive(Serialize)]
pub struct EnvFile {
    pub content: String,
    pub secret: Vec<String>,
}

pub async fn project_env_get(
    State(state): State<Arc<PlayState>>,
    Path(name): Path<String>,
) -> Result<Json<EnvFile>, (StatusCode, String)> {
    let root = require_project(&state)?;
    validate_env_name(&name)?;
    let content = super::project::read_dotenv(&root, &name)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                format!("Environment '{}' not found", name),
            )
        })?;
    let secret = secret_names(&super::project::parse_dotenv(&content));
    Ok(Json(EnvFile { content, secret }))
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
    validate_env_name(&name)?;
    super::project::write_dotenv(&root, &name, &body.content)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(()))
}

#[derive(Serialize)]
pub struct EnvLocalStatus {
    pub exists: bool,
    pub content: Option<String>,
    pub secret: Vec<String>,
}

#[derive(Serialize)]
pub struct EnvMergedResponse {
    pub variables: std::collections::HashMap<String, String>,
    pub has_local: bool,
    pub address: Option<String>,
    pub secret: Vec<String>,
}

const SECRET_VAR_MARKS: &[&str] = &["TOKEN", "SECRET", "KEY", "PASSWORD", "PASSWD"];

pub(super) fn is_secret_var(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    SECRET_VAR_MARKS.iter().any(|mark| upper.contains(mark))
}

fn secret_names(variables: &std::collections::HashMap<String, String>) -> Vec<String> {
    let mut named: Vec<String> = variables
        .keys()
        .filter(|k| is_secret_var(k))
        .cloned()
        .collect();
    named.sort();
    named
}

pub async fn project_env_merged(
    State(state): State<Arc<PlayState>>,
    Path(name): Path<String>,
) -> Result<Json<EnvMergedResponse>, (StatusCode, String)> {
    let root = require_project(&state)?;
    validate_env_name(&name)?;
    let shared_raw = super::project::read_dotenv(&root, &name)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .unwrap_or_default();
    let local_raw = super::project::read_dotenv_local(&root, &name)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .unwrap_or_default();

    let shared = super::project::parse_dotenv(&shared_raw);
    let local = super::project::parse_dotenv(&local_raw);
    let mut variables = shared;
    for (k, v) in local {
        variables.insert(k, v);
    }

    let address = variables.remove("GRPC_ADDRESS");
    let secret = secret_names(&variables);
    Ok(Json(EnvMergedResponse {
        variables,
        has_local: !local_raw.is_empty(),
        address,
        secret,
    }))
}

pub async fn project_env_local_get(
    State(state): State<Arc<PlayState>>,
    Path(name): Path<String>,
) -> Result<Json<EnvLocalStatus>, (StatusCode, String)> {
    let root = require_project(&state)?;
    validate_env_name(&name)?;
    let content = super::project::read_dotenv_local(&root, &name)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let secret = secret_names(&super::project::parse_dotenv(
        content.as_deref().unwrap_or_default(),
    ));
    Ok(Json(EnvLocalStatus {
        exists: content.is_some(),
        content,
        secret,
    }))
}

pub async fn project_env_local_put(
    State(state): State<Arc<PlayState>>,
    Path(name): Path<String>,
    Json(body): Json<EnvPutBody>,
) -> Result<Json<()>, (StatusCode, String)> {
    let root = require_project(&state)?;
    validate_env_name(&name)?;
    super::project::write_dotenv_local(&root, &name, &body.content)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(()))
}

pub async fn project_env_local_delete(
    State(state): State<Arc<PlayState>>,
    Path(name): Path<String>,
) -> Result<Json<()>, (StatusCode, String)> {
    let root = require_project(&state)?;
    validate_env_name(&name)?;
    super::project::delete_dotenv_local(&root, &name)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(()))
}

#[derive(Serialize)]
pub struct VariableUse {
    pub name: String,
    pub files: Vec<String>,
    pub count: usize,
}

pub async fn list_variables(
    State(state): State<Arc<PlayState>>,
) -> Result<Json<Vec<VariableUse>>, (StatusCode, String)> {
    const FILES_PER_NAME: usize = 5;
    let dirs = state.collections_dirs.clone();

    let uses = tokio::task::spawn_blocking(move || {
        let mut by_name: std::collections::BTreeMap<String, (Vec<String>, usize)> =
            std::collections::BTreeMap::new();

        for dir in &dirs {
            if !dir.is_dir() {
                continue;
            }
            for file in crate::utils::FileUtils::collect_test_files(dir, &[]) {
                let rel = file
                    .strip_prefix(dir)
                    .unwrap_or(&file)
                    .to_string_lossy()
                    .to_string();
                let doc = crate::parser::parse_with_recovery(&file).document;
                let mut produced: Vec<String> = Vec::new();
                for d in doc.iter_chain() {
                    let parsed = parse_collection(d);
                    for name in placeholder_names(&parsed) {
                        if name.is_empty()
                            || produced.contains(&name)
                            || name.starts_with("dataset.")
                        {
                            continue;
                        }
                        let entry = by_name.entry(name).or_insert_with(|| (Vec::new(), 0));
                        if !entry.0.contains(&rel) {
                            entry.1 += 1;
                            if entry.0.len() < FILES_PER_NAME {
                                entry.0.push(rel.clone());
                            }
                        }
                    }
                    produced.extend(parsed.extracts.keys().cloned());
                }
            }
        }

        by_name
            .into_iter()
            .map(|(name, (files, count))| VariableUse { name, files, count })
            .collect::<Vec<_>>()
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(uses))
}

pub async fn project_env_delete(
    State(state): State<Arc<PlayState>>,
    Path(name): Path<String>,
) -> Result<Json<()>, (StatusCode, String)> {
    let root = require_project(&state)?;
    validate_env_name(&name)?;
    super::project::delete_dotenv(&root, &name)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(()))
}

pub async fn project_history_get(
    State(state): State<Arc<PlayState>>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let root = require_project(&state)?;
    let sessions = super::project::list_history_sessions(&root)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    const MAX_ENTRIES: usize = 2000;
    let mut map = serde_json::Map::new();
    let mut kept = 0usize;
    for sid in &sessions {
        if kept >= MAX_ENTRIES {
            break;
        }
        if let Ok(lines) = super::project::read_history_session(&root, sid) {
            let room = MAX_ENTRIES - kept;
            let mut entries: Vec<serde_json::Value> = lines
                .iter()
                .rev()
                .take(room)
                .filter_map(|l| serde_json::from_str(l).ok())
                .collect();
            entries.reverse();
            if !entries.is_empty() {
                kept += entries.len();
                map.insert(sid.clone(), serde_json::Value::Array(entries));
            }
        }
    }
    Ok(Json(serde_json::Value::Object(map)))
}

pub async fn delete_collection(
    State(state): State<Arc<PlayState>>,
    Path(path): Path<String>,
) -> Result<Json<()>, (StatusCode, String)> {
    reject_traversal(&path)?;
    let file_path = resolve_file(&state, &path)
        .ok_or_else(|| (StatusCode::NOT_FOUND, "File not found".to_string()))?;
    require_collection_item(&path, &file_path)?;
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

pub async fn create_directory(
    State(state): State<Arc<PlayState>>,
    Path(path): Path<String>,
) -> Result<Json<()>, (StatusCode, String)> {
    let dir_path = resolve_write_path(primary_dir(&state), &path)?;
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

const PATH_KEYS: &[(&str, &[&str])] = &[
    ("PROTO", &["files", "import_paths", "descriptor"]),
    (
        "TLS",
        &[
            "ca_cert",
            "ca_file",
            "client_cert",
            "cert",
            "cert_file",
            "client_key",
            "key",
            "key_file",
        ],
    ),
];

fn spell_from(from_dir: &[&str], to: &[&str]) -> String {
    let mut shared = 0;
    while shared < from_dir.len() && shared + 1 < to.len() && from_dir[shared] == to[shared] {
        shared += 1;
    }
    let mut parts: Vec<String> = vec!["..".to_string(); from_dir.len() - shared];
    parts.extend(to[shared..].iter().map(|p| (*p).to_string()));
    parts.join("/")
}

fn respell(old_dir: &str, new_dir: &str, value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || trimmed.starts_with('/')
        || trimmed.starts_with("{{")
        || trimmed.contains("://")
    {
        return value.to_string();
    }
    let mut target: Vec<String> = old_dir
        .split('/')
        .filter(|p| !p.is_empty())
        .map(str::to_string)
        .collect();
    for piece in trimmed.split('/') {
        match piece {
            "" | "." => {}
            ".." => {
                target.pop();
            }
            other => target.push(other.to_string()),
        }
    }
    let from: Vec<&str> = new_dir.split('/').filter(|p| !p.is_empty()).collect();
    let to: Vec<&str> = target.iter().map(String::as_str).collect();
    spell_from(&from, &to)
}

pub fn respell_paths(content: &str, old_dir: &str, new_dir: &str) -> (String, Vec<String>) {
    if old_dir == new_dir {
        return (content.to_string(), Vec::new());
    }
    let mut out = String::with_capacity(content.len());
    let mut section: Option<&'static str> = None;
    let mut changed: Vec<String> = Vec::new();
    for line in content.split_inclusive('\n') {
        let body = line.trim_end_matches(['\n', '\r']);
        let trimmed = body.trim();
        if trimmed.starts_with("---") {
            let name = trimmed.trim_matches('-').trim().to_uppercase();
            section = PATH_KEYS
                .iter()
                .find(|(s, _)| name == *s || name.starts_with(&format!("{s} ")))
                .map(|(s, _)| *s);
            out.push_str(line);
            continue;
        }
        let Some(current) = section else {
            out.push_str(line);
            continue;
        };
        let keys = PATH_KEYS
            .iter()
            .find(|(s, _)| *s == current)
            .map(|(_, k)| *k)
            .unwrap_or(&[]);
        let Some((key, value)) = body.split_once(':') else {
            out.push_str(line);
            continue;
        };
        if !keys.contains(&key.trim()) {
            out.push_str(line);
            continue;
        }
        let respelled: Vec<String> = value
            .split(',')
            .map(|piece| {
                let core = piece.trim();
                if core.is_empty() {
                    return piece.to_string();
                }
                let spelled = respell(old_dir, new_dir, core);
                if spelled == core {
                    return piece.to_string();
                }
                changed.push(format!("{}: {core} → {spelled}", key.trim()));
                piece.replacen(core, &spelled, 1)
            })
            .collect();
        out.push_str(key);
        out.push(':');
        out.push_str(&respelled.join(","));
        out.push_str(&line[body.len()..]);
    }
    (out, changed)
}

fn files_under(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                stack.push(path);
            } else {
                out.push(path);
            }
        }
    }
    out
}

pub fn rename_variable(content: &str, from: &str, to: &str) -> (String, usize) {
    let mut out = String::with_capacity(content.len());
    let mut section = String::new();
    let mut touched = 0usize;
    for line in content.split_inclusive('\n') {
        let body = line.trim_end_matches(['\n', '\r']);
        let trimmed = body.trim();
        if trimmed.starts_with("---") {
            section = trimmed
                .trim_matches('-')
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_uppercase();
            out.push_str(line);
            continue;
        }
        let rewritten = match section.as_str() {
            "EXTRACT" => match body.split_once('=') {
                Some((name, rest)) if name.trim() == from => {
                    touched += 1;
                    format!("{}{to}{}={rest}", leading(name), trailing(name))
                }
                _ => body.to_string(),
            },
            "ASSERTS" => {
                let (line, n) = rename_dollar(body, from, to);
                touched += n;
                line
            }
            _ => {
                let (line, n) = rename_braces(body, from, to);
                touched += n;
                line
            }
        };
        out.push_str(&rewritten);
        out.push_str(&line[body.len()..]);
    }
    (out, touched)
}

fn leading(text: &str) -> &str {
    &text[..text.len() - text.trim_start().len()]
}

fn trailing(text: &str) -> &str {
    &text[text.trim_end().len()..]
}

fn rename_braces(line: &str, from: &str, to: &str) -> (String, usize) {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    let mut count = 0;
    while let Some(open) = rest.find("{{") {
        let Some(close) = rest[open..].find("}}").map(|at| open + at + 2) else {
            break;
        };
        out.push_str(&rest[..open]);
        let inner = &rest[open + 2..close - 2];
        if inner.trim() == from {
            out.push_str(&format!("{{{{{to}}}}}"));
            count += 1;
        } else {
            out.push_str(&rest[open..close]);
        }
        rest = &rest[close..];
    }
    out.push_str(rest);
    (out, count)
}

fn rename_dollar(line: &str, from: &str, to: &str) -> (String, usize) {
    let needle = format!("${from}");
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    let mut count = 0;
    while let Some(at) = rest.find(&needle) {
        let after = &rest[at + needle.len()..];
        let ends = after
            .chars()
            .next()
            .is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '_' || c == '.'));
        out.push_str(&rest[..at]);
        if ends {
            out.push_str(&format!("${to}"));
            count += 1;
        } else {
            out.push_str(&needle);
        }
        rest = &rest[at + needle.len()..];
    }
    out.push_str(rest);
    (out, count)
}

pub fn rename_dataset_column(content: &str, from: &str, to: &str) -> (String, usize) {
    let mut out = String::with_capacity(content.len());
    let mut section = String::new();
    let mut touched = 0usize;
    for line in content.split_inclusive('\n') {
        let body = line.trim_end_matches(['\n', '\r']);
        let trimmed = body.trim();
        if trimmed.starts_with("---") {
            section = trimmed
                .trim_matches('-')
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_uppercase();
            out.push_str(line);
            continue;
        }
        let rewritten = match section.as_str() {
            "DATASET" => match rename_yaml_key(body, from, to) {
                Some(renamed) => {
                    touched += 1;
                    renamed
                }
                None => body.to_string(),
            },
            "ASSERTS" | "EXTRACT" => body.to_string(),
            _ => {
                let (renamed, n) =
                    rename_braces(body, &format!("dataset.{from}"), &format!("dataset.{to}"));
                touched += n;
                renamed
            }
        };
        out.push_str(&rewritten);
        out.push_str(&line[body.len()..]);
    }
    (out, touched)
}

fn rename_yaml_key(line: &str, from: &str, to: &str) -> Option<String> {
    let (indent, rest) = line.split_at(line.len() - line.trim_start().len());
    let (dash, rest) = match rest.strip_prefix("- ") {
        Some(after) => ("- ", after),
        None => ("", rest),
    };
    let (key, tail) = rest.split_once(':')?;
    let bare = key.trim();
    let quoted = bare.starts_with('"') && bare.ends_with('"') && bare.len() >= 2;
    let name = if quoted {
        &bare[1..bare.len() - 1]
    } else {
        bare
    };
    if name != from {
        return None;
    }
    let written = if quoted {
        format!("\"{to}\"")
    } else {
        to.to_string()
    };
    Some(format!("{indent}{dash}{written}:{tail}"))
}

#[derive(Deserialize)]
pub struct RenameVariableRequest {
    pub path: String,
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub dataset: bool,
}

#[derive(Serialize)]
pub struct RenameVariableResponse {
    pub rewritten: usize,
}

pub async fn rename_variable_endpoint(
    State(state): State<Arc<PlayState>>,
    Json(req): Json<RenameVariableRequest>,
) -> Result<Json<RenameVariableResponse>, (StatusCode, String)> {
    reject_traversal(&req.path)?;
    require_gctf(&req.path)?;
    let to = req.to.trim();
    if to.is_empty() || !is_variable_name(to) {
        return Err((
            StatusCode::BAD_REQUEST,
            "A name is a letter or _ followed by letters, digits, _ or .".to_string(),
        ));
    }
    let file_path = resolve_file(&state, &req.path)
        .ok_or_else(|| (StatusCode::NOT_FOUND, "File not found".to_string()))?;
    let text = std::fs::read_to_string(&file_path)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let (renamed, touched) = if req.dataset {
        rename_dataset_column(&text, req.from.trim(), to)
    } else {
        rename_variable(&text, req.from.trim(), to)
    };
    if touched > 0 {
        std::fs::write(&file_path, &renamed)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        state
            .collections_mtime
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    Ok(Json(RenameVariableResponse { rewritten: touched }))
}

fn is_variable_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
}

pub fn references_to(dirs: &[std::path::PathBuf], target: &str) -> Vec<String> {
    let wanted = target.trim_matches('/');
    let inside = folder_contents(dirs, wanted);
    let targets: Vec<&str> = if inside.is_empty() {
        vec![wanted]
    } else {
        inside.iter().map(String::as_str).collect()
    };
    let within = |rel: &str| !inside.is_empty() && rel.starts_with(&format!("{wanted}/"));

    let mut out: Vec<String> = Vec::new();
    for dir in dirs {
        if !dir.is_dir() {
            continue;
        }
        for file in crate::utils::FileUtils::collect_test_files(dir, &[]) {
            let Ok(rel) = file.strip_prefix(dir) else {
                continue;
            };
            let rel = rel.to_string_lossy().replace('\\', "/");
            if within(&rel) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&file) else {
                continue;
            };
            if targets
                .iter()
                .any(|t| names_path(&text, &parent_of(&rel), t))
                && !out.contains(&rel)
            {
                out.push(rel);
            }
        }
    }
    out.sort();
    out
}

fn folder_contents(dirs: &[std::path::PathBuf], target: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for dir in dirs {
        let candidate = dir.join(target);
        if !candidate.is_dir() {
            continue;
        }
        for file in files_under(&candidate) {
            let Ok(rel) = file.strip_prefix(dir) else {
                continue;
            };
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
    out
}

fn names_path(content: &str, dir: &str, target: &str) -> bool {
    let mut section = String::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("---") {
            section = trimmed
                .trim_matches('-')
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_uppercase();
            continue;
        }
        let keys = PATH_KEYS
            .iter()
            .find(|(name, _)| *name == section)
            .map(|(_, keys)| *keys)
            .unwrap_or(&[]);
        if keys.is_empty() {
            continue;
        }
        let Some((key, value)) = trimmed.split_once(':') else {
            continue;
        };
        if !keys.contains(&key.trim()) {
            continue;
        }
        for piece in value.split(',') {
            let named = piece.trim();
            if named.is_empty() || named.starts_with('/') || named.starts_with("{{") {
                continue;
            }
            let mut parts: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
            for step in named.split('/') {
                match step {
                    "" | "." => {}
                    ".." => {
                        parts.pop();
                    }
                    other => parts.push(other),
                }
            }
            if parts.join("/") == target {
                return true;
            }
        }
    }
    false
}

pub async fn list_references(
    State(state): State<Arc<PlayState>>,
    Path(path): Path<String>,
) -> Result<Json<Vec<String>>, (StatusCode, String)> {
    reject_traversal(&path)?;
    let dirs = state.collections_dirs.clone();
    let found = tokio::task::spawn_blocking(move || references_to(&dirs, &path))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(found))
}

pub async fn move_item(
    State(state): State<Arc<PlayState>>,
    Json(req): Json<MoveRequest>,
) -> Result<Json<MoveResponse>, (StatusCode, String)> {
    reject_traversal(&req.from)?;
    reject_traversal(&req.to)?;
    let src =
        resolve_file(&state, &req.from).unwrap_or_else(|| primary_dir(&state).join(&req.from));
    require_collection_item(&req.from, &src).map_err(|(status, why)| match status {
        StatusCode::NOT_FOUND => (status, format!("Source not found: {}", req.from)),
        _ => (status, why),
    })?;
    let dst = resolve_write_path(primary_dir(&state), &req.to)?;

    if dst.exists() {
        return Err((
            StatusCode::CONFLICT,
            format!("Destination already exists: {}", req.to),
        ));
    }

    std::fs::rename(&src, &dst).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to move: {}", e),
        )
    })?;

    let moved = match std::fs::symlink_metadata(&dst) {
        Ok(moved) => moved,
        Err(e) => {
            return Err(undo_move(
                &dst,
                &src,
                &req.from,
                format!("`{}` cannot be read back after the move: {e}", req.to),
            ));
        }
    };
    if moved.file_type().is_symlink() {
        return Err(undo_move(
            &dst,
            &src,
            &req.from,
            format!("`{}` became a link while it was being moved", req.to),
        ));
    }

    let mut rewritten: Vec<String> = Vec::new();
    if moved.is_file() {
        if require_gctf(&req.to).is_ok()
            && let Ok(text) = std::fs::read_to_string(&dst)
        {
            let (respelled, changed) =
                respell_paths(&text, &parent_of(&req.from), &parent_of(&req.to));
            if !changed.is_empty() && std::fs::write(&dst, &respelled).is_ok() {
                rewritten = changed;
            }
        }
    } else {
        for file in files_under(&dst) {
            let Ok(inside) = file.strip_prefix(&dst) else {
                continue;
            };
            let inside = inside.to_string_lossy().replace('\\', "/");
            let name = format!("{}/{inside}", req.to.trim_end_matches('/'));
            if require_gctf(&name).is_err() {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&file) else {
                continue;
            };
            let was = format!("{}/{inside}", req.from.trim_end_matches('/'));
            let (respelled, changed) = respell_paths(&text, &parent_of(&was), &parent_of(&name));
            if !changed.is_empty() && std::fs::write(&file, &respelled).is_ok() {
                rewritten.extend(changed.into_iter().map(|c| format!("{inside} — {c}")));
            }
        }
    }

    state
        .collections_mtime
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    Ok(Json(MoveResponse { rewritten }))
}

fn undo_move(
    dst: &std::path::Path,
    src: &std::path::Path,
    from: &str,
    why: String,
) -> (StatusCode, String) {
    let restored = std::fs::rename(dst, src).is_ok();
    (
        StatusCode::CONFLICT,
        if restored {
            format!("{why} — `{from}` was put back where it was")
        } else {
            format!("{why} — and `{from}` could not be put back")
        },
    )
}

fn parent_of(path: &str) -> String {
    match path.rfind('/') {
        Some(at) => path[..at].to_string(),
        None => String::new(),
    }
}

#[derive(Serialize, Debug)]
pub struct MoveResponse {
    pub rewritten: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg_attr(miri, ignore)]
    fn a_folder_is_asked_about_as_a_whole() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::create_dir_all(root.join("shared")).expect("mkdir");
        std::fs::write(root.join("shared/schema.bin"), b"x").expect("write");
        std::fs::write(
            root.join("outside.gctf"),
            "--- ENDPOINT ---\na.B/C\n\n--- PROTO ---\ndescriptor: shared/schema.bin\n\n--- REQUEST ---\n{}\n",
        )
        .expect("write");
        std::fs::write(
            root.join("shared/inside.gctf"),
            "--- ENDPOINT ---\na.B/C\n\n--- PROTO ---\ndescriptor: schema.bin\n\n--- REQUEST ---\n{}\n",
        )
        .expect("write");

        let dirs = vec![root.to_path_buf()];
        assert_eq!(references_to(&dirs, "shared"), vec!["outside.gctf"]);
        assert_eq!(
            references_to(&dirs, "shared/schema.bin"),
            vec!["outside.gctf", "shared/inside.gctf"]
        );
        assert!(references_to(&dirs, "nothing").is_empty());
    }

    #[test]
    fn the_probe_opens_what_the_call_would() {
        assert_eq!(
            probe_target("localhost:50051").as_deref(),
            Some("localhost:50051")
        );
        assert_eq!(
            probe_target("  api.internal:443 ").as_deref(),
            Some("api.internal:443")
        );
        assert_eq!(
            probe_target("http://example.com").as_deref(),
            Some("example.com:80")
        );
        assert_eq!(
            probe_target("https://example.com/v1/users").as_deref(),
            Some("example.com:443")
        );
        assert_eq!(
            probe_target("http://example.com:8080/x").as_deref(),
            Some("example.com:8080")
        );
    }

    #[test]
    fn an_address_with_no_port_has_nothing_to_open() {
        assert!(probe_target("").is_none());
        assert!(probe_target("   ").is_none());
        assert!(probe_target("localhost").is_none());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn the_rail_finds_a_file_by_the_tag_the_runner_selects_it_with() {
        let dir = tempfile::tempdir().expect("tempdir");
        let attributed = dir.path().join("attr.gctf");
        std::fs::write(
            &attributed,
            "--- ENDPOINT ---\na.B/C\n\n#[tag(smoke, slow)]\n--- REQUEST ---\n{}\n",
        )
        .expect("write");
        assert_eq!(extract_tags(&attributed), vec!["smoke", "slow"]);

        let both = dir.path().join("meta.gctf");
        std::fs::write(
            &both,
            "--- META ---\ntags:\n  - api\n\n--- ENDPOINT ---\na.B/C\n\n#[tag(smoke)]\n--- REQUEST ---\n{}\n",
        )
        .expect("write");
        assert_eq!(extract_tags(&both), vec!["api"]);
    }

    #[tokio::test]
    #[cfg_attr(miri, ignore)]
    async fn the_panels_run_refuses_what_the_command_line_refuses() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::write(
            root.join("tagged.gctf"),
            "--- META ---\ntags: smoke, auth\n\n--- ENDPOINT ---\na.B/C\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n.ok\n",
        )
        .expect("write");
        let state = Arc::new(PlayState {
            collections_dir: root.to_path_buf(),
            collections_dirs: vec![root.to_path_buf()],
            shares_dir: root.join("shares"),
            project_root: None,
            project_settings: None,
            history_lock: tokio::sync::Mutex::new(()),
            write_lock: tokio::sync::Mutex::new(()),
            collections_mtime: Arc::new(AtomicU64::new(0)),
            jobs: Default::default(),
        });

        let answered = execute_test(
            axum::extract::State(state),
            Json(RunTestRequest {
                collection_path: "tagged.gctf".to_string(),
                session_id: None,
                timeout_seconds: None,
                data: None,
            }),
        )
        .await
        .expect("answered")
        .0;

        assert!(!answered.success);
        let said = answered.error.unwrap_or_default();
        assert!(said.starts_with("Parse error:"), "{said}");
        assert!(said.contains("tags"), "{said}");
    }

    #[tokio::test]
    #[cfg_attr(miri, ignore)]
    async fn versions_answer_for_the_files_that_are_asked_about() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::create_dir_all(root.join("auth")).expect("dir");
        std::fs::write(root.join("auth/login.gctf"), "--- ENDPOINT ---\na.B/C\n").expect("write");
        let state = Arc::new(PlayState {
            collections_dir: root.to_path_buf(),
            collections_dirs: vec![root.to_path_buf()],
            shares_dir: root.join("shares"),
            project_root: None,
            project_settings: None,
            history_lock: tokio::sync::Mutex::new(()),
            write_lock: tokio::sync::Mutex::new(()),
            collections_mtime: Arc::new(AtomicU64::new(0)),
            jobs: Default::default(),
        });

        let answered = file_versions(
            axum::extract::State(state.clone()),
            Json(VersionsRequest {
                paths: vec![
                    "auth/login.gctf".to_string(),
                    "auth/gone.gctf".to_string(),
                    "../outside.gctf".to_string(),
                ],
            }),
        )
        .await
        .0;

        let held = answered
            .get("auth/login.gctf")
            .expect("the file that is there")
            .clone()
            .expect("a version for it");
        assert!(held.hash.starts_with("sha256:"), "{}", held.hash);
        assert!(
            answered
                .get("auth/gone.gctf")
                .expect("asked about")
                .is_none(),
            "a file that is not there has no version"
        );
        assert!(
            !answered.contains_key("../outside.gctf"),
            "a path that climbs out is not answered at all"
        );

        std::fs::write(root.join("auth/login.gctf"), "--- ENDPOINT ---\na.B/D\n").expect("write");
        let again = file_versions(
            axum::extract::State(state),
            Json(VersionsRequest {
                paths: vec!["auth/login.gctf".to_string()],
            }),
        )
        .await
        .0;
        let now = again.get("auth/login.gctf").unwrap().clone().unwrap();
        assert_ne!(now.hash, held.hash, "the change is in the hash");
    }

    use std::path::PathBuf;
    use std::sync::atomic::AtomicU64;

    #[cfg_attr(miri, ignore)]
    #[cfg(unix)]
    #[test]
    fn write_path_refuses_symlinks_and_traversal() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("collections");
        std::fs::create_dir_all(&root).unwrap();
        let outside = dir.path().join("victim.gctf");
        std::fs::write(&outside, "ORIGINAL").unwrap();
        std::os::unix::fs::symlink(&outside, root.join("link.gctf")).unwrap();

        assert!(resolve_write_path(&root, "link.gctf").is_err());
        assert!(resolve_write_path(&root, "../escape.gctf").is_err());
        assert!(resolve_write_path(&root, "/etc/passwd").is_err());
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "ORIGINAL");

        let ok = resolve_write_path(&root, "sub/ok.gctf").unwrap();
        assert!(ok.starts_with(&root));
    }

    #[test]
    fn reject_traversal_valid() {
        assert!(reject_traversal("foo.gctf").is_ok());
        assert!(reject_traversal("dir/foo.gctf").is_ok());
        assert!(reject_traversal("a/b/c.gctf").is_ok());
        assert!(reject_traversal("foo..gctf").is_ok());
        assert!(reject_traversal("dir/foo..bar.gctf").is_ok());
        assert!(reject_traversal("my..dir/foo.gctf").is_ok());
    }

    #[test]
    fn reject_traversal_invalid() {
        assert!(reject_traversal("../foo.gctf").is_err());
        assert!(reject_traversal("dir/../../foo.gctf").is_err());
        assert!(reject_traversal("dir/..").is_err());
        assert!(reject_traversal("/etc/passwd").is_err());
        assert!(reject_traversal("..\\foo.gctf").is_err());
        assert!(reject_traversal("dir\\..\\..\\foo.gctf").is_err());
        assert!(reject_traversal("\\server\\share").is_err());
        assert!(reject_traversal("C:\\Windows\\system32").is_err());
        assert!(reject_traversal("C:/Windows/system32").is_err());
    }

    #[test]
    fn an_http_endpoint_is_a_method_and_a_path() {
        assert_eq!(
            split_http_endpoint("post /v1/users"),
            Some(("POST".to_string(), "/v1/users".to_string()))
        );
        assert_eq!(
            split_http_endpoint("  PROPFIND   /dav/  "),
            Some(("PROPFIND".to_string(), "/dav/".to_string()))
        );
        assert_eq!(split_http_endpoint("/v1/users"), None);
        assert_eq!(split_http_endpoint("GET"), None);
        assert_eq!(split_http_endpoint(""), None);
    }

    #[test]
    fn an_endpoint_with_a_method_is_an_http_call_even_without_a_file() {
        assert!(split_http_endpoint("GET /v1/users").is_some());
        assert!(split_http_endpoint("users.UserService/GetUser").is_none());
        assert!(split_http_endpoint("a.B/C").is_none());
    }

    #[test]
    fn test_require_gctf() {
        assert!(require_gctf("foo.gctf").is_ok());
        assert!(require_gctf("dir/foo.GCTF").is_ok());
        assert!(require_gctf("foo.httf").is_ok());
        assert!(require_gctf("dir/foo.HTTF").is_ok());
        assert!(require_gctf("foo.proto").is_err());
        assert!(require_gctf("foo.sh").is_err());
        assert!(require_gctf("gctf").is_err());
        assert!(require_gctf("httf").is_err());
    }

    #[test]
    fn parse_collection_preserves_source_order_of_options_and_extracts() {
        let src = "--- ENDPOINT ---\npkg.Svc/M\n\n\
             --- OPTIONS ---\ntimeout: 5\nretry: 2\ncompression: gzip\nno_retry: false\n\n\
             --- REQUEST ---\n{}\n\n\
             --- RESPONSE ---\n{}\n\n\
             --- EXTRACT ---\nzulu = .a\nmike = .b\nalpha = .c\n";
        let doc = crate::parser::parse_gctf_from_str(src, "order.gctf").expect("parse");
        let parsed = parse_collection(&doc);
        assert_eq!(
            parsed
                .options
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["timeout", "retry", "compression", "no_retry"],
        );
        assert_eq!(
            parsed
                .extracts
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["zulu", "mike", "alpha"],
        );
    }

    #[test]
    fn test_validate_env_name() {
        assert!(validate_env_name("staging").is_ok());
        assert!(validate_env_name("prod-eu.1").is_ok());
        assert!(validate_env_name("").is_err());
        assert!(validate_env_name("../secrets").is_err());
        assert!(validate_env_name("..").is_err());
        assert!(validate_env_name("a/b").is_err());
        assert!(validate_env_name("a\\b").is_err());
        assert!(validate_env_name("C:x").is_err());
    }

    #[cfg(not(miri))]
    #[test]
    fn resolve_file_nonexistent() {
        let state = PlayState {
            collections_dir: PathBuf::from("/tmp/nonexistent_XXXX"),
            collections_dirs: vec![PathBuf::from("/tmp/nonexistent_XXXX")],
            shares_dir: PathBuf::from("/tmp/nonexistent_XXXX/shares"),
            project_root: None,
            project_settings: None,
            history_lock: tokio::sync::Mutex::new(()),
            write_lock: tokio::sync::Mutex::new(()),
            collections_mtime: Arc::new(AtomicU64::new(0)),
            jobs: Default::default(),
        };
        assert!(resolve_file(&state, "foo.gctf").is_none());
    }

    #[cfg(not(miri))]
    #[cfg_attr(miri, ignore)]
    #[test]
    fn get_collection_returns_404_for_directory() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("emptydir.gctf");
        std::fs::create_dir(&sub).unwrap();

        let state = Arc::new(PlayState {
            collections_dir: dir.path().to_path_buf(),
            collections_dirs: vec![dir.path().to_path_buf()],
            shares_dir: dir.path().join("shares"),
            project_root: None,
            project_settings: None,
            history_lock: tokio::sync::Mutex::new(()),
            write_lock: tokio::sync::Mutex::new(()),
            collections_mtime: Arc::new(AtomicU64::new(0)),
            jobs: Default::default(),
        });

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(get_collection(
            State(state),
            Path("emptydir.gctf".to_string()),
        ));
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::NOT_FOUND);
    }

    #[cfg(not(miri))]
    #[cfg_attr(miri, ignore)]
    #[test]
    fn get_collection_ok_for_gctf_file() {
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
            shares_dir: dir.path().join("shares"),
            project_root: None,
            project_settings: None,
            history_lock: tokio::sync::Mutex::new(()),
            write_lock: tokio::sync::Mutex::new(()),
            collections_mtime: Arc::new(AtomicU64::new(0)),
            jobs: Default::default(),
        });

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(get_collection(State(state), Path("test.gctf".to_string())));
        let resp = result.expect("get_collection must succeed");
        assert!(resp.path.contains("test.gctf"));
    }

    #[cfg(not(miri))]
    #[cfg_attr(miri, ignore)]
    #[test]
    fn only_a_missing_symbol_is_worth_re_dialling_for() {
        for message in [
            "gRPC error code=5 message=Service 'auth.v1.AuthService' not found",
            "Method 'List' not found",
            "unimplemented: unknown service pkg.Svc",
        ] {
            assert!(names_unknown_symbol(message), "{message}");
        }

        for message in [
            "connection refused",
            "deadline exceeded",
            "not found: /path/to/file.proto",
        ] {
            assert!(
                !names_unknown_symbol(message),
                "{message} is not a stale schema and must not clear one"
            );
        }
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn the_directory_a_run_writes_into_is_not_a_collection() {
        let dir = std::env::temp_dir().join(format!("gctf-generated-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("grpctestify-reports/j-1")).unwrap();
        std::fs::create_dir_all(dir.join("auth")).unwrap();
        std::fs::write(dir.join("auth/login.gctf"), "--- ENDPOINT ---\npkg.Svc/M\n").unwrap();

        let mut seen = std::collections::HashSet::new();
        let mut items = Vec::new();
        collect_empty_dirs(&dir, &dir, &mut seen, &mut items);

        let listed: Vec<_> = items.iter().map(|i| i.path.as_str()).collect();
        assert!(listed.contains(&"auth"), "a real folder is still listed");
        assert!(
            !listed.iter().any(|p| p.starts_with("grpctestify-reports")),
            "a run wrote that folder; it holds output, not tests"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn list_collections_includes_empty_dirs() {
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
            shares_dir: dir.path().join("shares"),
            project_root: None,
            project_settings: None,
            history_lock: tokio::sync::Mutex::new(()),
            write_lock: tokio::sync::Mutex::new(()),
            collections_mtime: Arc::new(AtomicU64::new(0)),
            jobs: Default::default(),
        });

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(list_collections(State(state)));
        let items = result.expect("list_collections must succeed");

        let dir_item = items.iter().find(|i| i.path == "emptydir");
        assert!(dir_item.is_some(), "empty dir must be listed");
        assert!(dir_item.unwrap().is_dir, "empty dir must have is_dir: true");

        let file_item = items.iter().find(|i| i.path == "test.gctf");
        assert!(file_item.is_some(), "gctf file must be listed");
        assert!(!file_item.unwrap().is_dir, "file must have is_dir: false");
    }

    #[cfg(not(miri))]
    #[cfg_attr(miri, ignore)]
    #[test]
    fn list_collections_empty_dir_with_gitkeep() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("projects");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join(".gitkeep"), "").unwrap();

        let state = Arc::new(PlayState {
            collections_dir: dir.path().to_path_buf(),
            collections_dirs: vec![dir.path().to_path_buf()],
            shares_dir: dir.path().join("shares"),
            project_root: None,
            project_settings: None,
            history_lock: tokio::sync::Mutex::new(()),
            write_lock: tokio::sync::Mutex::new(()),
            collections_mtime: Arc::new(AtomicU64::new(0)),
            jobs: Default::default(),
        });

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(list_collections(State(state)));
        let items = result.expect("list_collections must succeed");

        let dir_item = items.iter().find(|i| i.path == "projects");
        assert!(
            dir_item.is_some(),
            "projects dir must be listed even with .gitkeep"
        );
        assert!(dir_item.unwrap().is_dir, "projects must have is_dir: true");
    }
    #[cfg_attr(miri, ignore)]
    #[test]
    fn stale_version_is_a_conflict_not_a_clobber() {
        let dir = std::env::temp_dir().join(format!("gctf-ver-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("v.gctf");
        std::fs::write(&file, "--- ENDPOINT ---\npkg.Svc/M\n").unwrap();

        let read = std::fs::read_to_string(&file).unwrap();
        let held = file_version(&file, &read);

        assert!(check_version(&file, Some(&held)).is_ok());

        std::fs::write(&file, "--- ENDPOINT ---\npkg.Svc/Other\n").unwrap();
        let err = check_version(&file, Some(&held)).unwrap_err();
        assert_eq!(err.0, StatusCode::CONFLICT);
        let body: serde_json::Value = serde_json::from_str(&err.1).unwrap();
        assert!(body["content"].as_str().unwrap().contains("pkg.Svc/Other"));
        assert_ne!(body["version"]["hash"].as_str().unwrap(), held.hash);

        assert!(check_version(&file, None).is_ok());

        std::fs::remove_file(&file).unwrap();
        assert!(check_version(&file, Some(&held)).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn saving_a_chain_file_keeps_every_document() {
        let src = "--- ADDRESS ---\nlocalhost:4770\n\n             --- ENDPOINT ---\nauth.v1.AuthService/Login\n\n             --- REQUEST ---\n{}\n\n             --- EXTRACT ---\ntoken = .auth.token\n\n             --- ENDPOINT ---\nfeed.v1.FeedService/List\n\n             --- REQUEST ---\n{}\n\n             --- ASSERTS ---\n.items | length > 0\n";
        let doc = crate::parser::parse_gctf_from_str(src, "chain.gctf").expect("parse");
        assert_eq!(doc.iter_chain().count(), 2);

        let round_tripped = crate::parser::serialize_gctf(&doc);
        let reparsed =
            crate::parser::parse_gctf_from_str(&round_tripped, "chain.gctf").expect("reparse");
        let endpoints: Vec<String> = reparsed
            .iter_chain()
            .map(|d| d.get_endpoint().unwrap_or_default())
            .collect();
        assert_eq!(
            endpoints,
            vec!["auth.v1.AuthService/Login", "feed.v1.FeedService/List"]
        );
        assert!(round_tripped.contains("--- ASSERTS ---"));
        assert!(round_tripped.contains("--- EXTRACT ---"));
    }

    #[test]
    fn saving_an_edited_section_keeps_the_attributes_written_above_it() {
        let src = "--- ENDPOINT ---\npkg.Svc/M\n\n#[repeat(3)]\n--- REQUEST ---\n{\"a\": 1}\n\n#[skip]\n--- ASSERTS ---\n.ok == true\n";
        let original = crate::parser::parse_gctf_from_str(src, "m.gctf").expect("parse");

        let req = SaveRequestStructured {
            path: "m.gctf".to_string(),
            endpoint: "pkg.Svc/M".to_string(),
            bodies: Some(vec!["{\"a\": 2}".to_string()]),
            asserts: Some(vec![".ok == true".to_string()]),
            ..Default::default()
        };
        let edited = crate::parser::GctfDocumentBuilder::new()
            .endpoint("pkg.Svc/M")
            .request(serde_json::json!({"a": 2}))
            .asserts(vec![".ok == true".to_string()])
            .build();

        let out = crate::parser::serialize_gctf(&stitch_into_chain(&original, edited, 0, &req));
        assert!(out.contains("#[repeat(3)]"), "{out}");
        assert!(out.contains("#[skip]"), "{out}");
        assert!(
            out.contains("\"a\": 2"),
            "the edit is still the edit: {out}"
        );
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn reflection_gives_up_within_the_wait_it_was_given() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    let _ = tokio::io::AsyncReadExt::read(&mut socket, &mut buf).await;
                    std::future::pending::<()>().await;
                });
            }
        });

        let dir = tempfile::tempdir().unwrap();
        let state = Arc::new(PlayState {
            collections_dir: dir.path().to_path_buf(),
            collections_dirs: vec![dir.path().to_path_buf()],
            shares_dir: dir.path().join("shares"),
            project_root: None,
            project_settings: None,
            history_lock: tokio::sync::Mutex::new(()),
            write_lock: tokio::sync::Mutex::new(()),
            collections_mtime: Arc::new(AtomicU64::new(0)),
            jobs: Default::default(),
        });

        let started = std::time::Instant::now();
        let answer = reflect_server(
            State(state),
            Json(ReflectRequest {
                address: addr.to_string(),
                timeout_seconds: Some(1),
                ..Default::default()
            }),
        )
        .await;

        let waited = started.elapsed();
        assert!(
            waited < std::time::Duration::from_secs(3),
            "reflection given one second waited {waited:?}"
        );
        assert!(answer.0.services.is_empty());
        let error = answer.0.error.unwrap_or_default();
        assert!(error.contains("No schema within 1s"), "{error}");
    }

    #[test]
    fn a_timeout_says_the_same_thing_whichever_layer_noticed_it() {
        for reported in [
            "Reflection failed at 127.0.0.1:49209: Cancelled Timeout expired",
            "status: DeadlineExceeded, message: \"deadline exceeded\"",
            "operation timed out",
        ] {
            assert!(ran_out_of_time(reported), "{reported}");
        }
        assert!(!ran_out_of_time("Connection refused (os error 61)"));
        assert!(no_schema_within(1).contains("No schema within 1s"));
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn a_call_gives_up_within_the_wait_it_was_given() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    let _ = tokio::io::AsyncReadExt::read(&mut socket, &mut buf).await;
                    std::future::pending::<()>().await;
                });
            }
        });

        let dir = tempfile::tempdir().unwrap();
        let state = Arc::new(PlayState {
            collections_dir: dir.path().to_path_buf(),
            collections_dirs: vec![dir.path().to_path_buf()],
            shares_dir: dir.path().join("shares"),
            project_root: None,
            project_settings: None,
            history_lock: tokio::sync::Mutex::new(()),
            write_lock: tokio::sync::Mutex::new(()),
            collections_mtime: Arc::new(AtomicU64::new(0)),
            jobs: Default::default(),
        });

        let started = std::time::Instant::now();
        let answer = execute_call(
            State(state),
            Json(CallRequest {
                endpoint: "grpc.health.v1.Health/Check".to_string(),
                address: Some(addr.to_string()),
                bodies_raw: Some(vec!["{}".to_string()]),
                timeout_seconds: Some(1),
                ..Default::default()
            }),
        )
        .await
        .expect("the handler answers rather than erroring");

        let waited = started.elapsed();
        assert!(
            waited < std::time::Duration::from_secs(3),
            "a call set to wait one second waited {waited:?}"
        );
        let error = answer.0.error.unwrap_or_default();
        assert!(error.contains("No answer within 1s"), "{error}");
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn a_dataset_file_is_read_as_its_rows() {
        let dir = std::env::temp_dir().join(format!("play-rows-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("rows.gctf");
        std::fs::write(
            &path,
            "--- ENDPOINT ---\ns.S/M\n\n--- REQUEST ---\n{\"id\": \"{{dataset.id}}\"}\n\n--- DATASET ---\n- id: \"1\"\n- id: \"2\"\n",
        )
        .expect("write");

        let rows = super::super::jobs::dataset_rows(&path).expect("the file has a DATASET");
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[1].get("dataset.id"),
            Some(&serde_json::json!("2")),
            "the second case runs with the second row"
        );

        let plain = dir.join("plain.gctf");
        std::fs::write(&plain, "--- ENDPOINT ---\ns.S/M\n\n--- REQUEST ---\n{}\n").expect("write");
        assert!(
            super::super::jobs::dataset_rows(&plain).is_none(),
            "a file without rows is one case"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_section_says_when_the_file_writes_it_with_comments() {
        let src = "--- ENDPOINT ---\npkg.Svc/M\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n# why\n.ok == true\n\n--- EXTRACT ---\ntoken = .token\n";
        let parsed =
            parse_collection(&crate::parser::parse_gctf_from_str(src, "a.gctf").expect("parse"));
        assert_eq!(
            parsed
                .sections_as_written
                .get("ASSERTS")
                .map(String::as_str),
            Some("# why\n.ok == true")
        );
        assert!(
            parsed.sections_as_written.get("EXTRACT").is_none(),
            "a section that says what it shows says nothing twice"
        );
    }

    #[test]
    fn a_message_says_when_the_file_writes_it_differently() {
        let json5 =
            "--- ENDPOINT ---\npkg.Svc/M\n\n--- REQUEST ---\n{\n  // who\n  message: \"hi\",\n}\n";
        let parsed =
            parse_collection(&crate::parser::parse_gctf_from_str(json5, "a.gctf").expect("parse"));
        assert_eq!(parsed.bodies, vec!["{\n  \"message\": \"hi\"\n}"]);
        assert_eq!(
            parsed.bodies_as_written,
            vec!["{\n  // who\n  message: \"hi\",\n}"]
        );

        let plain = "--- ENDPOINT ---\npkg.Svc/M\n\n--- REQUEST ---\n{\n  \"message\": \"hi\"\n}\n";
        let parsed =
            parse_collection(&crate::parser::parse_gctf_from_str(plain, "a.gctf").expect("parse"));
        assert!(
            parsed.bodies_as_written.is_empty(),
            "a file that says what it shows says nothing twice: {:?}",
            parsed.bodies_as_written
        );
    }

    #[test]
    fn saving_keeps_the_file_in_the_order_its_author_wrote_it() {
        let src = "--- ENDPOINT ---\npkg.Svc/M\n\n--- REQUEST ---\n{\"a\": 1}\n\n--- RESPONSE ---\n{\"ok\": true}\n\n--- ASSERTS ---\n.ok == true\n";
        let original = crate::parser::parse_gctf_from_str(src, "m.gctf").expect("parse");

        let req = SaveRequestStructured {
            path: "m.gctf".to_string(),
            endpoint: "pkg.Svc/M".to_string(),
            bodies: Some(vec!["{\"a\": 2}".to_string()]),
            ..Default::default()
        };
        let edited = crate::parser::GctfDocumentBuilder::new()
            .endpoint("pkg.Svc/M")
            .request(serde_json::json!({"a": 2}))
            .build();

        let out = crate::parser::serialize_gctf_as_written(&stitch_into_chain(
            &original, edited, 0, &req,
        ));
        let order: Vec<&str> = out.lines().filter(|l| l.starts_with("--- ")).collect();
        assert_eq!(
            order,
            vec![
                "--- ENDPOINT ---",
                "--- REQUEST ---",
                "--- RESPONSE ---",
                "--- ASSERTS ---",
            ],
            "{out}"
        );
        assert!(
            out.contains("\"a\": 2"),
            "the edit is still the edit: {out}"
        );
    }

    #[test]
    fn preserved_meta_is_still_written_first() {
        let src = "--- META ---\nname: original\n\n             --- ENDPOINT ---\npkg.Svc/M\n\n             --- REQUEST ---\n{}\n\n             --- ASSERTS ---\n.ok == true\n";
        let doc = crate::parser::parse_gctf_from_str(src, "m.gctf").expect("parse");

        let mut rebuilt = crate::parser::GctfDocumentBuilder::new()
            .endpoint("pkg.Svc/M")
            .request(serde_json::json!({}))
            .build();
        for sec in &doc.sections {
            if matches!(
                sec.section_type,
                crate::parser::SectionType::Meta | crate::parser::SectionType::Asserts
            ) {
                rebuilt.sections.push(sec.clone());
            }
        }

        let out = crate::parser::serialize_gctf(&rebuilt);
        let first = out
            .lines()
            .find(|l| l.starts_with("--- "))
            .unwrap_or_default();
        assert_eq!(first, "--- META ---");
        assert!(crate::parser::parse_gctf_from_str(&out, "m.gctf").is_ok());
    }

    #[test]
    fn chain_summary_names_every_step_and_the_variables_between_them() {
        let src = "--- ADDRESS ---\nlocalhost:4770\n\n             --- ENDPOINT ---\nauth.v1.AuthService/Login\n\n             --- REQUEST ---\n{\"email\": \"a@b.io\"}\n\n             --- ASSERTS ---\n.token != \"\"\n\n             --- EXTRACT ---\ntoken = .auth.token\n\n             --- ENDPOINT ---\nfeed.v1.FeedService/List\n\n             --- REQUEST_HEADERS ---\nauthorization: Bearer {{token}}\n\n             --- REQUEST ---\n{}\n\n             --- RESPONSE ---\n{}\n\n             --- RESPONSE ---\n{}\n";
        let doc = crate::parser::parse_gctf_from_str(src, "chain.gctf").expect("parse");
        let steps = summarize_chain(&doc);

        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].endpoint, "auth.v1.AuthService/Login");
        assert_eq!(steps[0].kind, "unary");
        assert_eq!(steps[0].address_source, "section");
        assert_eq!(steps[0].produces, vec!["token"]);

        assert_eq!(steps[1].kind, "server");
        assert_eq!(steps[1].address_source, "inherited");
        assert_eq!(steps[1].consumes, vec!["token"]);
        assert!(steps[1].produces.is_empty());
    }

    #[test]
    fn a_calls_connection_comes_from_the_file_when_the_file_names_one() {
        use crate::grpc::{TlsConfig, WireProtocol};

        assert_eq!(
            protocol_for_call(Some(WireProtocol::GrpcWeb), Some("grpc")),
            WireProtocol::GrpcWeb,
            "the file's OPTIONS.protocol wins, as it does for a run",
        );
        assert_eq!(
            protocol_for_call(None, Some("connectrpc")),
            WireProtocol::ConnectRpc,
            "and the workbench decides when the file says nothing",
        );

        let file = TlsConfig {
            ca_cert_path: Some("/ca.pem".into()),
            ..Default::default()
        };
        let client = TlsConfig {
            insecure_skip_verify: true,
            ..Default::default()
        };
        assert_eq!(
            tls_for_call(Some(file.clone()), Some(client.clone()))
                .unwrap()
                .ca_cert_path,
            Some("/ca.pem".to_string()),
        );
        assert!(
            tls_for_call(None, Some(client))
                .unwrap()
                .insecure_skip_verify,
            "a file with no TLS section leaves the workbench's setting alone",
        );
        assert!(tls_for_call(None, None).is_none());
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn a_scaffold_carries_the_files_proto_section() {
        let dir = std::env::temp_dir().join(format!("gctf-scaffold-proto-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("withproto.gctf"),
            "--- PROTO ---\nfiles: auth.proto, user.proto\nimport_paths: ./proto\n\n--- ENDPOINT ---\na.A/One\n\n--- REQUEST ---\n{}\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("plain.gctf"),
            "--- ENDPOINT ---\na.A/One\n\n--- REQUEST ---\n{}\n",
        )
        .unwrap();

        let state = PlayState {
            collections_dir: dir.clone(),
            collections_dirs: vec![dir.clone()],
            shares_dir: dir.join("shares"),
            project_root: None,
            project_settings: None,
            history_lock: tokio::sync::Mutex::new(()),
            write_lock: tokio::sync::Mutex::new(()),
            collections_mtime: Arc::new(AtomicU64::new(0)),
            jobs: Default::default(),
        };

        let carried = proto_ref_of(&state, "withproto.gctf").expect("a PROTO section is carried");
        assert_eq!(carried.files, vec!["auth.proto", "user.proto"]);
        assert_eq!(carried.import_paths, vec!["./proto"]);
        assert!(carried.descriptor.is_none());

        assert!(proto_ref_of(&state, "plain.gctf").is_none());
    }

    #[test]
    fn referenced_variables_reads_both_placeholder_forms() {
        let src = "--- ENDPOINT ---\npkg.Svc/M\n\n             --- REQUEST ---\n{\"id\": \"{{user_id}}\"}\n\n             --- ASSERTS ---\n.name == $expected_name\n";
        let doc = crate::parser::parse_gctf_from_str(src, "v.gctf").expect("parse");
        let vars = referenced_variables(&parse_collection(&doc));
        assert!(vars.contains(&"user_id".to_string()));
        assert!(vars.contains(&"expected_name".to_string()));
    }

    fn call_of(bodies: Vec<&str>) -> CallRequest {
        CallRequest {
            endpoint: "pkg.Svc/M".to_string(),
            body: serde_json::Value::Null,
            bodies_raw: Some(bodies.into_iter().map(str::to_string).collect()),
            headers: None,
            tls: None,
            tls_insecure: None,
            tls_ca: None,
            tls_cert: None,
            tls_key: None,
            address: None,
            protocol: None,
            collection_path: None,
            session_id: None,
            timeout_seconds: None,
            document_index: 0,
            dataset_row: None,
        }
    }

    #[test]
    fn history_records_the_request_as_it_was_typed() {
        let req = call_of(vec!["{\"token\": \"{{AUTH_TOKEN}}\"}"]);
        assert_eq!(
            recorded_bodies(&req),
            vec!["{\"token\": \"{{AUTH_TOKEN}}\"}".to_string()],
        );
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn an_http_call_writes_no_credential_into_the_project_history() {
        use std::io::{Read, Write};

        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join(".grpctestify");
        std::fs::create_dir_all(root.join("collections")).expect("dirs");
        std::fs::write(root.join("settings.json"), r#"{"active_env":"example"}"#)
            .expect("settings");
        std::fs::write(
            root.join(".env.example"),
            "AUTH_TOKEN=sk-live-abc123
",
        )
        .expect("env");

        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let address = format!("http://{}", listener.local_addr().expect("addr"));
        let served = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().expect("accept");
            let mut buffer = [0u8; 2048];
            let _ = socket.read(&mut buffer);
            socket
                .write_all(b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 2\r\n\r\n{}")
                .expect("respond");
        });

        let mut state = share_state_in(&root.join("collections"));
        Arc::get_mut(&mut state).expect("sole owner").project_root = Some(root.clone());

        let mut req = call_of(vec![r#"{"token": "{{AUTH_TOKEN}}"}"#]);
        req.endpoint = "POST /login".to_string();
        req.address = Some(address);
        req.session_id = Some("probe".to_string());
        let _ = execute_call(State(state), Json(req)).await.expect("call");
        served.join().expect("server thread");

        let written =
            std::fs::read_to_string(root.join("history").join("probe.jsonl")).expect("history");
        assert!(
            written.contains("{{AUTH_TOKEN}}"),
            "the history records the request as it was typed: {written}",
        );
        assert!(
            !written.contains("sk-live-abc123"),
            "a value out of the environment reached a file in the project: {written}",
        );
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn a_call_takes_the_row_it_was_given() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("rows.gctf");
        std::fs::write(
            &file,
            "--- ENDPOINT ---\npkg.Svc/M\n\n--- DATASET ---\n- who: World\n- who: nobody\n\n--- REQUEST ---\n{ \"name\": \"{{dataset.who}}\" }\n",
        )
        .expect("write");
        let state = share_state_in(dir.path());

        let mut req = call_of(vec!["{}"]);
        req.collection_path = Some("rows.gctf".to_string());

        req.dataset_row = Some(0);
        assert_eq!(
            dataset_variables(&state, &req).get("dataset.who"),
            Some(&serde_json::json!("World")),
        );

        req.dataset_row = Some(1);
        assert_eq!(
            dataset_variables(&state, &req).get("dataset.who"),
            Some(&serde_json::json!("nobody")),
        );

        req.dataset_row = Some(9);
        assert!(dataset_variables(&state, &req).is_empty());

        req.dataset_row = None;
        assert!(dataset_variables(&state, &req).is_empty());
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn a_call_binds_what_the_step_extracts() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("chain.gctf");
        std::fs::write(
            &file,
            "--- ENDPOINT ---\npkg.Svc/One\n\n--- EXTRACT ---\nwho = .status\nsize = .items | length\n\n--- ENDPOINT ---\npkg.Svc/Two\n\n--- REQUEST ---\n{ \"name\": \"{{who}}\" }\n",
        )
        .expect("write");
        let state = share_state_in(dir.path());

        let mut req = call_of(vec!["{}"]);
        req.collection_path = Some("chain.gctf".to_string());
        let answer = serde_json::json!({ "status": "ok", "items": [1, 2, 3] });

        assert_eq!(
            extracted_by_call(&state, &req, Some(&answer)),
            vec![
                ("who".to_string(), "ok".to_string()),
                ("size".to_string(), "3".to_string()),
            ],
        );

        req.document_index = 1;
        assert!(extracted_by_call(&state, &req, Some(&answer)).is_empty());

        req.document_index = 0;
        assert!(extracted_by_call(&state, &req, None).is_empty());
    }

    fn share_state_in(dir: &std::path::Path) -> Arc<PlayState> {
        Arc::new(PlayState {
            collections_dir: dir.to_path_buf(),
            collections_dirs: vec![dir.to_path_buf()],
            shares_dir: dir.join("shares"),
            project_root: None,
            project_settings: None,
            history_lock: tokio::sync::Mutex::new(()),
            write_lock: tokio::sync::Mutex::new(()),
            collections_mtime: Arc::new(AtomicU64::new(0)),
            jobs: Default::default(),
        })
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn a_share_travels_without_its_credentials() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = share_state_in(dir.path());
        let mut headers = std::collections::HashMap::new();
        headers.insert(
            "authorization".to_string(),
            "Bearer secret-token".to_string(),
        );
        headers.insert("x-tenant".to_string(), "acme".to_string());

        let created = create_share(
            axum::extract::State(state.clone()),
            Json(ShareRequest {
                endpoint: "pkg.Svc/M".to_string(),
                headers: Some(headers),
                bodies: vec!["{}".to_string()],
                address: None,
                protocol: None,
                tls: None,
                tls_insecure: None,
                ttl_days: None,
                include_secrets: false,
                omitted: Vec::new(),
            }),
        )
        .await
        .expect("share created")
        .0;

        let held = std::fs::read_to_string(
            dir.path()
                .join("shares")
                .join(format!("{}.json", created.id)),
        )
        .expect("the share on disk");
        assert!(
            !held.contains("secret-token"),
            "the value must not be written: {held}"
        );
        assert!(
            held.contains("acme"),
            "everything else still travels: {held}"
        );

        let share: ShareState = serde_json::from_str(&held).expect("share json");
        assert_eq!(share.redacted, vec!["authorization".to_string()]);
        assert!(!share.headers.contains_key("authorization"));
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn a_credential_ticked_on_purpose_travels() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = share_state_in(dir.path());
        let mut headers = std::collections::HashMap::new();
        headers.insert("authorization".to_string(), "Bearer chosen".to_string());

        let created = create_share(
            axum::extract::State(state.clone()),
            Json(ShareRequest {
                endpoint: "pkg.Svc/M".to_string(),
                headers: Some(headers),
                bodies: vec!["{}".to_string()],
                address: None,
                protocol: None,
                tls: None,
                tls_insecure: None,
                ttl_days: None,
                include_secrets: true,
                omitted: Vec::new(),
            }),
        )
        .await
        .expect("share created")
        .0;

        let held = std::fs::read_to_string(
            dir.path()
                .join("shares")
                .join(format!("{}.json", created.id)),
        )
        .expect("the share on disk");
        let share: ShareState = serde_json::from_str(&held).expect("share json");
        assert_eq!(
            share.headers.get("authorization").map(String::as_str),
            Some("Bearer chosen")
        );
        assert!(share.redacted.is_empty());
    }

    #[test]
    fn offsets_keep_the_moment_a_message_arrived() {
        assert_eq!(rebased_offsets(0, 5005, &[0], 1), vec![0]);
        assert_eq!(rebased_offsets(0, 12, &[0], 1), vec![0]);
        assert_eq!(rebased_offsets(12, 30, &[3], 1), vec![15]);
    }

    #[test]
    fn offsets_fall_back_to_the_end_when_the_transport_timed_nothing() {
        assert_eq!(rebased_offsets(0, 40, &[], 2), vec![40, 40]);
    }

    #[test]
    fn renaming_a_dataset_column_reaches_every_step() {
        let file = "--- DATASET ---\n- path: a.json\n  keep: 1\n- path: b.json\n  keep: 2\n\n--- ENDPOINT ---\nGET /{{dataset.path}}\n\n--- ASSERTS ---\n.a == \"{{dataset.path}}\"\n\n--- ENDPOINT ---\nGET /again/{{ dataset.path }}\n";
        let (out, touched) = rename_dataset_column(file, "path", "file");
        assert!(out.contains("- file: a.json"), "{out}");
        assert!(out.contains("- file: b.json"), "{out}");
        assert!(
            out.contains("  keep: 1"),
            "another column is untouched: {out}"
        );
        assert!(out.contains("GET /{{dataset.file}}"), "{out}");
        assert!(out.contains("GET /again/{{dataset.file}}"), "{out}");
        assert!(
            out.contains(".a == \"{{dataset.path}}\""),
            "ASSERTS reads the braces as written, so nothing there is a reader: {out}"
        );
        assert_eq!(touched, 4);
    }

    #[test]
    fn a_quoted_dataset_key_keeps_its_quotes() {
        let file = "--- DATASET ---\n- \"path\": a.json\n";
        let (out, _) = rename_dataset_column(file, "path", "file");
        assert!(out.contains("- \"file\": a.json"), "{out}");
    }

    #[test]
    fn renaming_a_variable_takes_every_reader_with_it() {
        let file = "--- ENDPOINT ---\nGET /a\n\n--- EXTRACT ---\nuser = .id\n\n--- ASSERTS ---\n.id == $user\n\n--- ENDPOINT ---\nGET /b/{{user}}\n\n--- REQUEST_HEADERS ---\nx-who: {{ user }}\n";
        let (out, touched) = rename_variable(file, "user", "account");
        assert!(out.contains("account = .id"), "{out}");
        assert!(out.contains(".id == $account"), "{out}");
        assert!(out.contains("GET /b/{{account}}"), "{out}");
        assert!(out.contains("x-who: {{account}}"), "{out}");
        assert_eq!(touched, 4);
    }

    #[test]
    fn renaming_a_variable_leaves_other_names_alone() {
        let file = "--- EXTRACT ---\nuser = .id\nusername = .name\n\n--- ASSERTS ---\n.a == $username\n\n--- ENDPOINT ---\nGET /b/{{username}}\n";
        let (out, touched) = rename_variable(file, "user", "account");
        assert!(out.contains("account = .id"), "{out}");
        assert!(out.contains("username = .name"), "{out}");
        assert!(out.contains("$username"), "{out}");
        assert!(out.contains("{{username}}"), "{out}");
        assert_eq!(touched, 1);
    }

    #[test]
    fn renaming_a_variable_nothing_reads_touches_only_the_extraction() {
        let file = "--- EXTRACT ---\nuser = .id\n";
        let (_, touched) = rename_variable(file, "user", "account");
        assert_eq!(touched, 1);
    }

    #[test]
    fn moving_a_file_respells_what_it_names() {
        let file = "--- PROTO ---\nfiles: demo.proto, sub/other.proto\nimport_paths: .\n\n--- TLS ---\nca_cert: certs/ca.pem\ninsecure: false\n";
        let (out, changed) = respell_paths(file, "", "auth");
        assert!(
            out.contains("files: ../demo.proto, ../sub/other.proto"),
            "{out}"
        );
        assert!(out.contains("import_paths: .."), "{out}");
        assert!(out.contains("ca_cert: ../certs/ca.pem"), "{out}");
        assert!(
            out.contains("insecure: false"),
            "a key that is not a path is untouched: {out}"
        );
        assert_eq!(changed.len(), 4, "{changed:?}");
    }

    #[test]
    fn moving_a_file_out_of_a_folder_shortens_what_it_names() {
        let file = "--- PROTO ---\nfiles: ../demo.proto\n";
        let (out, changed) = respell_paths(file, "auth", "");
        assert!(out.contains("files: demo.proto"), "{out}");
        assert_eq!(changed.len(), 1);
    }

    #[test]
    fn a_rename_in_place_touches_nothing() {
        let file = "--- PROTO ---\nfiles: demo.proto\n";
        let (out, changed) = respell_paths(file, "auth", "auth");
        assert_eq!(out, file);
        assert!(changed.is_empty());
    }

    #[test]
    fn an_absolute_path_and_a_variable_are_left_as_written() {
        let file = "--- PROTO ---\nfiles: /opt/demo.proto\ndescriptor: {{SCHEMA}}\n";
        let (out, changed) = respell_paths(file, "", "auth");
        assert_eq!(out, file, "{changed:?}");
        assert!(changed.is_empty());
    }

    #[test]
    fn a_call_compresses_what_the_file_says_it_compresses() {
        use apif_grpc_transport::CompressionMode;
        let doc = |body: &str| {
            crate::parser::parse_content_with_recovery(body, "compression.gctf").document
        };

        let gzip = doc(
            "--- ENDPOINT ---\na.Svc/One\n\n--- OPTIONS ---\ncompression: gzip\n\n--- REQUEST ---\n{}\n",
        );
        assert_eq!(
            compression_for_call(&gzip, CompressionMode::None),
            Ok(CompressionMode::Gzip),
            "Execute sent every message uncompressed whatever OPTIONS said"
        );

        let silent = doc("--- ENDPOINT ---\na.Svc/One\n\n--- REQUEST ---\n{}\n");
        assert_eq!(
            compression_for_call(&silent, CompressionMode::Gzip),
            Ok(CompressionMode::Gzip),
            "GRPCTESTIFY_COMPRESSION is read by every run and was read by no call"
        );

        let wrong = doc(
            "--- ENDPOINT ---\na.Svc/One\n\n--- OPTIONS ---\ncompression: brotli\n\n--- REQUEST ---\n{}\n",
        );
        assert!(
            compression_for_call(&wrong, CompressionMode::None).is_err(),
            "a compression the runner refuses must not be silently dropped to none"
        );
    }

    #[test]
    fn messages_with_no_schema_go_out_as_one_stream() {
        assert!(
            sends_one_stream(None, 2),
            "two messages and no schema is a stream"
        );
        assert!(!sends_one_stream(None, 1), "one message is one call");
        assert!(
            !sends_one_stream(None, 0),
            "nothing to send is not a stream"
        );
    }

    #[test]
    fn the_schema_outranks_the_count() {
        assert!(!sends_one_stream(Some(false), 3));
        assert!(sends_one_stream(Some(true), 1));
    }

    #[test]
    fn a_step_is_dialled_where_a_run_would_dial_it() {
        let file = crate::parser::parse_content_with_recovery(
            "--- ADDRESS ---\nhttp://127.0.0.1:8899\n\n--- ENDPOINT ---\nGET /health\n\n--- ASSERTS ---\n@status() == 200\n\n--- ADDRESS ---\n127.0.0.1:4770\n\n--- ENDPOINT ---\na.Svc/One\n\n--- REQUEST ---\n{}\n\n--- ENDPOINT ---\nGET /again\n\n--- ASSERTS ---\n@status() == 200\n",
            "chain.apif",
        )
        .document;

        let (first, first_at) = step_of_file(&file, 0);
        assert_eq!(first_at.as_deref(), Some("http://127.0.0.1:8899"));
        assert_eq!(first.get_endpoint().as_deref(), Some("GET /health"));

        let (second, second_at) = step_of_file(&file, 1);
        assert_eq!(second_at.as_deref(), Some("127.0.0.1:4770"));
        assert_eq!(second.get_endpoint().as_deref(), Some("a.Svc/One"));

        let (_, third_at) = step_of_file(&file, 2);
        assert_eq!(third_at.as_deref(), Some("http://127.0.0.1:8899"));

        assert_eq!(
            step_of_file(&file, 9).1.as_deref(),
            Some("http://127.0.0.1:8899")
        );
    }

    #[test]
    fn a_section_attribute_outranks_the_options_line() {
        let doc =
            |body: &str| crate::parser::parse_content_with_recovery(body, "waits.gctf").document;

        let attributed = doc(
            "--- ENDPOINT ---\na.Svc/One\n\n--- OPTIONS ---\ntimeout: 30\n\n#[timeout(5)]\n--- REQUEST ---\n{}\n",
        );
        assert_eq!(
            timeout_for_file(&attributed),
            Some(5),
            "a run bounds this file at five seconds and Execute read the OPTIONS line"
        );

        let options_only = doc(
            "--- ENDPOINT ---\na.Svc/One\n\n--- OPTIONS ---\ntimeout: 30\n\n--- REQUEST ---\n{}\n",
        );
        assert_eq!(timeout_for_file(&options_only), Some(30));

        let silent = doc("--- ENDPOINT ---\na.Svc/One\n\n--- REQUEST ---\n{}\n");
        assert_eq!(timeout_for_file(&silent), None);
    }

    #[test]
    fn a_call_waits_what_the_file_says_then_what_the_workbench_says() {
        assert_eq!(timeout_for_call(Some(120), Some(5)), 120);
        assert_eq!(timeout_for_call(None, Some(5)), 5);
        assert_eq!(timeout_for_call(None, None), 30);
        assert_eq!(timeout_for_call(Some(0), Some(0)), 30);
        assert_eq!(timeout_for_call(Some(0), Some(5)), 5);
    }

    #[tokio::test]
    #[cfg_attr(miri, ignore)]
    async fn a_copied_line_carries_the_schema_the_file_names() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::write(root.join("schema.bin"), b"not read by the renderer").expect("write");
        std::fs::write(
            root.join("greet.gctf"),
            "--- ADDRESS ---\nlocalhost:50051\n\n--- ENDPOINT ---\npkg.Svc/M\n\n--- PROTO ---\ndescriptor: schema.bin\n\n--- REQUEST ---\n{}\n",
        )
        .expect("write");

        let state = Arc::new(PlayState {
            collections_dir: root.to_path_buf(),
            collections_dirs: vec![root.to_path_buf()],
            shares_dir: root.join("shares"),
            project_root: None,
            project_settings: None,
            history_lock: tokio::sync::Mutex::new(()),
            write_lock: tokio::sync::Mutex::new(()),
            collections_mtime: Arc::new(AtomicU64::new(0)),
            jobs: Default::default(),
        });

        let with_file = generate_grpcurl(
            axum::extract::State(state.clone()),
            Json(CallRequest {
                endpoint: "pkg.Svc/M".to_string(),
                address: Some("localhost:50051".to_string()),
                collection_path: Some("greet.gctf".to_string()),
                ..call_of(vec!["{}"])
            }),
        )
        .await;
        assert!(
            with_file.0.command.contains("-protoset"),
            "{}",
            with_file.0.command
        );
        assert!(
            with_file.0.command.contains("schema.bin"),
            "{}",
            with_file.0.command
        );

        let bare = generate_grpcurl(
            axum::extract::State(state),
            Json(CallRequest {
                endpoint: "pkg.Svc/M".to_string(),
                address: Some("localhost:50051".to_string()),
                ..call_of(vec!["{}"])
            }),
        )
        .await;
        assert!(!bare.0.command.contains("-protoset"), "{}", bare.0.command);
    }

    #[test]
    fn a_grpcurl_line_survives_being_read_back() {
        let mut headers = std::collections::HashMap::new();
        headers.insert("authorization".to_string(), "Bearer x y".to_string());
        let doc = crate::parser::GctfDocumentBuilder::new()
            .with_file_path("<convert>")
            .endpoint("echo.EchoService/SayHello")
            .address("localhost:50051")
            .request(serde_json::json!({"message": "it's \"quoted\""}))
            .request_headers(headers)
            .build();

        let rendered = crate::commands::grpcurl::build_grpcurl_command(
            &doc,
            std::path::Path::new("<inline>"),
            std::path::Path::new("."),
            1,
        )
        .expect("renders");

        let args = split_command_line(&rendered.command);
        let parsed =
            crate::grpc::grpcurl_invocation::ParsedGrpcurl::parse(&args[1..]).expect("reads back");

        assert_eq!(parsed.symbol, "echo.EchoService/SayHello");
        assert_eq!(parsed.address, "localhost:50051");
        assert_eq!(
            parsed.headers.get("authorization").map(String::as_str),
            Some("Bearer x y")
        );
        assert_eq!(
            parsed.request_body,
            serde_json::json!({"message": "it's \"quoted\""})
        );
    }

    #[test]
    fn a_command_line_is_split_the_way_a_shell_splits_it() {
        assert_eq!(
            split_command_line("grpcurl -H \"authorization: Bearer x\" host:1 pkg.S/M"),
            vec![
                "grpcurl",
                "-H",
                "authorization: Bearer x",
                "host:1",
                "pkg.S/M"
            ]
        );
        assert_eq!(
            split_command_line("grpcurl -d '{\"a\": 1}' host:1 pkg.S/M"),
            vec!["grpcurl", "-d", "{\"a\": 1}", "host:1", "pkg.S/M"]
        );
        assert_eq!(split_command_line("a  b\\ c   d"), vec!["a", "b c", "d"]);
        assert!(split_command_line("   ").is_empty());
    }

    #[test]
    fn a_well_known_sample_is_in_its_json_shape() {
        assert!(
            well_known_sample("google.protobuf.FieldMask")
                .unwrap()
                .is_string()
        );
        assert!(
            well_known_sample("google.protobuf.Timestamp")
                .unwrap()
                .is_string()
        );
        assert_eq!(
            well_known_sample("google.protobuf.Duration").unwrap(),
            "30s"
        );
        assert!(
            well_known_sample("google.protobuf.Struct")
                .unwrap()
                .is_object()
        );
        assert!(
            well_known_sample("google.protobuf.BoolValue")
                .unwrap()
                .is_boolean()
        );
        assert!(
            well_known_sample("google.protobuf.Int32Value")
                .unwrap()
                .is_number()
        );
        assert_eq!(
            well_known_sample("google.protobuf.Empty").unwrap(),
            serde_json::json!({})
        );

        let any = well_known_sample("google.protobuf.Any").unwrap();
        assert!(any["@type"].as_str().unwrap().contains("replace"), "{any}");

        assert!(well_known_sample("demo.Req").is_none());
    }

    #[test]
    fn a_bytes_sample_is_sendable() {
        use prost_reflect::Kind;
        let value = fake_value("payload", &Kind::Bytes);
        let text = value.as_str().expect("a string");
        assert!(
            text.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '='),
            "base64 only: {text}"
        );
        assert!(!text.contains(' '), "base64 has no spaces: {text}");
    }

    #[test]
    fn a_sample_value_fits_the_field_it_is_for() {
        use prost_reflect::Kind;
        let url = fake_value("avatar_url", &Kind::String);
        assert!(
            url.as_str().unwrap().starts_with("https://"),
            "a url field takes a url, not an email: {url}"
        );

        for name in ["path", "data", "state", "format"] {
            let value = fake_value(name, &Kind::String);
            assert_ne!(
                value.as_str().unwrap(),
                "2024-06-15T10:30:00Z",
                "{name} is not a timestamp"
            );
        }

        for name in ["created_at", "updated_at", "start_time", "birth_date"] {
            assert_eq!(
                fake_value(name, &Kind::String).as_str().unwrap(),
                "2024-06-15T10:30:00Z",
                "{name} is a timestamp"
            );
        }
    }

    #[test]
    fn an_environment_name_is_refused_in_words() {
        let err = validate_env_name("a/b").expect_err("refused");
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
        assert!(err.1.contains("`a/b`"), "{}", err.1);
        assert!(err.1.contains(".env."), "{}", err.1);
        assert!(validate_env_name("staging").is_ok());
    }

    #[test]
    fn an_unparseable_message_is_named_not_dropped() {
        let err = messages_of(&call_of(vec!["{}", "{\"a\": }"])).expect_err("refused");
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
        assert!(err.1.contains("message #2"), "names the message: {}", err.1);
    }

    #[test]
    fn empty_messages_are_skipped_not_refused() {
        let messages = messages_of(&call_of(vec!["", "  ", "{\"a\": 1}"])).expect("parses");
        assert_eq!(messages.len(), 1);
    }

    fn save_state(dir: &std::path::Path) -> Arc<PlayState> {
        Arc::new(PlayState {
            collections_dir: dir.to_path_buf(),
            collections_dirs: vec![dir.to_path_buf()],
            shares_dir: dir.join("shares"),
            project_root: None,
            project_settings: None,
            history_lock: tokio::sync::Mutex::new(()),
            write_lock: tokio::sync::Mutex::new(()),
            collections_mtime: Arc::new(AtomicU64::new(0)),
            jobs: Default::default(),
        })
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn a_curl_command_is_not_imported_as_grpcurl() {
        let refused = import_grpcurl(Json(ImportGrpcurlRequest {
            args: Some(vec![
                "curl".to_string(),
                "-X".to_string(),
                "POST".to_string(),
                "https://api.example.com/v1/users".to_string(),
            ]),
            command: None,
        }))
        .await
        .err()
        .expect("refused");
        assert_eq!(refused.0, StatusCode::BAD_REQUEST);
        assert!(
            refused.1.0.error.contains("curl command"),
            "{}",
            refused.1.0.error
        );
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn a_grpcurl_command_named_by_path_is_imported_the_same() {
        let by_path = import_grpcurl(Json(ImportGrpcurlRequest {
            args: Some(vec![
                "/usr/local/bin/grpcurl".to_string(),
                "-plaintext".to_string(),
                "localhost:4770".to_string(),
                "pkg.Svc/Method".to_string(),
            ]),
            command: None,
        }))
        .await
        .unwrap_or_else(|_| panic!("imported"));
        assert_eq!(by_path.0.endpoint, "pkg.Svc/Method");
        assert_eq!(by_path.0.address, "localhost:4770");
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn a_grpcurl_command_is_imported() {
        let imported = import_grpcurl(Json(ImportGrpcurlRequest {
            args: Some(vec![
                "grpcurl".to_string(),
                "-plaintext".to_string(),
                "localhost:4770".to_string(),
                "pkg.Svc/Method".to_string(),
            ]),
            command: None,
        }))
        .await
        .unwrap_or_else(|_| panic!("imported"));
        assert_eq!(imported.0.endpoint, "pkg.Svc/Method");
        assert_eq!(imported.0.address, "localhost:4770");
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn reports_go_beside_the_project_not_into_the_collections() {
        let dir = std::env::temp_dir().join(format!("reports-base-{}", std::process::id()));
        let dot = dir.join(".grpctestify");
        let collections = dot.join("collections");
        std::fs::create_dir_all(&collections).expect("dirs");

        let with_project = PlayState {
            collections_dir: collections.clone(),
            collections_dirs: vec![collections.clone()],
            shares_dir: dot.join("shares"),
            project_root: Some(dot.clone()),
            project_settings: None,
            history_lock: tokio::sync::Mutex::new(()),
            write_lock: tokio::sync::Mutex::new(()),
            collections_mtime: Arc::new(AtomicU64::new(0)),
            jobs: Default::default(),
        };
        assert_eq!(reports_base(&with_project), dir.as_path());
        assert_eq!(
            crate::serve::reports::dir_for(reports_base(&with_project), "j-1"),
            dot.join("reports").join("j-1")
        );

        let without = PlayState {
            project_root: None,
            ..with_project
        };
        assert_eq!(reports_base(&without), collections.as_path());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn a_structured_save_over_an_unreadable_file_is_refused() {
        let dir = std::env::temp_dir().join(format!("unreadable-save-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(
            dir.join("broken.gctf"),
            "--- META ---\nname: keep me\n\n--- ENDPOINT ---\npkg.Svc/M\n\n--- REQUEST ---\nnot json\n\n--- RESPONSE ---\n{}\n",
        )
        .expect("write");
        let state = save_state(&dir);

        let mut req = save_of("broken.gctf", None);
        req.original_path = Some("broken.gctf".to_string());
        let refused = refuse_unreadable_original(&state, &req).expect_err("refused");
        assert_eq!(refused.0, StatusCode::BAD_REQUEST);
        assert!(refused.1.contains("edit it as text"), "{}", refused.1);

        std::fs::write(
            dir.join("fine.gctf"),
            "--- ENDPOINT ---\npkg.Svc/M\n\n--- REQUEST ---\n{}\n\n--- RESPONSE ---\n{}\n",
        )
        .expect("write");
        let mut ok = save_of("fine.gctf", None);
        ok.original_path = Some("fine.gctf".to_string());
        assert!(refuse_unreadable_original(&state, &ok).is_ok());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_schema_reads_as_proto() {
        let pool =
            prost_reflect::DescriptorPool::decode(tonic_health::pb::FILE_DESCRIPTOR_SET).unwrap();
        let service = pool.get_service_by_name("grpc.health.v1.Health").unwrap();
        let source = render_service_schema(&service);

        assert!(source.contains("service grpc.health.v1.Health {"));
        assert!(
            source.contains("rpc Watch(HealthCheckRequest) returns (stream HealthCheckResponse);")
        );
        assert!(
            source.contains("  string service = 1;"),
            "fields read as proto:\n{source}"
        );
        assert!(source.contains("  ServingStatus status = 1;"), "{source}");
        assert!(source.contains("enum ServingStatus {"), "{source}");
        assert!(source.contains("  SERVING = 1;"), "{source}");
        assert_eq!(source.matches("message HealthCheckRequest {").count(), 1);
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn diagnostics_answer_in_the_voice_they_are_asked_for() {
        let content = "--- ENDPOINT ---\na.B/C\n\n--- REQUEST ---\n{}\n".to_string();
        let severity_of = |diags: &Vec<tower_lsp::lsp_types::Diagnostic>, start: &str| {
            diags
                .iter()
                .find(|d| d.message.starts_with(start))
                .and_then(|d| d.severity)
        };

        let state = Arc::new(PlayState {
            collections_dir: PathBuf::from("/tmp/nonexistent_XXXX"),
            collections_dirs: vec![PathBuf::from("/tmp/nonexistent_XXXX")],
            shares_dir: PathBuf::from("/tmp/nonexistent_XXXX/shares"),
            project_root: None,
            project_settings: None,
            history_lock: tokio::sync::Mutex::new(()),
            write_lock: tokio::sync::Mutex::new(()),
            collections_mtime: Arc::new(AtomicU64::new(0)),
            jobs: Default::default(),
        });

        let editor = get_diagnostics(
            State(state.clone()),
            Json(DiagnosticsRequest {
                content: content.clone(),
                file_name: Some("t.gctf".to_string()),
                voice: None,
            }),
        )
        .await;
        assert_eq!(
            severity_of(&editor, "Nothing verifies the answer"),
            Some(tower_lsp::lsp_types::DiagnosticSeverity::WARNING),
        );

        let checked = get_diagnostics(
            State(state),
            Json(DiagnosticsRequest {
                content,
                file_name: Some("t.gctf".to_string()),
                voice: Some("check".to_string()),
            }),
        )
        .await;
        assert_eq!(
            severity_of(&checked, "At least one verification section"),
            Some(tower_lsp::lsp_types::DiagnosticSeverity::ERROR),
        );
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn a_check_reports_only_the_files_with_something_to_say() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("clean.gctf"),
            "--- ADDRESS ---\nlocalhost:4770\n\n--- ENDPOINT ---\npkg.Svc/M\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n.ok == true\n",
        )
        .expect("write");
        std::fs::write(
            dir.path().join("bare.gctf"),
            "--- ENDPOINT ---\npkg.Svc/M\n",
        )
        .expect("write");

        let state = Arc::new(PlayState {
            collections_dir: dir.path().to_path_buf(),
            collections_dirs: vec![dir.path().to_path_buf()],
            shares_dir: dir.path().join("shares"),
            project_root: None,
            project_settings: None,
            history_lock: tokio::sync::Mutex::new(()),
            write_lock: tokio::sync::Mutex::new(()),
            collections_mtime: Arc::new(AtomicU64::new(0)),
            jobs: Default::default(),
        });
        let answer = check_files(State(state), Json(CheckRequest { paths: vec![] }))
            .await
            .expect("checked");
        assert_eq!(answer.checked, 2, "both files are read");
        assert!(!answer.truncated);
        assert_eq!(answer.files.len(), 1, "only the one with something to say");
        let said = &answer.files[0];
        assert_eq!(said.path, "bare.gctf");
        assert_eq!(said.errors, 1, "{said:?}");
        assert!(said.warnings > 0);
        assert!(
            said.first
                .as_deref()
                .is_some_and(|m| m.starts_with("At least one verification section")),
            "{said:?}",
        );
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn a_streaming_request_is_written_back_as_one_section() {
        let doc = crate::parser::parse_gctf_from_str(
            "--- ENDPOINT ---\nstream.Svc/Up\n\n--- REQUEST ---\n{\"chunk\": 1}\n{\"chunk\": 2}\n\n--- RESPONSE ---\n{\"ok\": true}\n",
            "t.gctf",
        )
        .expect("parses");
        let parsed = parse_collection(&doc);
        assert!(parsed.bodies_stream);
        assert_eq!(parsed.bodies.len(), 2);

        let dir = std::env::temp_dir().join(format!("stream-save-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let state = save_state(&dir);
        let mut req = save_of("t.gctf", None);
        req.endpoint = parsed.endpoint.clone();
        req.bodies = Some(parsed.bodies.clone());
        req.bodies_stream = parsed.bodies_stream;
        let content = render_structured(&state, &req);
        std::fs::remove_dir_all(&dir).ok();

        assert_eq!(content.matches("--- REQUEST ---").count(), 1, "{content}");
        assert!(
            content.contains("{\"chunk\":1}\n{\"chunk\":2}"),
            "{content}"
        );
    }

    fn save_of(path: &str, expect: Option<ExpectSave>) -> SaveRequestStructured {
        SaveRequestStructured {
            path: path.to_string(),
            endpoint: "a.A/One".to_string(),
            address: None,
            headers: None,
            bodies: Some(vec!["{}".to_string()]),
            bodies_stream: false,
            options: None,
            asserts: None,
            extract: None,
            meta: None,
            tls: None,
            proto: None,
            bench: None,
            dataset: None,
            expect,
            original_path: Some(path.to_string()),
            parallel: false,
            fmt: None,
            version: None,
            document_index: 0,
        }
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn a_bodyless_http_file_is_written_back_without_a_request_section() {
        let dir = std::env::temp_dir().join(format!("httf-body-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let state = save_state(&dir);
        let mut req = save_of("probe.httf", None);
        req.endpoint = "GET /data.json".to_string();
        assert!(!render_structured(&state, &req).contains("--- REQUEST ---"));

        req.bodies = Some(vec!["{\"q\": 1}".to_string()]);
        assert!(render_structured(&state, &req).contains("--- REQUEST ---"));
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn an_empty_body_is_still_written_for_a_grpc_file() {
        let dir = std::env::temp_dir().join(format!("gctf-body-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let state = save_state(&dir);
        let req = save_of("probe.gctf", None);
        assert!(render_structured(&state, &req).contains("--- REQUEST ---"));
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn a_save_keeps_the_step_in_its_group() {
        let dir = std::env::temp_dir().join(format!("gctf-parallel-write-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("p.gctf"),
            "--- ENDPOINT parallel ---\na.A/One\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n.ok == true\n",
        )
        .unwrap();

        let state = save_state(&dir);
        let read = crate::parser::parse_gctf_from_str(
            &std::fs::read_to_string(dir.join("p.gctf")).unwrap(),
            "p.gctf",
        )
        .unwrap();
        assert!(parse_collection(&read).parallel, "the file says so");

        let mut req = save_of("p.gctf", None);
        req.parallel = parse_collection(&read).parallel;
        let rendered = render_structured(&state, &req);

        assert!(rendered.contains("--- ENDPOINT parallel ---"), "{rendered}");
        let back = crate::parser::parse_gctf_from_str(&rendered, "p.gctf").unwrap();
        assert!(parse_collection(&back).parallel);
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn a_save_writes_the_expectation_with_its_options() {
        let dir = std::env::temp_dir().join(format!("gctf-expect-write-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("e.gctf"),
            "--- ENDPOINT ---\na.A/One\n\n--- REQUEST ---\n{}\n",
        )
        .unwrap();

        let state = save_state(&dir);
        let rendered = render_structured(
            &state,
            &save_of(
                "e.gctf",
                Some(ExpectSave {
                    responses: vec![ExpectMessage {
                        body: "{\"ok\": true}".to_string(),
                        partial: true,
                        tolerance: Some(0.5),
                        ..Default::default()
                    }],
                    error: None,
                }),
            ),
        );

        assert!(
            rendered.contains("--- RESPONSE partial tolerance=0.5 ---"),
            "{rendered}"
        );
        assert!(rendered.contains("\"ok\": true"), "{rendered}");

        let back = crate::parser::parse_gctf_from_str(&rendered, "e.gctf").unwrap();
        let parsed = parse_collection(&back);
        assert_eq!(parsed.expect_responses.len(), 1);
        assert!(parsed.expect_responses[0].partial);
        assert_eq!(parsed.expect_responses[0].tolerance, Some(0.5));
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn an_error_expectation_replaces_the_response_one() {
        let dir = std::env::temp_dir().join(format!("gctf-expect-error-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("e.gctf"),
            "--- ENDPOINT ---\na.A/One\n\n--- REQUEST ---\n{}\n\n--- RESPONSE ---\n{\"ok\": true}\n",
        )
        .unwrap();

        let state = save_state(&dir);
        let rendered = render_structured(
            &state,
            &save_of(
                "e.gctf",
                Some(ExpectSave {
                    responses: vec![],
                    error: Some(ExpectMessage {
                        body: "{\"code\": 5}".to_string(),
                        ..Default::default()
                    }),
                }),
            ),
        );

        assert!(rendered.contains("--- ERROR ---"), "{rendered}");
        assert!(!rendered.contains("--- RESPONSE"), "{rendered}");
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn an_expectation_survives_a_save_that_does_not_carry_it() {
        let dir = std::env::temp_dir().join(format!("gctf-expect-keep-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("e.gctf"),
            "--- ENDPOINT ---\na.A/One\n\n--- REQUEST ---\n{}\n\n--- RESPONSE partial=true ---\n{\"ok\": true}\n",
        )
        .unwrap();

        let state = save_state(&dir);
        let rendered = render_structured(&state, &save_of("e.gctf", None));

        assert!(rendered.contains("--- RESPONSE partial ---"), "{rendered}");
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn an_empty_expectation_removes_both_sections() {
        let dir = std::env::temp_dir().join(format!("gctf-expect-drop-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("e.gctf"),
            "--- ENDPOINT ---\na.A/One\n\n--- REQUEST ---\n{}\n\n--- RESPONSE ---\n{\"ok\": true}\n",
        )
        .unwrap();

        let state = save_state(&dir);
        let rendered = render_structured(&state, &save_of("e.gctf", Some(ExpectSave::default())));

        assert!(!rendered.contains("RESPONSE"), "{rendered}");
        assert!(!rendered.contains("ERROR"), "{rendered}");
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn an_empty_response_survives_a_save() {
        let dir = std::env::temp_dir().join(format!("gctf-expect-empty-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("e.gctf"),
            "--- ENDPOINT ---\na.A/One\n\n--- REQUEST ---\n{}\n\n--- RESPONSE ---\n",
        )
        .unwrap();

        let state = save_state(&dir);
        let empty = ExpectMessage {
            body: String::new(),
            ..Default::default()
        };
        let rendered = render_structured(
            &state,
            &save_of(
                "e.gctf",
                Some(ExpectSave {
                    responses: vec![empty],
                    error: None,
                }),
            ),
        );

        assert!(rendered.contains("--- RESPONSE ---"), "{rendered}");
        let parsed = crate::parser::parse_content_with_recovery(&rendered, "e.gctf").document;
        let section = parsed
            .first_section(crate::parser::SectionType::Response)
            .expect("the block is still there");
        assert!(
            matches!(section.content, crate::parser::SectionContent::Empty),
            "an empty block must not come back as a body: {:?}",
            section.content
        );
    }

    #[test]
    fn a_long_stream_is_cut_and_says_so() {
        let many: Vec<serde_json::Value> =
            (0..500).map(|i| serde_json::json!({ "i": i })).collect();
        let (kept, total, truncated) = cap_messages(many);
        assert_eq!(total, 500);
        assert!(truncated);
        assert_eq!(kept.len(), MAX_SHOWN_MESSAGES);
        assert_eq!(
            kept[0],
            serde_json::json!({ "i": 0 }),
            "the first messages are the ones kept"
        );
    }

    #[test]
    fn a_heavy_stream_is_cut_by_size() {
        let heavy: Vec<serde_json::Value> = (0..40)
            .map(|_| serde_json::json!({ "blob": "x".repeat(64 * 1024) }))
            .collect();
        let (kept, total, truncated) = cap_messages(heavy);
        assert_eq!(total, 40);
        assert!(truncated);
        assert!(kept.len() < 40, "the byte cap cut it before the count cap");
    }

    #[test]
    fn a_short_answer_is_left_alone() {
        let (kept, total, truncated) = cap_messages(vec![serde_json::json!({ "ok": true })]);
        assert_eq!(kept.len(), 1);
        assert_eq!(total, 1);
        assert!(!truncated);
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn an_empty_section_removes_the_one_that_was_there() {
        let dir = std::env::temp_dir().join(format!("gctf-drop-section-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("one.gctf");
        std::fs::write(
            &file,
            "--- ENDPOINT ---\na.A/One\n\n--- META ---\nname: login\nowner: qa\n\n--- OPTIONS ---\ntimeout: 7\n\n--- REQUEST ---\n{}\n",
        )
        .unwrap();

        let state = Arc::new(PlayState {
            collections_dir: dir.clone(),
            collections_dirs: vec![dir.clone()],
            shares_dir: dir.join("shares"),
            project_root: None,
            project_settings: None,
            history_lock: tokio::sync::Mutex::new(()),
            write_lock: tokio::sync::Mutex::new(()),
            collections_mtime: Arc::new(AtomicU64::new(0)),
            jobs: Default::default(),
        });
        let req = SaveRequestStructured {
            path: "one.gctf".to_string(),
            endpoint: "a.A/One".to_string(),
            address: None,
            headers: None,
            bodies: Some(vec!["{}".to_string()]),
            bodies_stream: false,
            options: Some(vec![]),
            asserts: None,
            extract: None,
            meta: Some(crate::parser::FileMeta::default()),
            tls: None,
            proto: None,
            bench: None,
            dataset: None,
            expect: None,
            original_path: Some("one.gctf".to_string()),
            parallel: false,
            fmt: None,
            version: None,
            document_index: 0,
        };

        let rendered = render_structured(&state, &req);
        assert!(!rendered.contains("META"), "META is gone: {rendered}");
        assert!(!rendered.contains("OPTIONS"), "OPTIONS is gone: {rendered}");
        assert!(
            rendered.contains("a.A/One"),
            "the rest of the file stays: {rendered}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn base64_round_trips_the_bytes_a_descriptor_set_is_made_of() {
        let raw = vec![0x0a, 0x00, 0xff, 0x7f, 0x41];
        let encoded = "CgD/f0E=";
        assert_eq!(decode_base64(encoded), Some(raw));
    }

    #[test]
    fn base64_accepts_unpadded_and_whitespace_and_refuses_the_rest() {
        assert_eq!(decode_base64("QQ==").as_deref(), Some(&b"A"[..]));
        assert_eq!(decode_base64("QQ").as_deref(), Some(&b"A"[..]));
        assert_eq!(decode_base64("QUJD\nRA==").as_deref(), Some(&b"ABCD"[..]));
        assert!(decode_base64("Q").is_none(), "one sextet cannot be a byte");
        assert!(decode_base64("not base64!").is_none());
        assert!(
            decode_base64("QQ=X").is_none(),
            "padding is the end of the data"
        );
    }

    #[test]
    fn a_proto_section_names_sources_or_a_compiled_set() {
        assert_eq!(proto_kind("auth.proto"), Some("proto"));
        assert_eq!(proto_kind("schema.pb"), Some("descriptor"));
        assert_eq!(proto_kind("schema.bin"), Some("descriptor"));
        assert_eq!(proto_kind("schema.desc"), Some("descriptor"));
        assert_eq!(proto_kind("SCHEMA.PROTOSET"), Some("descriptor"));
        assert_eq!(proto_kind("notes.txt"), None);
        assert_eq!(proto_kind("proto"), None);
    }

    #[test]
    fn a_chain_survives_being_parsed_and_written_back() {
        let source = concat!(
            "--- ENDPOINT ---\na.A/One\n\n--- REQUEST ---\n{}\n\n--- REQUEST ---\n{\"n\": 2}\n\n--- ASSERTS ---\n.ok == true\n\n",
            "--- ENDPOINT ---\nb.B/Two\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n.two == true\n",
        );
        let doc = crate::parser::parse_gctf_from_str(source, "chain.gctf").unwrap();
        let written = crate::parser::serialize_gctf(&doc);

        assert_eq!(
            written.matches("--- REQUEST ---").count(),
            3,
            "every REQUEST keeps its header: {written}"
        );
        let again = crate::parser::parse_gctf_from_str(&written, "chain.gctf").unwrap();
        assert_eq!(
            chain_documents(&again).len(),
            2,
            "still two steps: {written}"
        );
    }

    #[test]
    fn an_extraction_carries_the_type_it_was_written_with() {
        let source = "--- ENDPOINT ---\na.A/One\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n$price >= 0\n\n--- EXTRACT ---\nprice:number = .price\nname = .user.name\n";
        let doc = crate::parser::parse_gctf_from_str(source, "typed.gctf").unwrap();

        let parsed = parse_collection(&doc);
        assert_eq!(
            parsed.extracts.get("price").map(String::as_str),
            Some(".price")
        );
        assert_eq!(
            parsed.extract_types.get("price").map(String::as_str),
            Some("number")
        );
        assert!(parsed.extract_types.get("name").is_none());
    }

    #[test]
    fn a_chain_of_http_steps_grows_an_http_step() {
        let source = "--- ADDRESS ---\nhttps://api.test\n\n--- ENDPOINT ---\nGET /v1/users\n\n--- ASSERTS ---\n@status() == 200\n";
        let doc = crate::parser::parse_gctf_from_str(source, "chain.httf").unwrap();

        let grown = apply_chain_op(&doc, "chain.httf", "append", 0).expect("append");
        let rendered = crate::parser::serialize_gctf(&grown);

        assert_eq!(
            rendered.matches("--- ENDPOINT ---").count(),
            2,
            "{rendered}"
        );
        assert_eq!(
            rendered.matches("--- REQUEST ---").count(),
            0,
            "a GET sends no body: {rendered}"
        );
        assert_eq!(
            rendered.matches("@status() == 200").count(),
            2,
            "{rendered}"
        );

        let back = crate::parser::parse_gctf_from_str(&rendered, "chain.httf").unwrap();
        crate::parser::validate_document_chain(&back).expect("valid");
    }

    #[test]
    fn a_mixed_chain_grows_the_step_its_last_one_is() {
        let source = "--- ADDRESS ---\nhttps://api.test\n\n--- ENDPOINT ---\nGET /v1/users\n\n--- ASSERTS ---\n@status() == 200\n\n--- ADDRESS ---\n127.0.0.1:4770\n\n--- ENDPOINT ---\na.A/One\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n.ok == true\n";
        let doc = crate::parser::parse_gctf_from_str(source, "checkout.apif").unwrap();

        let grown = apply_chain_op(&doc, "checkout.apif", "append", 0).expect("append");
        let steps = chain_documents(&grown);
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[2].get_endpoint().as_deref(), Some("a.A/One"));
        assert!(
            steps[2]
                .first_section(crate::parser::SectionType::Request)
                .is_some(),
            "a gRPC step carries a message",
        );
    }

    #[test]
    fn appending_a_step_starts_from_the_one_before_it() {
        let source =
            "--- ENDPOINT ---\na.A/One\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n.ok == true\n";
        let doc = crate::parser::parse_gctf_from_str(source, "chain.gctf").unwrap();

        let grown = apply_chain_op(&doc, "chain.gctf", "append", 0).expect("append");
        let steps = chain_documents(&grown);
        assert_eq!(steps.len(), 2, "a step was added");
        assert_eq!(steps[1].get_endpoint().as_deref(), Some("a.A/One"));

        let rendered = crate::parser::serialize_gctf(&grown);
        assert_eq!(
            rendered.matches("--- ENDPOINT ---").count(),
            2,
            "{rendered}"
        );
        assert_eq!(
            rendered.matches("--- ASSERTS ---").count(),
            1,
            "the new step invents no expectation: {rendered}"
        );
    }

    #[test]
    fn an_http_step_starts_by_expecting_a_status() {
        let source = "--- ADDRESS ---\nhttps://api.test\n\n--- ENDPOINT ---\nGET /v1/users\n\n--- ASSERTS ---\n@status() == 200\n";
        let doc = crate::parser::parse_gctf_from_str(source, "chain.httf").unwrap();
        let grown = apply_chain_op(&doc, "chain.httf", "append", 0).expect("append");
        let rendered = crate::parser::serialize_gctf(&grown);
        assert_eq!(
            rendered.matches("@status() == 200").count(),
            2,
            "{rendered}"
        );
        assert!(
            !rendered.contains("--- REQUEST ---"),
            "a GET sends no body: {rendered}"
        );
    }

    #[test]
    fn deleting_a_step_keeps_the_others_and_refuses_the_last_one() {
        let source = concat!(
            "--- ENDPOINT ---\na.A/One\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n.one == true\n\n",
            "--- ENDPOINT ---\nb.B/Two\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n.two == true\n\n",
            "--- ENDPOINT ---\nc.C/Three\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n.three == true\n",
        );
        let doc = crate::parser::parse_gctf_from_str(source, "chain.gctf").unwrap();

        let shorter = apply_chain_op(&doc, "chain.gctf", "delete", 1).expect("delete");
        let steps = chain_documents(&shorter);
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].get_endpoint().as_deref(), Some("a.A/One"));
        assert_eq!(steps[1].get_endpoint().as_deref(), Some("c.C/Three"));
        let rendered = crate::parser::serialize_gctf(&shorter);
        assert!(
            !rendered.contains("b.B/Two"),
            "the step is gone: {rendered}"
        );
        assert!(
            rendered.contains(".three == true"),
            "the one after it survives: {rendered}"
        );

        let single = crate::parser::parse_gctf_from_str(
            "--- ENDPOINT ---\na.A/One\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n.ok == true\n",
            "one.gctf",
        )
        .unwrap();
        assert!(
            apply_chain_op(&single, "one.gctf", "delete", 0).is_err(),
            "a file keeps one step"
        );
        assert!(
            apply_chain_op(&doc, "chain.gctf", "delete", 9).is_err(),
            "no such step"
        );
        assert!(
            apply_chain_op(&doc, "chain.gctf", "reverse", 0).is_err(),
            "unknown operation"
        );
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn a_save_without_headers_keeps_the_headers_that_were_there() {
        let dir = std::env::temp_dir().join(format!("gctf-keep-headers-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("one.gctf");
        std::fs::write(
            &file,
            "--- ENDPOINT ---\na.A/One\n\n--- REQUEST_HEADERS ---\nauthorization: Bearer t\n\n--- REQUEST ---\n{}\n",
        )
        .unwrap();

        let state = Arc::new(PlayState {
            collections_dir: dir.clone(),
            collections_dirs: vec![dir.clone()],
            shares_dir: dir.join("shares"),
            project_root: None,
            project_settings: None,
            history_lock: tokio::sync::Mutex::new(()),
            write_lock: tokio::sync::Mutex::new(()),
            collections_mtime: Arc::new(AtomicU64::new(0)),
            jobs: Default::default(),
        });
        let req = SaveRequestStructured {
            path: "one.gctf".to_string(),
            endpoint: "a.A/One".to_string(),
            address: None,
            headers: None,
            bodies: Some(vec!["{}".to_string()]),
            bodies_stream: false,
            options: None,
            asserts: None,
            extract: None,
            meta: None,
            tls: None,
            proto: None,
            bench: None,
            dataset: None,
            expect: None,
            original_path: Some("one.gctf".to_string()),
            parallel: false,
            fmt: None,
            version: None,
            document_index: 0,
        };

        let rendered = render_structured(&state, &req);
        assert!(
            rendered.contains("authorization: Bearer t"),
            "headers survive a save that does not carry them: {rendered}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn a_step_save_without_headers_keeps_that_step_headers() {
        let dir = std::env::temp_dir().join(format!("gctf-step-headers-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("chain.gctf");
        std::fs::write(
            &file,
            "--- ENDPOINT ---\na.A/One\n\n--- REQUEST ---\n{}\n\n--- ENDPOINT ---\nb.B/Two\n\n--- REQUEST_HEADERS ---\nauthorization: Bearer t\n\n--- REQUEST ---\n{}\n",
        )
        .unwrap();

        let state = Arc::new(PlayState {
            collections_dir: dir.clone(),
            collections_dirs: vec![dir.clone()],
            shares_dir: dir.join("shares"),
            project_root: None,
            project_settings: None,
            history_lock: tokio::sync::Mutex::new(()),
            write_lock: tokio::sync::Mutex::new(()),
            collections_mtime: Arc::new(AtomicU64::new(0)),
            jobs: Default::default(),
        });
        let req = SaveRequestStructured {
            path: "chain.gctf".to_string(),
            endpoint: "b.B/TwoEdited".to_string(),
            address: None,
            headers: None,
            bodies: Some(vec!["{}".to_string()]),
            bodies_stream: false,
            options: None,
            asserts: None,
            extract: None,
            meta: None,
            tls: None,
            proto: None,
            bench: None,
            dataset: None,
            expect: None,
            original_path: Some("chain.gctf".to_string()),
            parallel: false,
            fmt: None,
            version: None,
            document_index: 1,
        };

        let rendered = render_structured(&state, &req);
        assert!(
            rendered.contains("authorization: Bearer t"),
            "step two keeps its headers: {rendered}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn saving_a_middle_step_leaves_the_others_alone() {
        let dir = std::env::temp_dir().join(format!("gctf-step-save-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("chain.gctf");
        std::fs::write(
            &file,
            "--- ENDPOINT ---\na.A/One\n\n--- REQUEST ---\n{\"n\": 1}\n\n--- EXTRACT ---\ntoken = .token\n\n--- ENDPOINT ---\nb.B/Two\n\n--- REQUEST ---\n{\"n\": 2}\n\n--- ASSERTS ---\n.ok == true\n\n--- ENDPOINT ---\nc.C/Three\n\n--- REQUEST ---\n{\"n\": 3}\n",
        )
        .unwrap();

        let state = Arc::new(PlayState {
            collections_dir: dir.clone(),
            collections_dirs: vec![dir.clone()],
            shares_dir: dir.join("shares"),
            project_root: None,
            project_settings: None,
            history_lock: tokio::sync::Mutex::new(()),
            write_lock: tokio::sync::Mutex::new(()),
            collections_mtime: Arc::new(AtomicU64::new(0)),
            jobs: Default::default(),
        });
        let req = SaveRequestStructured {
            path: "chain.gctf".to_string(),
            endpoint: "b.B/TwoEdited".to_string(),
            address: None,
            headers: None,
            bodies: Some(vec!["{\"n\": 22}".to_string()]),
            bodies_stream: false,
            options: None,
            asserts: None,
            extract: None,
            meta: None,
            tls: None,
            proto: None,
            bench: None,
            dataset: None,
            expect: None,
            original_path: Some("chain.gctf".to_string()),
            parallel: false,
            fmt: None,
            version: None,
            document_index: 1,
        };

        let rendered = render_structured(&state, &req);
        let doc = crate::parser::parse_gctf_from_str(&rendered, "chain.gctf").expect("reparses");
        let steps: Vec<_> = doc.iter_chain().collect();
        assert_eq!(steps.len(), 3, "the chain keeps its length");

        let endpoint_of = |d: &crate::parser::GctfDocument| {
            d.sections
                .iter()
                .find(|s| s.section_type == crate::parser::SectionType::Endpoint)
                .map(|s| s.raw_content.trim().to_string())
                .unwrap_or_default()
        };
        assert_eq!(endpoint_of(steps[0]), "a.A/One");
        assert_eq!(
            endpoint_of(steps[1]),
            "b.B/TwoEdited",
            "step two is the one edited"
        );
        assert_eq!(endpoint_of(steps[2]), "c.C/Three");

        assert!(
            rendered.contains(".ok == true"),
            "step two keeps its ASSERTS: {rendered}"
        );
        assert!(
            rendered.contains("token = .token"),
            "step one keeps its EXTRACT"
        );
        assert!(rendered.contains("\"n\": 22"), "the new body is written");
        assert!(!rendered.contains("\"n\": 2\n"), "the old body is gone");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_body_that_will_not_parse_stops_the_save() {
        let req = SaveRequestStructured {
            path: "x.gctf".to_string(),
            endpoint: "pkg.Svc/M".to_string(),
            address: None,
            headers: None,
            bodies: Some(vec!["{\"probe\": 1}\"}".to_string()]),
            bodies_stream: false,
            options: None,
            asserts: None,
            extract: None,
            meta: None,
            tls: None,
            proto: None,
            bench: None,
            dataset: None,
            expect: None,
            original_path: None,
            parallel: false,
            fmt: None,
            version: None,
            document_index: 0,
        };
        let err = check_bodies(&req).expect_err("invalid JSON is refused");
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
        assert!(err.1.contains("message #1"), "names the message: {}", err.1);
    }

    #[test]
    fn an_empty_body_is_not_a_parse_failure() {
        let req = SaveRequestStructured {
            path: "x.gctf".to_string(),
            endpoint: "pkg.Svc/M".to_string(),
            address: None,
            headers: None,
            bodies: Some(vec![String::new(), "  ".to_string(), "{}".to_string()]),
            bodies_stream: false,
            options: None,
            asserts: None,
            extract: None,
            meta: None,
            tls: None,
            proto: None,
            bench: None,
            dataset: None,
            expect: None,
            original_path: None,
            parallel: false,
            fmt: None,
            version: None,
            document_index: 0,
        };
        assert!(check_bodies(&req).is_ok());
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn saving_over_a_chain_head_keeps_the_tail() {
        let dir = std::env::temp_dir().join(format!("gctf-chain-save-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("chain.gctf");
        std::fs::write(
            &file,
            "--- ENDPOINT ---\nauth.v1.AuthService/Login\n\n             --- REQUEST ---\n{}\n\n             --- EXTRACT ---\ntoken = .auth.token\n\n             --- ENDPOINT ---\nfeed.v1.FeedService/List\n\n             --- REQUEST ---\n{}\n\n             --- ASSERTS ---\n.items | length > 0\n",
        )
        .unwrap();

        let state = Arc::new(PlayState {
            collections_dir: dir.clone(),
            collections_dirs: vec![dir.clone()],
            shares_dir: dir.join("shares"),
            project_root: None,
            project_settings: None,
            history_lock: tokio::sync::Mutex::new(()),
            write_lock: tokio::sync::Mutex::new(()),
            collections_mtime: Arc::new(AtomicU64::new(0)),
            jobs: Default::default(),
        });
        let req = SaveRequestStructured {
            path: "chain.gctf".to_string(),
            endpoint: "auth.v1.AuthService/Login".to_string(),
            address: None,
            headers: None,
            bodies: Some(vec!["{\"email\": \"a@b.io\"}".to_string()]),
            bodies_stream: false,
            options: None,
            asserts: None,
            extract: None,
            meta: None,
            tls: None,
            proto: None,
            bench: None,
            dataset: None,
            expect: None,
            original_path: Some("chain.gctf".to_string()),
            parallel: false,
            fmt: None,
            version: None,
            document_index: 0,
        };
        let rendered = render_structured(&state, &req);
        let reparsed =
            crate::parser::parse_gctf_from_str(&rendered, "chain.gctf").expect("reparses");
        assert_eq!(
            reparsed.iter_chain().count(),
            2,
            "the tail of the chain must survive a head edit"
        );
        assert!(rendered.contains("feed.v1.FeedService/List"));
        assert!(rendered.contains("a@b.io"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn fmt_on_save_renders_what_the_formatter_would_write() {
        let dir = std::env::temp_dir().join(format!("gctf-fmtsave-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let state = Arc::new(PlayState {
            collections_dir: dir.clone(),
            collections_dirs: vec![dir.clone()],
            shares_dir: dir.join("shares"),
            project_root: None,
            project_settings: None,
            history_lock: tokio::sync::Mutex::new(()),
            write_lock: tokio::sync::Mutex::new(()),
            collections_mtime: Arc::new(AtomicU64::new(0)),
            jobs: Default::default(),
        });
        let mut req = SaveRequestStructured {
            path: "fmt.gctf".to_string(),
            endpoint: "auth.v1.AuthService/Login".to_string(),
            address: None,
            headers: None,
            bodies: Some(vec!["{\"a\":1}".to_string()]),
            bodies_stream: false,
            options: None,
            asserts: Some(vec![".ok == true".to_string()]),
            extract: None,
            meta: None,
            tls: None,
            proto: None,
            bench: None,
            dataset: None,
            expect: None,
            original_path: None,
            parallel: false,
            fmt: None,
            version: None,
            document_index: 0,
        };
        let plain = render_structured(&state, &req);
        req.fmt = Some(true);
        let formatted = render_structured(&state, &req);
        assert_eq!(
            formatted,
            crate::commands::fmt::format_gctf_content(&plain, "fmt.gctf").expect("formats"),
            "the preview must show exactly what the formatter produces"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn every_section_survives_a_structured_save() {
        let dir = std::env::temp_dir().join(format!("gctf-allsections-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let state = Arc::new(PlayState {
            collections_dir: dir.clone(),
            collections_dirs: vec![dir.clone()],
            shares_dir: dir.join("shares"),
            project_root: None,
            project_settings: None,
            history_lock: tokio::sync::Mutex::new(()),
            write_lock: tokio::sync::Mutex::new(()),
            collections_mtime: Arc::new(AtomicU64::new(0)),
            jobs: Default::default(),
        });

        let req = SaveRequestStructured {
            path: "full.gctf".to_string(),
            endpoint: "auth.v1.AuthService/Login".to_string(),
            address: Some("localhost:4770".to_string()),
            headers: None,
            bodies: Some(vec!["{}".to_string()]),
            bodies_stream: false,
            options: Some(vec![("timeout".to_string(), "5".to_string())]),
            asserts: Some(vec![".ok == true".to_string()]),
            extract: Some(vec![("token".to_string(), ".auth.token".to_string())]),
            meta: Some(crate::parser::FileMeta {
                name: Some("login".to_string()),
                tags: vec!["smoke".to_string()],
                ..Default::default()
            }),
            tls: Some(vec![("insecure".to_string(), "true".to_string())]),
            proto: Some(vec![("files".to_string(), "auth.proto".to_string())]),
            bench: Some(vec![("concurrency".to_string(), "10".to_string())]),
            dataset: Some(vec![serde_json::json!({"id": "1"})]),
            expect: Some(ExpectSave {
                responses: vec![ExpectMessage {
                    body: "{\"ok\": true}".to_string(),
                    partial: true,
                    ..Default::default()
                }],
                error: None,
            }),
            original_path: None,
            parallel: false,
            fmt: None,
            version: None,
            document_index: 0,
        };

        let rendered = render_structured(&state, &req);
        for section in [
            "--- META ---",
            "--- BENCH ---",
            "--- DATASET ---",
            "--- ADDRESS ---",
            "--- ENDPOINT ---",
            "--- TLS ---",
            "--- PROTO ---",
            "--- OPTIONS ---",
            "--- REQUEST ---",
            "--- ASSERTS ---",
            "--- EXTRACT ---",
        ] {
            assert!(
                rendered.contains(section),
                "{section} missing from:\n{rendered}"
            );
        }

        let doc = crate::parser::parse_gctf_from_str(&rendered, "full.gctf").expect("parses");
        let parsed = parse_collection(&doc);
        assert_eq!(parsed.dataset.len(), 1);
        assert_eq!(
            parsed.bench.get("concurrency").map(String::as_str),
            Some("10")
        );
        assert_eq!(parsed.tls.get("insecure").map(String::as_str), Some("true"));
        assert_eq!(
            parsed.proto.get("files").map(String::as_str),
            Some("auth.proto")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_attribute_says_which_message_it_belongs_to() {
        let src = "--- ENDPOINT ---\npkg.Svc/M\n\n--- REQUEST ---\n{\"a\":1}\n\n#[skip]\n--- REQUEST ---\n{\"b\":2}\n";
        let doc = crate::parser::parse_gctf_from_str(src, "a.gctf").expect("parse");
        let parsed = parse_collection(&doc);
        assert_eq!(
            parsed.attributes,
            vec![SectionAttribute {
                section: "REQUEST".to_string(),
                index: 1,
                name: "skip".to_string(),
                value: "true".to_string(),
            }],
            "a run skips the second message and the browser was told only that something was skipped"
        );
    }

    #[test]
    fn section_attributes_reach_the_playground() {
        let src = "#[skip]\n#[repeat(3)]\n--- ENDPOINT ---\npkg.Svc/M\n\n--- REQUEST ---\n{}\n";
        let doc = crate::parser::parse_gctf_from_str(src, "a.gctf").expect("parse");
        let parsed = parse_collection(&doc);
        assert_eq!(
            parsed.attributes,
            vec![
                SectionAttribute {
                    section: "ENDPOINT".to_string(),
                    index: 0,
                    name: "skip".to_string(),
                    value: "true".to_string(),
                },
                SectionAttribute {
                    section: "ENDPOINT".to_string(),
                    index: 0,
                    name: "repeat".to_string(),
                    value: "3".to_string(),
                },
            ]
        );
    }

    fn state_at(root: &std::path::Path) -> Arc<PlayState> {
        Arc::new(PlayState {
            collections_dir: root.to_path_buf(),
            collections_dirs: vec![root.to_path_buf()],
            shares_dir: root.join("shares"),
            project_root: None,
            project_settings: None,
            history_lock: tokio::sync::Mutex::new(()),
            write_lock: tokio::sync::Mutex::new(()),
            collections_mtime: Arc::new(AtomicU64::new(0)),
            jobs: Default::default(),
        })
    }

    #[test]
    fn what_counts_as_a_file_the_workbench_manages() {
        for named in [
            "a.gctf",
            "a.httf",
            "a.apif",
            "A.GCTF",
            "schema.proto",
            "s.pb",
            "s.bin",
            "s.desc",
            "s.protoset",
            "rows.csv",
            "rows.json",
            "README.md",
            "notes.txt",
        ] {
            assert!(is_collection_file(named), "{named}");
        }
        for named in [".env", "main.rs", "Makefile", "a.gctf.bak", "", "lib.so"] {
            assert!(!is_collection_file(named), "{named}");
        }
    }

    #[test]
    fn a_dot_anywhere_in_a_path_makes_it_hidden() {
        assert!(has_hidden_component(".env"));
        assert!(has_hidden_component(".git/config"));
        assert!(has_hidden_component("suite/.hidden/a.gctf"));
        assert!(has_hidden_component("suite\\.hidden\\a.gctf"));
        assert!(has_hidden_component("a/.b"));
        assert!(!has_hidden_component("suite/a.gctf"));
        assert!(!has_hidden_component("a.gctf"));
        assert!(!has_hidden_component("v1.2/a.gctf"));
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn a_folder_the_finder_touched_is_still_a_folder_of_tests() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let gctf = "--- ENDPOINT ---\na.B/C\n\n--- REQUEST ---\n{}\n";
        std::fs::create_dir_all(root.join("suite/nested")).unwrap();
        std::fs::write(root.join("suite/a.gctf"), gctf).unwrap();
        std::fs::write(root.join("suite/nested/b.gctf"), gctf).unwrap();
        std::fs::write(root.join("suite/.DS_Store"), b"\0").unwrap();
        std::fs::write(root.join("suite/nested/.gitkeep"), b"").unwrap();
        std::fs::write(root.join("suite/README.md"), "# how these run\n").unwrap();
        assert_eq!(
            stranger_in(&root.join("suite")),
            None,
            "an inert dot-file and a readme do not make a folder unmanageable"
        );

        std::fs::write(root.join("suite/main.rs"), "fn main() {}\n").unwrap();
        assert_eq!(
            stranger_in(&root.join("suite")),
            Some(root.join("suite/main.rs"))
        );
        std::fs::remove_file(root.join("suite/main.rs")).unwrap();

        std::fs::create_dir_all(root.join("suite/.git")).unwrap();
        assert_eq!(
            stranger_in(&root.join("suite")),
            Some(root.join("suite/.git")),
            "a hidden folder is still refused"
        );
    }

    #[cfg(unix)]
    #[cfg_attr(miri, ignore)]
    #[test]
    fn a_link_inside_a_folder_is_a_stranger() {
        let outside = tempfile::tempdir().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("suite")).unwrap();
        std::fs::write(
            root.join("suite/a.gctf"),
            "--- ENDPOINT ---\na.B/C\n\n--- REQUEST ---\n{}\n",
        )
        .unwrap();
        std::os::unix::fs::symlink(outside.path(), root.join("suite/away")).unwrap();
        assert_eq!(
            stranger_in(&root.join("suite")),
            Some(root.join("suite/away"))
        );
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn the_workbench_never_writes_a_path_it_could_not_remove() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let state = state_at(root);

        for refused in [".staging", "grpctestify-reports/x"] {
            let err = create_directory(State(state.clone()), Path(refused.to_string()))
                .await
                .unwrap_err();
            assert_eq!(err.0, StatusCode::BAD_REQUEST, "{refused}");
            assert!(!root.join(refused).exists(), "{refused}");
        }

        let err = save_collection(
            State(state.clone()),
            Json(SaveRequest {
                path: ".hidden/a.gctf".to_string(),
                content: "--- ENDPOINT ---\na.B/C\n\n--- REQUEST ---\n{}\n".to_string(),
                version: None,
                create_only: false,
                original_path: None,
            }),
        )
        .await
        .unwrap_err();
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
        assert!(!root.join(".hidden").exists());

        let _ = create_directory(State(state), Path("plain".to_string()))
            .await
            .expect("an ordinary folder is still made");
        assert!(root.join("plain").is_dir());
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn a_refused_move_leaves_the_file_where_it_started() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let body = "--- ENDPOINT ---\na.B/C\n\n--- REQUEST ---\n{}\n";
        let src = root.join("a.gctf");
        let dst = root.join("b.gctf");
        std::fs::write(&dst, body).unwrap();

        let (status, said) = undo_move(&dst, &src, "a.gctf", "it became a link".to_string());
        assert_eq!(status, StatusCode::CONFLICT);
        assert!(said.contains("was put back"), "{said}");
        assert!(src.is_file(), "the source is back where the user left it");
        assert!(!dst.exists(), "and nothing is left at the destination");
        assert_eq!(std::fs::read_to_string(&src).unwrap(), body);

        let (status, said) = undo_move(
            &root.join("gone.gctf"),
            &root.join("nowhere/x.gctf"),
            "gone.gctf",
            "it became a link".to_string(),
        );
        assert_eq!(status, StatusCode::CONFLICT);
        assert!(said.contains("could not be put back"), "{said}");
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn the_reader_serves_test_files_and_not_the_dotenv_beside_them() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join(".env"), "SECRET=hidden\n").unwrap();
        std::fs::write(
            root.join("ok.gctf"),
            "--- ENDPOINT ---\na.B/C\n\n--- REQUEST ---\n{}\n",
        )
        .unwrap();
        let state = state_at(root);

        let denied = get_collection(State(state.clone()), Path(".env".to_string()))
            .await
            .unwrap_err();
        assert_eq!(denied.0, StatusCode::BAD_REQUEST);

        let served = get_collection(State(state), Path("ok.gctf".to_string()))
            .await
            .expect("a test file is served");
        assert_eq!(served.path, "ok.gctf");
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn a_delete_leaves_hidden_source_and_mixed_folders_alone() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let gctf = "--- ENDPOINT ---\na.B/C\n\n--- REQUEST ---\n{}\n";
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
        std::fs::create_dir_all(root.join("mixed")).unwrap();
        std::fs::write(root.join("mixed/a.gctf"), gctf).unwrap();
        std::fs::write(root.join("mixed/build.sh"), "#!/bin/sh\n").unwrap();
        std::fs::create_dir_all(root.join("suite/nested")).unwrap();
        std::fs::write(root.join("suite/a.gctf"), gctf).unwrap();
        std::fs::write(
            root.join("suite/nested/b.httf"),
            "--- ENDPOINT ---\nGET /\n",
        )
        .unwrap();
        std::fs::write(root.join("suite/.DS_Store"), b"\0").unwrap();
        std::fs::write(root.join("suite/README.md"), "# these run together\n").unwrap();
        std::fs::write(root.join(".env"), "SECRET=hidden\n").unwrap();
        std::fs::write(root.join("schema.proto"), "syntax = \"proto3\";\n").unwrap();
        std::fs::create_dir_all(root.join("grpctestify-reports")).unwrap();
        std::fs::write(root.join("grpctestify-reports/r.gctf"), gctf).unwrap();
        let state = state_at(root);

        for kept in [".git", "src", "mixed", ".env", "grpctestify-reports"] {
            let refused = delete_collection(State(state.clone()), Path(kept.to_string()))
                .await
                .unwrap_err();
            assert_eq!(refused.0, StatusCode::BAD_REQUEST, "{kept}: {}", refused.1);
            assert!(root.join(kept).exists(), "{kept} is still there");
        }

        let _ = delete_collection(State(state.clone()), Path("suite".to_string()))
            .await
            .expect("a folder of test files goes");
        assert!(!root.join("suite").exists());
        let _ = delete_collection(State(state), Path("schema.proto".to_string()))
            .await
            .expect("a schema the workbench uploaded goes");
        assert!(!root.join("schema.proto").exists());
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn a_move_takes_only_collection_items_to_visible_places() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join(".env"), "SECRET=hidden\n").unwrap();
        std::fs::write(
            root.join("a.gctf"),
            "--- ENDPOINT ---\na.B/C\n\n--- REQUEST ---\n{}\n",
        )
        .unwrap();
        let state = state_at(root);

        let refused = move_item(
            State(state.clone()),
            Json(MoveRequest {
                from: ".env".to_string(),
                to: "x.gctf".to_string(),
            }),
        )
        .await
        .unwrap_err();
        assert_eq!(refused.0, StatusCode::BAD_REQUEST);
        assert!(root.join(".env").exists());
        assert!(!root.join("x.gctf").exists());

        let hidden = move_item(
            State(state.clone()),
            Json(MoveRequest {
                from: "a.gctf".to_string(),
                to: ".hidden/a.gctf".to_string(),
            }),
        )
        .await
        .unwrap_err();
        assert_eq!(hidden.0, StatusCode::BAD_REQUEST);
        assert!(root.join("a.gctf").exists());

        let _ = move_item(
            State(state),
            Json(MoveRequest {
                from: "a.gctf".to_string(),
                to: "moved/a.gctf".to_string(),
            }),
        )
        .await
        .expect("a test file moves");
        assert!(root.join("moved/a.gctf").exists());
    }

    #[cfg(unix)]
    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn a_move_leaves_links_where_they_are() {
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(
            outside.path().join("secret.gctf"),
            "--- ENDPOINT ---\na.B/C\n",
        )
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::os::unix::fs::symlink(outside.path().join("secret.gctf"), root.join("link.gctf"))
            .unwrap();
        std::fs::create_dir_all(root.join("folder")).unwrap();
        std::os::unix::fs::symlink(outside.path(), root.join("folder/away")).unwrap();
        let state = state_at(root);

        let linked = move_item(
            State(state.clone()),
            Json(MoveRequest {
                from: "link.gctf".to_string(),
                to: "moved.gctf".to_string(),
            }),
        )
        .await
        .unwrap_err();
        assert_eq!(linked.0, StatusCode::BAD_REQUEST, "{}", linked.1);
        assert!(std::fs::symlink_metadata(root.join("link.gctf")).is_ok());

        let holding = move_item(
            State(state),
            Json(MoveRequest {
                from: "folder".to_string(),
                to: "elsewhere".to_string(),
            }),
        )
        .await
        .unwrap_err();
        assert_eq!(holding.0, StatusCode::BAD_REQUEST, "{}", holding.1);
        assert!(root.join("folder").exists());
        assert!(outside.path().join("secret.gctf").exists());
    }

    #[cfg(unix)]
    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn a_folder_is_not_made_through_a_link() {
        let outside = tempfile::tempdir().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::os::unix::fs::symlink(outside.path(), root.join("link")).unwrap();
        let state = state_at(root);

        let refused = create_directory(State(state.clone()), Path("link/sub".to_string()))
            .await
            .unwrap_err();
        assert_eq!(refused.0, StatusCode::BAD_REQUEST);
        assert!(!outside.path().join("sub").exists());

        let _ = create_directory(State(state), Path("plain/sub".to_string()))
            .await
            .expect("a folder inside the collections is made");
        assert!(root.join("plain/sub").is_dir());
    }

    #[test]
    fn a_relative_tls_file_is_read_from_beside_the_test_file_like_the_runner_reads_it() {
        let root = std::path::Path::new("/proj");
        let beside = std::path::Path::new("/proj/collections/auth");
        let built = |given: &str| {
            tls_config_from_request(
                root,
                beside,
                Some(true),
                &Some(given.to_string()),
                &None,
                &None,
                None,
            )
        };

        assert_eq!(
            built("ca.pem").unwrap().unwrap().ca_cert_path.as_deref(),
            Some("/proj/collections/auth/ca.pem"),
            "the runner resolves a relative TLS path against the test file's own directory"
        );
        assert_eq!(
            built("../certs/ca.pem")
                .unwrap()
                .unwrap()
                .ca_cert_path
                .as_deref(),
            Some("/proj/collections/certs/ca.pem"),
            "`..` is a normal layout when certificates sit beside the collections"
        );

        let outside = built("../../../etc/ca.pem").unwrap_err();
        assert!(outside.contains("lands outside the project"), "{outside}");

        let elsewhere = if cfg!(windows) {
            "C:\\keys\\client.pem"
        } else {
            "/home/me/keys/client.pem"
        };
        let kept = tls_config_from_request(
            root,
            beside,
            Some(true),
            &None,
            &Some(elsewhere.to_string()),
            &None,
            None,
        )
        .expect("a certificate outside the project is the usual case")
        .expect("tls is on");
        assert_eq!(
            kept.client_cert_path.as_deref(),
            Some(elsewhere),
            "an absolute path is dialled as written"
        );
        assert!(
            !kept.insecure_skip_verify,
            "certificates are verified by default"
        );

        let asked =
            tls_config_from_request(root, beside, Some(true), &None, &None, &None, Some(true))
                .unwrap()
                .unwrap();
        assert!(asked.insecure_skip_verify);

        let plaintext = tls_config_from_request(
            root,
            beside,
            None,
            &Some("../../../etc/ca.pem".to_string()),
            &None,
            &None,
            None,
        )
        .unwrap();
        assert!(plaintext.is_none(), "nothing is resolved when TLS is off");
    }

    #[test]
    fn a_path_is_folded_before_it_is_judged() {
        let fold = |p: &str| lexically_normal(std::path::Path::new(p));
        assert_eq!(fold("/a/b/../c"), std::path::PathBuf::from("/a/c"));
        assert_eq!(fold("/a/./b"), std::path::PathBuf::from("/a/b"));
        assert_eq!(fold("/a/b/../.."), std::path::PathBuf::from("/"));
        assert_eq!(fold("a/../b"), std::path::PathBuf::from("b"));
    }

    #[test]
    fn a_send_waits_exactly_as_long_as_a_run_would() {
        assert_eq!(
            timeout_for_call(Some(100_000), None),
            100_000,
            "the file's own OPTIONS timeout is honoured, the way the runner honours it"
        );
        assert_eq!(timeout_for_call(None, Some(301)), 301);
        assert_eq!(timeout_for_call(None, None), 30);
    }

    #[test]
    fn a_dial_the_panel_makes_on_its_own_is_bounded() {
        assert_eq!(dial_timeout(None), 10);
        assert_eq!(dial_timeout(Some(0)), 10);
        assert_eq!(dial_timeout(Some(7)), 7);
        assert_eq!(dial_timeout(Some(u64::MAX)), MAX_DIAL_TIMEOUT_SECS);
    }

    #[test]
    fn paths_are_told_relative_to_the_project() {
        let base = std::path::Path::new("/home/me/app");
        assert_eq!(
            shown_path(
                base,
                std::path::Path::new("/home/me/app/.grpctestify/collections")
            ),
            ".grpctestify/collections"
        );
        assert_eq!(
            shown_path(base, std::path::Path::new("/home/me/app")),
            "app"
        );
        assert_eq!(
            shown_path(base, std::path::Path::new("/elsewhere/tests")),
            "tests"
        );
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn project_info_names_no_absolute_path() {
        let dir = tempfile::tempdir().unwrap();
        let dot = dir.path().join(".grpctestify");
        std::fs::create_dir_all(dot.join("collections")).unwrap();
        let state = PlayState {
            collections_dir: dot.join("collections"),
            collections_dirs: vec![dot.join("collections")],
            shares_dir: dot.join("shares"),
            project_root: Some(dot.clone()),
            project_settings: None,
            history_lock: tokio::sync::Mutex::new(()),
            write_lock: tokio::sync::Mutex::new(()),
            collections_mtime: Arc::new(AtomicU64::new(0)),
            jobs: Default::default(),
        };
        let info = project_info_inner(&state);
        assert_eq!(info.project_dir.as_deref(), Some(".grpctestify"));
        assert_eq!(info.collections_dir, ".grpctestify/collections");
        let told = info
            .project_dir_abs
            .expect("the status bar is told where it is");
        assert!(told.ends_with(".grpctestify"), "{told}");
        assert!(std::path::Path::new(&told).is_absolute(), "{told}");
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn both_raw_env_files_name_their_secrets_without_touching_the_text() {
        let dir = tempfile::tempdir().unwrap();
        super::super::project::init_project_dir(dir.path()).unwrap();
        let root = dir.path().join(".grpctestify");
        let shared = "TENANT=acme\nAPI_TOKEN=t0k\n";
        let local = "DB_PASSWORD=pw\nREGION=eu\n";
        super::super::project::write_dotenv(&root, "staging", shared).unwrap();
        super::super::project::write_dotenv_local(&root, "staging", local).unwrap();

        let state = Arc::new(PlayState {
            collections_dir: root.join("collections"),
            collections_dirs: vec![root.join("collections")],
            shares_dir: root.join("shares"),
            project_root: Some(root.clone()),
            project_settings: None,
            history_lock: tokio::sync::Mutex::new(()),
            write_lock: tokio::sync::Mutex::new(()),
            collections_mtime: Arc::new(AtomicU64::new(0)),
            jobs: Default::default(),
        });

        let file = project_env_get(State(state.clone()), Path("staging".to_string()))
            .await
            .expect("the editor reads the shared file")
            .0;
        assert_eq!(
            file.content, shared,
            "the editor still gets the text as written"
        );
        assert_eq!(file.secret, vec!["API_TOKEN".to_string()]);

        let beside = project_env_local_get(State(state.clone()), Path("staging".to_string()))
            .await
            .expect("the editor reads the local file")
            .0;
        assert!(beside.exists);
        assert_eq!(beside.content.as_deref(), Some(local));
        assert_eq!(beside.secret, vec!["DB_PASSWORD".to_string()]);

        let missing = project_env_local_get(State(state), Path("example".to_string()))
            .await
            .expect("a missing local file is not an error")
            .0;
        assert!(!missing.exists);
        assert!(missing.secret.is_empty());
    }

    #[test]
    fn secret_looking_variables_are_named_but_still_substitute() {
        assert!(is_secret_var("API_TOKEN"));
        assert!(is_secret_var("db_password"));
        assert!(is_secret_var("AwsSecretKey"));
        assert!(is_secret_var("PASSWD"));
        assert!(!is_secret_var("GRPC_ADDRESS"));
        assert!(!is_secret_var("TENANT"));

        let vars: std::collections::HashMap<String, String> = [
            ("API_TOKEN", "t0k"),
            ("db_password", "pw"),
            ("TENANT", "acme"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();

        assert_eq!(
            secret_names(&vars),
            vec!["API_TOKEN".to_string(), "db_password".to_string()]
        );
        assert_eq!(
            vars["API_TOKEN"], "t0k",
            "a call still expands the real value"
        );
        assert!(secret_names(&std::collections::HashMap::new()).is_empty());
    }
}
