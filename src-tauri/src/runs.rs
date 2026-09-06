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

use crate::placement::harness::{HarnessSpec, Transport};

/// Milliseconds in a day — the unit the civil-date conversion counts in.
const MS_PER_DAY: i64 = 86_400_000;

/// The agent-facing export, written into every run at archive time.
const EVENTS_JSON: &str = "events.json";
/// What the run is and what else is in its directory, for a reader arriving cold.
const MANIFEST_JSON: &str = "manifest.json";

/// **What `transcripts/` holds, said without naming a vendor** (M24, #39).
///
/// It used to say "that pane's Claude Code session `.jsonl` files", which was true
/// of every run a one-harness fleet could produce and becomes a false statement to
/// any Critic reading a mixed run cold — the wrong kind of false, too: a reader
/// who believes it will look for a format that is not there and conclude the
/// evidence is missing rather than that the sentence is. It now says what is
/// invariant (one directory per pane, raw, this run only) and points at `panes`
/// for what varies.
const TRANSCRIPTS_ARE: &str = "one directory per pane — orch and each worker — holding that pane's \
     own transcripts for this run only, exactly as its harness wrote them. Nothing here is \
     converted or normalized: `panes` below says which harness ran in each seat and what format \
     its transcripts are in.";

/// What the `panes` object is, for the same cold reader.
const PANES_ARE: &str = "one entry per pane that was placed, keyed by the seat name the \
     transcripts/ directories use — the harness it ran, the model it was pointed at where the \
     fleet chose one, and the format of its transcripts. A run whose panes never started has \
     none, and a pane placed before this record existed is absent rather than guessed at.";

/// The sentence that tells a cold reader what the two records are *for*. Vendor-free
/// already, and unchanged.
const READING_THIS: &str = "The event log is what the panes said to each other. The transcripts \
     are what each pane did between saying things. Neither records terminal output.";

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
    ///
    /// **It is no longer also what a whole harness contributing nothing looks
    /// like** (M24, #39). While the harvest could only take a transcript with a
    /// rename, a pane whose transcript was a live database contributed zero and
    /// the run read as an ordinary one — a real gap in a normal run's clothes,
    /// which is why registering such a harness was refused rather than allowed to
    /// produce quiet archives. Every registered harness's transport is now one the
    /// harvest implements, so a zero here means what it says.
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
    /// What each pane of this run was placed as, keyed by the seat name its
    /// configuration directory is called — the same name the harvest files
    /// transcripts under. Written at spawn by [`record_pane`], archived into
    /// `manifest.json` by rotation (M24).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    panes: BTreeMap<String, PaneRecord>,
}

/// What a mixed run's manifest says about one pane (M24, #39).
///
/// **The archive is the only place this can be read from afterwards, which is why
/// it is written while the run is alive.** A pane's configuration directory is
/// named for its *seat* — `orch`, `worker-1` — and carries no record of which
/// vendor's binary was pointed at it; by the time rotation archives a run its
/// panes are gone and the fleet that placed them is gone with them (C33). So the
/// harvest still finds transcripts by looking under every registered harness's
/// answer, and this is what tells a Critic reading the result cold *which* vendor
/// wrote the files it is now holding, and in what format.
///
/// C33 named this exact reversal: "a run manifest that already records each pane's
/// harness, at which point the harvest should read that rather than guess". Only
/// half of it applies — the manifest records it now, and the harvest still does
/// not read it, because this file describes the run being *archived* and the
/// harvest's own dedup makes the guess harmless. The orphan sweep still records
/// nothing, deliberately: its registry is not a manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneRecord {
    /// The harness this pane ran, by [`HarnessSpec::name`].
    pub harness: String,
    /// The model it was pointed at, where the fleet chose one. `None` on an
    /// attended seat, which runs the operator's own login and whatever model that
    /// account defaults to — the honest answer rather than a guessed name (M2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Checkpoint 13's `format`, so the transcripts filed under this pane can be
    /// named without knowing which vendor wrote them.
    pub transcript_format: String,
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
    // The panes are empty here and are filled in one at a time by
    // [`record_pane`], because at bootstrap there are none: rotation has just run
    // and the first pane is several operator gestures away.
    let meta = LiveMeta {
        started_ms: Some(started_ms),
        target: Some(target.display().to_string()),
        panes: BTreeMap::new(),
    };
    if let Ok(text) = serde_json::to_string_pretty(&meta) {
        let _ = std::fs::write(live_meta(shell), text);
    }
}

