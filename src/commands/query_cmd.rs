use crate::bench::sources::{
    SourceDefinition, SourceIndex, SourceReader, SourceRow, detect_format,
};
use crate::cli::args::QueryArgs;
use crate::parser::query_ast::{FilterExpr, parse_query};
use anyhow::{Context, Result, bail};
use rustyline::Editor;
use std::collections::HashMap;
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub fn handle_query(args: &QueryArgs) -> Result<()> {
    let mut sources = SourceCollection::new();

    for file in &args.files {
        if file.to_string_lossy() == "-" {
            let mut stdin = std::io::stdin();
            let mut content = String::new();
            stdin
                .read_to_string(&mut content)
                .context("Failed to read stdin")?;

            let ext = detect_format_from_content(&content);
            let source_name = "stdin".to_string();

            if let Some(format) = ext {
                sources.add_from_stdin(&source_name, &content, format)?;
            } else {
                anyhow::bail!("Could not detect format from stdin content");
            }
        } else if file.is_dir() {
            for entry in WalkDir::new(file)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("gctf"))
            {
                load_gctf_file(entry.path(), &mut sources)?;
            }
        } else if file.extension().and_then(|s| s.to_str()) == Some("gctf") {
            load_gctf_file(file, &mut sources)?;
        } else {
            let name = file
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            sources.add_direct_file(&name, file, args.indexed_by.as_deref())?;
        }
    }

    match decide_query_action(!args.files.is_empty(), args.query.is_some(), args.shell) {
        QueryAction::Shell => run_shell(sources, args)?,
        QueryAction::Execute => {
            // `Execute` is only chosen when a query is present.
            let query = args.query.as_deref().unwrap_or_default();
            execute_query(query, &sources, args)?;
        }
        QueryAction::Preview => preview_sources(&sources)?,
    }

    Ok(())
}

/// What `query` should do given the presence of file args, a `-q` query, and
/// the `-s/--shell` flag.
#[derive(Debug, PartialEq, Eq)]
enum QueryAction {
    /// Start the interactive shell.
    Shell,
    /// Execute the provided `-q` query.
    Execute,
    /// Files given but no query and no shell: preview the loaded sources.
    Preview,
}

fn decide_query_action(has_files: bool, has_query: bool, shell: bool) -> QueryAction {
    if shell {
        // Explicit `-s/--shell` always starts the shell (sources preloaded).
        QueryAction::Shell
    } else if has_query {
        QueryAction::Execute
    } else if !has_files {
        // No files, no query, no shell flag: default to the interactive shell.
        QueryAction::Shell
    } else {
        // Files given but nothing to run: preview schema + sample rows so the
        // command is never a silent no-op.
        QueryAction::Preview
    }
}

/// Print each loaded source's schema and a few sample rows. Used when files are
/// given without a `-q` query or `-s/--shell` flag, so `query <file>` is
/// informative rather than a silent success.
fn preview_sources(sources: &SourceCollection) -> Result<()> {
    let mut names = sources.list_sources();
    names.sort();
    if names.is_empty() {
        bail!("no sources loaded (use -q to run a query or -s for the interactive shell)");
    }

    for name in names {
        let Some(source) = sources.get(name) else {
            continue;
        };
        let columns = source.columns();
        println!(
            "Source '{}' ({} columns): {}",
            name,
            columns.len(),
            columns.join(", ")
        );
        let rows = source.scan(&[])?;
        let preview = rows.len().min(5);
        println!("Showing {} of {} rows:", preview, rows.len());
        print_rows(&rows[..preview], &columns, "table", true);
        println!();
    }

    println!("(no query provided — use -q <expr> to filter or -s for the interactive shell)");
    Ok(())
}

