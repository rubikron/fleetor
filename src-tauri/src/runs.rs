//! Past runs as long-term memory (WP-11, D-058).
//!
//! A **run** is one fleet from start to teardown, and it owns a database. The
//! live one is always `~/.fleetor/_shell/state.db`; every earlier one is a
//! frozen `~/.fleetor/runs/<id>/state.db` that nothing appends to. That is why a
//! new run cannot be corrupted by an old one: they never share a file, so the
//! isolation is physical rather than a `WHERE` clause somebody has to remember
//! to write (D-058 records the design that lost).
//!
//! **The boundary is cut at start, not at teardown.** Teardown is the thing that
//! fails — `orphans.rs` exists because a force-quit skips it — and a crash under
//! rotate-on-stop would silently reopen the previous run and append to it, which
//! is exactly the bug this module removes. So [`rotate`] runs at the top of
//! `fleet_bootstrap`, before the store is opened and long before any pane or
//! socket exists. Nothing here is on the message path and nothing here may ever
//! acquire a way to be (Tier 1.4).
//!
//! **Rotation may not fail a boot.** A history feature that stops the app
//! starting is worse than the bug it fixes, so every failure path here degrades:
//! freeze-and-move, else move the three files untouched, else leave them alone
//! and put a `Warn` on the feed. The operator loses history, never a fleet.
//!
//! The index is a **cache, not the truth.** `runs/index.json` supplies labels;
//! the directories supply existence. [`list`] scans and merges, so a lost or
//! corrupt index costs labels and nothing else — which is also why a run's id is
//! its directory name and its label is a separate mutable string. Renaming is a
//! JSON edit; it never moves a file.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use fleetor_core::event::{FleetEvent, NoticeLevel};
use fleetor_db::archive;
use serde::{Deserialize, Serialize};

/// Milliseconds in a day — the unit the civil-date conversion counts in.
const MS_PER_DAY: i64 = 86_400_000;

/// The agent-facing export, written into every run at archive time.
const EVENTS_JSON: &str = "events.json";
/// What the run is and what else is in its directory, for a reader arriving cold.
const MANIFEST_JSON: &str = "manifest.json";

/// One past run, as the History view lists it.
///
/// Everything except `label` is derived from the archived log and re-derivable
/// by deleting the index. `label` is the only thing a human authored, which is
/// the whole reason the index exists.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunRecord {
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_ms: Option<i64>,
    pub events: i64,
    pub messages: i64,
    pub tasks: i64,
    pub bytes: u64,
    /// How many pane transcripts were archived with the run, `orch`'s included
    /// (see [`harvest_transcripts`]). `0` is ordinary — a run whose panes never
    /// started has none.
    #[serde(default)]
    pub transcripts: u32,
}

/// What the live run knows about itself before it has a log worth reading:
/// when it started and what it was pointed at. Written at bootstrap, archived
/// with the run it describes.
///
/// The target cannot be recovered from the event log — it appears there only as
/// prose inside a boot notice — so a run that ended without this file lists with
/// an unknown target rather than a parsed guess.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct LiveMeta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    started_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    target: Option<String>,
}

// --- locations ----------------------------------------------------------------

/// The archive root. A sibling of `_shell/` rather than a child, because
/// `_shell` is explicitly the disposable working state and this is the one thing
/// under `~/.fleetor` the operator might mind losing.
pub(crate) fn runs_dir(fleetor: &Path) -> PathBuf {
    fleetor.join("runs")
}

fn index_path(runs: &Path) -> PathBuf {
    runs.join("index.json")
}

fn live_db(shell: &Path) -> PathBuf {
    shell.join("state.db")
}

fn live_meta(shell: &Path) -> PathBuf {
    shell.join("run.json")
}

// --- the live run -------------------------------------------------------------

/// Record what the run that is starting now is pointed at, for the History list
/// it will appear in after the *next* start.
///
/// Best-effort on purpose: a failure here costs one row's target column, and is
/// not worth failing a boot over.
pub fn begin(shell: &Path, started_ms: i64, target: &Path) {
    let meta = LiveMeta {
        started_ms: Some(started_ms),
        target: Some(target.display().to_string()),
    };
    if let Ok(text) = serde_json::to_string_pretty(&meta) {
        let _ = std::fs::write(live_meta(shell), text);
    }
}