/// Write down what one pane was placed as, for the manifest of the run it belongs
/// to (M24, #39).
///
/// Called from the spawn path, once per pane, after placement has succeeded — so
/// what is recorded is what a pane was actually placed *as*, never what a caller
/// intended. **Not on the message path and never able to be** (Tier 1.4): a
/// `fleet send` neither writes this nor reads it, and a spawn is already several
/// filesystem writes deep by the time it gets here.
///
/// Best-effort, like [`begin`], and for the same reason: this is one object in a
/// manifest an agent reads later, and a fleet that refused to start a pane because
/// it could not describe it would be trading the run for the record of it. A pane
/// missing here is a pane the manifest does not name; its transcripts are still
/// harvested and still filed under its seat.
pub fn record_pane(shell: &Path, pane: &str, harness: &'static HarnessSpec, model: Option<&str>) {
    let mut meta = read_live_meta(shell);
    meta.panes.insert(
        pane.to_string(),
        PaneRecord {
            harness: harness.name.to_string(),
            model: model.map(str::to_string),
            transcript_format: harness.transcript.format.to_string(),
        },
    );
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
            if let Err(e) = write_agent_view(&dest, &record, &meta.panes) {
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

/// Move every pane's transcript into the run it belongs to, from wherever its
/// harness leaves them.
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
///
/// **Where and what it looks for is checkpoint 13's** (WP-25, C12):
/// [`Transcript::subdir`](crate::placement::harness::Transcript::subdir) and
/// [`file_ext`](crate::placement::harness::Transcript::file_ext), across every
/// registered harness — see [`transcript_locations`] for why the whole registry
/// rather than this pane's own harness.
///
/// **How each one is taken is checkpoint 13's third answer, and this function
/// reads it** (#39). [`Transport::Rename`] is a plain move, which is what
/// append-only files want. [`Transport::SqliteBackup`] is a live database — `db`
/// plus `-wal` plus `-shm` — and renaming the `.sqlite` alone would leave every
/// transaction still in the write-ahead log behind, so it goes through SQLite's
/// own backup path. Rotation running before any pane exists would make a
/// three-file copy *untorn*, which is a different property from complete and not
/// the one an archive needs.
///
/// **Reading the flag to *skip* such a harness stays rejected**, as it was before
/// the mechanism existed: it archives nothing while the run looks ordinary. What
/// changed is that there is now nothing to skip.
fn harvest_transcripts(shell: &Path, dest: &Path) -> u32 {
    // `Take::Move` is this function's whole contract and is stated at the call
    // site rather than inside the walk — see [`copy_transcripts`] for the twin.
    walk_transcripts(shell, dest, Take::Move)
}

/// Take one transcript into the archive the way its harness says it may be taken,
/// and answer whether it arrived.
///
/// The two verbs are separated from the two transports on purpose: whether the
/// original survives is the *caller's* contract (rotation moves, the live snapshot
/// copies), and how a file is read is the *harness's*. Crossing them would give
/// four bespoke branches instead of two facts.
fn take_transcript(from: &Path, to: &Path, transport: Transport, take: Take) -> bool {
    match transport {
        Transport::Rename => match take {
            Take::Move => std::fs::rename(from, to).is_ok(),
            Take::Copy => std::fs::copy(from, to).is_ok(),
        },
        Transport::SqliteBackup => {
            if sqlite_backup(from, to).is_err() {
                return false;
            }
            if take == Take::Move {
                // The whole database, not the file that shares its name: a `-wal`
                // left beside a store whose contents have been archived is a
                // fragment of the next run's evidence, and the `-shm` is scratch.
                let _ = std::fs::remove_file(from);
                for suffix in ["-wal", "-shm"] {
                    let _ = std::fs::remove_file(with_suffix(from, suffix));
                }
            }
            true
        }
    }
}

/// Read a live SQLite database and write one self-contained copy of it — the
/// mechanism [`Transport::SqliteBackup`] names (C12, #39).
///
/// **`VACUUM INTO` rather than a file copy**, and the difference is not
/// theoretical: in WAL mode the committed rows live in the `-wal` until something
/// checkpoints, so the main file on its own can be a schema-less header while the
/// database it names is full of turns. This opens the database, which is what
/// makes SQLite read the log as part of it, and writes a fresh file with no
/// journal beside it — exactly what an archive wants, since a `-wal` that got left
/// behind would be a second file the reader has to know to keep.
///
/// Opened read-write rather than read-only, because a read-only connection to a
/// WAL database still needs to build the shared-memory index and fails where it
/// cannot. Rotation runs before any pane exists (see [`rotate`]), and `VACUUM
/// INTO` is read-only with respect to the source in any case.
fn sqlite_backup(from: &Path, to: &Path) -> Result<(), String> {
    let to = to.to_str().ok_or_else(|| format!("{} is not utf-8", to.display()))?;
    let conn = rusqlite::Connection::open(from).map_err(|e| format!("open {}: {e}", from.display()))?;
    conn.execute("VACUUM INTO ?1", [to]).map_err(|e| format!("VACUUM INTO {to}: {e}"))?;
    Ok(())
}

/// Whether the original survives the archive. Rotation's answer and the live
/// snapshot's answer, named rather than passed as a `bool` — the accumulation bug
/// this distinction prevents is described on [`copy_transcripts`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Take {
    Move,
    Copy,
}

/// The walk both [`harvest_transcripts`] and [`copy_transcripts`] are, so the
/// archive and the live snapshot cannot come to look in different places or read a
/// database two different ways.
fn walk_transcripts(shell: &Path, dest: &Path, take: Take) -> u32 {
    let Ok(panes) = std::fs::read_dir(crate::placement::pane_config_root(shell)) else { return 0 };
    let mut taken = 0;

    for pane in panes.flatten() {
        let name = pane.file_name();
        let into = dest.join("transcripts").join(&name);

        for at in transcript_locations() {
            for from in transcript_files(&pane.path().join(at.subdir), at.file_ext) {
                let Some(file_name) = from.file_name() else { continue };
                if std::fs::create_dir_all(&into).is_err() {
                    continue;
                }
                if take_transcript(&from, &into.join(file_name), at.transport, take) {
                    taken += 1;
                }
            }
        }
    }
    taken
}

/// Every file under a harness's transcript directory that carries its extension:
/// **the directory itself, and one level below it.**
///
/// Two depths because that is the whole of the variation the registry has, and
/// because the intervening level is the one thing checkpoint 13 does not answer.
/// Claude Code keeps `projects/<per-project directory>/<session>.jsonl`; codex
/// keeps its thread store in `CODEX_HOME` itself, with no per-project level at all.
/// What that directory would be *called* is not one of the fourteen answers and
/// this function does not need it to be — it is looking for files, not
/// constructing a path — which is precisely the half of the gap that can be closed
/// without inventing a general rule out of one vendor's (C54).
///
/// Not recursive, deliberately. A configuration directory holds a great deal that
/// is not a transcript, and a harness whose extension is as ordinary as `sqlite`
/// would have its settings and its caches archived as evidence by a walk that kept
/// descending.
fn transcript_files(root: &Path, file_ext: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else { return found };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let Ok(below) = std::fs::read_dir(&path) else { continue };
            found.extend(below.flatten().map(|e| e.path()).filter(|p| has_ext(p, file_ext)));
        } else if has_ext(&path, file_ext) {
            found.push(path);
        }
    }
    found
}

