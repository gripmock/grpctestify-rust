use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::cli::args::DocsArgs;
use crate::parser::ast::{SectionContent, SectionType};
use crate::parser::{self};
use crate::report::coverage::CoverageReport;
use crate::report::kernel;
use crate::utils::FileUtils;

#[derive(Clone)]
struct FlowStep {
    endpoint: Option<String>,
    group: Option<String>,
    call_line: Option<String>,
    grpcurl_line: Option<String>,
    method: Option<String>,
    requests: Vec<serde_json::Value>,
    responses: Vec<serde_json::Value>,
    error: Option<serde_json::Value>,
    asserts: Vec<String>,
    extracts: Vec<(String, String)>,
    runtime: Vec<(String, String)>,
    shape: Option<String>,
    headers: Vec<(String, String)>,
    dataset: Vec<serde_json::Map<String, serde_json::Value>>,
}

#[derive(Clone)]
struct MethodFlow {
    file: PathBuf,
    title: String,
    summary: Option<String>,
    tags: Vec<String>,
    owner: Option<String>,
    links: Vec<String>,
    steps: Vec<FlowStep>,
}

pub struct DocPage {
    pub name: String,
    pub markdown: String,
}

pub fn render_pages(paths: &[PathBuf], coverage: Option<&CoverageReport>) -> Vec<DocPage> {
    let mut files = Vec::new();
    for path in paths {
        if path.is_dir() {
            files.extend(FileUtils::collect_test_files(path, &[]));
        } else if path.is_file() {
            files.push(path.clone());
        }
    }
    files.sort();
    files.dedup();

    let mut unread = Vec::new();
    let by_service = collect_flows_by_service(&files, &mut unread);
    if by_service.is_empty() && unread.is_empty() {
        return Vec::new();
    }

    let files = page_files(by_service.keys().cloned());
    let mut pages = vec![DocPage {
        name: "index.md".to_string(),
        markdown: render_index(&by_service, coverage, &unread, &files),
    }];
    for (service, flows) in &by_service {
        pages.push(DocPage {
            name: files
                .get(service)
                .cloned()
                .unwrap_or_else(|| page_file(service)),
            markdown: render_service_page(service, flows, coverage, &files),
        });
    }
    pages
}

pub fn handle_docs(args: &DocsArgs) -> Result<()> {
    let paths: Vec<PathBuf> = if args.paths.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        args.paths.clone()
    };

    let coverage = load_coverage(args)?;
    let pages = render_pages(&paths, coverage.as_ref());

    if pages.is_empty() {
        println!("No ENDPOINT-bearing tests found — nothing to document.");
        return Ok(());
    }

    std::fs::create_dir_all(&args.output).with_context(|| {
        format!(
            "failed to create output directory: {}",
            args.output.display()
        )
    })?;

    for page in &pages {
        let page_path = args.output.join(&page.name);
        std::fs::write(&page_path, &page.markdown)
            .with_context(|| format!("failed to write {}", page_path.display()))?;
    }

    println!(
        "{} Wrote {} page(s) to {}",
        crate::report::style::pass_icon(),
        pages.len(),
        args.output.display()
    );
    Ok(())
}

fn load_coverage(args: &DocsArgs) -> Result<Option<CoverageReport>> {
    let Some(path) = &args.coverage else {
        return Ok(None);
    };
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read coverage report: {}", path.display()))?;
    let report: CoverageReport = serde_json::from_str(&content)
        .with_context(|| format!("failed to parse coverage report: {}", path.display()))?;
    Ok(Some(report))
}

fn collect_flows_by_service(
    files: &[PathBuf],
    unread: &mut Vec<(PathBuf, String)>,
) -> BTreeMap<String, Vec<MethodFlow>> {
    let mut by_service: BTreeMap<String, Vec<MethodFlow>> = BTreeMap::new();

    for file in files {
        let doc = match parser::parse_gctf(file) {
            Ok(doc) => doc,
            Err(e) => {
                unread.push((file.clone(), e.to_string()));
                continue;
            }
        };
        let labels = kernel::collect_grpc_labels(&file.to_string_lossy());
        let http_host = http_group(&doc);
        if labels.services.is_empty() && http_host.is_none() {
            continue;
        }
        let package = labels.packages.first().cloned().unwrap_or_default();

        let meta = crate::commands::run::extract_test_meta(&doc);
        let title = meta.name.clone().unwrap_or_else(|| {
            file.file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| file.display().to_string())
        });

        let mut chain_address: Option<String> = None;
        let steps: Vec<FlowStep> = doc
            .iter_chain()
            .map(|d| {
                if let Some(own) = d.get_address(None).filter(|a| !a.trim().is_empty()) {
                    chain_address = Some(own);
                }
                build_flow_step(d, chain_address.as_deref(), Some(file.as_path()))
            })
            .collect();

        let flow = MethodFlow {
            file: file.clone(),
            title,
            summary: meta.summary.clone(),
            tags: meta.tags.clone(),
            owner: meta.owner.clone(),
            links: meta.links.clone(),
            steps,
        };

        let mut groups: Vec<String> = flow.steps.iter().filter_map(|s| s.group.clone()).collect();
        groups.dedup();
        if groups.is_empty() {
            if let Some(host) = http_host {
                groups.push(host);
            }
            for service in &labels.services {
                groups.push(service_key(&package, service));
            }
        }
        let mut filed: Vec<String> = Vec::new();
        for group in groups {
            if filed.contains(&group) {
                continue;
            }
            by_service
                .entry(group.clone())
                .or_default()
                .push(flow.clone());
            filed.push(group);
        }
    }

    for flows in by_service.values_mut() {
        flows.sort_by(|a, b| a.title.cmp(&b.title));
    }
    by_service
}

fn http_group(doc: &parser::ast::GctfDocument) -> Option<String> {
    doc.iter_chain()
        .find(|d| d.transport() == parser::ast::Transport::Http)
        .map(|d| path_group(&d.parse_http_endpoint().unwrap_or_default().1))
}

fn path_group(path: &str) -> String {
    let path = path.trim();
    let path = path.split_once("://").map_or(path, |(_, rest)| {
        rest.split_once('/').map_or("", |(_, p)| p)
    });
    let first = path
        .trim_start_matches('/')
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("");
    if first.is_empty() {
        "/".to_string()
    } else {
        format!("/{first}")
    }
}

fn page_file(group: &str) -> String {
    let name: String = group
        .trim_start_matches('/')
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let name = name.trim_matches('-').to_string();
    if name.is_empty() {
        "root.md".to_string()
    } else {
        format!("{name}.md")
    }
}

fn page_files(groups: impl IntoIterator<Item = String>) -> BTreeMap<String, String> {
    let mut taken: Vec<String> = Vec::new();
    let mut out = BTreeMap::new();
    for group in groups {
        let wanted = page_file(&group);
        let mut name = wanted.clone();
        let mut n = 2;
        while taken.contains(&name) {
            name = format!("{}-{n}.md", wanted.trim_end_matches(".md"));
            n += 1;
        }
        taken.push(name.clone());
        out.insert(group, name);
    }
    out
}