// --- rotation -----------------------------------------------------------------

/// Archive the previous run, if there is one, and leave `_shell` ready for a
/// fresh database. Returns the notices the caller should put on the feed.
///
/// Called before the store is opened. Never returns `Err`: see the module doc.
pub fn rotate(shell: &Path, runs: &Path, now_ms: i64) -> Vec<(NoticeLevel, String)> {
    let live = live_db(shell);
    if !live.is_file() {
        return Vec::new(); // first run on this machine — nothing to archive
    }

    let meta = read_live_meta(shell);
    let started = meta.started_ms.or_else(|| file_started_ms(&live)).unwrap_or(now_ms);
    let dest = match reserve(runs, &timestamp_id(started)) {
        Ok(dir) => dir,
        Err(e) => return vec![(NoticeLevel::Warn, format!("could not archive the previous run: {e}"))],
    };

    let mut notices = Vec::new();
    if let Err(e) = archive_files(&live, &dest) {
        let _ = std::fs::remove_dir(&dest);
        return vec![(
            NoticeLevel::Warn,
            format!(
                "the previous run was left in place, not archived ({e}). \
                 Its log is still at {} and the fleet is starting on top of it.",
                live.display()
            ),
        )];
    }
    let _ = std::fs::remove_file(live_meta(shell));
    let transcripts = harvest_transcripts(shell, &dest);

    let id = dest.file_name().unwrap_or_default().to_string_lossy().to_string();
    match record_for(&dest, &id, meta.target.as_deref()) {
        Ok(mut record) => {
            record.transcripts = transcripts;
            if let Err(e) = write_agent_view(&dest, &record) {
                notices.push((
                    NoticeLevel::Warn,
                    format!("archived run {id}, but its JSON export did not write: {e}"),
                ));
            }
            let label = record.label.clone();
            if let Err(e) = upsert(runs, record) {
                notices.push((NoticeLevel::Warn, format!("archived run {id}, but its index entry did not save: {e}")));
            } else {
                notices.push((NoticeLevel::Info, format!("previous run archived as “{label}” — see History")));
            }
        }
        Err(e) => notices
            .push((NoticeLevel::Warn, format!("archived run {id}, but could not read it back: {e}"))),
    }
    notices
}

/// Freeze the live database and move it, leaving one self-contained file.
///
/// The fallback is not decoration. Freezing opens the database, and a database
/// that cannot be opened — corrupt, or held by something that outlived its
/// process — must still be archivable, because those are exactly the runs whose
/// evidence is worth keeping. Moving all three files preserves it byte for byte
/// (`docs/notes/run-rotation-notes.md`, strategy C).
fn archive_files(live: &Path, dest: &Path) -> std::io::Result<()> {
    if archive::freeze(live).is_ok() {
        std::fs::rename(live, dest.join("state.db"))?;
        for suffix in ["-wal", "-shm"] {
            let _ = std::fs::remove_file(with_suffix(live, suffix));
        }
        return Ok(());
    }
    std::fs::rename(live, dest.join("state.db"))?;
    for suffix in ["-wal", "-shm"] {
        let from = with_suffix(live, suffix);
        if from.is_file() {
            std::fs::rename(&from, dest.join(format!("state.db{suffix}")))?;
        }
    }
    Ok(())
}