fn has_ext(path: &Path, file_ext: &str) -> bool {
    path.extension().and_then(|e| e.to_str()) == Some(file_ext)
}

/// Every place a pane could have left a transcript: checkpoint 13's
/// subdirectory, extension and transport, one row per registered harness.
///
/// **The whole registry, not the pane's own harness, because nothing on disk
/// says which harness a pane ran.** A pane's config directory is named for the
/// seat (`orch`, `worker-1`), and the run being archived is the *previous* one —
/// its panes are gone and the fleet that placed them is gone with them. Reading
/// each pane's harness back would mean the placement writing a marker file into
/// the config dir purely so rotation could read it, which is state kept alive
/// across a crash for the benefit of a directory walk. Looking under every
/// registered harness's answer costs one failed `read_dir` per harness per pane
/// and cannot mis-file anything: what it finds under a subdirectory is filed
/// under the seat it was found in, never under a harness name.
///
/// **`manifest.json` now records each pane's harness (M24), and this still does
/// not read it** — C33's named reversal applies to a manifest describing the run
/// being archived, and the one rotation is about to write is the *output* of this
/// walk rather than an input to it.
///
/// Deduplicated, so two harnesses that agree on a location do not have the same
/// file counted twice — the count is what the run manifest reports as its
/// evidence, and a doubled one would read as transcripts that are not there. Two
/// harnesses agreeing on a location and disagreeing on how it may be read is not
/// deduplicated, because those are two different reads of the same file and
/// silently picking one would be picking a vendor.
fn transcript_locations() -> Vec<Location> {
    let mut seen: Vec<Location> = Vec::new();
    for harness in crate::placement::harness::registered() {
        let transcript = &harness.spec().transcript;
        let at = Location {
            subdir: transcript.subdir,
            file_ext: transcript.file_ext,
            transport: transcript.transport,
        };
        if !seen.contains(&at) {
            seen.push(at);
        }
    }
    seen
}