fn service_key(package: &str, service: &str) -> String {
    if package.is_empty() {
        service.to_string()
    } else {
        format!("{package}.{service}")
    }
}

fn tool_of(line: &str) -> &str {
    line.split_whitespace().next().unwrap_or("sh")
}

fn build_flow_step(
    d: &parser::ast::GctfDocument,
    chain_address: Option<&str>,
    source: Option<&std::path::Path>,
) -> FlowStep {
    let (endpoint, method) = match d.parse_endpoint() {
        Some((pkg, service, method)) => {
            let ep = if pkg.is_empty() {
                format!("{service}/{method}")
            } else {
                format!("{pkg}.{service}/{method}")
            };
            (Some(ep), Some(method))
        }
        None => (d.get_endpoint(), None),
    };

    let runs = |kind: SectionType| -> Vec<&parser::ast::Section> {
        d.sections_by_type(kind)
            .into_iter()
            .filter(|section| !section.get_skip())
            .collect()
    };
    let messages_of = |sections: Vec<&parser::ast::Section>| -> Vec<serde_json::Value> {
        sections
            .into_iter()
            .flat_map(|section| match &section.content {
                SectionContent::Json(v) => vec![v.clone()],
                SectionContent::JsonLines(vs) => vs.clone(),
                _ => Vec::new(),
            })
            .collect()
    };
    let dataset: Vec<serde_json::Map<String, serde_json::Value>> = d
        .first_section(SectionType::Dataset)
        .map(|section| match &section.content {
            SectionContent::Rows(rows) => rows
                .iter()
                .filter_map(|row| row.as_object().cloned())
                .collect(),
            _ => Vec::new(),
        })
        .unwrap_or_default();
    let row_vars: std::collections::HashMap<String, serde_json::Value> = dataset
        .first()
        .map(|row| {
            row.iter()
                .map(|(k, v)| (format!("dataset.{k}"), v.clone()))
                .collect()
        })
        .unwrap_or_default();
    let fill = |text: &str| -> String {
        if row_vars.is_empty() {
            return text.to_string();
        }
        crate::execution::runner_helpers::interpolate_variables(text, &row_vars)
            .unwrap_or_else(|| text.to_string())
    };
    let requests = messages_of(runs(SectionType::Request));
    let responses = messages_of(runs(SectionType::Response));
    let error = messages_of(runs(SectionType::Error)).into_iter().next();
    let shape = call_shape(requests.len(), responses.len(), error.is_some());
    let asserts: Vec<String> = runs(SectionType::Asserts)
        .into_iter()
        .flat_map(|section| match &section.content {
            SectionContent::Assertions(lines) => lines.clone(),
            _ => Vec::new(),
        })
        .collect();
    let extracts: Vec<(String, String)> = runs(SectionType::Extract)
        .into_iter()
        .flat_map(|section| match &section.content {
            SectionContent::Extract(bindings) => bindings
                .iter()
                .map(|(name, filter)| (name.clone(), filter.clone()))
                .collect::<Vec<(String, String)>>(),
            _ => Vec::new(),
        })
        .collect();

    let address = d
        .get_address(None)
        .filter(|a| !a.trim().is_empty())
        .or_else(|| chain_address.map(str::to_string));
    let body = match requests.as_slice() {
        [only] => Some(fill(&serde_json::to_string(only).unwrap_or_default())),
        _ => None,
    };
    let streaming_request = requests.len() > 1;
    let grpcurl_body = if streaming_request {
        Some(
            requests
                .iter()
                .map(|r| fill(&serde_json::to_string(r).unwrap_or_default()))
                .collect::<Vec<_>>()
                .join("\n"),
        )
    } else {
        body.clone()
    };
    let protocol = d.get_options().and_then(|o| o.get("protocol").cloned());
    let tls = d.get_tls_config().is_some();
    let headers: Vec<(String, String)> = d
        .get_request_headers()
        .map(|h| h.into_iter().collect())
        .unwrap_or_default();
    let effective = crate::execution::runner_helpers::resolve_effective_runtime_options(
        d,
        crate::execution::runner_helpers::CliRuntimeDefaults {
            timeout_seconds: 30,
            retry: 0,
            retry_delay_seconds: 1.0,
            no_retry: false,
        },
    )
    .ok();
    use crate::execution::runner_helpers::RuntimeOptionSource;
    let from_file =
        |source: &RuntimeOptionSource| !matches!(source, RuntimeOptionSource::CliDefaults);
    let max_time = effective
        .as_ref()
        .filter(|e| from_file(&e.timeout_seconds.source))
        .map(|e| e.timeout_seconds.value);
    let mut runtime: Vec<(String, String)> = Vec::new();
    if let Some(e) = effective.as_ref() {
        if from_file(&e.timeout_seconds.source) {
            runtime.push((
                "timeout".to_string(),
                format!("{} s", e.timeout_seconds.value),
            ));
        }
        if from_file(&e.retry.source) && e.retry.value > 0 {
            runtime.push((
                "retries".to_string(),
                format!("{}, {} s apart", e.retry.value, e.retry_delay_seconds.value),
            ));
        }
        if from_file(&e.no_retry.source) && e.no_retry.value {
            runtime.push(("retries".to_string(), "off".to_string()));
        }
        if from_file(&e.compression.source) && e.compression.value != "none" {
            runtime.push(("compression".to_string(), e.compression.value.clone()));
        }
    }

    if d.transport() == parser::ast::Transport::Http {
        let (verb, path) = d.parse_http_endpoint().unwrap_or_default();
        let url = apif_http_transport::url_for(address.as_deref(), &path);
        return FlowStep {
            endpoint: d.get_endpoint(),
            group: Some(path_group(&path)),
            call_line: Some(crate::commands::call_line::curl_line(
                &verb,
                &url,
                &headers,
                body.as_deref(),
                max_time,
            )),
            grpcurl_line: None,
            method: Some(if path.is_empty() {
                verb
            } else {
                format!("{verb} {path}")
            }),
            requests,
            responses,
            error,
            asserts,
            extracts,
            runtime,
            dataset,
            shape,
            headers,
        };
    }

    let tls_config =
        source.and_then(|file| crate::execution::runner_helpers::build_tls_config(d, file));
    let tls_paths = crate::commands::call_line::TlsPaths {
        ca: tls_config.as_ref().and_then(|t| t.ca_cert_path.as_deref()),
        cert: tls_config
            .as_ref()
            .and_then(|t| t.client_cert_path.as_deref()),
        key: tls_config
            .as_ref()
            .and_then(|t| t.client_key_path.as_deref()),
    };
    let insecure = tls_config.as_ref().is_some_and(|t| t.insecure_skip_verify);
    let call_line = endpoint.as_ref().filter(|_| !streaming_request).map(|ep| {
        crate::commands::call_line::grpctestify_call(
            ep,
            address.as_deref(),
            protocol.as_deref(),
            body.as_deref(),
            insecure,
            !tls && !insecure,
            &headers,
            tls_paths,
            max_time,
        )
    });
    let protoset = source
        .and_then(|file| crate::execution::runner_helpers::build_proto_config(d, file))
        .and_then(|proto| proto.descriptor);
    let grpc_wire = protocol.as_deref().unwrap_or("grpc") == "grpc";
    let grpcurl_line = endpoint.as_ref().filter(|_| grpc_wire).map(|ep| {
        crate::commands::call_line::grpcurl_line(
            ep,
            address.as_deref().unwrap_or("localhost:4770"),
            grpcurl_body.as_deref(),
            !tls && !insecure,
            &headers,
            protoset.as_deref(),
            tls_paths,
            insecure,
            max_time,
        )
    });

    FlowStep {
        group: d
            .parse_endpoint()
            .map(|(package, service, _)| service_key(&package, &service)),
        endpoint,
        call_line,
        grpcurl_line,
        method,
        requests,
        responses,
        error,
        asserts,
        extracts,
        runtime,
        dataset,
        shape,
        headers,
    }
}