/// Move every pane's Claude Code transcript into the run it belongs to.
///
/// The event log records what the panes said **to each other**; a transcript
/// records what one pane actually did — the tool calls, the reasoning, the work
/// between two messages. An evaluator reading a past run wants both, and only
/// one of them was being kept.
///
/// **Moved, not copied**, for two reasons. Transcripts live in a per-pane
/// config dir that outlives a run, so copying would leave last run's sessions
/// sitting beside this run's and every archive would accumulate its
/// predecessors. And moving is what makes the split clean: each run's directory
/// holds that run's transcripts and no others. Safe because rotation runs before
/// any pane exists, and panes are spawned fresh every time — nothing resumes a
/// previous session.
///
/// **`orch` is included, and this function did not have to learn about it**
/// (WP-14, D-062). It was absent while `orch` ran on the operator's own
/// `CLAUDE_CONFIG_DIR` — outside `~/.fleetor`, where reaching in would cross
/// Tier 1.1. `orch` now has a fleet-owned config dir under the same
/// `pane-config/` root a worker's lives in, so the scan below finds it by the
/// name it already walks and files it under `transcripts/orch/`.
fn harvest_transcripts(shell: &Path, dest: &Path) -> u32 {
    let Ok(panes) = std::fs::read_dir(shell.join("pane-config")) else { return 0 };
    let mut moved = 0;

    for pane in panes.flatten() {
        let name = pane.file_name();
        let Ok(slugs) = std::fs::read_dir(pane.path().join("projects")) else { continue };
        let into = dest.join("transcripts").join(&name);

        for slug in slugs.flatten() {
            let Ok(files) = std::fs::read_dir(slug.path()) else { continue };
            for file in files.flatten() {
                let from = file.path();
                if from.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                    continue;
                }
                if std::fs::create_dir_all(&into).is_err() {
                    continue;
                }
                if std::fs::rename(&from, into.join(file.file_name())).is_ok() {
                    moved += 1;
                }
            }
        }
    }
    moved
}

/// Write the two files an agent reads: the log as JSON, and a manifest saying
/// what is in the directory.
///
/// Written **at archive time**, not generated on demand by an export button.
/// The reader this is for is a `claude -p` with a shell, and it should be able
/// to `cat` a run without SQLite, without this app running, and without knowing
/// that a GUI exists.
fn write_agent_view(dest: &Path, record: &RunRecord) -> std::io::Result<()> {
    let json = fleetor_db::archive::to_json(&dest.join("state.db"))
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    std::fs::write(dest.join(EVENTS_JSON), json)?;

    let manifest = serde_json::json!({
        "run": record,
        "layout": {
            "events.json": "the whole event log, one JSON array, oldest first; `seq` and `ts` are the row's own columns",
            "state.db": "the same log as SQLite — the source of truth events.json is generated from",
            "transcripts/": "one directory per pane — orch and each worker — holding that pane's Claude Code session .jsonl files for this run only",
        },
        "reading_this": "The event log is what the panes said to each other. The transcripts are what each pane did between saying things. Neither records terminal output.",
    });
    std::fs::write(
        dest.join(MANIFEST_JSON),
        serde_json::to_string_pretty(&manifest).map_err(|e| std::io::Error::other(e.to_string()))?,
    )
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    path.with_file_name(name)
}

/// Claim an unused directory for `id`, disambiguating rather than overwriting.
/// Two runs can share a start second — a fast restart, or a clock that did not
/// move — and silently merging them would lose one.
fn reserve(runs: &Path, id: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(runs)?;
    for attempt in 0..100 {
        let name = if attempt == 0 { id.to_string() } else { format!("{id}-{}", attempt + 1) };
        let dir = runs.join(&name);
        match std::fs::create_dir(&dir) {
            Ok(()) => return Ok(dir),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::other(format!("{id} and 99 variants of it are all taken")))
}

// --- the index ----------------------------------------------------------------

/// Every archived run, newest first.
///
/// The directories are authoritative: an indexed run whose directory is gone is
/// dropped, and a directory the index has never heard of is read and added. So
/// deleting `index.json` costs labels and nothing else.
pub fn list(runs: &Path) -> Vec<RunRecord> {
    let mut indexed = read_index(runs);
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(runs) else { return out };

    let mut healed = false;
    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.join("state.db").is_file() {
            continue;
        }
        let id = entry.file_name().to_string_lossy().to_string();
        match indexed.remove(&id) {
            Some(mut record) => {
                record.bytes = dir_bytes(&dir);
                out.push(record);
            }
            None => {
                if let Ok(record) = record_for(&dir, &id, None) {
                    healed = true;
                    out.push(record);
                }
            }
        }
    }

    out.sort_by(|a, b| b.started_ms.cmp(&a.started_ms).then_with(|| b.id.cmp(&a.id)));
    if healed || !indexed.is_empty() {
        let _ = write_index(runs, &out);
    }
    out
}