fn detect_format_from_content(content: &str) -> Option<crate::bench::sources::SourceFormat> {
    let first_line = content.lines().next()?;
    if first_line.contains('\t') && !first_line.contains(',') {
        Some(crate::bench::sources::SourceFormat::Tsv)
    } else if first_line.trim().starts_with('{') {
        // Could be NDJSON - check if lines look like JSON objects
        if content
            .lines()
            .all(|l| l.trim().is_empty() || l.trim().starts_with('{'))
        {
            Some(crate::bench::sources::SourceFormat::Ndjson)
        } else {
            None
        }
    } else if first_line.contains(',') {
        Some(crate::bench::sources::SourceFormat::Csv)
    } else {
        None
    }
}

fn load_gctf_file(path: &Path, sources: &mut SourceCollection) -> Result<()> {
    let ast = crate::parser::parse_gctf(path)
        .with_context(|| format!("failed to parse {}", path.display()))?;

    for section in &ast.sections {
        if section.section_type == crate::parser::ast::SectionType::Bench
            && let crate::parser::ast::SectionContent::KeyValues(kv) = &section.content
            && let Some(sources_yaml) = kv.get("sources")
            && let Ok(defs) = serde_yaml_ng::from_str::<Vec<SourceDefinition>>(sources_yaml)
        {
            for def in defs {
                sources.add_from_definition(path, def)?;
            }
        }
    }

    Ok(())
}

struct ShellState {
    output: Option<PathBuf>,
    format: String,
    show_headers: bool,
}

fn run_shell(mut sources: SourceCollection, args: &QueryArgs) -> Result<()> {
    println!("grpctestify query shell");
    println!("Type '.help' for commands, '.quit' to exit\n");

    let mut shell_state = ShellState {
        output: None,
        format: args.format.clone(),
        show_headers: !args.no_header,
    };

    let mut rl = Editor::<(), rustyline::history::FileHistory>::new()
        .map_err(|e| anyhow::anyhow!("Failed to initialize editor: {}", e))?;

    loop {
        let readline = rl.readline("query> ");
        match readline {
            Ok(line) => {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                let _ = rl.add_history_entry(line);

                match process_shell_line(line, &mut sources, &mut shell_state) {
                    Ok(true) => break,
                    Ok(false) => {}
                    Err(e) => eprintln!("Error: {}", e),
                }
            }
            Err(rustyline::error::ReadlineError::Eof) => break,
            Err(e) => {
                eprintln!("Error: {}", e);
                break;
            }
        }
    }

    Ok(())
}

fn process_shell_line(
    line: &str,
    sources: &mut SourceCollection,
    shell_state: &mut ShellState,
) -> Result<bool> {
    if line.starts_with('.') {
        return process_meta_command(line, sources, shell_state);
    }

    execute_query_with_output(line, sources, shell_state)?;
    Ok(false)
}

