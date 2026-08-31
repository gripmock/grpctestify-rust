use axum::Json;
use serde::{Deserialize, Serialize};
use tower_lsp::lsp_types::{CompletionItem, Hover};

use crate::lsp::handlers;
use crate::parser::SectionType;

#[derive(Deserialize)]
pub struct PositionRequest {
    pub content: String,
    pub file_name: Option<String>,
    pub line: usize,
    pub character: u32,
}

pub async fn complete(Json(req): Json<PositionRequest>) -> Json<Vec<CompletionItem>> {
    Json(completions_at(&req))
}

fn completions_at(req: &PositionRequest) -> Vec<CompletionItem> {
    let mut items: Vec<CompletionItem> = Vec::new();

    let line_raw = req.content.lines().nth(req.line).unwrap_or("");
    let line = line_raw.trim();
    let prefix: String = line_raw.chars().take(req.character as usize).collect();
    let prefix = prefix.trim();

    let file_name = req.file_name.as_deref().unwrap_or("playground.gctf");
    let family = crate::parser::ast::Family::of(file_name);

    let typing_header =
        !prefix.is_empty() && prefix.chars().all(|c| c == '-') || prefix.starts_with("--- ");
    if typing_header {
        items.extend(handlers::section_completions_for(family));
    }

    let Ok(doc) = crate::parser::parse_gctf_from_str(&req.content, file_name) else {
        if items.is_empty() && line.is_empty() {
            items.extend(handlers::section_completions_for(family));
        }
        return items;
    };

    let on_header_line = line.starts_with("---") && line.ends_with("---");

    if line.is_empty() && handlers::section_index_at_line(&doc.sections, req.line).is_none() {
        items.extend(handlers::section_completions_for(family));
    }

    for section in &doc.sections {
        if section.start_line > req.line || req.line >= section.end_line {
            continue;
        }
        let on_header = req.line == section.start_line || on_header_line;
        if on_header {
            items.extend(handlers::get_section_header_option_completions(
                &section.section_type,
            ));
            break;
        }

        match section.section_type {
            SectionType::Address => items.extend(handlers::address_completions_for(family)),
            SectionType::Request | SectionType::RequestHeaders => {
                items.extend(handlers::get_variable_completions(&doc, req.line));
            }
            SectionType::Asserts => {
                items.extend(handlers::assertion_completions_for(family));
                items.extend(handlers::get_variable_completions(&doc, req.line));
            }
            SectionType::Extract => items.extend(handlers::get_extract_completions()),
            SectionType::Proto | SectionType::Tls | SectionType::Options | SectionType::Bench => {
                items.extend(handlers::section_key_completions_for(
                    family,
                    &section.section_type,
                ));
            }
            _ => {}
        }
        break;
    }

    let mut seen = std::collections::HashSet::new();
    items.retain(|item| seen.insert(item.label.clone()));
    items
}

#[derive(Deserialize)]
pub struct ExplainRequest {
    pub content: String,
    pub file_name: Option<String>,
}

#[derive(Serialize)]
pub struct ExplainResponse {
    pub documents: Vec<crate::execution::runner::ExecutionPlan>,
    pub runtime: Vec<Vec<RuntimeOption>>,
    pub sections: Vec<SectionSpan>,
    pub mermaid: String,
    pub error: Option<String>,
}

#[derive(Serialize)]
pub struct RuntimeOption {
    pub key: String,
    pub value: String,
    pub source: String,
}

#[derive(Serialize)]
pub struct SectionSpan {
    pub section: String,
    pub start_line: usize,
    pub end_line: usize,
    pub content: String,
}