/// Give a run a new label. The only mutable thing about an archive.
pub fn rename(runs: &Path, id: &str, label: &str) -> Result<(), String> {
    let label = label.trim();
    if label.is_empty() {
        return Err("a run label cannot be empty".into());
    }
    let mut all = list(runs);
    let found = all.iter_mut().find(|r| r.id == id).ok_or_else(|| format!("no run {id}"))?;
    found.label = label.to_string();
    write_index(runs, &all).map_err(|e| e.to_string())
}

/// Delete a run and everything in its directory.
pub fn delete(runs: &Path, id: &str) -> Result<(), String> {
    let dir = run_dir(runs, id)?;
    std::fs::remove_dir_all(&dir).map_err(|e| format!("removing {}: {e}", dir.display()))?;
    let remaining = list(runs);
    write_index(runs, &remaining).map_err(|e| e.to_string())
}

/// Replay one archived run.
pub fn events(runs: &Path, id: &str, after: i64) -> Result<Vec<(i64, FleetEvent)>, String> {
    let dir = run_dir(runs, id)?;
    archive::events(&dir.join("state.db"), after).map_err(|e| e.to_string())
}

/// Resolve `id` to a directory, refusing anything that is not a plain name.
/// The id reaches this module from the webview, so `..` has to die here rather
/// than be assumed impossible upstream.
fn run_dir(runs: &Path, id: &str) -> Result<PathBuf, String> {
    let plain = !id.is_empty()
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == ':');
    if !plain {
        return Err(format!("{id:?} is not a run id"));
    }
    let dir = runs.join(id);
    if !dir.join("state.db").is_file() {
        return Err(format!("no run {id}"));
    }
    Ok(dir)
}

fn record_for(dir: &Path, id: &str, target: Option<&str>) -> Result<RunRecord, String> {
    let digest = archive::digest(&dir.join("state.db")).map_err(|e| e.to_string())?;
    Ok(RunRecord {
        label: suggest_label(target, digest.headline.as_deref()),
        id: id.to_string(),
        target: target.map(str::to_string),
        started_ms: digest.first_ts,
        ended_ms: digest.last_ts,
        events: digest.events,
        messages: digest.messages,
        tasks: digest.tasks,
        bytes: dir_bytes(dir),
        transcripts: count_transcripts(dir),
    })
}

/// How many transcripts the run's directory actually holds, so a rebuilt index
/// reports what is on disk rather than what rotation once returned.
fn count_transcripts(dir: &Path) -> u32 {
    let Ok(panes) = std::fs::read_dir(dir.join("transcripts")) else { return 0 };
    panes
        .flatten()
        .filter_map(|pane| std::fs::read_dir(pane.path()).ok())
        .map(|files| files.flatten().count() as u32)
        .sum()
}

/// What the run gets called until the operator calls it something else.
///
/// A run with no messages and no blocks is named as such rather than given a
/// plausible title — the counts in the list are what make it obviously
/// deletable, and inventing a name for an empty run hides that.
fn suggest_label(target: Option<&str>, headline: Option<&str>) -> String {
    let repo = target
        .map(Path::new)
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_string());
    match (repo, headline) {
        (Some(repo), Some(head)) => format!("{repo} — {head}"),
        (Some(repo), None) => format!("{repo} — no activity"),
        (None, Some(head)) => head.to_string(),
        (None, None) => "no activity".to_string(),
    }
}

fn read_index(runs: &Path) -> BTreeMap<String, RunRecord> {
    std::fs::read_to_string(index_path(runs))
        .ok()
        .and_then(|text| serde_json::from_str::<Vec<RunRecord>>(&text).ok())
        .map(|rows| rows.into_iter().map(|r| (r.id.clone(), r)).collect())
        .unwrap_or_default()
}