fn process_meta_command(
    line: &str,
    sources: &mut SourceCollection,
    shell_state: &mut ShellState,
) -> Result<bool> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    let cmd = parts.first().unwrap_or(&"");

    match *cmd {
        ".help" => {
            println!("Commands:");
            println!("  .tables              List loaded sources");
            println!("  .schema <source>    Show source schema");
            println!("  .indexes <source>   Show indexes");
            println!("  .info               Show session info");
            println!("  .count <source>     Count rows in source");
            println!("  .sample <source> [n] Show n sample rows (default 5)");
            println!("  .load <file.gctf>  Load sources from GCTF");
            println!("  .add <name> <file> [-i column]  Add direct file");
            println!("  .remove <name>     Remove source");
            println!("  .mode <format>     Set output format (json|csv|table|line|tsv)");
            println!("  .headers <on|off>   Toggle headers");
            println!("  .output <file>      Set output file (format from ext)");
            println!("  .quit, .exit       Exit shell");
            println!();
            println!("Query syntax:");
            println!("  <source> [filter_expr]");
            println!();
            println!("Filter operators:");
            println!("  key=value           equals");
            println!("  key!=value          not equals");
            println!("  key>=value          greater or equal");
            println!("  key<=value          less or equal");
            println!("  key>value           greater");
            println!("  key<value           less");
            println!("  key~glob*           LIKE glob pattern");
            println!("  key~re:pattern      regex match");
            println!("  key=v1,v2,v3        IN (equals any)");
            Ok(false)
        }

        ".quit" | ".exit" => Ok(true),

        ".tables" | ".sources" => {
            let names = sources.list_sources();
            if names.is_empty() {
                println!("No sources loaded. Use .load or .add to add sources.");
            } else {
                for name in names {
                    println!("  {}", name);
                }
            }
            Ok(false)
        }

        ".schema" => {
            let name = parts.get(1).unwrap_or(&"");
            if let Some(source) = sources.get(name) {
                println!("Schema for '{}':", name);
                for col in source.columns() {
                    println!("  {}", col);
                }
            } else {
                bail!("source '{}' not found", name);
            }
            Ok(false)
        }

        ".indexes" => {
            let name = parts.get(1).unwrap_or(&"");
            if let Some(source) = sources.get(name) {
                if let Some(info) = source.index_info() {
                    println!("Index for '{}':", name);
                    println!("  Key column: {}", info.key_column);
                    println!("  Entries: {}", info.entries);
                } else {
                    println!("No index for '{}'", name);
                }
            } else {
                bail!("source '{}' not found", name);
            }
            Ok(false)
        }

        ".load" => {
            let path = parts.get(1).unwrap_or(&"");
            if path.is_empty() {
                bail!("usage: .load <file.gctf>");
            }
            let path = PathBuf::from(path);
            load_gctf_file(&path, sources)?;
            println!("Loaded sources from {}", path.display());
            Ok(false)
        }

        ".add" => {
            let name = parts.get(1).unwrap_or(&"");
            let file = parts.get(2).unwrap_or(&"");
            if name.is_empty() || file.is_empty() {
                bail!("usage: .add <name> <file> [-i column]");
            }
            let file_path = PathBuf::from(file);
            sources.add_direct_file(name, &file_path, None)?;
            println!("Added '{}' from {}", name, file);
            Ok(false)
        }

        ".remove" => {
            let name = parts.get(1).unwrap_or(&"");
            if name.is_empty() {
                bail!("usage: .remove <name>");
            }
            sources.remove(name);
            println!("Removed '{}'", name);
            Ok(false)
        }

        ".mode" => {
            let format = parts.get(1).unwrap_or(&"");
            if !format.is_empty() {
                shell_state.format = format.to_string();
            }
            println!("Output format: {}", shell_state.format);
            Ok(false)
        }

        ".headers" => {
            let setting = parts.get(1).unwrap_or(&"");
            match *setting {
                "off" | "no" | "false" => {
                    shell_state.show_headers = false;
                }
                _ => {
                    shell_state.show_headers = true;
                }
            }
            println!(
                "Headers: {}",
                if shell_state.show_headers {
                    "on"
                } else {
                    "off"
                }
            );
            Ok(false)
        }

        ".output" => {
            let path = parts.get(1).unwrap_or(&"");
            if path.is_empty() {
                shell_state.output = None;
                println!("Output cleared (console only)");
            } else {
                shell_state.output = Some(PathBuf::from(path));
                println!("Output: {}", path);
            }
            Ok(false)
        }

        ".count" => {
            let name = parts.get(1).unwrap_or(&"");
            if name.is_empty() {
                bail!("usage: .count <source>");
            }
            if let Some(source) = sources.get(name) {
                let rows = source.scan(&[])?;
                println!("{}", rows.len());
            } else {
                bail!("source '{}' not found", name);
            }
            Ok(false)
        }

        ".sample" => {
            let name = parts.get(1).unwrap_or(&"");
            let n: usize = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(5);
            if name.is_empty() {
                bail!("usage: .sample <source> [n]");
            }
            if let Some(source) = sources.get(name) {
                let rows = source.scan(&[])?;
                let count = rows.len().min(n);
                println!("Showing {} of {} rows:", count, rows.len());
                let columns = source.columns();
                print_rows(&rows[..count], &columns, "table", true);
            } else {
                bail!("source '{}' not found", name);
            }
            Ok(false)
        }

        ".info" => {
            println!("Sources: {}", sources.list_sources().len());
            for name in sources.list_sources() {
                if let Some(source) = sources.get(name) {
                    let cols = source.columns();
                    let info = source.index_info();
                    let idx_info = if let Some(i) = info {
                        format!(" (indexed on {}, {} entries)", i.key_column, i.entries)
                    } else {
                        String::new()
                    };
                    println!("  {}: {} columns{}", name, cols.len(), idx_info);
                }
            }
            Ok(false)
        }

        _ => {
            bail!("unknown command: {}", cmd);
        }
    }
}

