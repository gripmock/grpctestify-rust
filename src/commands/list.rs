// List command - list GCTF test files

use anyhow::Result;
use std::path::Path;
use tracing::error;

use crate::cli::args::ListArgs;
use crate::parser;
use crate::utils::FileUtils;

pub fn handle_list(args: &ListArgs) -> Result<()> {
    let path = args.path.as_deref().unwrap_or_else(|| Path::new("."));

    if !path.exists() {
        error!("Path not found: {}", path.display());
        std::process::exit(1);
    }

    let files = FileUtils::collect_test_files(path);

    if args.format == "json" {
        let tests: Vec<serde_json::Value> = files
            .iter()
            .map(|file| {
                let relative = file.strip_prefix(path).unwrap_or(file);
                let id = relative.to_string_lossy().replace('\\', "/");
                let label = file
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| id.clone());
                let uri = format!(
                    "file://{}",
                    file.canonicalize()
                        .unwrap_or_else(|_| file.to_path_buf())
                        .to_string_lossy()
                        .replace('\\', "/")
                );

                let mut test = serde_json::json!({
                    "id": id,
                    "label": label,
                    "uri": uri,
                    "children": []
                });

                if args.with_range
                    && let Ok(doc) = parser::parse_gctf(file)
                {
                    let line_count = doc
                        .metadata
                        .source
                        .as_ref()
                        .map(|s| s.lines().count())
                        .unwrap_or(1);
                    test["range"] = serde_json::json!({
                        "start": {"line": 1, "column": 1},
                        "end": {"line": line_count, "column": 1}
                    });
                }

                test
            })
            .collect();

        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({ "tests": tests }))?
        );
    } else {
        for file in &files {
            println!("{}", file.display());
        }
    }

    Ok(())
}
