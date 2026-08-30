use anyhow::Result;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::cli::args::GraphArgs;
use crate::commands::run::DirFixtures;
use crate::parser;
use crate::utils::FileUtils;

pub fn handle_graph(args: &GraphArgs) -> Result<()> {
    let paths: Vec<PathBuf> = if args.paths.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        args.paths.clone()
    };

    let mut files = Vec::new();
    for path in &paths {
        if path.is_dir() {
            files.extend(FileUtils::collect_test_files(path, &[]));
        } else if path.is_file() {
            files.push(path.clone());
        }
    }
    files.sort();
    files.dedup();

    if files.is_empty() {
        println!("No .gctf test files found.");
        return Ok(());
    }

    let by_dir = group_by_directory(files);

    if args.format.eq_ignore_ascii_case("mermaid") {
        print!("{}", render_mermaid(&by_dir));
    } else {
        print!("{}", render_text(&by_dir));
    }
    Ok(())
}

struct DirGroup {
    tests: Vec<PathBuf>,
    fixtures: DirFixtures,
}

fn group_by_directory(files: Vec<PathBuf>) -> BTreeMap<PathBuf, DirGroup> {
    let (tests, mut fixtures) = crate::commands::run::partition_fixtures(files);

    let mut by_dir: BTreeMap<PathBuf, DirGroup> = BTreeMap::new();
    for test in tests {
        let dir = test.parent().map(Path::to_path_buf).unwrap_or_default();
        by_dir
            .entry(dir)
            .or_insert_with(|| DirGroup {
                tests: Vec::new(),
                fixtures: DirFixtures::default(),
            })
            .tests
            .push(test);
    }
    for (dir, fx) in fixtures.drain() {
        by_dir
            .entry(dir)
            .or_insert_with(|| DirGroup {
                tests: Vec::new(),
                fixtures: DirFixtures::default(),
            })
            .fixtures = fx;
    }
    for group in by_dir.values_mut() {
        group.tests.sort();
    }
    by_dir
}

fn chain_steps(path: &Path) -> Vec<String> {
    let Ok(doc) = parser::parse_gctf(path) else {
        return Vec::new();
    };
    doc.iter_chain()
        .map(|d| {
            if d.transport() == crate::parser::ast::Transport::Http {
                return d
                    .parse_http_endpoint()
                    .map(|(method, path)| format!("{method} {path}"))
                    .unwrap_or_else(|| "?".to_string());
            }
            d.parse_endpoint()
                .map(|(_, _, method)| method)
                .unwrap_or_else(|| "?".to_string())
        })
        .collect()
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string())
}

fn node_label(path: &Path) -> String {
    let name = file_name(path);
    let steps = chain_steps(path);
    if steps.len() > 1 {
        format!("{name} ({})", steps.join(" → "))
    } else {
        name
    }
}

fn render_text(by_dir: &BTreeMap<PathBuf, DirGroup>) -> String {
    let mut out = String::new();
    for (dir, group) in by_dir {
        let dir_display = if dir.as_os_str().is_empty() {
            ".".to_string()
        } else {
            dir.display().to_string()
        };
        out.push_str(&dir_display);
        out.push('\n');

        let mut rows: Vec<(String, bool)> = Vec::new();
        if let Some(setup) = &group.fixtures.setup {
            rows.push((format!("{} (setup)", node_label(setup)), false));
        }
        for test in &group.tests {
            rows.push((node_label(test), false));
        }
        if let Some(teardown) = &group.fixtures.teardown {
            rows.push((format!("{} (teardown)", node_label(teardown)), false));
        }

        let last = rows.len().saturating_sub(1);
        for (i, (label, _)) in rows.iter().enumerate() {
            let branch = if i == last { "└─ " } else { "├─ " };
            out.push_str(branch);
            out.push_str(label);
            out.push('\n');
        }
        out.push('\n');
    }
    out
}

fn render_mermaid(by_dir: &BTreeMap<PathBuf, DirGroup>) -> String {
    let mut out = String::from("```mermaid\nflowchart TD\n");
    let mut next_id = 0usize;
    let mut id = || {
        let n = next_id;
        next_id += 1;
        format!("n{n}")
    };

    for (dir, group) in by_dir {
        let dir_display = if dir.as_os_str().is_empty() {
            ".".to_string()
        } else {
            dir.display().to_string()
        };
        let subgraph_id = format!("s{}", out.matches("subgraph").count());
        out.push_str(&format!(
            "  subgraph {subgraph_id}[\"{}\"]\n",
            mermaid_escape(&dir_display)
        ));

        let setup_id = group.fixtures.setup.as_ref().map(|p| {
            let node = id();
            out.push_str(&format!(
                "    {node}[\"{}\"]\n",
                mermaid_escape(&node_label(p))
            ));
            node
        });
        let test_ids: Vec<String> = group
            .tests
            .iter()
            .map(|p| {
                let node = id();
                out.push_str(&format!(
                    "    {node}[\"{}\"]\n",
                    mermaid_escape(&node_label(p))
                ));
                node
            })
            .collect();
        let teardown_id = group.fixtures.teardown.as_ref().map(|p| {
            let node = id();
            out.push_str(&format!(
                "    {node}[\"{}\"]\n",
                mermaid_escape(&node_label(p))
            ));
            node
        });

        if let Some(setup) = &setup_id {
            for t in &test_ids {
                out.push_str(&format!("    {setup} --> {t}\n"));
            }
        }
        if let Some(teardown) = &teardown_id {
            for t in &test_ids {
                out.push_str(&format!("    {t} --> {teardown}\n"));
            }
            if test_ids.is_empty()
                && let Some(setup) = &setup_id
            {
                out.push_str(&format!("    {setup} --> {teardown}\n"));
            }
        }
        out.push_str("  end\n");
    }
    out.push_str("```\n");
    out
}