fn execute_query(query: &str, sources: &SourceCollection, args: &QueryArgs) -> Result<()> {
    let parsed = parse_query(query)?;

    let source = sources
        .get(&parsed.source)
        .ok_or_else(|| anyhow::anyhow!("source '{}' not found", parsed.source))?;

    let mut rows = source.scan(&parsed.filters)?;

    if let Some(ref order_by) = args.order_by {
        let (col, desc) = if let Some(rest) = order_by.strip_prefix('-') {
            (rest, true)
        } else {
            (order_by.as_str(), false)
        };
        rows.sort_by(|a, b| {
            let empty = String::new();
            let a_val = a.get(col).unwrap_or(&empty);
            let b_val = b.get(col).unwrap_or(&empty);
            if desc {
                b_val.cmp(a_val)
            } else {
                a_val.cmp(b_val)
            }
        });
    }

    let limit = args.limit.unwrap_or(100);
    let offset = args.offset.unwrap_or(0);

    let rows: Vec<_> = rows.into_iter().skip(offset).take(limit).collect();

    let columns = args
        .columns
        .as_ref()
        .map(|c| c.split(',').map(|s| s.trim().to_string()).collect())
        .unwrap_or_else(|| source.columns());

    if let Some(ref output_path) = args.output {
        let ext = output_path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        let format = match ext {
            "csv" => "csv",
            "tsv" => "tsv",
            "ndjson" | "jsonl" => "json",
            "json" => "json",
            _ => &args.format,
        };
        let output = std::fs::File::create(output_path)?;
        let mut buf_writer = std::io::BufWriter::new(output);
        write_rows_to_output(&rows, &columns, format, !args.no_header, &mut buf_writer)?;
        buf_writer.flush()?;
        println!("Saved {} rows to {}", rows.len(), output_path.display());
    } else {
        print_rows(&rows, &columns, &args.format, !args.no_header);
    }

    Ok(())
}

fn execute_query_with_output(
    query: &str,
    sources: &SourceCollection,
    shell_state: &mut ShellState,
) -> Result<()> {
    let parsed = parse_query(query)?;

    let source = sources
        .get(&parsed.source)
        .ok_or_else(|| anyhow::anyhow!("source '{}' not found", parsed.source))?;

    let rows = source.scan(&parsed.filters)?;

    let columns = source.columns();

    if let Some(ref output_path) = shell_state.output {
        let ext = output_path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        let format = match ext {
            "csv" => "csv",
            "tsv" => "tsv",
            "ndjson" | "jsonl" => "json",
            "json" => "json",
            _ => &shell_state.format,
        };
        let output = std::fs::File::create(output_path)?;
        let mut buf_writer = std::io::BufWriter::new(output);
        write_rows_to_output(
            &rows,
            &columns,
            format,
            shell_state.show_headers,
            &mut buf_writer,
        )?;
        buf_writer.flush()?;
        println!("Saved {} rows to {}", rows.len(), output_path.display());
    } else {
        print_rows(
            &rows,
            &columns,
            &shell_state.format,
            shell_state.show_headers,
        );
    }

    Ok(())
}

