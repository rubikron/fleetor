//! **The two indexes agree with what they index** (D-124, guards G2 and G4).
//!
//! G2: every `decisions/D-XXX.md` has one `INDEX.md` row, its frontmatter `id` is its
//! filename, and its `status` is the row's. G4: every file under `docs/` is named in
//! `docs/README.md`, or for `docs/roadmap/`, in `roadmap/00-index.md` (D-057).
//!
//! Each check is a function over strings, and the last test runs each against a
//! source that breaks it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("src-tauri has a parent").to_path_buf()
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("{} must exist to be checked: {e}", path.display()))
}

// --- G2: decisions ------------------------------------------------------------

/// `| [D-001](D-001.md) | meta | alive | title |` → `D-001 → (D-001.md, alive)`.
fn index_rows(index: &str) -> BTreeMap<String, (String, String)> {
    let mut rows = BTreeMap::new();
    for line in index.lines() {
        let cells: Vec<&str> = line.split('|').map(str::trim).collect();
        let Some(link) = cells.get(1).and_then(|c| c.strip_prefix('[')) else { continue };
        let Some((id, target)) = link.split_once("](") else { continue };
        let (Some(target), Some(status)) = (target.strip_suffix(')'), cells.get(3)) else {
            continue;
        };
        rows.insert(id.to_string(), (target.to_string(), status.to_string()));
    }
    rows
}

fn frontmatter_field(text: &str, key: &str) -> Option<String> {
    let body = text.strip_prefix("---\n")?.split("\n---").next()?;
    body.lines()
        .find_map(|l| l.strip_prefix(key)?.strip_prefix(':'))
        .map(|v| v.trim().trim_matches('"').to_string())
}

/// Every way `files` (stem, contents) and the index disagree.
fn decision_problems(index: &str, files: &[(String, String)]) -> Vec<String> {
    let rows = index_rows(index);
    let mut problems = Vec::new();
    for (stem, text) in files {
        if frontmatter_field(text, "id").as_deref() != Some(stem.as_str()) {
            problems.push(format!("{stem}.md: frontmatter `id` is not `{stem}`"));
        }
        let status = frontmatter_field(text, "status");
        match rows.get(stem) {
            None => problems.push(format!("{stem}.md has no row in INDEX.md")),
            Some((target, _)) if *target != format!("{stem}.md") => {
                problems.push(format!("{stem}'s row links `{target}`"))
            }
            Some((_, row)) if status.as_deref() != Some(row.as_str()) => problems
                .push(format!("{stem}: frontmatter status {status:?} but INDEX.md says `{row}`")),
            Some(_) => {}
        }
    }
    for id in rows.keys() {
        if !files.iter().any(|(stem, _)| stem == id) {
            problems.push(format!("INDEX.md has a row for {id} but no {id}.md"));
        }
    }
    problems
}

#[test]
fn every_decision_has_one_index_row_with_its_status() {
    let dir = repo_root().join("decisions");
    let mut files = Vec::new();
    for entry in std::fs::read_dir(&dir).expect("decisions/ exists") {
        let path = entry.unwrap().path();
        let stem = path.file_stem().unwrap().to_string_lossy().to_string();
        if stem.starts_with("D-") && path.extension().is_some_and(|e| e == "md") {
            files.push((stem, read(&path)));
        }
    }
    assert!(files.len() > 100, "found only {} decision files; is this the right dir?", files.len());
    let problems = decision_problems(&read(&dir.join("INDEX.md")), &files);
    assert!(problems.is_empty(), "decisions/ and INDEX.md disagree:\n{}", problems.join("\n"));
}

// --- G4: docs -----------------------------------------------------------------

/// The docs (paths relative to `docs/`) that neither index names.
fn unlisted(docs: &[String], readme: &str, roadmap_index: &str) -> Vec<String> {
    docs.iter()
        .filter(|rel| {
            let name = rel.rsplit('/').next().unwrap();
            let in_roadmap = rel.starts_with("roadmap/") && roadmap_index.contains(name);
            rel.as_str() != "README.md" && !readme.contains(name) && !in_roadmap
        })
        .cloned()
        .collect()
}

fn walk(dir: &Path, base: &Path, out: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.file_name().unwrap().to_string_lossy().starts_with('.') {
            continue;
        }
        if path.is_dir() {
            walk(&path, base, out);
        } else {
            out.push(path.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/"));
        }
    }
}

#[test]
fn every_doc_is_named_in_its_index() {
    let docs_dir = repo_root().join("docs");
    let mut docs = Vec::new();
    walk(&docs_dir, &docs_dir, &mut docs);
    assert!(docs.len() > 20, "found only {} docs; is this the right dir?", docs.len());
    let missing = unlisted(
        &docs,
        &read(&docs_dir.join("README.md")),
        &read(&docs_dir.join("roadmap/00-index.md")),
    );
    assert!(
        missing.is_empty(),
        "not named in docs/README.md (or roadmap/00-index.md), so they don't exist (D-057):\n{}",
        missing.join("\n")
    );
}

// --- the negative control -----------------------------------------------------

#[test]
fn the_checks_fire_on_a_source_that_violates_them() {
    let decision = |id: &str, status: &str| {
        (id.to_string(), format!("---\nid: {id}\ntitle: \"t\"\nstatus: {status}\n---\n\nbody"))
    };
    let index = "| ID | Phase | Status | Title |\n|----|----|----|----|\n\
                 | [D-001](D-001.md) | meta | alive | one |\n\
                 | [D-002](D-002.md) | meta | reversed | two |\n";
    assert!(decision_problems(index, &[decision("D-001", "alive"), decision("D-002", "reversed")])
        .is_empty());

    let cases = [
        (vec![decision("D-001", "alive")], "a row with no file"),
        (vec![decision("D-001", "alive"), decision("D-002", "alive")], "a status mismatch"),
        (
            vec![
                decision("D-001", "alive"),
                decision("D-002", "reversed"),
                decision("D-003", "alive"),
            ],
            "a file with no row",
        ),
        (
            vec![decision("D-001", "alive"), ("D-002".into(), decision("D-009", "reversed").1)],
            "an id that is not the filename",
        ),
    ];
    for (files, what) in cases {
        assert!(!decision_problems(index, &files).is_empty(), "G2 missed {what}");
    }

    let docs =
        ["README.md".to_string(), "a.md".into(), "notes/b.md".into(), "roadmap/01-c.md".into()];
    assert!(unlisted(&docs, "[`a.md`](./a.md) `b.md`", "01-c.md").is_empty());
    assert_eq!(unlisted(&docs, "[`a.md`](./a.md)", "01-c.md"), ["notes/b.md"]);
    assert_eq!(unlisted(&docs, "a.md b.md", ""), ["roadmap/01-c.md"]);
}
