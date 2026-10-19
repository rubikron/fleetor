//! **Nothing types into a pane unless someone sent it** (D-110's guard, D-124 G1).
//!
//! Every way to put bytes on a pane's pty, and every way app code can ask the hub to
//! deliver, is listed below with its exact production call sites. A new caller fails
//! here, and since this file is D-110's `guard:`, widening the list needs the
//! `ci-reviewed` label. Allowed today: the message path (`deliver.rs`, fed by the
//! hub), the operator's keystrokes (`pty_write`) and composer (`fleet_send`) and task
//! forms, and two terminal mechanics: colour replies (D-097) and the bring-up press
//! (D-098).
//!
//! It reads source, so it cannot see a call it has no needle for. The needles are
//! the pty's own write surface; reaching a pane any other way means adding one.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const ROOTS: [&str; 2] = ["src-tauri/src", "crates/fleetor-server/src"];

const ALLOWED: &[(&str, &[(&str, usize)])] = &[
    ("write_paste(", &[("src-tauri/src/deliver.rs", 1), ("src-tauri/src/pty.rs", 1)]),
    ("write_command(", &[("src-tauri/src/deliver.rs", 1), ("src-tauri/src/pty.rs", 1)]),
    ("type_framed(", &[("src-tauri/src/pty.rs", 3)]),
    ("take_writer(", &[("src-tauri/src/pty.rs", 1)]),
    ("write_all(", &[("src-tauri/src/pty.rs", 5)]),
    (".write(", &[("src-tauri/src/pty.rs", 1)]),
    (
        "AppCommand::Deliver",
        &[("crates/fleetor-server/src/hub.rs", 1), ("src-tauri/src/deliver.rs", 1)],
    ),
    (
        "AppCommand::Command",
        &[("crates/fleetor-server/src/hub.rs", 1), ("src-tauri/src/deliver.rs", 1)],
    ),
    (".handle(PaneId::", &[("src-tauri/src/fleet.rs", 2)]),
];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("src-tauri has a parent").to_path_buf()
}

/// The file without its `#[cfg(test)]` modules and without line comments.
fn production(src: &str) -> String {
    let mut out = String::new();
    let mut skipping: Option<usize> = None;
    let mut cfg_test = false;
    for line in src.lines() {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        if let Some(depth) = skipping {
            if trimmed == "}" && indent == depth {
                skipping = None;
            }
            continue;
        }
        if trimmed.starts_with("#[cfg(test)]") {
            cfg_test = true;
            continue;
        }
        if cfg_test && trimmed.starts_with("mod ") && trimmed.ends_with('{') {
            skipping = Some(indent);
            cfg_test = false;
            continue;
        }
        cfg_test = false;
        if !trimmed.starts_with("//") {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// `needle` → file → count, over production code.
fn sites(files: &[(String, String)]) -> BTreeMap<&'static str, BTreeMap<String, usize>> {
    let mut found = BTreeMap::new();
    for (needle, _) in ALLOWED {
        let per_file: BTreeMap<String, usize> = files
            .iter()
            .map(|(name, src)| (name.clone(), production(src).matches(needle).count()))
            .filter(|(_, n)| *n > 0)
            .collect();
        found.insert(*needle, per_file);
    }
    found
}

fn allowed() -> BTreeMap<&'static str, BTreeMap<String, usize>> {
    ALLOWED
        .iter()
        .map(|(needle, files)| (*needle, files.iter().map(|(f, n)| (f.to_string(), *n)).collect()))
        .collect()
}

fn rust_files(dir: &Path, root: &Path, out: &mut Vec<(String, String)>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, root, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            let name = path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
            out.push((name, std::fs::read_to_string(&path).unwrap()));
        }
    }
}

#[test]
fn only_the_allowed_paths_type_into_a_pane() {
    let root = repo_root();
    let mut files = Vec::new();
    for dir in ROOTS {
        rust_files(&root.join(dir), &root, &mut files);
    }
    let found = sites(&files);
    let mut diff = Vec::new();
    for (needle, expected) in allowed() {
        if found[needle] != expected {
            diff.push(format!("`{needle}`: allowed {expected:?}, found {:?}", found[needle]));
        }
    }
    assert!(
        diff.is_empty(),
        "a pane can now be typed into from somewhere new. If this is a launch-time path it \
         breaks D-110; if it only runs on a message someone sent, update ALLOWED (needs the \
         ci-reviewed label):\n{}",
        diff.join("\n")
    );
}

#[test]
fn the_check_fires_on_a_new_caller_and_ignores_tests_and_comments() {
    let base = "fn deliver() { registry.write_paste(pane, &body) }\n";
    let count = |src: &str| production(src).matches("write_paste(").count();
    assert_eq!(count(base), 1);
    assert_eq!(
        count(&format!("{base}fn on_launch() {{ r.write_paste(PaneId::Orch, \"go\") }}\n")),
        2
    );
    assert_eq!(count(&format!("{base}// r.write_paste(PaneId::Orch, \"go\")\n")), 1);
    assert_eq!(
        count(&format!(
            "{base}#[cfg(test)]\nmod tests {{\n    fn t() {{ r.write_paste(p, \"x\") }}\n}}\nfn after() {{ r.write_paste(p, \"y\") }}\n"
        )),
        2,
        "a test module is skipped, and only up to its closing brace"
    );
}
