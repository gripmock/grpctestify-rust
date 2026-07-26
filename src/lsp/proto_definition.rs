// LSP go-to-definition for proto service/method symbols in ENDPOINT sections.
//
// Only resolves when the descriptor pool was compiled from local `.proto`
// files (`PROTO.files=`) — that's the only source that keeps
// `SourceCodeInfo` (server-reflection-loaded pools have it stripped, see
// `apif_grpc_transport::tonic::descriptor::load_via_reflection`), so there is
// no source position to jump to for a reflection-only schema.

use crate::lsp::position::utf16_col_to_byte;
use crate::parser::GctfDocument;
use crate::parser::ast::SectionType;
use prost_reflect::{DescriptorPool, FileDescriptor};
use std::path::{Path, PathBuf};
use tower_lsp::lsp_types::{Location, Position, Range, Url};

struct EndpointSymbol {
    service_name: String,
    method_name: String,
    on_method: bool,
}

fn endpoint_symbol_at_position(
    doc: &GctfDocument,
    content: &str,
    position: Position,
) -> Option<EndpointSymbol> {
    let line0 = position.line as usize;
    let in_endpoint_section = doc.sections.iter().any(|section| {
        section.section_type == SectionType::Endpoint
            && section.start_line <= line0
            && line0 < section.end_line
    });
    if !in_endpoint_section {
        return None;
    }

    let line = content.lines().nth(line0)?;
    let trimmed = line.trim();
    let slash = trimmed.find('/')?;
    let service_name = trimmed[..slash].to_string();
    let method_name = trimmed[slash + 1..].trim().to_string();
    if service_name.is_empty() || method_name.is_empty() {
        return None;
    }

    let leading_ws = line.len() - line.trim_start().len();
    let slash_byte = leading_ws + slash;
    let byte_idx = utf16_col_to_byte(line, position.character as usize);

    Some(EndpointSymbol {
        service_name,
        method_name,
        on_method: byte_idx > slash_byte,
    })
}

fn resolve_proto_file_path(logical_name: &str, import_paths: &[String]) -> Option<PathBuf> {
    import_paths
        .iter()
        .map(|root| Path::new(root).join(logical_name))
        .find(|candidate| candidate.is_file())
}

fn location_in_file(
    parent_file: &FileDescriptor,
    path: &[i32],
    import_paths: &[String],
) -> Option<Location> {
    let proto = parent_file.file_descriptor_proto();
    let source_info = proto.source_code_info.as_ref()?;
    let location = source_info
        .location
        .iter()
        .find(|loc| loc.path.as_slice() == path)?;

    let file_path = resolve_proto_file_path(parent_file.name(), import_paths)?;
    let uri = Url::from_file_path(&file_path).ok()?;

    let (start_line, start_col, end_line, end_col) = match location.span.as_slice() {
        [start_line, start_col, end_line, end_col] => {
            (*start_line, *start_col, *end_line, *end_col)
        }
        [start_line, start_col, end_col] => (*start_line, *start_col, *start_line, *end_col),
        _ => return None,
    };

    Some(Location {
        uri,
        range: Range {
            start: Position {
                line: start_line as u32,
                character: start_col as u32,
            },
            end: Position {
                line: end_line as u32,
                character: end_col as u32,
            },
        },
    })
}

