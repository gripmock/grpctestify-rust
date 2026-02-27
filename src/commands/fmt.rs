// Fmt command - format GCTF files

use anyhow::Result;
use tracing::{error, warn};

use crate::cli::args::FmtArgs;
use crate::parser;
use crate::utils::FileUtils;

pub async fn handle_fmt(args: &FmtArgs) -> Result<()> {
    let mut files = Vec::new();
    let mut has_error = false;

    for path in &args.files {
        if path.is_dir() {
            files.extend(FileUtils::collect_test_files(path));
        } else if path.is_file() {
            files.push(path.clone());
        } else {
            error!("Path not found: {}", path.display());
            has_error = true;
        }
    }

    if files.is_empty() {
        if !has_error {
            warn!("No .gctf files found to format");
        }
        return Ok(());
    }

    for file in files {
        // Parse
        let doc = match parser::parse_gctf(&file) {
            Ok(d) => d,
            Err(e) => {
                error!("Failed to parse {}: {}", file.display(), e);
                has_error = true;
                continue;
            }
        };

        // Format/Serialize
        let formatted = crate::serialize_gctf(&doc);

        if args.write {
            // Read original content to compare
            let original = std::fs::read_to_string(&file).unwrap_or_default();

            // Only write if content changed (idempotent check)
            if formatted != original
                && let Err(e) = std::fs::write(&file, &formatted)
            {
                error!("Failed to write {}: {}", file.display(), e);
                has_error = true;
            }
            // Silent success - standard fmt behavior
            // If content unchanged, no output (idempotent)
        } else {
            println!("{}", formatted);
        }
    }

    if has_error {
        std::process::exit(1);
    }

    Ok(())
}