fn write_rows_to_output(
    rows: &[HashMap<String, String>],
    columns: &[String],
    format: &str,
    show_header: bool,
    writer: &mut dyn std::io::Write,
) -> Result<()> {
    match format {
        "json" => {
            for row in rows {
                let mut obj = serde_json::Map::new();
                for col in columns {
                    if let Some(val) = row.get(col) {
                        obj.insert(col.clone(), serde_json::Value::String(val.clone()));
                    }
                }
                writeln!(writer, "{}", serde_json::to_string(&obj)?)?;
            }
        }
        "csv" => {
            if show_header {
                writeln!(writer, "{}", columns.join(","))?;
            }
            for row in rows {
                let vals: Vec<String> = columns
                    .iter()
                    .map(|col| row.get(col).unwrap_or(&"".to_string()).to_string())
                    .collect();
                writeln!(writer, "{}", vals.join(","))?;
            }
        }
        "tsv" => {
            if show_header {
                writeln!(writer, "{}", columns.join("\t"))?;
            }
            for row in rows {
                let vals: Vec<String> = columns
                    .iter()
                    .map(|col| row.get(col).unwrap_or(&"".to_string()).to_string())
                    .collect();
                writeln!(writer, "{}", vals.join("\t"))?;
            }
        }
        "line" => {
            for row in rows {
                let pairs: Vec<String> = columns
                    .iter()
                    .map(|col| {
                        let empty = String::new();
                        let val = row.get(col).unwrap_or(&empty);
                        format!("{}={}", col, val)
                    })
                    .collect();
                writeln!(writer, "{}", pairs.join(" "))?;
            }
        }
        _ => {
            // table format
            let widths: Vec<usize> = columns
                .iter()
                .map(|col| {
                    let max_val = rows
                        .iter()
                        .filter_map(|r| r.get(col))
                        .map(|v| v.len())
                        .max()
                        .unwrap_or(0);
                    col.len().max(max_val)
                })
                .collect();

            let sep = || {
                widths
                    .iter()
                    .map(|w| "-".repeat(*w + 2))
                    .collect::<Vec<_>>()
                    .join("+")
            };

            if show_header {
                writeln!(writer, "+{}+", sep())?;
                let header: Vec<String> = columns
                    .iter()
                    .enumerate()
                    .map(|(i, col)| format!(" {:<width$} ", col, width = widths[i]))
                    .collect();
                writeln!(writer, "|{}|", header.join("|"))?;
                writeln!(writer, "+{}+", sep())?;
            }

            for row in rows {
                let vals: Vec<String> = columns
                    .iter()
                    .enumerate()
                    .map(|(i, col)| {
                        format!(
                            " {:<width$} ",
                            row.get(col).unwrap_or(&"".to_string()),
                            width = widths[i]
                        )
                    })
                    .collect();
                writeln!(writer, "|{}|", vals.join("|"))?;
            }

            if show_header {
                writeln!(writer, "+{}+", sep())?;
                writeln!(writer, "({} rows)", rows.len())?;
            }
        }
    }
    Ok(())
}

struct SourceCollection {
    sources: HashMap<String, Box<dyn QuerySource>>,
}

impl SourceCollection {
    fn new() -> Self {
        Self {
            sources: HashMap::new(),
        }
    }

    fn list_sources(&self) -> Vec<&String> {
        self.sources.keys().collect()
    }

    fn get(&self, name: &str) -> Option<&dyn QuerySource> {
        self.sources.get(name).map(|s| s.as_ref())
    }

    fn add_from_definition(&mut self, doc_path: &Path, def: SourceDefinition) -> Result<()> {
        let name = def.name.clone().unwrap_or_else(|| {
            Path::new(&def.file)
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string()
        });

        let resolved = resolve_relative_path(doc_path, &def.file);

        let source: Box<dyn QuerySource> = if def.indexed_by.is_some() {
            let idx_path = resolved.with_extension("gcti");
            if idx_path.exists() {
                let index = SourceIndex::read_from_file(&idx_path)?;
                Box::new(IndexedSource {
                    index,
                    path: resolved,
                })
            } else {
                Box::new(StreamingSource { path: resolved })
            }
        } else {
            Box::new(StreamingSource { path: resolved })
        };

        self.sources.insert(name, source);
        Ok(())
    }