/// One row of [`transcript_locations`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Location {
    subdir: &'static str,
    file_ext: &'static str,
    transport: Transport,
}

// --- the live run, laid out for a reader (WP-15) --------------------------------

/// Lay the run that is **still being written** out in the archive's own shape,
/// somewhere a reader can be pointed at.
///
/// Rotation (above) archives the *previous* run at bootstrap, so at the moment
/// `orch` hands back, the run worth reading is the live one: `_shell/state.db`
/// in WAL mode with this process's own writer holding it open, and transcripts
/// still being appended to inside `pane-config/`. This produces the same three
/// things an archived run holds — `events.json`, `manifest.json`, `transcripts/`
/// — without disturbing any of it.
///
/// **Three decisions, all measured rather than reasoned
/// (`docs/notes/live-run-snapshot-notes.md`):**
///
///  - **The log is read in place, read-only.** `archive::to_json` opens
///    `SQLITE_OPEN_READ_ONLY` and sees every committed row while the writer is
///    live; the spike confirmed the writer is undisturbed by it. So this is the
///    *same function* rotation calls, and the `events.json` an agent reads
///    mid-run cannot drift from the one it reads afterwards (D-059's rule).
///  - **`archive::freeze` is never called here.** It flips the database out of
///    WAL mode, which against a live writer fails by design — and reusing
///    rotation's file-moving path wholesale would try it.
///  - **Transcripts are copied, not moved.** [`harvest_transcripts`] moves them,
///    which is right at rotation because no pane exists then. Here every pane is
///    alive and its harness still has those files open; moving one is a
///    data-loss bug in the very run being judged. The two are named functions
///    over one walk rather than a `copy: bool` — see [`copy_transcripts`] for why
///    the distinction survived being factored.
///
/// The directory is rebuilt from scratch on every call, so a second handoff in
/// one run gets a snapshot of the run at *that* moment rather than a merge of
/// two.
pub fn snapshot_live_run(shell: &Path, dest: &Path, run_id: &str) -> Result<u32, String> {
    let live = live_db(shell);
    if !live.is_file() {
        return Err(format!("there is no live run at {}", live.display()));
    }
    let _ = std::fs::remove_dir_all(dest);
    std::fs::create_dir_all(dest).map_err(|e| format!("create {}: {e}", dest.display()))?;

    let json = fleetor_db::archive::to_json(&live)
        .map_err(|e| format!("reading the live run at {}: {e}", live.display()))?;
    std::fs::write(dest.join(EVENTS_JSON), json)
        .map_err(|e| format!("writing {}: {e}", dest.join(EVENTS_JSON).display()))?;

    let transcripts = copy_transcripts(shell, dest);
    let meta = read_live_meta(shell);
    let manifest = serde_json::json!({
        "run": {
            "id": run_id,
            "state": "live",
            "started_ms": meta.started_ms,
            "target": meta.target,
            "transcripts": transcripts,
        },
        "panes": meta.panes,
        "layout": {
            "events.json": "the whole event log so far, one JSON array, oldest first; `seq` and `ts` are the row's own columns",
            "transcripts/": TRANSCRIPTS_ARE,
            "panes": PANES_ARE,
        },
        "reading_this": READING_THIS,
        "this_is_a_snapshot": format!(
            "Taken when the mission was handed back, while the run was still open. It holds \
             the run up to that moment and nothing after it. There is no state.db here — the \
             live database belongs to the running app. The final archive at runs/{run_id}/ \
             supersedes this after the next fleet start."
        ),
    });
    std::fs::write(
        dest.join(MANIFEST_JSON),
        serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("writing {}: {e}", dest.join(MANIFEST_JSON).display()))?;
    Ok(transcripts)
}