/// If `position` sits on the ENDPOINT line's `pkg.Service` or `Method` half,
/// resolve it against `pool` and return its declaration site in the local
/// `.proto` source. `import_paths` maps the pool's logical file name (e.g.
/// `myservice.proto`) back to a real path on disk. Returns `None` for
/// reflection-loaded pools (no `SourceCodeInfo`), an unresolved symbol, or a
/// proto file that can't be found under `import_paths`.
pub fn find_proto_definition(
    doc: &GctfDocument,
    content: &str,
    position: Position,
    pool: &DescriptorPool,
    import_paths: &[String],
) -> Option<Location> {
    let symbol = endpoint_symbol_at_position(doc, content, position)?;
    let service = pool.get_service_by_name(&symbol.service_name)?;

    if symbol.on_method {
        let method = service
            .methods()
            .find(|method| method.name() == symbol.method_name)?;
        location_in_file(&method.parent_file(), method.path(), import_paths)
    } else {
        location_in_file(&service.parent_file(), service.path(), import_paths)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser;
    use std::io::Write;

    const CONTENT: &str = "--- ADDRESS ---\nlocalhost:50051\n\n--- PROTO ---\nfiles=svc.proto\nimport_paths=.\n\n--- ENDPOINT ---\nexample.Greeter/SayHello\n\n--- REQUEST ---\n{}\n";

    fn build_pool(dir: &Path) -> DescriptorPool {
        let proto_path = dir.join("svc.proto");
        let mut f = std::fs::File::create(&proto_path).unwrap();
        writeln!(
            f,
            "syntax = \"proto3\";\npackage example;\n\nservice Greeter {{\n  rpc SayHello (HelloRequest) returns (HelloReply);\n}}\n\nmessage HelloRequest {{}}\nmessage HelloReply {{}}\n"
        )
        .unwrap();

        let fds = protox::compile([&proto_path], [dir]).expect("compile");
        let mut pool = DescriptorPool::new();
        pool.add_file_descriptor_set(fds).expect("add fds");
        pool
    }

    fn parse_doc() -> GctfDocument {
        parser::parse_gctf_from_str(CONTENT, "test.gctf").expect("parse")
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn resolves_method_definition_when_cursor_on_method_half() {
        let tmp = tempfile::tempdir().unwrap();
        let pool = build_pool(tmp.path());
        let doc = parse_doc();
        let import_paths = vec![tmp.path().to_string_lossy().to_string()];

        // Line 8 (0-based) is "example.Greeter/SayHello"; put the cursor on
        // "SayHello", past the '/'.
        let position = Position {
            line: 8,
            character: 20,
        };

        let loc = find_proto_definition(&doc, CONTENT, position, &pool, &import_paths)
            .expect("method definition found");
        assert!(loc.uri.path().ends_with("svc.proto"));
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn resolves_service_definition_when_cursor_on_service_half() {
        let tmp = tempfile::tempdir().unwrap();
        let pool = build_pool(tmp.path());
        let doc = parse_doc();
        let import_paths = vec![tmp.path().to_string_lossy().to_string()];

        let position = Position {
            line: 8,
            character: 2,
        };

        let loc = find_proto_definition(&doc, CONTENT, position, &pool, &import_paths)
            .expect("service definition found");
        assert!(loc.uri.path().ends_with("svc.proto"));
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn none_when_cursor_outside_endpoint_section() {
        let tmp = tempfile::tempdir().unwrap();
        let pool = build_pool(tmp.path());
        let doc = parse_doc();
        let import_paths = vec![tmp.path().to_string_lossy().to_string()];

        // Line 10 (0-based) is inside REQUEST, not ENDPOINT.
        let position = Position {
            line: 10,
            character: 0,
        };

        assert!(find_proto_definition(&doc, CONTENT, position, &pool, &import_paths).is_none());
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn none_when_source_code_info_is_stripped() {
        let tmp = tempfile::tempdir().unwrap();
        let mut pool = build_pool(tmp.path());
        // Simulate a reflection-loaded pool: rebuild without source info.
        let mut fds = pool.file_descriptor_protos().cloned().collect::<Vec<_>>();
        for f in &mut fds {
            f.source_code_info = None;
        }
        pool = DescriptorPool::new();
        for f in fds {
            pool.add_file_descriptor_proto(f).unwrap();
        }
        let doc = parse_doc();
        let import_paths = vec![tmp.path().to_string_lossy().to_string()];

        let position = Position {
            line: 8,
            character: 20,
        };

        assert!(find_proto_definition(&doc, CONTENT, position, &pool, &import_paths).is_none());
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn none_when_symbol_not_found_in_pool() {
        let tmp = tempfile::tempdir().unwrap();
        let pool = build_pool(tmp.path());
        let doc = parser::parse_gctf_from_str(
            "--- ENDPOINT ---\nexample.Unknown/Method\n\n--- REQUEST ---\n{}\n",
            "test.gctf",
        )
        .expect("parse");
        let import_paths = vec![tmp.path().to_string_lossy().to_string()];

        let position = Position {
            line: 1,
            character: 20,
        };

        assert!(find_proto_definition(&doc, "unused", position, &pool, &import_paths).is_none());
    }
}