    fn add_direct_file(&mut self, name: &str, path: &Path, index_col: Option<&str>) -> Result<()> {
        let source: Box<dyn QuerySource> = if index_col.is_some() {
            let idx_path = path.with_extension("gcti");
            if idx_path.exists() {
                let index = SourceIndex::read_from_file(&idx_path)?;
                Box::new(IndexedSource {
                    index,
                    path: path.to_path_buf(),
                })
            } else {
                Box::new(DirectFileSource {
                    path: path.to_path_buf(),
                })
            }
        } else {
            Box::new(DirectFileSource {
                path: path.to_path_buf(),
            })
        };

        self.sources.insert(name.to_string(), source);
        Ok(())
    }

    fn add_from_stdin(
        &mut self,
        name: &str,
        content: &str,
        format: crate::bench::sources::SourceFormat,
    ) -> Result<()> {
        let source: Box<dyn QuerySource> = Box::new(StdinSource {
            content: content.to_string(),
            format,
        });
        self.sources.insert(name.to_string(), source);
        Ok(())
    }

    fn remove(&mut self, name: &str) {
        self.sources.remove(name);
    }
}

trait QuerySource {
    fn columns(&self) -> Vec<String>;
    fn scan(&self, filters: &[FilterExpr]) -> Result<Vec<HashMap<String, String>>>;
    fn index_info(&self) -> Option<IndexInfo>;
}

struct IndexInfo {
    key_column: String,
    entries: usize,
}

struct IndexedSource {
    index: SourceIndex,
    path: PathBuf,
}

impl QuerySource for IndexedSource {
    fn columns(&self) -> Vec<String> {
        vec![self.index.key_column().to_string()]
    }

    fn scan(&self, filters: &[FilterExpr]) -> Result<Vec<HashMap<String, String>>> {
        let mut reader = open_source_reader_from_path(&self.path)?;
        let mut results = Vec::new();
        let headers = reader.headers().to_vec();

        while let Some(row) = reader.next_row()? {
            let map = row_to_map(&headers, &row);
            if filters.iter().all(|f| f.matches(&map)) {
                results.push(map);
            }
        }

        Ok(results)
    }

    fn index_info(&self) -> Option<IndexInfo> {
        Some(IndexInfo {
            key_column: self.index.key_column().to_string(),
            entries: self.index.entry_count() as usize,
        })
    }
}

struct StreamingSource {
    path: PathBuf,
}

impl QuerySource for StreamingSource {
    fn columns(&self) -> Vec<String> {
        let mut reader = open_source_reader_from_path(&self.path).ok();
        if let Some(ref mut r) = reader {
            r.headers().to_vec()
        } else {
            vec![]
        }
    }

    fn scan(&self, filters: &[FilterExpr]) -> Result<Vec<HashMap<String, String>>> {
        let mut reader = open_source_reader_from_path(&self.path)?;
        let mut results = Vec::new();
        let headers = reader.headers().to_vec();

        while let Some(row) = reader.next_row()? {
            let map = row_to_map(&headers, &row);
            if filters.iter().all(|f| f.matches(&map)) {
                results.push(map);
            }
        }

        Ok(results)
    }

    fn index_info(&self) -> Option<IndexInfo> {
        None
    }
}

struct DirectFileSource {
    path: PathBuf,
}

impl QuerySource for DirectFileSource {
    fn columns(&self) -> Vec<String> {
        let mut reader = match open_source_reader_from_path(&self.path).ok() {
            Some(r) => r,
            None => return vec![],
        };
        // For NdjsonReader, we need to read the first row to discover headers
        if reader.headers().is_empty() {
            let _ = reader.next_row();
        }
        reader.headers().to_vec()
    }