fn coverage_line(coverage: Option<&CoverageReport>, service: &str) -> Option<String> {
    let report = coverage?;
    let uri = format!("grpc://{service}");
    let file = report.files.iter().find(|f| f.uri == uri)?;
    let pct = if file.statements.total > 0 {
        (file.statements.covered as f64 / file.statements.total as f64) * 100.0
    } else {
        0.0
    };
    Some(format!(
        "**Coverage:** {}/{} methods called ({:.1}%)\n",
        file.statements.covered, file.statements.total, pct
    ))
}

fn render_index(
    by_service: &BTreeMap<String, Vec<MethodFlow>>,
    coverage: Option<&CoverageReport>,
    unread: &[(PathBuf, String)],
    files: &BTreeMap<String, String>,
) -> String {
    let mut out = String::from("# API Documentation\n\nGenerated by `grpctestify docs`.\n\n");

    if let Some(report) = coverage {
        let pct = if report.summary.total > 0 {
            (report.summary.covered as f64 / report.summary.total as f64) * 100.0
        } else {
            0.0
        };
        out.push_str(&format!(
            "**Overall coverage:** {}/{} methods called ({:.1}%)\n\n",
            report.summary.covered, report.summary.total, pct
        ));
    }

    let http_rows = by_service
        .iter()
        .filter(|(service, flows)| {
            flows.iter().all(|f| {
                f.steps
                    .iter()
                    .filter(|step| step.group.as_deref() == Some(service.as_str()))
                    .all(|step| step.grpcurl_line.is_none())
            })
        })
        .count();
    let (first, second) = if http_rows == by_service.len() && http_rows > 0 {
        ("Path", "Endpoints")
    } else if http_rows == 0 {
        ("Service", "Methods")
    } else {
        ("Service or path", "Methods")
    };
    out.push_str(&format!("| {first} | {second} | Tests |\n|---|---|---|\n"));
    for (service, flows) in by_service {
        let methods: std::collections::BTreeSet<&str> = flows
            .iter()
            .flat_map(|f| {
                f.steps
                    .iter()
                    .filter(|step| step.group.as_deref() == Some(service.as_str()))
                    .filter_map(|s| s.method.as_deref())
            })
            .collect();
        out.push_str(&format!(
            "| [{}]({}) | {} | {} |\n",
            md_cell(service),
            files
                .get(service)
                .cloned()
                .unwrap_or_else(|| page_file(service)),
            methods.len(),
            flows.len()
        ));
    }
    if !unread.is_empty() {
        out.push_str(&format!(
            "\n## Not documented\n\n{} could not be read:\n\n",
            if unread.len() == 1 {
                "One file"
            } else {
                "Some files"
            },
        ));
        for (file, why) in unread {
            out.push_str(&format!(
                "- {} — {}\n",
                md_code(&file.display().to_string()),
                one_line(why),
            ));
        }
        out.push('\n');
    }
    out
}