pub async fn explain(Json(req): Json<ExplainRequest>) -> Json<ExplainResponse> {
    let file_name = req.file_name.as_deref().unwrap_or("playground.gctf");
    match crate::parser::parse_gctf_from_str(&req.content, file_name) {
        Ok(doc) => Json(ExplainResponse {
            documents: doc
                .iter_chain()
                .map(crate::execution::runner::ExecutionPlan::from_document)
                .collect(),
            runtime: doc.iter_chain().map(runtime_options).collect(),
            sections: section_spans(&doc),
            mermaid: crate::commands::explain::mermaid_sequence(&doc),
            error: None,
        }),
        Err(e) => Json(ExplainResponse {
            documents: vec![],
            runtime: vec![],
            sections: vec![],
            mermaid: String::new(),
            error: Some(e.to_string()),
        }),
    }
}

fn runtime_options(doc: &crate::parser::GctfDocument) -> Vec<RuntimeOption> {
    use apif_execution::helpers::{CliRuntimeDefaults, resolve_effective_runtime_options};

    let Ok(effective) = resolve_effective_runtime_options(
        doc,
        CliRuntimeDefaults {
            timeout_seconds: 30,
            retry: 0,
            retry_delay_seconds: 1.0,
            no_retry: false,
        },
    ) else {
        return vec![];
    };

    let source = |s: apif_execution::helpers::RuntimeOptionSource| {
        match s {
            apif_execution::helpers::RuntimeOptionSource::SectionAttribute => "attribute",
            apif_execution::helpers::RuntimeOptionSource::FileOptions => "OPTIONS",
            apif_execution::helpers::RuntimeOptionSource::CliDefaults => "CLI default",
        }
        .to_string()
    };

    let mut rows = vec![
        RuntimeOption {
            key: "timeout".to_string(),
            value: format!("{} s", effective.timeout_seconds.value),
            source: source(effective.timeout_seconds.source),
        },
        RuntimeOption {
            key: "retry".to_string(),
            value: effective.retry.value.to_string(),
            source: source(effective.retry.source),
        },
        RuntimeOption {
            key: "retry_delay".to_string(),
            value: format!("{} s", effective.retry_delay_seconds.value),
            source: source(effective.retry_delay_seconds.source),
        },
        RuntimeOption {
            key: "no_retry".to_string(),
            value: effective.no_retry.value.to_string(),
            source: source(effective.no_retry.source),
        },
        RuntimeOption {
            key: "compression".to_string(),
            value: effective.compression.value.clone(),
            source: source(effective.compression.source),
        },
    ];

    if doc.transport() == crate::parser::ast::Transport::Grpc {
        let named = doc
            .get_options()
            .and_then(|o| o.get("protocol").cloned())
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());
        rows.push(RuntimeOption {
            key: "protocol".to_string(),
            value: named.clone().unwrap_or_else(|| "grpc".to_string()),
            source: if named.is_some() {
                "OPTIONS".to_string()
            } else {
                "CLI default".to_string()
            },
        });
    }

    rows
}

fn screaming_snake(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 2);
    for (i, ch) in name.chars().enumerate() {
        if ch.is_uppercase() && i > 0 {
            out.push('_');
        }
        out.push(ch.to_ascii_uppercase());
    }
    out
}

fn section_spans(doc: &crate::parser::GctfDocument) -> Vec<SectionSpan> {
    use crate::parser::ast::SectionContent;

    doc.iter_chain()
        .flat_map(|d| d.sections.iter())
        .map(|s| SectionSpan {
            section: screaming_snake(&format!("{:?}", s.section_type)),
            start_line: s.start_line + 1,
            end_line: s.end_line.max(s.start_line + 1),
            content: match &s.content {
                SectionContent::Single(_) => "single".to_string(),
                SectionContent::Json(_) => "json".to_string(),
                SectionContent::JsonLines(v) => format!("json lines · {}", v.len()),
                SectionContent::KeyValues(kv) => format!("key-values · {}", kv.len()),
                SectionContent::Extract(kv) => format!("extract · {}", kv.len()),
                SectionContent::Assertions(a) => format!("assertions · {}", a.len()),
                SectionContent::Meta(_) => "meta".to_string(),
                SectionContent::Rows(rows) => format!("rows · {}", rows.len()),
                SectionContent::Empty => "empty".to_string(),
            },
        })
        .collect()
}