    fn scan(&self, filters: &[FilterExpr]) -> Result<Vec<HashMap<String, String>>> {
        let mut reader = open_source_reader_from_path(&self.path)?;
        let mut results = Vec::new();
        let mut headers = reader.headers().to_vec();

        // For NdjsonReader, headers are populated after first next_row() call
        if headers.is_empty()
            && let Some(row) = reader.next_row()?
        {
            headers = reader.headers().to_vec();
            let map = row_to_map(&headers, &row);
            if filters.iter().all(|f| f.matches(&map)) {
                results.push(map);
            }
        }

        while let Some(row) = reader.next_row()? {
            let map = row_to_map(&headers, &row);
            if filters.iter().all(|f| f.matches(&map)) {
                results.push(map);
            }
        }

        Ok(results)
    }

    fn index_info(&self) -> Option<IndexInfo> {
        None
    }
}

type ParsedContent = (Vec<String>, Vec<HashMap<String, String>>);

struct StdinSource {
    content: String,
    format: crate::bench::sources::SourceFormat,
}

impl StdinSource {
    fn parse_content(&self) -> Result<ParsedContent> {
        let reader = BufReader::new(self.content.as_bytes());
        match self.format {
            crate::bench::sources::SourceFormat::Csv => {
                let mut csv_reader = crate::bench::sources::CsvReader::new(reader, b',')?;
                let headers = csv_reader.headers().to_vec();
                let mut rows = Vec::new();
                while let Some(row) = csv_reader.next_row()? {
                    rows.push(row_to_map(&headers, &row));
                }
                Ok((headers, rows))
            }
            crate::bench::sources::SourceFormat::Tsv => {
                let mut tsv_reader = crate::bench::sources::TsvReader::new(reader)?;
                let headers = tsv_reader.headers().to_vec();
                let mut rows = Vec::new();
                while let Some(row) = tsv_reader.next_row()? {
                    rows.push(row_to_map(&headers, &row));
                }
                Ok((headers, rows))
            }
            crate::bench::sources::SourceFormat::Ndjson => {
                let mut ndjson_reader = crate::bench::sources::NdjsonReader::new(reader);
                let mut rows = Vec::new();
                while let Some(row) = ndjson_reader.next_row()? {
                    rows.push(row);
                }
                let headers = ndjson_reader.headers().to_vec();
                let rows_with_map: Vec<HashMap<String, String>> =
                    rows.into_iter().map(|r| row_to_map(&headers, &r)).collect();
                Ok((headers, rows_with_map))
            }
        }
    }
}

impl QuerySource for StdinSource {
    fn columns(&self) -> Vec<String> {
        self.parse_content().map(|(h, _)| h).unwrap_or_default()
    }

    fn scan(&self, filters: &[FilterExpr]) -> Result<Vec<HashMap<String, String>>> {
        let (_, rows) = self.parse_content()?;
        let results = rows
            .into_iter()
            .filter(|row| filters.iter().all(|f| f.matches(row)))
            .collect();
        Ok(results)
    }

    fn index_info(&self) -> Option<IndexInfo> {
        None
    }
}

fn open_source_reader_from_path(path: &Path) -> Result<Box<dyn SourceReader>> {
    let file =
        std::fs::File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let reader = BufReader::new(file);
    let format = detect_format(path)?;
    match format {
        crate::bench::sources::SourceFormat::Csv => Ok(Box::new(
            crate::bench::sources::CsvReader::new(reader, b',')?,
        )),
        crate::bench::sources::SourceFormat::Tsv => {
            Ok(Box::new(crate::bench::sources::TsvReader::new(reader)?))
        }
        crate::bench::sources::SourceFormat::Ndjson => {
            Ok(Box::new(crate::bench::sources::NdjsonReader::new(reader)))
        }
    }
}

fn row_to_map(headers: &[String], row: &SourceRow) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for col in headers {
        if let Some(val) = row.get(col) {
            map.insert(col.clone(), val.to_string());
        }
    }
    map
}

fn resolve_relative_path(doc_path: &Path, file: &str) -> PathBuf {
    if Path::new(file).is_absolute() {
        PathBuf::from(file)
    } else {
        doc_path.parent().unwrap_or(Path::new(".")).join(file)
    }
}