/// Add a freshly archived run to the index without disturbing the rows already
/// there — including any the operator has renamed.
fn upsert(runs: &Path, record: RunRecord) -> std::io::Result<()> {
    let mut all: Vec<RunRecord> = read_index(runs).into_values().collect();
    all.retain(|r| r.id != record.id);
    all.push(record);
    all.sort_by(|a, b| b.started_ms.cmp(&a.started_ms).then_with(|| b.id.cmp(&a.id)));
    write_index(runs, &all)
}

fn write_index(runs: &Path, records: &[RunRecord]) -> std::io::Result<()> {
    std::fs::create_dir_all(runs)?;
    let text = serde_json::to_string_pretty(records)
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    std::fs::write(index_path(runs), text)
}

/// Everything under the run, transcripts included — the figure the History list
/// shows is what deleting the run would actually reclaim.
fn dir_bytes(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else { return 0 };
    entries
        .flatten()
        .map(|e| match e.metadata() {
            Ok(m) if m.is_dir() => dir_bytes(&e.path()),
            Ok(m) => m.len(),
            Err(_) => 0,
        })
        .sum()
}

/// Copy a run's JSON export somewhere the operator chose.
///
/// A copy of a file that already exists rather than a fresh serialization: the
/// download and the thing an agent reads must be the same bytes, or "export the
/// logs" and "read the run" quietly become two formats.
pub fn export(runs: &Path, id: &str, to: &Path) -> Result<(), String> {
    let dir = run_dir(runs, id)?;
    let source = dir.join(EVENTS_JSON);
    // A run archived before the export existed has no events.json; generate it
    // once, into the archive, so the next reader finds it there too.
    if !source.is_file() {
        let json = fleetor_db::archive::to_json(&dir.join("state.db")).map_err(|e| e.to_string())?;
        std::fs::write(&source, json).map_err(|e| format!("writing {}: {e}", source.display()))?;
    }
    std::fs::copy(&source, to)
        .map(|_| ())
        .map_err(|e| format!("writing {}: {e}", to.display()))
}

fn read_live_meta(shell: &Path) -> LiveMeta {
    std::fs::read_to_string(live_meta(shell))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// When the run started, for a `run.json` that was never written — the database
/// file's own creation time, which is when the store was opened.
fn file_started_ms(db: &Path) -> Option<i64> {
    let meta = std::fs::metadata(db).ok()?;
    let created = meta.created().or_else(|_| meta.modified()).ok()?;
    let since = created.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(since.as_millis() as i64)
}

// --- the id -------------------------------------------------------------------

/// A sortable, human-scannable UTC directory name: `2026-08-07T14-32-05Z`.
///
/// UTC and not local time, deliberately: the id is stable for the life of the
/// archive and must not shift under a timezone change or a DST boundary. The
/// History view formats `started_ms` in the operator's own zone — that is the
/// one a human reads, and this is the one a filesystem sorts.
///
/// Hand-rolled because the workspace has no date crate and this is not worth
/// one: the civil-from-days conversion is a closed-form algorithm with no
/// configuration and no locale.
fn timestamp_id(ms: i64) -> String {
    let days = ms.div_euclid(MS_PER_DAY);
    let ms_of_day = ms.rem_euclid(MS_PER_DAY);
    let (y, m, d) = civil_from_days(days);
    let secs = ms_of_day / 1000;
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}-{:02}-{:02}Z",
        secs / 3600,
        (secs / 60) % 60,
        secs % 60
    )
}