fn render_service_page(
    service: &str,
    flows: &[MethodFlow],
    coverage: Option<&CoverageReport>,
    files: &BTreeMap<String, String>,
) -> String {
    let mut out = format!("# {service}\n\n");
    if let Some(line) = coverage_line(coverage, service) {
        out.push_str(&line);
        out.push('\n');
    }

    for flow in flows {
        out.push_str(&format!("## {}\n\n", one_line(&flow.title)));
        if let Some(summary) = &flow.summary {
            out.push_str(summary);
            out.push_str("\n\n");
        }
        if !flow.tags.is_empty() {
            out.push_str(&format!(
                "Tags: {}\n\n",
                flow.tags
                    .iter()
                    .map(|t| format!("`{t}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if let Some(owner) = &flow.owner {
            out.push_str(&format!("Owner: {owner}\n\n"));
        }
        if !flow.links.is_empty() {
            out.push_str("Links:\n\n");
            for link in &flow.links {
                out.push_str(&format!("- <{link}>\n"));
            }
            out.push('\n');
        }
        out.push_str(&format!("Source: `{}`\n\n", flow.file.display()));

        out.push_str("Run the file:\n\n```sh\n");
        out.push_str(&format!("grpctestify run {}\n", flow.file.display()));
        out.push_str("```\n\n");

        if flow.steps.len() > 1 {
            out.push_str(&render_sequence_diagram(flow));
        }

        for (i, step) in flow.steps.iter().enumerate() {
            let mine = step.group.as_deref() == Some(service) || flow.steps.len() == 1;
            if flow.steps.len() > 1 {
                out.push_str(&format!("### Step {}\n\n", i + 1));
            }
            if !mine {
                out.push_str(&render_step_elsewhere(step, files));
                continue;
            }
            if let Some(endpoint) = &step.endpoint {
                out.push_str(&format!("**Endpoint:** `{endpoint}`"));
                let grpc = step.grpcurl_line.is_some()
                    || step
                        .call_line
                        .as_deref()
                        .is_some_and(|l| l.starts_with("grpctestify"));
                if let Some(shape) = step.shape.as_deref().filter(|_| grpc) {
                    out.push_str(&format!(" — {shape}"));
                }
                out.push_str("\n\n");
            }
            if !step.dataset.is_empty() {
                out.push_str(&render_dataset(&step.dataset));
            }
            if let Some(line) = &step.call_line {
                out.push_str(&format!(
                    "Call it with `{}`:\n\n```sh\n{line}\n```\n\n",
                    tool_of(line)
                ));
            }
            if let Some(line) = &step.grpcurl_line {
                let lead = if step.call_line.is_some() {
                    "The same call with `grpcurl`"
                } else {
                    "Call it with `grpcurl`"
                };
                out.push_str(&format!("{lead}:\n\n```sh\n{line}\n```\n\n"));
            }
            if !step.runtime.is_empty() {
                out.push_str("Runs with:\n\n");
                for (name, value) in &step.runtime {
                    out.push_str(&format!("- {name}: `{value}`\n"));
                }
                let carried = step.runtime.iter().any(|(name, _)| name == "timeout");
                let only_run = step.runtime.iter().any(|(name, _)| name != "timeout");
                if only_run {
                    out.push_str(&format!(
                        "\n{} `grpctestify run` applies {}; a command line above cannot.\n\n",
                        if carried {
                            "The timeout is on the lines above."
                        } else {
                            ""
                        },
                        if carried { "the rest" } else { "them" },
                    ));
                } else {
                    out.push('\n');
                }
            }
            let carried: Vec<&str> = [step.call_line.as_deref(), step.grpcurl_line.as_deref()]
                .into_iter()
                .flatten()
                .collect();
            let named = unresolved_names(&carried);
            if !named.is_empty() {
                let many_lines = carried.len() > 1;
                let spelled = named
                    .iter()
                    .map(|n| format!("`{{{{{n}}}}}`"))
                    .collect::<Vec<_>>()
                    .join(", ");
                out.push_str(&format!(
                    "The line{} above still name{} {spelled} — `grpctestify run` read{} from the environment (`.env`, `--env`); a shell sends the braces as they are.\n\n",
                    if many_lines { "s" } else { "" },
                    if many_lines { "" } else { "s" },
                    if named.len() == 1 { "s it" } else { "s them" },
                ));
            }
            if !step.headers.is_empty() {
                out.push_str("Headers:\n\n");
                for (name, value) in &step.headers {
                    out.push_str(&format!("- {}: {}\n", md_code(name), md_code(value)));
                }
                out.push('\n');
            }
            match step.requests.as_slice() {
                [] => {}
                [only] => {
                    out.push_str("Request:\n\n```json\n");
                    out.push_str(&pretty_json(only));
                    out.push_str("\n```\n\n");
                }
                many => {
                    out.push_str(&format!("Request — {} messages, in order:\n\n", many.len()));
                    for message in many {
                        out.push_str("```json\n");
                        out.push_str(&pretty_json(message));
                        out.push_str("\n```\n\n");
                    }
                }
            }
            match step.responses.as_slice() {
                [] => {}
                [only] => {
                    out.push_str("Response:\n\n```json\n");
                    out.push_str(&pretty_json(only));
                    out.push_str("\n```\n\n");
                }
                many => {
                    out.push_str(&format!(
                        "Response — {} messages, in order:\n\n",
                        many.len()
                    ));
                    for message in many {
                        out.push_str("```json\n");
                        out.push_str(&pretty_json(message));
                        out.push_str("\n```\n\n");
                    }
                }
            }
            if let Some(error) = &step.error {
                out.push_str("Error:\n\n```json\n");
                out.push_str(&pretty_json(error));
                out.push_str("\n```\n\n");
            }
            if !step.asserts.is_empty() {
                out.push_str("Asserts:\n\n");
                for assertion in &step.asserts {
                    out.push_str(&format!("- {}\n", md_code(assertion)));
                }
                out.push('\n');
            }
            if !step.extracts.is_empty() {
                out.push_str("Extracts, for the steps after this one:\n\n");
                for (name, filter) in &step.extracts {
                    out.push_str(&format!("- `{{{{{name}}}}}` = `{filter}`\n"));
                }
                out.push('\n');
            }
        }
        out.push_str("---\n\n");
    }
    out
}

fn call_shape(requests: usize, responses: usize, has_error: bool) -> Option<String> {
    let base = match (requests > 1, responses > 1) {
        (true, true) => "bidirectional streaming",
        (true, false) => "client streaming",
        (false, true) => "server streaming",
        (false, false) if responses == 0 && !has_error => return None,
        (false, false) => "unary",
    };
    Some(if has_error {
        format!("{base}, expects an error")
    } else {
        base.to_string()
    })
}

fn unresolved_names(lines: &[&str]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in lines {
        let mut rest = *line;
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

fn md_code(value: &str) -> String {
    let longest = value
        .as_bytes()
        .split(|b| *b != b'`')
        .map(<[u8]>::len)
        .max()
        .unwrap_or(0);
    let fence = "`".repeat(longest + 1);
    let pad = if value.starts_with('`') || value.ends_with('`') {
        " "
    } else {
        ""
    };
    format!("{fence}{pad}{value}{pad}{fence}")
}

fn one_line(value: &str) -> String {
    value.replace(['\n', '\r'], " ").trim().to_string()
}

fn md_cell(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('|', "\\|")
        .replace(['\n', '\r'], " ")
}

fn render_dataset(rows: &[serde_json::Map<String, serde_json::Value>]) -> String {
    let mut columns: Vec<&String> = Vec::new();
    for row in rows {
        for name in row.keys() {
            if !columns.contains(&name) {
                columns.push(name);
            }
        }
    }
    if columns.is_empty() {
        return String::new();
    }
    let cell = |value: Option<&serde_json::Value>| match value {
        Some(serde_json::Value::String(text)) => text.clone(),
        Some(other) => other.to_string(),
        None => String::new(),
    };
    let mut out = format!(
        "Runs once per `DATASET` row — {}:\n\n",
        if rows.len() == 1 {
            "1 row".to_string()
        } else {
            format!("{} rows", rows.len())
        },
    );
    out.push_str(&format!(
        "| {} |\n|{}|\n",
        columns
            .iter()
            .map(|c| md_cell(&md_code(c)))
            .collect::<Vec<_>>()
            .join(" | "),
        columns.iter().map(|_| "---").collect::<Vec<_>>().join("|"),
    ));
    for row in rows {
        out.push_str(&format!(
            "| {} |\n",
            columns
                .iter()
                .map(|c| md_cell(&cell(row.get(*c))))
                .collect::<Vec<_>>()
                .join(" | "),
        ));
    }
    out.push_str(
        "\nThe examples below are the first row: a `run` sends each row in turn, substituting \
         `{{dataset.<column>}}` as it goes.\n\n",
    );
    out
}

fn render_step_elsewhere(step: &FlowStep, files: &BTreeMap<String, String>) -> String {
    let mut out = String::new();
    if let Some(endpoint) = &step.endpoint {
        out.push_str(&format!("**Endpoint:** `{endpoint}`"));
        if let Some(group) = &step.group {
            let file = files
                .get(group)
                .cloned()
                .unwrap_or_else(|| page_file(group));
            out.push_str(&format!(" — documented under [{group}]({file})"));
        }
        out.push_str("\n\n");
    }
    if !step.extracts.is_empty() {
        out.push_str("Binds, for the steps after it:\n\n");
        for (name, filter) in &step.extracts {
            out.push_str(&format!("- `{{{{{name}}}}}` = `{filter}`\n"));
        }
        out.push('\n');
    }
    out
}

fn render_sequence_diagram(flow: &MethodFlow) -> String {
    let mut out = String::from(
        "```mermaid\nsequenceDiagram\n    participant Client\n    participant Server\n",
    );
    for step in &flow.steps {
        let method = step.method.as_deref().unwrap_or("?");
        out.push_str(&format!("    Client->>Server: {method}\n"));
        if step.error.is_some() {
            out.push_str("    Server-->>Client: error\n");
        } else {
            out.push_str("    Server-->>Client: response\n");
        }
    }
    out.push_str("```\n\n");
    out
}

fn pretty_json(value: &serde_json::Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_key_joins_package_and_service() {
        assert_eq!(service_key("users", "UserService"), "users.UserService");
    }

    #[test]
    fn service_key_bare_service_when_no_package() {
        assert_eq!(service_key("", "Calculator"), "Calculator");
    }

    #[test]
    fn a_streaming_request_is_documented_as_the_stream_it_is() {
        let dir = std::env::temp_dir().join(format!("docs-stream-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let file = dir.join("upload.gctf");
        std::fs::write(
            &file,
            "--- ADDRESS ---\nlocalhost:4770\n\n--- ENDPOINT ---\nstream.Svc/Upload\n\n--- REQUEST ---\n{\"chunk\": 1}\n{\"chunk\": 2}\n{\"chunk\": 3}\n\n--- RESPONSE ---\n{\"ok\": true}\n",
        )
        .expect("write");

        let md = render_pages(&[file], None)
            .into_iter()
            .find(|p| p.name == "stream.Svc.md")
            .expect("page")
            .markdown;

        assert!(md.contains("Request — 3 messages, in order:"), "{md}");
        assert!(md.contains("\"chunk\": 3"), "{md}");
        assert!(!md.contains("Call it with `grpctestify`:"), "{md}");
        assert!(md.contains("Call it with `grpcurl`:"), "{md}");
        assert!(
            md.contains("{\"chunk\":1}\n{\"chunk\":2}\n{\"chunk\":3}"),
            "{md}"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_documented_call_carries_the_headers_the_file_sends() {
        let dir = std::env::temp_dir().join(format!("docs-headers-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let file = dir.join("guarded.gctf");
        std::fs::write(
            &file,
            "--- ADDRESS ---\nlocalhost:4770\n\n--- ENDPOINT ---\npkg.Svc/M\n\n--- REQUEST_HEADERS ---\nauthorization: Bearer t0ken\n\n--- REQUEST ---\n{}\n\n--- RESPONSE ---\n{}\n",
        )
        .expect("write");

        let md = render_pages(&[file], None)
            .into_iter()
            .find(|p| p.name == "pkg.Svc.md")
            .expect("page")
            .markdown;

        assert!(
            md.contains("-H 'authorization: Bearer t0ken'"),
            "the call line carries it: {md}"
        );
        assert_eq!(
            md.matches("-H 'authorization: Bearer t0ken'").count(),
            2,
            "the grpcurl line carries it too: {md}"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_documented_grpcurl_line_carries_the_schema() {
        let dir = std::env::temp_dir().join(format!("docs-proto-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(dir.join("schema.bin"), b"x").expect("write");
        let file = dir.join("typed.gctf");
        std::fs::write(
            &file,
            "--- ADDRESS ---\nlocalhost:4770\n\n--- ENDPOINT ---\npkg.Svc/M\n\n--- PROTO ---\ndescriptor: schema.bin\n\n--- REQUEST ---\n{}\n\n--- RESPONSE ---\n{}\n",
        )
        .expect("write");

        let md = render_pages(&[file], None)
            .into_iter()
            .find(|p| p.name == "pkg.Svc.md")
            .expect("page")
            .markdown;

        assert!(md.contains("-protoset"), "{md}");
        assert!(md.contains("schema.bin"), "{md}");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_index_names_what_the_set_holds() {
        let dir = std::env::temp_dir().join(format!("docs-index-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let http = dir.join("users.httf");
        std::fs::write(
            &http,
            "--- ADDRESS ---\nhttps://api.example.com\n\n--- ENDPOINT ---\nGET /v1/users\n\n--- ASSERTS ---\n@status() == 200\n",
        )
        .expect("write");
        let grpc = dir.join("greet.gctf");
        std::fs::write(
            &grpc,
            "--- ADDRESS ---\nlocalhost:4770\n\n--- ENDPOINT ---\ngreet.Greeter/Hello\n\n--- REQUEST ---\n{}\n\n--- RESPONSE ---\n{}\n",
        )
        .expect("write");

        let index_of = |paths: &[std::path::PathBuf]| {
            render_pages(paths, None)
                .into_iter()
                .find(|p| p.name == "index.md")
                .expect("index")
                .markdown
        };

        assert!(index_of(std::slice::from_ref(&http)).contains("| Path | Endpoints | Tests |"));
        assert!(index_of(std::slice::from_ref(&grpc)).contains("| Service | Methods | Tests |"));
        assert!(index_of(&[http, grpc]).contains("| Service or path | Methods | Tests |"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_http_file_is_filed_under_the_path_it_calls() {
        let dir = std::env::temp_dir().join(format!("httf-docs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let file = dir.join("users.httf");
        std::fs::write(
            &file,
            "--- ADDRESS ---\nhttps://api.example.com\n\n--- ENDPOINT ---\nPOST /v1/users\n\n--- REQUEST_HEADERS ---\nauthorization: Bearer t\n\n--- REQUEST ---\n{\"name\": \"Ada\"}\n\n--- ASSERTS ---\n@status() == 201\n",
        )
        .expect("write");

        let pages = render_pages(&[file], None);
        let page = pages.iter().find(|p| p.name == "v1.md").unwrap_or_else(|| {
            panic!(
                "pages: {:?}",
                pages.iter().map(|p| &p.name).collect::<Vec<_>>()
            )
        });

        assert!(
            page.markdown.contains("POST /v1/users"),
            "{}",
            page.markdown
        );
        assert!(
            page.markdown
                .contains("curl -L -X POST 'https://api.example.com/v1/users'"),
            "{}",
            page.markdown
        );
        assert!(
            page.markdown.contains("-H 'authorization: Bearer t'"),
            "{}",
            page.markdown
        );
        assert!(!page.markdown.contains("grpcurl"), "{}", page.markdown);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_page_offers_the_file_first_and_one_command_per_block() {
        let dir = std::env::temp_dir().join(format!("docs-blocks-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let file = dir.join("login.gctf");
        std::fs::write(
            &file,
            "--- ADDRESS ---\nlocalhost:4770\n\n--- ENDPOINT ---\nauth.v1.AuthService/Login\n\n--- REQUEST ---\n{\"id\": \"a\"}\n\n--- ASSERTS ---\n.ok == true\n",
        )
        .expect("write");

        let pages = render_pages(std::slice::from_ref(&file), None);
        let page = pages
            .iter()
            .find(|p| p.name == "auth.v1.AuthService.md")
            .expect("a page for the service");
        let md = &page.markdown;

        assert!(
            md.contains(&format!("grpctestify run {}", file.display())),
            "{md}"
        );
        assert!(md.contains("Call it with `grpctestify`:"), "{md}");
        assert!(md.contains("The same call with `grpcurl`:"), "{md}");

        for block in md.split("```sh").skip(1) {
            let commands = block
                .split("```")
                .next()
                .unwrap_or_default()
                .lines()
                .filter(|l| !l.trim().is_empty())
                .count();
            assert_eq!(commands, 1, "one command per block\n{md}");
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_chain_step_carries_the_address_and_the_binding_it_reads() {
        let dir = std::env::temp_dir().join(format!("docs-chain-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let file = dir.join("chain.httf");
        std::fs::write(
            &file,
            "--- ADDRESS ---\nhttps://api.example.com\n\n--- ENDPOINT ---\nGET /v1/users\n\n--- ASSERTS ---\n@status() == 200\n\n--- EXTRACT ---\nuser = .id\n\n--- ENDPOINT ---\nGET /v1/users/{{user}}\n\n--- ASSERTS ---\n@status() == 200\n",
        )
        .expect("write");

        let pages = render_pages(&[file], None);
        let page = pages
            .iter()
            .find(|p| p.name == "v1.md")
            .expect("a page for the path");
        let md = &page.markdown;

        assert!(
            md.contains("curl -L 'https://api.example.com/v1/users/{{user}}'"),
            "{md}"
        );
        assert!(md.contains("`{{user}}` = `.id`"), "{md}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    fn report_with_uri(uri: &str, covered: usize, total: usize) -> CoverageReport {
        use crate::report::coverage::{CoverageFile, CoverageStats};
        CoverageReport {
            files: vec![CoverageFile {
                uri: uri.to_string(),
                statements: CoverageStats { covered, total },
                branches: None,
                functions: None,
                fields: None,
                methods: Vec::new(),
            }],
            messages: Vec::new(),
            summary: CoverageStats { covered, total },
            field_summary: CoverageStats {
                covered: 0,
                total: 0,
            },
        }
    }

    #[test]
    fn coverage_line_matches_a_service_by_its_full_name() {
        let report = report_with_uri("grpc://users.UserService", 2, 5);
        let line = coverage_line(Some(&report), "users.UserService").unwrap();
        assert!(line.contains("2/5 methods called (40.0%)"), "{line}");
    }

    #[test]
    fn coverage_line_does_not_match_a_bare_name_from_another_package() {
        let report = report_with_uri("grpc://other.UserService", 2, 5);
        assert!(coverage_line(Some(&report), "users.UserService").is_none());
    }

    #[test]
    fn coverage_line_none_when_service_not_in_report() {
        let report = report_with_uri("grpc://OtherService", 1, 1);
        assert!(coverage_line(Some(&report), "users.UserService").is_none());
    }

    #[test]
    fn coverage_line_none_without_report() {
        assert!(coverage_line(None, "users.UserService").is_none());
    }

    #[test]
    fn render_sequence_diagram_marks_error_steps_distinctly() {
        let flow = MethodFlow {
            file: PathBuf::from("t.gctf"),
            title: "t".to_string(),
            summary: None,
            tags: Vec::new(),
            owner: None,
            links: Vec::new(),
            steps: vec![
                FlowStep {
                    group: None,
                    runtime: Vec::new(),
                    dataset: Vec::new(),
                    shape: None,
                    headers: Vec::new(),
                    call_line: None,
                    grpcurl_line: None,
                    endpoint: None,
                    method: Some("Get".to_string()),
                    requests: Vec::new(),
                    responses: vec![serde_json::json!({})],
                    error: None,
                    asserts: Vec::new(),
                    extracts: Vec::new(),
                },
                FlowStep {
                    group: None,
                    runtime: Vec::new(),
                    dataset: Vec::new(),
                    shape: None,
                    headers: Vec::new(),
                    call_line: None,
                    grpcurl_line: None,
                    endpoint: None,
                    method: Some("Delete".to_string()),
                    requests: Vec::new(),
                    responses: Vec::new(),
                    error: Some(serde_json::json!({"code": 5})),
                    asserts: Vec::new(),
                    extracts: Vec::new(),
                },
            ],
        };
        let diagram = render_sequence_diagram(&flow);
        assert!(diagram.contains("Client->>Server: Get"));
        assert!(diagram.contains("Client->>Server: Delete"));
        assert!(diagram.contains("Server-->>Client: response"));
        assert!(diagram.contains("Server-->>Client: error"));
    }

    #[test]
    fn a_step_is_written_out_on_its_own_page_and_linked_from_the_other() {
        let dir = std::env::temp_dir().join(format!("docs-cross-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let file = dir.join("checkout.apif");
        std::fs::write(
            &file,
            "--- ADDRESS ---\nhttp://127.0.0.1:8899\n\n--- ENDPOINT ---\nGET /api/health\n\n--- EXTRACT ---\nwho = .status\n\n--- ADDRESS ---\nlocalhost:4770\n\n--- ENDPOINT ---\npkg.Svc/M\n\n--- REQUEST ---\n{\"name\": \"{{who}}\"}\n\n--- RESPONSE ---\n{}\n",
        )
        .expect("write");

        let pages = render_pages(&[file], None);
        let page = |name: &str| {
            pages
                .iter()
                .find(|p| p.name == name)
                .unwrap_or_else(|| {
                    panic!(
                        "no {name} in {:?}",
                        pages.iter().map(|p| &p.name).collect::<Vec<_>>()
                    )
                })
                .markdown
                .clone()
        };

        let host = page("api.md");
        assert!(host.contains("curl -L"), "its own step in full: {host}");
        assert!(
            host.contains("documented under [pkg.Svc](pkg.Svc.md)"),
            "the other step is a link: {host}"
        );
        assert!(
            !host.contains("grpcurl"),
            "not the other page's command: {host}"
        );

        let service = page("pkg.Svc.md");
        assert!(service.contains("grpcurl"), "{service}");
        assert!(
            service.contains("documented under [/api](api.md)"),
            "{service}"
        );
        assert!(service.contains("`{{who}}` = `.status`"), "{service}");
        assert!(!service.contains("curl -L"), "{service}");

        let index = page("index.md");
        assert!(index.contains("| [/api](api.md) | 1 |"), "{index}");
        assert!(index.contains("| [pkg.Svc](pkg.Svc.md) | 1 |"), "{index}");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn every_message_the_answer_holds_is_documented() {
        let dir = std::env::temp_dir().join(format!("docs-answers-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let file = dir.join("feed.gctf");
        std::fs::write(
            &file,
            "--- ADDRESS ---\nlocalhost:4770\n\n--- ENDPOINT ---\nfeed.Svc/Watch\n\n--- REQUEST ---\n{}\n\n--- RESPONSE ---\n{\"n\": 1}\n{\"n\": 2}\n\n--- RESPONSE ---\n{\"n\": 3}\n",
        )
        .expect("write");

        let md = render_pages(&[file], None)
            .into_iter()
            .find(|p| p.name == "feed.Svc.md")
            .expect("page")
            .markdown;

        assert!(md.contains("Response — 3 messages, in order:"), "{md}");
        assert!(md.contains("\"n\": 3"), "{md}");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn what_a_run_skips_is_not_documented() {
        let dir = std::env::temp_dir().join(format!("docs-skip-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let file = dir.join("partial.gctf");
        std::fs::write(
            &file,
            "--- ADDRESS ---\nlocalhost:4770\n\n--- ENDPOINT ---\npkg.Svc/M\n\n--- REQUEST ---\n{\"sent\": true}\n\n--- RESPONSE ---\n{}\n\n#[skip]\n--- ASSERTS ---\n.never == 1\n\n#[skip]\n--- EXTRACT ---\nunused = .x\n",
        )
        .expect("write");

        let md = render_pages(&[file], None)
            .into_iter()
            .find(|p| p.name == "pkg.Svc.md")
            .expect("page")
            .markdown;

        assert!(md.contains("\"sent\": true"), "{md}");
        assert!(
            !md.contains(".never == 1"),
            "a skipped check is not documented: {md}"
        );
        assert!(
            !md.contains("unused"),
            "a skipped binding is not documented: {md}"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_file_on_another_wire_is_not_documented_with_grpcurl() {
        let dir = std::env::temp_dir().join(format!("docs-wire-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let file = dir.join("connect.gctf");
        std::fs::write(
            &file,
            "--- ADDRESS ---\nlocalhost:4770\n\n--- ENDPOINT ---\npkg.Svc/M\n\n--- OPTIONS ---\nprotocol: connectrpc\n\n--- REQUEST ---\n{}\n\n--- RESPONSE ---\n{}\n",
        )
        .expect("write");

        let md = render_pages(&[file], None)
            .into_iter()
            .find(|p| p.name == "pkg.Svc.md")
            .expect("page")
            .markdown;

        assert!(md.contains("--protocol connectrpc"), "{md}");
        assert!(!md.contains("grpcurl"), "{md}");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_documented_call_carries_the_certificates_the_file_dials_with() {
        let dir = std::env::temp_dir().join(format!("docs-tls-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let file = dir.join("secure.gctf");
        std::fs::write(
            &file,
            "--- ADDRESS ---\nlocalhost:4770\n\n--- ENDPOINT ---\npkg.Svc/M\n\n--- TLS ---\nca_cert: ca.pem\nclient_cert: client.pem\nclient_key: client.key\n\n--- REQUEST ---\n{}\n\n--- RESPONSE ---\n{}\n",
        )
        .expect("write");

        let md = render_pages(&[file], None)
            .into_iter()
            .find(|p| p.name == "pkg.Svc.md")
            .expect("page")
            .markdown;

        assert!(md.contains("--tls-ca "), "the call line: {md}");
        assert!(md.contains("--tls-cert "), "{md}");
        assert!(md.contains("--tls-key "), "{md}");
        assert!(md.contains("-cacert "), "the grpcurl line: {md}");
        assert!(md.contains("-cert "), "{md}");
        assert!(md.contains("-key "), "{md}");
        assert!(!md.contains("-plaintext"), "{md}");
        assert!(!md.contains("--plaintext"), "{md}");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_page_says_how_the_file_bounds_the_call() {
        let dir = std::env::temp_dir().join(format!("docs-runtime-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let file = dir.join("bounded.gctf");
        std::fs::write(
            &file,
            "--- ADDRESS ---\nlocalhost:4770\n\n--- ENDPOINT ---\npkg.Svc/M\n\n--- OPTIONS ---\ntimeout: 5\nretry: 3\n\n--- REQUEST ---\n{}\n\n--- RESPONSE ---\n{}\n",
        )
        .expect("write");

        let md = render_pages(&[file], None)
            .into_iter()
            .find(|p| p.name == "pkg.Svc.md")
            .expect("page")
            .markdown;

        assert!(
            md.contains("--max-time 5"),
            "the call line waits what the file waits: {md}"
        );
        assert!(
            md.contains("-max-time 5"),
            "and so does the grpcurl line: {md}"
        );
        assert!(md.contains("- timeout: `5 s`"), "{md}");
        assert!(md.contains("- retries: `3, 1 s apart`"), "{md}");
        assert!(md.contains("`grpctestify run` applies the rest"), "{md}");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_file_that_bounds_nothing_says_nothing_about_it() {
        let dir = std::env::temp_dir().join(format!("docs-unbounded-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let file = dir.join("plain.gctf");
        std::fs::write(
            &file,
            "--- ADDRESS ---\nlocalhost:4770\n\n--- ENDPOINT ---\npkg.Svc/M\n\n--- REQUEST ---\n{}\n\n--- RESPONSE ---\n{}\n",
        )
        .expect("write");

        let md = render_pages(&[file], None)
            .into_iter()
            .find(|p| p.name == "pkg.Svc.md")
            .expect("page")
            .markdown;

        assert!(!md.contains("Runs with:"), "{md}");
        assert!(!md.contains("max-time"), "{md}");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_dataset_is_documented_as_the_rows_it_runs() {
        let dir = std::env::temp_dir().join(format!("docs-rows-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let file = dir.join("rows.gctf");
        std::fs::write(
            &file,
            "--- ADDRESS ---\nlocalhost:4770\n\n--- ENDPOINT ---\npkg.Svc/Greet\n\n--- DATASET ---\n- who: Ada\n- who: Grace\n\n--- REQUEST ---\n{\"name\": \"{{dataset.who}}\"}\n\n--- RESPONSE ---\n{}\n",
        )
        .expect("write");

        let md = render_pages(&[file], None)
            .into_iter()
            .find(|p| p.name == "pkg.Svc.md")
            .expect("page")
            .markdown;

        assert!(md.contains("Runs once per `DATASET` row — 2 rows:"), "{md}");
        assert!(md.contains("| Ada |") && md.contains("| Grace |"), "{md}");
        assert!(md.contains("-d '{\"name\":\"Ada\"}'"), "{md}");
        assert!(!md.contains("-d '{\"name\":\"{{dataset.who}}\"}'"), "{md}");
        assert!(md.contains("\"{{dataset.who}}\""), "{md}");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_file_that_cannot_be_read_is_named() {
        let dir = std::env::temp_dir().join(format!("docs-unread-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(dir.join("broken.gctf"), "--- WAT ---\n").expect("write");
        std::fs::write(
            dir.join("ok.gctf"),
            "--- ADDRESS ---\nlocalhost:4770\n\n--- ENDPOINT ---\npkg.Svc/M\n\n--- REQUEST ---\n{}\n\n--- RESPONSE ---\n{}\n",
        )
        .expect("write");

        let pages = render_pages(&[dir.clone()], None);
        let index = pages
            .iter()
            .find(|p| p.name == "index.md")
            .expect("index")
            .markdown
            .clone();

        assert!(index.contains("## Not documented"), "{index}");
        assert!(index.contains("broken.gctf"), "{index}");
        assert!(
            index.contains("WAT"),
            "the reason, not just the name: {index}"
        );
        assert!(pages.iter().any(|p| p.name == "pkg.Svc.md"), "{index}");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_page_names_the_shape_and_what_a_shell_will_not_fill_in() {
        let dir = std::env::temp_dir().join(format!("docs-shape-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let file = dir.join("watch.gctf");
        std::fs::write(
            &file,
            "--- ADDRESS ---\nlocalhost:4770\n\n--- ENDPOINT ---\nfeed.Svc/Watch\n\n--- REQUEST_HEADERS ---\nauthorization: Bearer {{token}}\n\n--- REQUEST ---\n{}\n\n--- RESPONSE ---\n{\"n\": 1}\n{\"n\": 2}\n",
        )
        .expect("write");

        let md = render_pages(&[file], None)
            .into_iter()
            .find(|p| p.name == "feed.Svc.md")
            .expect("page")
            .markdown;

        assert!(md.contains("`feed.Svc/Watch` — server streaming"), "{md}");
        assert!(md.contains("still name `{{token}}`"), "{md}");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_shape_counts_the_messages_a_run_sends() {
        assert_eq!(call_shape(1, 1, false).as_deref(), Some("unary"));
        assert_eq!(call_shape(3, 1, false).as_deref(), Some("client streaming"));
        assert_eq!(call_shape(1, 4, false).as_deref(), Some("server streaming"));
        assert_eq!(
            call_shape(2, 2, false).as_deref(),
            Some("bidirectional streaming")
        );
        assert_eq!(
            call_shape(1, 0, true).as_deref(),
            Some("unary, expects an error")
        );
        assert_eq!(call_shape(1, 0, false), None);
    }

    #[test]
    fn the_names_a_line_still_carries_are_listed_once_in_order() {
        assert_eq!(
            unresolved_names(&["a {{one}} b {{two}}", "c {{one}}"]),
            vec!["one".to_string(), "two".to_string()],
        );
        assert!(unresolved_names(&["nothing here", "{{unclosed"]).is_empty());
    }

    #[test]
    fn a_path_is_filed_by_its_root_however_it_is_written() {
        assert_eq!(path_group("/v1/users"), "/v1");
        assert_eq!(path_group("v1/users"), "/v1");
        assert_eq!(path_group("/v1/users?full=true"), "/v1");
        assert_eq!(path_group("https://api.example.com/v1/users"), "/v1");
        assert_eq!(path_group("/data.json"), "/data.json");
        assert_eq!(path_group("/"), "/");
        assert_eq!(page_file("/v1"), "v1.md");
        assert_eq!(page_file("/"), "root.md");
        assert_eq!(page_file("pkg.Svc"), "pkg.Svc.md");
        assert_eq!(page_file("/a|b"), "a-b.md");
        assert_eq!(page_file("/v1/users"), "v1-users.md");
    }

    #[test]
    fn a_page_carries_the_headers_and_the_links_the_file_names() {
        let dir = std::env::temp_dir().join(format!("docs-meta-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let file = dir.join("m.gctf");
        std::fs::write(
            &file,
            "--- META ---\nname: Greeting a user\nlinks:\n  - https://tickets.example.com/PAY-14\n\n--- ADDRESS ---\nlocalhost:4770\n\n--- ENDPOINT ---\npkg.Svc/M\n\n--- REQUEST_HEADERS ---\nauthorization: Bearer {{token}}\nx-tenant: acme\n\n--- REQUEST ---\n{}\n\n--- RESPONSE ---\n{}\n",
        )
        .expect("write");

        let md = render_pages(&[file], None)
            .into_iter()
            .find(|p| p.name == "pkg.Svc.md")
            .expect("page")
            .markdown;

        assert!(
            md.contains("- <https://tickets.example.com/PAY-14>"),
            "{md}"
        );
        assert!(md.contains("- `authorization`: `Bearer {{token}}`"), "{md}");
        assert!(md.contains("- `x-tenant`: `acme`"), "{md}");
        assert!(
            md.contains("The lines above still name `{{token}}`"),
            "{md}"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn two_pages_never_claim_one_file() {
        let files = page_files(["/v1".to_string(), "v1".to_string(), "pkg.Svc".to_string()]);
        assert_eq!(files.get("/v1").map(String::as_str), Some("v1.md"));
        assert_eq!(files.get("v1").map(String::as_str), Some("v1-2.md"));
        assert_eq!(files.get("pkg.Svc").map(String::as_str), Some("pkg.Svc.md"));
        let mut names: Vec<&String> = files.values().collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), 3);
    }

    #[test]
    fn a_link_follows_the_file_the_page_was_written_as() {
        let dir = std::env::temp_dir().join(format!("docs-clash-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let file = dir.join("clash.apif");
        std::fs::write(
            &file,
            "--- ADDRESS ---\nhttp://127.0.0.1:8899\n\n--- ENDPOINT ---\nGET /v1/users\n\n--- EXTRACT ---\nid = .id\n\n--- ADDRESS ---\nlocalhost:4770\n\n--- ENDPOINT ---\nv1/Get\n\n--- REQUEST ---\n{\"id\": \"{{id}}\"}\n\n--- RESPONSE ---\n{}\n",
        )
        .expect("write");

        let pages = render_pages(&[file], None);
        let names: Vec<&String> = pages.iter().map(|p| &p.name).collect();
        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(names.len(), unique.len(), "one file per page: {names:?}");

        let index = pages.iter().find(|p| p.name == "index.md").expect("index");
        for page in &pages {
            for link in page.markdown.split("](").skip(1) {
                let Some(target) = link.split(')').next() else {
                    continue;
                };
                if !target.ends_with(".md") {
                    continue;
                }
                assert!(
                    pages.iter().any(|p| p.name == target),
                    "{target} is linked and not written: {names:?}"
                );
            }
        }
        assert!(index.markdown.contains(".md)"), "{}", index.markdown);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_value_cannot_break_the_markdown_it_is_written_into() {
        assert_eq!(md_code("plain"), "`plain`");
        assert_eq!(md_code("has `one` inside"), "``has `one` inside``");
        assert_eq!(md_code("``two``"), "``` ``two`` ```");
        assert_eq!(md_cell("a|b"), "a\\|b");
        assert_eq!(md_cell("line1\nline2"), "line1 line2");
    }

    #[test]
    fn a_nasty_value_still_writes_one_row_and_one_header() {
        let dir = std::env::temp_dir().join(format!("docs-nasty-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let file = dir.join("nasty.gctf");
        std::fs::write(
            &file,
            "--- ADDRESS ---\nlocalhost:4770\n\n--- ENDPOINT ---\npkg.Svc/M\n\n--- DATASET ---\n- who: \"a|b\"\n\n--- REQUEST_HEADERS ---\nx-weird: has `backtick` inside\n\n--- REQUEST ---\n{\"name\": \"{{dataset.who}}\"}\n\n--- RESPONSE ---\n{}\n",
        )
        .expect("write");

        let md = render_pages(&[file], None)
            .into_iter()
            .find(|p| p.name == "pkg.Svc.md")
            .expect("page")
            .markdown;

        assert!(md.contains("— 1 row:"), "one row is not `1 rows`: {md}");
        assert!(md.contains("| a\\|b |"), "{md}");
        assert!(md.contains("``has `backtick` inside``"), "{md}");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_name_from_the_file_cannot_break_the_page_that_lists_it() {
        let dir = std::env::temp_dir().join(format!("docs-names-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let file = dir.join("pipe.httf");
        std::fs::write(
            &file,
            "--- META ---\nname: \"two\nlines\"\n\n--- ADDRESS ---\nhttp://127.0.0.1:8899\n\n--- ENDPOINT ---\nGET /a|b/c\n\n--- ASSERTS ---\n@status() == 200\n",
        )
        .expect("write");

        let pages = render_pages(&[file], None);
        let index = pages
            .iter()
            .find(|p| p.name == "index.md")
            .expect("index")
            .markdown
            .clone();
        let page = pages.iter().find(|p| p.name != "index.md").expect("page");

        assert_eq!(page.name, "a-b.md", "a file name holds no pipe");
        assert!(index.contains("| [/a\\|b](a-b.md) |"), "{index}");
        assert!(page.markdown.contains("## two lines"), "{}", page.markdown);

        std::fs::remove_dir_all(&dir).ok();
    }
}