fn mermaid_escape(s: &str) -> String {
    s.replace('"', "&quot;")
}

#[cfg(all(test, not(miri)))]
mod tests {
    use super::*;

    #[test]
    fn render_text_orders_setup_tests_teardown() {
        let mut by_dir = BTreeMap::new();
        by_dir.insert(
            PathBuf::from("tests/users"),
            DirGroup {
                tests: vec![
                    PathBuf::from("tests/users/get.gctf"),
                    PathBuf::from("tests/users/list.gctf"),
                ],
                fixtures: DirFixtures {
                    setup: Some(PathBuf::from("tests/users/_setup.gctf")),
                    teardown: Some(PathBuf::from("tests/users/_teardown.gctf")),
                },
            },
        );
        let text = render_text(&by_dir);
        let setup_pos = text.find("_setup.gctf (setup)").unwrap();
        let get_pos = text.find("get.gctf").unwrap();
        let list_pos = text.find("list.gctf").unwrap();
        let teardown_pos = text.find("_teardown.gctf (teardown)").unwrap();
        assert!(setup_pos < get_pos);
        assert!(get_pos < list_pos);
        assert!(list_pos < teardown_pos);
        assert!(text.trim_end().ends_with("_teardown.gctf (teardown)"));
    }

    #[test]
    fn render_text_directory_without_fixtures_lists_tests_only() {
        let mut by_dir = BTreeMap::new();
        by_dir.insert(
            PathBuf::from("tests/plain"),
            DirGroup {
                tests: vec![PathBuf::from("tests/plain/a.gctf")],
                fixtures: DirFixtures::default(),
            },
        );
        let text = render_text(&by_dir);
        assert!(!text.contains("setup"));
        assert!(!text.contains("teardown"));
        assert!(text.contains("a.gctf"));
    }

    #[test]
    fn render_mermaid_links_setup_to_tests_to_teardown() {
        let mut by_dir = BTreeMap::new();
        by_dir.insert(
            PathBuf::from("tests/users"),
            DirGroup {
                tests: vec![PathBuf::from("tests/users/get.gctf")],
                fixtures: DirFixtures {
                    setup: Some(PathBuf::from("tests/users/_setup.gctf")),
                    teardown: Some(PathBuf::from("tests/users/_teardown.gctf")),
                },
            },
        );
        let mermaid = render_mermaid(&by_dir);
        assert!(mermaid.starts_with("```mermaid\nflowchart TD\n"));
        assert!(mermaid.contains("n0 --> n1"));
        assert!(mermaid.contains("n1 --> n2"));
        assert!(mermaid.trim_end().ends_with("```"));
    }

    #[test]
    fn render_mermaid_setup_links_directly_to_teardown_when_no_tests() {
        let mut by_dir = BTreeMap::new();
        by_dir.insert(
            PathBuf::from("tests/empty"),
            DirGroup {
                tests: Vec::new(),
                fixtures: DirFixtures {
                    setup: Some(PathBuf::from("tests/empty/_setup.gctf")),
                    teardown: Some(PathBuf::from("tests/empty/_teardown.gctf")),
                },
            },
        );
        let mermaid = render_mermaid(&by_dir);
        assert!(mermaid.contains("n0 --> n1"));
    }

    #[test]
    fn node_label_annotates_multi_step_chains() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("chain.gctf");
        std::fs::write(
            &file,
            "--- ENDPOINT ---\nshop.Cart/AddItem\n\n--- REQUEST ---\n{}\n\n--- RESPONSE ---\n{}\n\n--- ENDPOINT ---\nshop.Cart/Checkout\n\n--- REQUEST ---\n{}\n\n--- RESPONSE ---\n{}\n",
        )
        .unwrap();
        let label = node_label(&file);
        assert_eq!(label, "chain.gctf (AddItem → Checkout)");
    }

    #[test]
    fn node_label_names_the_steps_of_an_http_chain() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("chain.httf");
        std::fs::write(
            &file,
            "--- ADDRESS ---\nhttp://api.example.com\n\n--- ENDPOINT ---\nGET /v1/users\n\n--- ASSERTS ---\n@status() == 200\n\n--- ENDPOINT ---\nPOST /v1/users/7/orders\n\n--- ASSERTS ---\n@status() == 201\n",
        )
        .unwrap();

        assert_eq!(
            node_label(&file),
            "chain.httf (GET /v1/users → POST /v1/users/7/orders)",
        );
    }

    #[test]
    fn node_label_single_document_is_bare_name() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("single.gctf");
        std::fs::write(
            &file,
            "--- ENDPOINT ---\nshop.Cart/AddItem\n\n--- REQUEST ---\n{}\n\n--- RESPONSE ---\n{}\n",
        )
        .unwrap();
        assert_eq!(node_label(&file), "single.gctf");
    }
}