/// [`harvest_transcripts`]'s non-destructive twin.
///
/// It reads the same checkpoint 13 answers through the same
/// [`transcript_locations`] and takes each file through the same
/// [`take_transcript`], so the live snapshot and the archive cannot come to look
/// in different places or read a database two different ways. Only [`Take`]
/// differs, and that is the point.
///
/// **Still two named functions rather than one with a `copy: bool`** (D-059's
/// rule, kept). What made the flag dangerous was that a caller could pass the
/// wrong value and get the accumulation bug back silently; what makes it worth
/// factoring now is that the walk contains a *database backup* and two copies of
/// one is a place for exactly the divergence this doc warns about. So the verb is
/// an enum with two named constants, each written at exactly one call site, in a
/// function whose name says which it passes.
///
/// **A `SqliteBackup` transcript is copied here by being backed up, not by
/// `fs::copy`** — the source is a live database with a pane still writing to it,
/// which is the case a file copy tears. `VACUUM INTO` takes a read transaction,
/// so the snapshot holds a consistent moment of a run in progress rather than a
/// mid-write page.
fn copy_transcripts(shell: &Path, dest: &Path) -> u32 {
    walk_transcripts(shell, dest, Take::Copy)
}

/// The directory name the live run *will* be archived under, computed from the
/// same `started_ms` rotation will use. Naming the snapshot after it means an
/// operator holding a retro in one hand and a `runs/` entry in the other does
/// not have to work out which is which.
pub fn live_run_id(shell: &Path, fallback_ms: i64) -> String {
    let meta = read_live_meta(shell);
    let started =
        meta.started_ms.or_else(|| file_started_ms(&live_db(shell))).unwrap_or(fallback_ms);
    timestamp_id(started)
}