#[derive(Serialize)]
pub struct Snippet {
    pub label: String,
    pub detail: Option<String>,
}

pub async fn snippets() -> Json<Vec<Snippet>> {
    Json(
        handlers::get_extract_completions()
            .into_iter()
            .map(|c| Snippet {
                label: c.label,
                detail: c.detail,
            })
            .collect(),
    )
}

#[derive(Serialize)]
pub struct HoverResponse {
    pub hover: Option<Hover>,
}

pub async fn hover(Json(req): Json<PositionRequest>) -> Json<HoverResponse> {
    Json(HoverResponse {
        hover: hover_at(&req),
    })
}

fn hover_at(req: &PositionRequest) -> Option<Hover> {
    use tower_lsp::lsp_types::{HoverContents, MarkedString};

    let file_name = req.file_name.as_deref().unwrap_or("playground.gctf");
    let doc = crate::parser::parse_gctf_from_str(&req.content, file_name).ok()?;

    if let Some(h) = handlers::get_plugin_hover(&doc, req.line, req.character) {
        return Some(h);
    }
    if let Some(h) = handlers::get_var_hover(&doc, req.line, req.character) {
        return Some(h);
    }

    let idx = handlers::section_index_at_line(&doc.sections, req.line)?;
    let section = doc.sections.get(idx)?;
    let text = handlers::section_hover_for(
        crate::parser::ast::Family::of(file_name),
        &section.section_type,
    )?;
    Some(Hover {
        contents: HoverContents::Scalar(MarkedString::String(text)),
        range: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(content: &str, line: usize, character: u32) -> Vec<CompletionItem> {
        completions_at(&PositionRequest {
            content: content.to_string(),
            file_name: None,
            line,
            character,
        })
    }

    fn at_in(content: &str, line: usize, character: u32, file_name: &str) -> Vec<CompletionItem> {
        completions_at(&PositionRequest {
            content: content.to_string(),
            file_name: Some(file_name.to_string()),
            line,
            character,
        })
    }

    #[test]
    fn the_runtime_says_which_transport_a_run_would_use() {
        let named = crate::parser::parse_gctf_from_str(
            "--- ADDRESS ---\nlocalhost:4770\n\n--- ENDPOINT ---\na.B/C\n\n--- OPTIONS ---\nprotocol: grpc-web\n\n--- REQUEST ---\n{}\n",
            "named.gctf",
        )
        .expect("parses");
        let row = runtime_options(&named)
            .into_iter()
            .find(|r| r.key == "protocol")
            .expect("the transport is a runtime value");
        assert_eq!(
            (row.value.as_str(), row.source.as_str()),
            ("grpc-web", "OPTIONS")
        );

        let silent = crate::parser::parse_gctf_from_str(
            "--- ADDRESS ---\nlocalhost:4770\n\n--- ENDPOINT ---\na.B/C\n\n--- REQUEST ---\n{}\n",
            "silent.gctf",
        )
        .expect("parses");
        let row = runtime_options(&silent)
            .into_iter()
            .find(|r| r.key == "protocol")
            .expect("a file that names none still runs over something");
        assert_eq!(
            (row.value.as_str(), row.source.as_str()),
            ("grpc", "CLI default")
        );
    }

    #[test]
    fn an_http_step_is_offered_no_transport_row() {
        let http = crate::parser::parse_gctf_from_str(
            "--- ADDRESS ---\nhttp://localhost:8080\n\n--- ENDPOINT ---\nGET /health\n\n--- ASSERTS ---\n@status() == 200\n",
            "one.httf",
        )
        .expect("parses");
        assert!(runtime_options(&http).iter().all(|r| r.key != "protocol"));
    }

    #[test]
    fn section_spans_are_the_lines_the_editor_shows() {
        let doc = crate::parser::parse_gctf_from_str(
            "--- ADDRESS ---\nlocalhost:4770\n\n--- ENDPOINT ---\na.B/C\n\n--- ASSERTS ---\n.ok == true\n",
            "spans.gctf",
        )
        .expect("parses");

        let spans = section_spans(&doc);
        let named = |name: &str| {
            spans
                .iter()
                .find(|s| s.section == name)
                .map(|s| (s.start_line, s.end_line))
                .expect("section is there")
        };

        assert_eq!(named("ADDRESS"), (1, 3));
        assert_eq!(named("ENDPOINT"), (4, 6));
        assert_eq!(named("ASSERTS").0, 7);
    }

    #[test]
    fn an_http_file_is_explained_like_any_other() {
        let doc = crate::parser::parse_gctf_from_str(
            "--- ADDRESS ---\nhttps://api.example.com\n\n--- ENDPOINT ---\nGET /v1/users\n\n--- ASSERTS ---\n@status() == 200\n",
            "plan.httf",
        )
        .expect("parses");

        let plan = crate::execution::runner::ExecutionPlan::from_document(&doc);
        assert_eq!(plan.connection.backend, "https");
        assert_eq!(plan.connection.source, "ADDRESS section [line 1]");
        assert_eq!(plan.target.endpoint, "GET /v1/users");
        assert_eq!(plan.target.service, None);
        assert_eq!(plan.target.method, None);

        let spans = section_spans(&doc);
        assert_eq!(spans.first().map(|s| s.section.as_str()), Some("ADDRESS"));
    }

    #[test]
    fn the_sections_offered_are_the_ones_that_family_can_carry() {
        let labels = |file: &str| -> Vec<String> {
            at_in("--- ", 0, 4, file)
                .into_iter()
                .map(|c| c.label)
                .collect()
        };

        let http = labels("x.httf");
        for gone in [
            "--- TLS ---",
            "--- PROTO ---",
            "--- BENCH ---",
            "--- ERROR ---",
        ] {
            assert!(!http.contains(&gone.to_string()), "{http:?}");
        }
        assert!(http.contains(&"--- ASSERTS ---".to_string()), "{http:?}");

        let grpc = labels("x.gctf");
        for kept in [
            "--- TLS ---",
            "--- PROTO ---",
            "--- BENCH ---",
            "--- ERROR ---",
        ] {
            assert!(grpc.contains(&kept.to_string()), "{grpc:?}");
        }
    }

    #[test]
    fn the_addresses_offered_are_the_shape_that_family_dials() {
        let content = "--- ADDRESS ---\n\n\n--- ENDPOINT ---\nGET /x\n";
        let labels = |file: &str| -> Vec<String> {
            at_in(content, 1, 0, file)
                .into_iter()
                .map(|c| c.label)
                .collect()
        };

        let http = labels("x.httf");
        assert!(http.iter().any(|l| l.starts_with("http")), "{http:?}");
        assert!(!http.contains(&"localhost:4770".to_string()), "{http:?}");

        assert!(labels("x.gctf").contains(&"localhost:4770".to_string()));
    }

    #[test]
    fn the_plugins_offered_are_the_ones_that_family_answers() {
        let content = "--- ENDPOINT ---\nGET /x\n\n--- ASSERTS ---\n@\n";
        let labels = |file: &str| -> Vec<String> {
            at_in(content, 4, 1, file)
                .into_iter()
                .map(|c| c.label)
                .collect()
        };

        let http = labels("x.httf");
        assert!(http.contains(&"@status(...)".to_string()), "{http:?}");
        assert!(!http.contains(&"@trailer(...)".to_string()), "{http:?}");
        assert!(!http.contains(&"@has_trailer(...)".to_string()), "{http:?}");

        let grpc = labels("x.gctf");
        assert!(grpc.contains(&"@trailer(...)".to_string()), "{grpc:?}");
        assert!(!grpc.contains(&"@status(...)".to_string()), "{grpc:?}");
    }

    #[test]
    fn a_section_is_explained_in_the_terms_of_its_family() {
        let content = "--- ENDPOINT ---\nGET /x\n\n--- ASSERTS ---\n@status() == 200\n";
        let said = |file: &str| -> String {
            let hover = hover_at(&PositionRequest {
                content: content.to_string(),
                file_name: Some(file.to_string()),
                line: 0,
                character: 6,
            })
            .expect("a hover over ENDPOINT");
            format!("{:?}", hover.contents)
        };

        let http = said("x.httf");
        assert!(http.contains("method and a path"), "{http}");
        assert!(!http.contains("package.Service/Method"), "{http}");
        assert!(said("x.gctf").contains("package.Service/Method"));
    }

    #[test]
    fn the_options_keys_offered_are_the_ones_that_family_reads() {
        let content =
            "--- ENDPOINT ---\nGET /x\n\n--- OPTIONS ---\n\n\n--- ASSERTS ---\n@status() == 200\n";
        let labels = |file: &str| -> Vec<String> {
            at_in(content, 4, 0, file)
                .into_iter()
                .map(|c| c.label)
                .collect()
        };

        let http = labels("x.httf");
        assert!(http.contains(&"timeout:".to_string()), "{http:?}");
        assert!(!http.contains(&"protocol:".to_string()), "{http:?}");
        assert!(!http.contains(&"compression:".to_string()), "{http:?}");

        let grpc = labels("x.gctf");
        assert!(grpc.contains(&"protocol:".to_string()), "{grpc:?}");
    }

    #[test]
    fn options_keys_come_from_the_validator_not_a_guess() {
        let content = "--- ENDPOINT ---\npkg.Svc/M\n\n--- OPTIONS ---\n\n\n--- REQUEST ---\n{}\n";
        let labels: Vec<String> = at(content, 4, 0).into_iter().map(|c| c.label).collect();
        assert!(labels.contains(&"protocol:".to_string()));
        assert!(labels.contains(&"timeout:".to_string()));
        assert!(!labels.contains(&"retries:".to_string()));
    }

    #[test]
    fn asserts_offer_operators_and_plugins() {
        let content = "--- ENDPOINT ---\npkg.Svc/M\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n\n";
        let labels: Vec<String> = at(content, 7, 0).into_iter().map(|c| c.label).collect();
        assert!(labels.iter().any(|l| l.starts_with('@')), "plugins offered");
        assert!(!labels.is_empty());
    }

    #[test]
    fn a_section_header_being_typed_offers_sections() {
        let labels: Vec<String> = at("--- \n", 0, 4).into_iter().map(|c| c.label).collect();
        assert!(labels.iter().any(|l| l.contains("ENDPOINT")));
    }

    #[test]
    fn hover_explains_the_section_under_the_cursor() {
        let content = "--- ENDPOINT ---\npkg.Svc/M\n";
        let h = hover_at(&PositionRequest {
            content: content.to_string(),
            file_name: None,
            line: 1,
            character: 2,
        });
        assert!(h.is_some(), "a line inside ENDPOINT hovers");
    }
    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn explain_returns_one_plan_per_document() {
        let content = "--- ADDRESS ---\nlocalhost:4770\n\n             --- ENDPOINT ---\nauth.v1.AuthService/Login\n\n             --- REQUEST ---\n{}\n\n             --- EXTRACT ---\ntoken = .auth.token\n\n             --- ENDPOINT ---\nfeed.v1.FeedService/List\n\n             --- REQUEST ---\n{}\n\n             --- ASSERTS ---\n.ok == true\n";
        let out = explain(Json(ExplainRequest {
            content: content.to_string(),
            file_name: None,
        }))
        .await
        .0;

        assert!(out.error.is_none());
        assert_eq!(out.documents.len(), 2);
        assert_eq!(
            out.documents[0].target.endpoint,
            "auth.v1.AuthService/Login"
        );
        assert_eq!(out.documents[0].summary.variable_extractions, 1);
        assert_eq!(out.documents[1].summary.assertion_blocks, 1);
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn explain_says_where_a_step_without_an_address_will_dial() {
        let content = "--- ADDRESS ---\nlocalhost:4770\n\n             --- ENDPOINT ---\npkg.Svc/A\n\n--- REQUEST ---\n{}\n\n             --- ENDPOINT ---\npkg.Svc/B\n\n--- REQUEST ---\n{}\n";
        let out = explain(Json(ExplainRequest {
            content: content.to_string(),
            file_name: None,
        }))
        .await
        .0;
        assert_eq!(out.documents[0].connection.address, "localhost:4770");
        assert!(out.documents[1].connection.source.contains("Environment"));
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn explain_says_what_each_runtime_option_resolves_to_and_why() {
        let out = explain(Json(ExplainRequest {
            content: "--- ENDPOINT ---\npkg.Svc/M\n\n--- OPTIONS ---\ntimeout: 7\n\n\
                      --- REQUEST ---\n{}\n\n--- ASSERTS ---\n.ok\n"
                .to_string(),
            file_name: None,
        }))
        .await
        .0;

        let head = out.runtime.first().expect("one document, one runtime");
        let timeout = head
            .iter()
            .find(|o| o.key == "timeout")
            .expect("timeout is always resolved");
        assert_eq!(timeout.value, "7 s");
        assert_eq!(
            timeout.source, "OPTIONS",
            "the file decided it, not the CLI"
        );

        let retry = head.iter().find(|o| o.key == "retry").expect("retry");
        assert_eq!(retry.source, "CLI default", "nothing in the file set it");
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn explain_resolves_the_runtime_of_every_step() {
        let out = explain(Json(ExplainRequest {
            content: "--- ENDPOINT ---\na.S/M\n\n--- OPTIONS ---\ntimeout: 7\n\n--- REQUEST ---\n{}\n\n\
                      --- ENDPOINT ---\nb.S/M\n\n--- OPTIONS ---\ntimeout: 11\n\n--- REQUEST ---\n{}\n"
                .to_string(),
            file_name: None,
        }))
        .await
        .0;

        assert_eq!(out.runtime.len(), 2, "one runtime per step");
        let timeout_of = |i: usize| {
            out.runtime[i]
                .iter()
                .find(|o| o.key == "timeout")
                .expect("timeout")
                .value
                .clone()
        };
        assert_eq!(timeout_of(0), "7 s");
        assert_eq!(timeout_of(1), "11 s", "step two has its own OPTIONS");
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn explain_lists_the_sections_the_parser_saw_and_draws_the_chain() {
        let out = explain(Json(ExplainRequest {
            content: "--- ENDPOINT ---\na.S/M\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n.ok\n\n\
                      --- ENDPOINT ---\nb.S/M\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n.ok\n"
                .to_string(),
            file_name: None,
        }))
        .await
        .0;

        let kinds: Vec<_> = out.sections.iter().map(|s| s.section.as_str()).collect();
        assert!(kinds.contains(&"ENDPOINT") && kinds.contains(&"ASSERTS"));
        assert!(
            out.sections.iter().all(|s| s.end_line >= s.start_line),
            "a span that ends before it starts is not a span"
        );

        assert!(out.mermaid.starts_with("sequenceDiagram"));
        assert_eq!(
            out.mermaid.matches("Client->>Server").count(),
            2,
            "one arrow per step of the chain"
        );
    }

    #[test]
    fn a_section_name_reads_the_way_the_file_spells_it() {
        assert_eq!(screaming_snake("RequestHeaders"), "REQUEST_HEADERS");
        assert_eq!(screaming_snake("Asserts"), "ASSERTS");
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn unparsable_content_is_an_error_not_a_panic() {
        let out = explain(Json(ExplainRequest {
            content: "#[repeat(\n--- ENDPOINT ---\npkg.Svc/M\n".to_string(),
            file_name: None,
        }))
        .await
        .0;
        assert!(out.documents.is_empty());
        assert!(out.error.is_some());
    }
}