/// Days since the Unix epoch → (year, month, day). Howard Hinnant's `civil_from_days`.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fleetor_core::pane::PaneId;
    use fleetor_core::Store;
    use fleetor_db::SqliteStore;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "fleetor-runs-{}-{name}-{}",
            std::process::id(),
            fleetor_core::ids::new_id("t")
        ));
        std::fs::create_dir_all(dir.join("_shell")).unwrap();
        dir
    }

    fn write_live(shell: &Path, bodies: &[&str]) {
        let store = SqliteStore::open(&shell.join("state.db")).unwrap();
        for body in bodies {
            store
                .append_event(&FleetEvent::Message {
                    id: fleetor_core::ids::new_id("msg"),
                    from: PaneId::Orch,
                    to: PaneId::Worker(1),
                    body: (*body).into(),
                    group: None,
                    accepted: true,
                    detail: None,
                })
                .unwrap();
        }
    }

    #[test]
    fn the_first_run_on_a_machine_has_nothing_to_archive() {
        let root = scratch("first");
        let notices = rotate(&root.join("_shell"), &runs_dir(&root), 0);
        assert!(notices.is_empty());
        assert!(list(&runs_dir(&root)).is_empty());
    }

    #[test]
    fn a_run_is_archived_and_the_live_slot_is_left_empty_for_the_next_one() {
        let root = scratch("rotate");
        let shell = root.join("_shell");
        begin(&shell, 1_800_000_000_000, Path::new("/tmp/logstat"));
        write_live(&shell, &["take the parser", "on it"]);

        let notices = rotate(&shell, &runs_dir(&root), 1_800_000_100_000);

        assert!(!shell.join("state.db").exists(), "the next run must open a fresh database");
        assert!(!shell.join("run.json").exists(), "the live marker belongs to the run that ended");
        assert!(notices.iter().any(|(l, _)| *l == NoticeLevel::Info), "{notices:?}");

        let runs = list(&runs_dir(&root));
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].messages, 2);
        assert_eq!(runs[0].target.as_deref(), Some("/tmp/logstat"));
        assert_eq!(runs[0].label, "logstat — take the parser");
        assert!(runs[0].bytes > 0);
    }

    #[test]
    fn two_runs_stay_separate_and_list_newest_first() {
        let root = scratch("two");
        let shell = root.join("_shell");
        let runs = runs_dir(&root);

        begin(&shell, 1_800_000_000_000, Path::new("/tmp/logstat"));
        write_live(&shell, &["first run"]);
        rotate(&shell, &runs, 0);

        begin(&shell, 1_900_000_000_000, Path::new("/tmp/logstat"));
        write_live(&shell, &["second run", "and more"]);
        rotate(&shell, &runs, 0);

        let all = list(&runs);
        assert_eq!(all.len(), 2, "{all:#?}");
        assert_eq!(all[0].messages, 2, "newest first");
        assert_eq!(all[1].messages, 1);
        // The point of the whole package: neither can see the other's log.
        assert_eq!(events(&runs, &all[1].id, 0).unwrap().len(), 1);
    }

    #[test]
    fn deleting_the_index_costs_labels_and_nothing_else() {
        let root = scratch("index");
        let shell = root.join("_shell");
        let runs = runs_dir(&root);
        begin(&shell, 1_800_000_000_000, Path::new("/tmp/logstat"));
        write_live(&shell, &["take the parser"]);
        rotate(&shell, &runs, 0);

        let id = list(&runs)[0].id.clone();
        rename(&runs, &id, "the good one").unwrap();
        assert_eq!(list(&runs)[0].label, "the good one");

        std::fs::remove_file(index_path(&runs)).unwrap();
        let rebuilt = list(&runs);
        assert_eq!(rebuilt.len(), 1, "the run itself must survive");
        assert_eq!(rebuilt[0].messages, 1);
        assert_eq!(rebuilt[0].label, "take the parser", "only the label is lost");
    }

    #[test]
    fn a_run_that_started_and_did_nothing_is_labelled_as_such() {
        let root = scratch("empty");
        let shell = root.join("_shell");
        begin(&shell, 1_800_000_000_000, Path::new("/tmp/logstat"));
        SqliteStore::open(&shell.join("state.db")).unwrap();
        rotate(&shell, &runs_dir(&root), 0);

        let all = list(&runs_dir(&root));
        assert_eq!(all.len(), 1, "an empty run is still the operator's to delete");
        assert_eq!(all[0].label, "logstat — no activity");
        assert_eq!(all[0].events, 0);
    }

    #[test]
    fn an_unreadable_previous_run_is_left_alone_and_announced() {
        let root = scratch("corrupt");
        let shell = root.join("_shell");
        std::fs::write(shell.join("state.db"), b"not a database").unwrap();

        let notices = rotate(&shell, &runs_dir(&root), 1_800_000_000_000);

        // It moves — a file that cannot be opened is still evidence (strategy C).
        assert!(!shell.join("state.db").exists());
        let all = list(&runs_dir(&root));
        assert!(all.is_empty(), "it cannot be summarized, so it does not list");
        assert!(notices.iter().any(|(l, _)| *l == NoticeLevel::Warn), "{notices:?}");
    }

    #[test]
    fn a_run_takes_its_workers_transcripts_with_it_and_leaves_none_behind() {
        let root = scratch("transcripts");
        let shell = root.join("_shell");
        let runs = runs_dir(&root);
        // Two workers, one with two sessions — the shape a real config dir has.
        for (worker, sessions) in [("worker-1", &["a", "b"][..]), ("worker-2", &["c"][..])] {
            let project = shell.join("pane-config").join(worker).join("projects").join("-tmp-slug");
            std::fs::create_dir_all(&project).unwrap();
            for s in sessions {
                std::fs::write(project.join(format!("{s}.jsonl")), r#"{"type":"user"}"#).unwrap();
            }
            std::fs::write(project.join("not-a-transcript.txt"), "ignore me").unwrap();
        }
        begin(&shell, 1_800_000_000_000, Path::new("/tmp/logstat"));
        write_live(&shell, &["take the parser"]);

        rotate(&shell, &runs, 0);

        let all = list(&runs);
        assert_eq!(all[0].transcripts, 3, "two workers, three sessions");
        let dir = runs.join(&all[0].id);
        assert!(dir.join("transcripts/worker-1/a.jsonl").is_file());
        assert!(dir.join("transcripts/worker-2/c.jsonl").is_file());
        assert!(!dir.join("transcripts/worker-1/not-a-transcript.txt").exists());

        // Moved, not copied: the next run must not inherit this one's sessions.
        let left = shell.join("pane-config/worker-1/projects/-tmp-slug");
        assert!(!left.join("a.jsonl").exists(), "a copied transcript would be archived twice");
        assert!(left.join("not-a-transcript.txt").is_file(), "only .jsonl moves");
    }

    /// WP-14, D-062: the pane that makes the decisions is archived like every
    /// other one. `orch`'s config dir is a sibling of the workers' under the same
    /// `pane-config/` root, so rotation files it under `transcripts/orch/` with
    /// no arm of its own — and an evaluator reading the run gets the reasoning
    /// behind the messages, not only the messages.
    #[test]
    fn a_run_takes_orchs_transcript_with_it_the_same_way_it_takes_a_workers() {
        let root = scratch("orch-transcript");
        let shell = root.join("_shell");
        let runs = runs_dir(&root);
        for (pane, session) in [("orch", "o1"), ("worker-1", "w1")] {
            let project = shell.join("pane-config").join(pane).join("projects").join("-tmp-slug");
            std::fs::create_dir_all(&project).unwrap();
            std::fs::write(project.join(format!("{session}.jsonl")), r#"{"type":"user"}"#).unwrap();
        }
        begin(&shell, 1_800_000_000_000, Path::new("/tmp/logstat"));
        write_live(&shell, &["take the parser"]);

        rotate(&shell, &runs, 0);

        let all = list(&runs);
        assert_eq!(all[0].transcripts, 2, "orch counts like any other pane");
        let dir = runs.join(&all[0].id);
        assert!(dir.join("transcripts/orch/o1.jsonl").is_file(), "the deciding pane is archived");
        assert!(dir.join("transcripts/worker-1/w1.jsonl").is_file());
        // Moved, not copied — the next run must not re-archive this one's session.
        assert!(!shell.join("pane-config/orch/projects/-tmp-slug/o1.jsonl").exists());

        let manifest: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join(MANIFEST_JSON)).unwrap()).unwrap();
        let layout = manifest["layout"]["transcripts/"].as_str().unwrap();
        assert!(layout.contains("orch"), "a cold reader is told orch is in there: {layout}");
    }

    #[test]
    fn an_archived_run_is_readable_without_sqlite_and_says_what_it_holds() {
        let root = scratch("agentview");
        let shell = root.join("_shell");
        let runs = runs_dir(&root);
        begin(&shell, 1_800_000_000_000, Path::new("/tmp/logstat"));
        write_live(&shell, &["take the parser", "on it"]);
        rotate(&shell, &runs, 0);

        let dir = runs.join(&list(&runs)[0].id);

        let events: Vec<serde_json::Value> =
            serde_json::from_str(&std::fs::read_to_string(dir.join(EVENTS_JSON)).unwrap()).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["body"], "take the parser");
        assert!(events[0]["seq"].is_i64() && events[0]["ts"].is_i64());

        let manifest: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join(MANIFEST_JSON)).unwrap())
                .unwrap();
        assert_eq!(manifest["run"]["messages"], 2);
        assert!(manifest["layout"][EVENTS_JSON].is_string(), "a cold reader is told the layout");
    }

    #[test]
    fn exporting_writes_the_same_bytes_the_archive_holds() {
        let root = scratch("export");
        let shell = root.join("_shell");
        let runs = runs_dir(&root);
        begin(&shell, 1_800_000_000_000, Path::new("/tmp/logstat"));
        write_live(&shell, &["take the parser"]);
        rotate(&shell, &runs, 0);
        let id = list(&runs)[0].id.clone();

        let out = root.join("downloaded.json");
        export(&runs, &id, &out).unwrap();
        assert_eq!(
            std::fs::read_to_string(&out).unwrap(),
            std::fs::read_to_string(runs.join(&id).join(EVENTS_JSON)).unwrap(),
            "the download and what an agent reads must not be two formats"
        );

        // A run archived before events.json existed still exports, and gains one.
        std::fs::remove_file(runs.join(&id).join(EVENTS_JSON)).unwrap();
        export(&runs, &id, &out).unwrap();
        assert!(runs.join(&id).join(EVENTS_JSON).is_file());
        assert!(export(&runs, "no-such-run", &out).is_err());
    }

    #[test]
    fn a_run_id_from_the_webview_cannot_escape_the_runs_directory() {
        let root = scratch("escape");
        let runs = runs_dir(&root);
        std::fs::create_dir_all(&runs).unwrap();
        for bad in ["../_shell", "..", "a/b", "", "with space"] {
            assert!(run_dir(&runs, bad).is_err(), "{bad:?} must be refused");
        }
    }

    #[test]
    fn deleting_a_run_removes_it_from_disk_and_from_the_index() {
        let root = scratch("delete");
        let shell = root.join("_shell");
        let runs = runs_dir(&root);
        begin(&shell, 1_800_000_000_000, Path::new("/tmp/logstat"));
        write_live(&shell, &["take the parser"]);
        rotate(&shell, &runs, 0);

        let id = list(&runs)[0].id.clone();
        delete(&runs, &id).unwrap();
        assert!(list(&runs).is_empty());
        assert!(!runs.join(&id).exists());
        assert!(delete(&runs, &id).is_err(), "a second delete has nothing to remove");
    }

    #[test]
    fn a_run_id_is_the_utc_calendar_time_it_started() {
        // Cross-checked against `date -u -r 1786098690`.
        assert_eq!(timestamp_id(1_786_098_690_000), "2026-08-07T10-31-30Z");
        assert_eq!(timestamp_id(0), "1970-01-01T00-00-00Z");
        // Leap day, and the year boundary the civil-from-days shift exists for.
        assert_eq!(&timestamp_id(1_709_164_800_000)[..10], "2024-02-29");
        assert_eq!(&timestamp_id(1_704_067_199_000)[..10], "2023-12-31");
    }

    #[test]
    fn two_runs_that_start_in_the_same_second_do_not_overwrite_each_other() {
        let root = scratch("collide");
        let runs = runs_dir(&root);
        let a = reserve(&runs, "2026-08-07T10-31-30Z").unwrap();
        let b = reserve(&runs, "2026-08-07T10-31-30Z").unwrap();
        assert_ne!(a, b);
        assert!(b.file_name().unwrap().to_string_lossy().ends_with("-2"));
    }
}