/// Write the two files an agent reads: the log as JSON, and a manifest saying
/// what is in the directory.
///
/// Written **at archive time**, not generated on demand by an export button.
/// The reader this is for is a `claude -p` with a shell, and it should be able
/// to `cat` a run without SQLite, without this app running, and without knowing
/// that a GUI exists.
fn write_agent_view(
    dest: &Path,
    record: &RunRecord,
    panes: &BTreeMap<String, PaneRecord>,
) -> std::io::Result<()> {
    let json = fleetor_db::archive::to_json(&dest.join("state.db"))
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    std::fs::write(dest.join(EVENTS_JSON), json)?;

    let manifest = serde_json::json!({
        "run": record,
        "panes": panes,
        "layout": {
            "events.json": "the whole event log, one JSON array, oldest first; `seq` and `ts` are the row's own columns",
            "state.db": "the same log as SQLite — the source of truth events.json is generated from",
            "transcripts/": TRANSCRIPTS_ARE,
            "panes": PANES_ARE,
        },
        "reading_this": READING_THIS,
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

    /// **The snapshot's whole reason for existing** (WP-15): the run the
    /// evaluator has to read is the *live* one, because rotation archives the
    /// previous run at bootstrap. The writer here is deliberately still open
    /// across the snapshot — that is the situation, and a version of this test
    /// that dropped the store first would pass while proving nothing.
    ///
    /// `docs/notes/live-run-snapshot-notes.md` measured the shell-level version
    /// of this; this is the same claim through the code that actually runs.
    #[test]
    fn a_live_run_is_readable_while_its_writer_still_holds_it() {
        let root = scratch("snapshot");
        let shell = root.join("_shell");
        begin(&shell, 1_800_000_000_000, Path::new("/tmp/logstat"));

        // The live writer, held open for the whole test.
        let store = SqliteStore::open(&shell.join("state.db")).unwrap();
        let say = |body: &str| {
            store
                .append_event(&FleetEvent::Message {
                    id: fleetor_core::ids::new_id("msg"),
                    from: PaneId::Orch,
                    to: PaneId::Worker(1),
                    body: body.into(),
                    group: None,
                    accepted: true,
                    detail: None,
                })
                .unwrap();
        };
        say("take the parser");
        say("on it");

        // A transcript, where a pane's config dir really puts one.
        let sessions = shell.join("pane-config/orch/projects/-tmp-logstat");
        std::fs::create_dir_all(&sessions).unwrap();
        std::fs::write(sessions.join("a1b2.jsonl"), "{\"role\":\"assistant\"}\n").unwrap();

        let dest = root.join("dev/retro/2026-08-07T14-32-05Z");
        let copied = snapshot_live_run(&shell, &dest, "2026-08-07T14-32-05Z").unwrap();

        let rows: Vec<serde_json::Value> =
            serde_json::from_str(&std::fs::read_to_string(dest.join(EVENTS_JSON)).unwrap()).unwrap();
        assert_eq!(rows.len(), 2, "every committed row, uncheckpointed WAL and all");
        assert_eq!(rows[0]["body"], "take the parser");

        assert_eq!(copied, 1);
        assert!(dest.join("transcripts/orch/a1b2.jsonl").is_file());
        // **Copied, never moved.** Claude Code still has this file open; moving
        // it would be a data-loss bug in the very run being judged.
        assert!(sessions.join("a1b2.jsonl").is_file(), "the live transcript stayed where it was");

        // No `state.db`: the live database belongs to the running app, and a
        // copy of it here would be the stale-prefix failure the notes measured.
        assert!(!dest.join("state.db").exists());
        let manifest: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dest.join(MANIFEST_JSON)).unwrap())
                .unwrap();
        assert_eq!(manifest["run"]["state"], "live");
        assert!(manifest["this_is_a_snapshot"].as_str().unwrap().contains("still open"));

        // And the writer is undisturbed: the run goes on after the retro starts.
        say("one more thing");
        assert_eq!(store.events_since(0).unwrap().len(), 3);
    }

    /// A second `fleet handoff` is a real sequence (D-064 refuses to argue with
    /// it), so the snapshot is rebuilt rather than merged — otherwise the
    /// evaluator reads a directory that is two runs' worth of one run.
    #[test]
    fn a_second_snapshot_replaces_the_first_rather_than_merging_into_it() {
        let root = scratch("resnapshot");
        let shell = root.join("_shell");
        begin(&shell, 1_800_000_000_000, Path::new("/tmp/logstat"));
        let store = SqliteStore::open(&shell.join("state.db")).unwrap();
        store.append_event(&FleetEvent::Notice { level: NoticeLevel::Info, text: "up".into() }).unwrap();

        let dest = root.join("dev/retro/run");
        snapshot_live_run(&shell, &dest, "run").unwrap();
        std::fs::write(dest.join("stale.txt"), "from the first handoff").unwrap();
        snapshot_live_run(&shell, &dest, "run").unwrap();

        assert!(!dest.join("stale.txt").exists(), "the directory is rebuilt, not added to");
        assert!(dest.join(EVENTS_JSON).is_file());
    }

    /// The retro directory is outside `_shell`, which is the only reason the
    /// fleet being judged cannot write into it: every pane's write guardrail
    /// has `_shell` as a root (D-065).
    #[test]
    fn the_snapshot_refuses_when_there_is_no_live_run() {
        let root = scratch("norun");
        let err = snapshot_live_run(&root.join("_shell"), &root.join("dev/retro/x"), "x")
            .expect_err("no database, no snapshot");
        assert!(err.contains("no live run"), "{err}");
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

    /// One live WAL-mode thread store, shaped like the one a codex pane leaves
    /// behind (C12) — **fabricated rather than produced by a vendor binary**, and
    /// that is the point: what is being asserted is the archive's mechanism, and a
    /// database whose committed rows are still in its write-ahead log is a thing
    /// this test can build in three lines and a probe would cost a real turn to
    /// get. The connection is returned rather than dropped because closing the last
    /// one checkpoints the log away, which is exactly the state that must survive
    /// to the harvest.
    fn a_live_thread_store(at: &Path, mark: &str) -> rusqlite::Connection {
        std::fs::create_dir_all(at.parent().expect("a directory to plant in")).unwrap();
        let conn = rusqlite::Connection::open(at).unwrap();
        conn.pragma_update(None, "journal_mode", "WAL").unwrap();
        conn.execute("CREATE TABLE thread_items (item_json TEXT NOT NULL)", []).unwrap();
        conn.execute("INSERT INTO thread_items (item_json) VALUES (?1)", [mark]).unwrap();
        conn
    }

    fn holds(path: &Path, mark: &str) -> bool {
        std::fs::read(path)
            .is_ok_and(|b| b.windows(mark.len()).any(|w| w == mark.as_bytes()))
    }

    /// **The mechanism, and the control that says it is a mechanism** (#39, C12,
    /// M24).
    ///
    /// A harness whose transcript is a live database gets SQLite's own backup path.
    /// The assertion that makes that more than a name is the second half: the same
    /// store taken with `fs::copy` — the naive implementation — does not contain
    /// the row at all, because the row is in the `-wal`. A rename of the `.sqlite`
    /// produces the identical loss and additionally strands the log.
    ///
    /// Driven at [`take_transcript`] rather than through [`rotate`] because
    /// `transcript_locations` reads the registry, and the only registered harness
    /// answers [`Transport::Rename`]. The end-to-end pass belongs to checkpoint 13,
    /// which runs it for every harness that *is* registered.
    #[test]
    fn a_live_database_is_archived_through_sqlites_own_backup_and_a_copy_would_have_torn_it() {
        const MARK: &str = "FLEETOR-RUNS-THREAD-ITEM";
        let root = scratch("thread-store");
        let from = root.join("pane-config/worker-1/thread_history_1.sqlite");
        let store = a_live_thread_store(&from, MARK);

        // The control, first: this is what a file-copy harvest would have filed.
        let torn = root.join("a-file-copy.sqlite");
        std::fs::copy(&from, &torn).unwrap();
        assert!(
            !holds(&torn, MARK),
            "the fixture is not a live write-ahead-logged database, so this test cannot tell \
             a backup from a copy",
        );

        let into = root.join("runs/r1/transcripts/worker-1/thread_history_1.sqlite");
        std::fs::create_dir_all(into.parent().unwrap()).unwrap();
        assert!(take_transcript(&from, &into, Transport::SqliteBackup, Take::Move));

        // Whole: the row that was only ever in the log is in the archive, and the
        // archive is a database rather than a file that resembles one.
        assert!(holds(&into, MARK), "the archived database lost the committed item");
        let archived = rusqlite::Connection::open(&into).unwrap();
        let rows: i64 = archived
            .query_row("SELECT count(*) FROM thread_items WHERE item_json = ?1", [MARK], |r| r.get(0))
            .expect("the archived database is readable");
        assert_eq!(rows, 1);

        // Self-contained: no journal beside it, because a `-wal` in an archive is a
        // second file the reader has to know to keep.
        for suffix in ["-wal", "-shm"] {
            assert!(!with_suffix(&into, suffix).exists(), "the archive holds a {suffix}");
            assert!(
                !with_suffix(&from, suffix).exists(),
                "the harvest left a {suffix} behind, so the next run archives a fragment of \
                 this one",
            );
        }
        assert!(!from.exists(), "a moved transcript does not stay where it was");
        drop(store);
    }

    /// **Both depths, because the per-project level is the one thing checkpoint 13
    /// does not answer** (C54).
    ///
    /// Claude Code keeps `projects/<per-project directory>/<session>.jsonl`; codex
    /// keeps its store in the configuration directory itself, with no such level.
    /// The harvest looks in the directory and one below it and stops there — a
    /// deeper walk would archive a harness's caches as evidence on any extension
    /// as ordinary as `sqlite`.
    #[test]
    fn the_harvest_finds_a_transcript_with_a_per_project_directory_and_without_one() {
        let root = scratch("depths");
        std::fs::create_dir_all(root.join("a-project")).unwrap();
        std::fs::create_dir_all(root.join("a-project/deeper")).unwrap();
        std::fs::write(root.join("at-the-root.sqlite"), "x").unwrap();
        std::fs::write(root.join("a-project/one-below.sqlite"), "x").unwrap();
        std::fs::write(root.join("a-project/deeper/two-below.sqlite"), "x").unwrap();
        std::fs::write(root.join("a-project/not-one.txt"), "x").unwrap();

        let mut found: Vec<String> = transcript_files(&root, "sqlite")
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        found.sort();
        assert_eq!(found, ["at-the-root.sqlite", "one-below.sqlite"]);
    }

    /// **What a Critic reading a mixed run cold is told** (M24, #39).
    ///
    /// Two things at once, because they are one failure: the manifest names each
    /// pane's harness, model and transcript format, and the sentence describing
    /// `transcripts/` no longer claims they are one vendor's session files. The old
    /// sentence was true of every run a one-harness fleet could produce, which is
    /// why it survived — and false in the worst way to a reader of the first mixed
    /// one, who would look for a format that is not there and conclude the evidence
    /// is missing rather than that the sentence is.
    #[test]
    fn a_mixed_runs_manifest_names_each_panes_harness_and_stops_naming_one_vendor() {
        let root = scratch("mixed-manifest");
        let shell = root.join("_shell");
        let runs = runs_dir(&root);
        begin(&shell, 1_800_000_000_000, Path::new("/tmp/logstat"));
        write_live(&shell, &["take the parser"]);

        // Two seats placed as two different harnesses — the run the old sentence
        // could not describe. Recorded through the production path, so what is
        // asserted is what a spawn writes.
        record_pane(&shell, "orch", crate::placement::harness::claude_code().spec(), None);
        record_pane(&shell, "worker-1", crate::placement::codex::codex().spec(), Some("a-model"));

        rotate(&shell, &runs, 0);

        let dir = runs.join(&list(&runs)[0].id);
        let manifest: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join(MANIFEST_JSON)).unwrap()).unwrap();

        assert_eq!(manifest["panes"]["orch"]["harness"], "claude-code");
        assert_eq!(manifest["panes"]["orch"]["transcript_format"], "claude-code-jsonl");
        assert!(
            manifest["panes"]["orch"]["model"].is_null(),
            "the attended seat runs the operator's login, and a guessed model name is worse \
             than an absent one",
        );
        assert_eq!(manifest["panes"]["worker-1"]["harness"], "codex");
        assert_eq!(manifest["panes"]["worker-1"]["model"], "a-model");
        assert_eq!(
            manifest["panes"]["worker-1"]["transcript_format"],
            "codex-thread-history-1-sqlite",
        );

        let sentence = manifest["layout"]["transcripts/"].as_str().unwrap();
        assert!(
            !sentence.contains("Claude Code") && !sentence.contains("jsonl"),
            "the manifest still describes every pane's transcripts as one vendor's: {sentence}",
        );
        assert!(sentence.contains("panes"), "and it points at what does vary: {sentence}");
        assert!(manifest["layout"]["panes"].is_string(), "a cold reader is told what `panes` is");
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