fn print_rows(
    rows: &[HashMap<String, String>],
    columns: &[String],
    format: &str,
    show_header: bool,
) {
    match format {
        "json" => print_json(rows, columns),
        "csv" => print_csv(rows, columns, show_header),
        "tsv" => print_tsv(rows, columns, show_header),
        "line" => print_line(rows, columns),
        _ => print_table(rows, columns, show_header),
    }
}

fn print_json(rows: &[HashMap<String, String>], columns: &[String]) {
    for row in rows {
        let mut obj = serde_json::Map::new();
        for col in columns {
            if let Some(val) = row.get(col) {
                obj.insert(col.clone(), serde_json::Value::String(val.clone()));
            }
        }
        println!("{}", serde_json::to_string(&obj).unwrap());
    }
}

fn print_csv(rows: &[HashMap<String, String>], columns: &[String], show_header: bool) {
    if show_header {
        println!("{}", columns.join(","));
    }
    for row in rows {
        let vals: Vec<String> = columns
            .iter()
            .map(|col| row.get(col).unwrap_or(&"".to_string()).to_string())
            .collect();
        println!("{}", vals.join(","));
    }
}

fn print_tsv(rows: &[HashMap<String, String>], columns: &[String], show_header: bool) {
    if show_header {
        println!("{}", columns.join("\t"));
    }
    for row in rows {
        let vals: Vec<String> = columns
            .iter()
            .map(|col| row.get(col).unwrap_or(&"".to_string()).to_string())
            .collect();
        println!("{}", vals.join("\t"));
    }
}

fn print_line(rows: &[HashMap<String, String>], columns: &[String]) {
    for row in rows {
        let pairs: Vec<String> = columns
            .iter()
            .map(|col| {
                let empty = String::new();
                let val = row.get(col).unwrap_or(&empty);
                format!("{}={}", col, val)
            })
            .collect();
        println!("{}", pairs.join(" "));
    }
}

fn print_table(rows: &[HashMap<String, String>], columns: &[String], show_header: bool) {
    let widths: Vec<usize> = columns
        .iter()
        .map(|col| {
            let max_val = rows
                .iter()
                .filter_map(|r| r.get(col))
                .map(|v| v.len())
                .max()
                .unwrap_or(0);
            col.len().max(max_val)
        })
        .collect();

    let sep = || {
        widths
            .iter()
            .map(|w| "-".repeat(*w + 2))
            .collect::<Vec<_>>()
            .join("+")
    };

    if show_header {
        println!("+{}+", sep());
        let header: Vec<String> = columns
            .iter()
            .enumerate()
            .map(|(i, col)| format!(" {:<width$} ", col, width = widths[i]))
            .collect();
        println!("|{}|", header.join("|"));
        println!("+{}+", sep());
    }

    for row in rows {
        let vals: Vec<String> = columns
            .iter()
            .enumerate()
            .map(|(i, col)| {
                format!(
                    " {:<width$} ",
                    row.get(col).unwrap_or(&"".to_string()),
                    width = widths[i]
                )
            })
            .collect();
        println!("|{}|", vals.join("|"));
    }

    if show_header {
        println!("+{}+", sep());
        println!("({} rows)", rows.len());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_flag_forces_shell() {
        // -s wins regardless of files/query.
        assert_eq!(decide_query_action(false, false, true), QueryAction::Shell);
        assert_eq!(decide_query_action(true, false, true), QueryAction::Shell);
        assert_eq!(decide_query_action(true, true, true), QueryAction::Shell);
    }

    #[test]
    fn no_args_defaults_to_shell() {
        assert_eq!(decide_query_action(false, false, false), QueryAction::Shell);
    }

    #[test]
    fn query_executes() {
        assert_eq!(
            decide_query_action(false, true, false),
            QueryAction::Execute
        );
        assert_eq!(decide_query_action(true, true, false), QueryAction::Execute);
    }

    #[test]
    fn file_without_query_previews() {
        // `query <file>` with no -q and no -s must not be a silent no-op.
        assert_eq!(
            decide_query_action(true, false, false),
            QueryAction::Preview
        );
    }
}
