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

use crate::credential_source::CredentialChoice;
use crate::placement::harness::{HarnessSpec, Transport};
use crate::placement::SessionsId;

/// Milliseconds in a day — the unit the civil-date conversion counts in.
const MS_PER_DAY: i64 = 86_400_000;

/// The agent-facing export, written into every run at archive time.
const EVENTS_JSON: &str = "events.json";
const TASKS_JSON: &str = "tasks.json";
const RUN_JSON: &str = "run.json";
/// What the run is and what else is in its directory, for a reader arriving cold.
const MANIFEST_JSON: &str = "manifest.json";

/// **What `transcripts/` holds, said without naming a vendor** (M24, #39).
///
/// It used to say "that pane's Claude Code session `.jsonl` files", which was true
/// of every run a one-harness fleet could produce and becomes a false statement to
/// any reader of a mixed run cold — the wrong kind of false, too: a reader
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
    /// **Which `pane-config/<id>/` this run's seats live in** (WP-27, R4) — its
    /// own id for a fresh run, the lineage root's for a reopened one. `None` on a
    /// run archived before WP-27, which is one of the two things that makes a run
    /// unopenable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sessions: Option<String>,
    /// The run this one was reopened from (WP-27, R1). Walked to build a lineage;
    /// History shows one row per lineage, the newest (R12).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// **How many times this lineage has been reopened** — derived from `parent`
    /// links at [`list`] time, never stored as a mutable number (Tier 1.6).
    #[serde(default)]
    pub reopened: u32,
    /// **Why this run cannot be reopened, if it cannot** (WP-27, R8, R10).
    ///
    /// A sentence naming the seat or the harness responsible, computed from the
    /// manifest at [`list`] time. `None` means it opens. R8 refuses a run with any
    /// seat that cannot be restored, so this is a whole-run answer — and stating
    /// it on the row is what keeps History from offering a click that fails.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cannot_reopen: Option<String>,
    /// Where the run is on its way into History (D-099). Derived at
    /// [`list_all`] time from where its folder is, never trusted from the index.
    #[serde(default)]
    pub state: RunState,
}

/// One row, three states: the run id is the same through all of them.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RunState {
    /// The fleet that is up.
    Running,
    /// Staged under `_shell/archiving/`, its archive not finished.
    Archiving,
    #[default]
    Archived,
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
    /// The id this run is archived under, claimed at launch ([`new_run_id`]).
    /// `None` on a run begun before D-099, which is named by its start time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    started_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    target: Option<String>,
    /// **Which `pane-config/<id>/` this run's seats live in** (WP-27, R4).
    ///
    /// Its own id for a fresh run; the *lineage root's* for a reopened one, because
    /// a lineage shares one directory (R4). Recorded rather than derived — that is
    /// what lets R15 delete a lineage's sessions by asking which runs name it, and
    /// what keeps a reopened run from archiving a directory that is not its own.
    ///
    /// `None` on a run archived before WP-27, which is also what makes such a run
    /// unopenable and is why the History row says so (R8).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sessions: Option<String>,
    /// The run this one was reopened from, if any (WP-27, R1). One link, walked to
    /// build a lineage; the row the operator sees is the newest of the chain (R12).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    parent: Option<String>,
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
/// answer, and this is what tells a reader of the result cold *which* vendor
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
    /// Whose usage the seat spent, so a reopen restores the pair the model belongs
    /// to. Absent on runs recorded before D-095.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential: Option<CredentialChoice>,
    /// Checkpoint 13's `format`, so the transcripts filed under this pane can be
    /// named without knowing which vendor wrote them.
    pub transcript_format: String,
    /// **The resumable session id for this seat** (WP-27, R6) — checkpoint 15's
    /// reader, run once at rotation against the seat's own directory.
    ///
    /// `None` when the seat opened no resumable session: a pane that never
    /// started, or one that stopped before its harness wrote anything. R8 refuses
    /// to reopen a run with any such seat, which is why this is the field History
    /// checks rather than a count.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

// --- locations ----------------------------------------------------------------

/// The archive root. A sibling of `_shell/` rather than a child, because
/// `_shell` is explicitly the disposable working state and this is the one thing
/// under `~/.fleetor` the operator might mind losing.
pub(crate) fn runs_dir(fleetor: &Path) -> PathBuf {
    fleetor.join("runs")
}

/// The `_shell/` beside an archive root — [`runs_dir`]'s inverse, for the one
/// reader handed only the archive root that needs a seat directory: the reopen
/// gate, asking whether a recorded session is still on disk (R19).
fn shell_for(runs: &Path) -> PathBuf {
    runs.parent().unwrap_or(runs).join("_shell")
}

fn index_path(runs: &Path) -> PathBuf {
    runs.join("index.json")
}

fn live_db(shell: &Path) -> PathBuf {
    shell.join("state.db")
}

fn live_meta(shell: &Path) -> PathBuf {
    shell.join(RUN_JSON)
}

/// Where a run torn down by a launch waits for its archive to be finished,
/// one folder per run id (D-099).
fn staging_dir(shell: &Path) -> PathBuf {
    shell.join("archiving")
}

// --- the live run -------------------------------------------------------------

/// Record what the run that is starting now is pointed at, for the History list
/// it will appear in after the *next* start.
///
/// Best-effort on purpose: a failure here costs one row's target column, and is
/// not worth failing a boot over.
pub fn begin(shell: &Path, started_ms: i64, target: &Path, sessions: &SessionsId, parent: Option<&str>) {
    begin_run(shell, None, started_ms, target, sessions, parent);
}

/// [`begin`] for a run whose id was claimed at launch, so the fleet and the
/// archive name it the same thing.
pub fn begin_as(
    shell: &Path,
    id: &str,
    started_ms: i64,
    target: &Path,
    sessions: &SessionsId,
    parent: Option<&str>,
) {
    begin_run(shell, Some(id), started_ms, target, sessions, parent);
}

/// An id for the run starting at `started_ms` that no archive and no seat
/// directory already uses. Two launches in one second would otherwise share a
/// seat directory, and the task store is keyed by it.
pub fn new_run_id(shell: &Path, runs: &Path, started_ms: i64) -> String {
    let base = timestamp_id_for(started_ms);
    let taken = |id: &str| {
        runs.join(id).exists()
            || staging_dir(shell).join(id).exists()
            || crate::placement::pane_config_run(shell, &SessionsId::new(id)).exists()
    };
    (1u32..)
        .map(|n| if n == 1 { base.clone() } else { format!("{base}-{n}") })
        .find(|id| !taken(id))
        .expect("an unbounded search")
}

fn begin_run(
    shell: &Path,
    id: Option<&str>,
    started_ms: i64,
    target: &Path,
    sessions: &SessionsId,
    parent: Option<&str>,
) {
    // The panes are empty here and are filled in one at a time by
    // [`record_pane`], because at bootstrap there are none: rotation has just run
    // and the first pane is several operator gestures away.
    let meta = LiveMeta {
        id: id.map(str::to_string),
        started_ms: Some(started_ms),
        target: Some(target.display().to_string()),
        sessions: Some(sessions.to_string()),
        parent: parent.map(str::to_string),
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
pub fn record_pane(
    shell: &Path,
    pane: &str,
    harness: &'static HarnessSpec,
    model: Option<&str>,
    credential: Option<CredentialChoice>,
) {
    let mut meta = read_live_meta(shell);
    meta.panes.insert(
        pane.to_string(),
        PaneRecord {
            harness: harness.name.to_string(),
            model: model.map(str::to_string),
            credential,
            transcript_format: harness.transcript.format.to_string(),
            // Read at rotation, not here: checkpoint 15's reader wants a finished
            // session, and this runs while the pane is still starting (R6).
            session_id: None,
        },
    );
    if let Ok(text) = serde_json::to_string_pretty(&meta) {
        let _ = std::fs::write(live_meta(shell), text);
    }
}

// --- reopening a past run (WP-27) ----------------------------------------------

/// Why run `id` cannot be reopened, or `None` if it can — the check a caller runs
/// **before tearing anything down** (R8).
///
/// Reading the manifest is cheap and the refusal is total, so there is no reason
/// to discover it after five panes are gone: a reopen that killed the live fleet
/// and then failed would be the worst version of this feature.
pub fn reopen_blocker_for(runs: &Path, id: &str) -> Option<String> {
    if run_dir(runs, id).ok().and_then(|dir| manifest_of(&dir)).is_none() {
        return Some("there is no such run in History".into());
    }
    reopen_blocker(runs, id)
}

/// **Each seat's resumable session id for a lineage** (WP-27, R6) — what the
/// spawn path hands its harness so a pane comes back on the session it left.
///
/// Resolved through the lineage rather than one run, for [`lineage_panes`]'s
/// reason: a reopen quit before its panes registered still knows its seats through
/// its parent, and every member shares one seat directory.
pub fn lineage_session_ids(runs: &Path, id: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let Some(panes) = lineage_panes(runs, id) else { return out };
    for (seat, rec) in panes {
        if let Some(session) = rec.get("session_id").and_then(|v| v.as_str()) {
            out.insert(seat, session.to_string());
        }
    }
    out
}

/// The target run `id` recorded, which a reopen launches against whatever the
/// gate holds (D-099). `None` for a run that recorded none.
pub fn recorded_target(runs: &Path, id: &str) -> Option<PathBuf> {
    let manifest = manifest_of(&run_dir(runs, id).ok()?)?;
    Some(PathBuf::from(manifest.get("run")?.get("target")?.as_str()?))
}

/// What each seat of a lineage was placed as (D-095): the harness, model and
/// credential a reopen brings it back on, whatever the gate is set to now.
pub fn recorded_panes(runs: &Path, id: &str) -> BTreeMap<String, PaneRecord> {
    lineage_panes(runs, id)
        .into_iter()
        .flatten()
        .filter_map(|(seat, rec)| Some((seat, serde_json::from_value(rec).ok()?)))
        .collect()
}

/// What a reopen tells [`begin`] about the run it is starting (WP-27, R1, R4).
pub struct Reopened {
    /// The lineage's seat directory — the parent's, because a lineage shares one.
    pub sessions: SessionsId,
    /// The run this one continues.
    pub parent: String,
}

/// Copy run `id`'s archived log into the live slot (R1).
///
/// Called by a reopen launch **after [`rotate`] and before the store is opened**,
/// the only window in which the slot is both empty and unopened.
///
/// **The parent is read, never written.** D-058's invariant is that a new run
/// cannot be corrupted by an old one; copying satisfies it exactly, and the
/// archive stays frozen (R1).
pub fn seed_reopen(shell: &Path, runs: &Path, id: &str) -> Result<Reopened, String> {
    let dir = run_dir(runs, id)?;
    if let Some(why) = reopen_blocker(runs, id) {
        return Err(format!("cannot reopen {id}: {why}"));
    }
    let manifest = manifest_of(&dir).ok_or_else(|| format!("run {id} has no manifest"))?;
    let sessions = manifest
        .get("run")
        .and_then(|r| r.get("sessions"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("run {id} records no session directory"))?
        .to_string();

    // The board and the message history come with the log, which is the whole
    // reason R1 copies it rather than starting empty.
    if let Err(e) = std::fs::copy(dir.join("state.db"), live_db(shell)) {
        let _ = std::fs::remove_file(live_db(shell));
        return Err(format!("seed the live run from {id}: {e}"));
    }
    // A copied database must not inherit a stale journal from the live slot.
    for suffix in ["-wal", "-shm"] {
        let _ = std::fs::remove_file(with_suffix(&live_db(shell), suffix));
    }
    Ok(Reopened { sessions: SessionsId::new(sessions), parent: id.to_string() })
}

// --- rotation -----------------------------------------------------------------

/// Archive whatever is not archived yet and leave `_shell` ready for a fresh
/// database: every staged run, then the live slot. Returns the notices the
/// caller should put on the feed.
///
/// The synchronous form of the one archive path, for app open and quit. A
/// launch calls [`stage`] itself and hands [`finish`] to the background.
/// Never returns `Err`: see the module doc.
pub fn rotate(shell: &Path, runs: &Path, now_ms: i64) -> Vec<(NoticeLevel, String)> {
    let mut notices = sweep(shell, runs);
    let (staged, staging) = stage(shell, runs, now_ms);
    notices.extend(staging);
    if let Some(id) = staged {
        notices.extend(finish(shell, runs, &id));
    }
    notices
}

/// **Stage the live run: the fast, foreground half of archiving** (D-099).
///
/// Moves the log and the run's identity into `_shell/archiving/<id>/`, with
/// its session ids, transcripts and `tasks.json` — everything that must be read
/// before the next fleet's panes exist, because a reopen shares its lineage's
/// seat directories. Answers the staged run's id, or `None` with no live run.
///
/// The log moves last: a folder without one was interrupted before anything
/// left the live slot, and [`sweep`] discards it.
pub fn stage(shell: &Path, runs: &Path, now_ms: i64) -> (Option<String>, Vec<(NoticeLevel, String)>) {
    let live = live_db(shell);
    if !live.is_file() {
        return (None, Vec::new()); // first run on this machine — nothing to archive
    }
    let left_in_place = |e: String| {
        (
            None,
            vec![(
                NoticeLevel::Warn,
                format!(
                    "the previous run was left in place, not archived ({e}). \
                     Its log is still at {} and the fleet is starting on top of it.",
                    live.display()
                ),
            )],
        )
    };

    let mut meta = read_live_meta(shell);
    let started = meta.started_ms.or_else(|| file_started_ms(&live)).unwrap_or(now_ms);
    let claimed = meta.id.clone().unwrap_or_else(|| timestamp_id_for(started));
    let dir = match reserve(shell, runs, &claimed) {
        Ok(dir) => dir,
        Err(e) => return left_in_place(e.to_string()),
    };
    let id = dir.file_name().unwrap_or_default().to_string_lossy().to_string();

    // **Which directory this run's seats were in** (R4). A run begun before
    // WP-27 has no answer and gets none invented: its `session_id`s stay `None`
    // and R8 refuses to reopen it.
    let sessions = meta.sessions.clone().map(SessionsId::new);
    if let Some(sessions) = &sessions {
        // Checkpoint 15's reader (R6): the run is over, so the id is settled.
        capture_session_ids(shell, sessions, &mut meta.panes);
        walk_transcripts(shell, sessions, &dir);
    }
    meta.id = Some(id.clone());
    let mut notices = Vec::new();
    notices.extend(stage_tasks(shell, &dir, &id, meta.target.as_deref()));

    let staged = serde_json::to_string_pretty(&meta)
        .map_err(|e| e.to_string())
        .and_then(|text| std::fs::write(dir.join(RUN_JSON), text).map_err(|e| e.to_string()))
        .and_then(|()| std::fs::rename(&live, dir.join("state.db")).map_err(|e| e.to_string()));
    if let Err(e) = staged {
        let _ = std::fs::remove_dir_all(&dir);
        return left_in_place(e);
    }
    for suffix in ["-wal", "-shm"] {
        let from = with_suffix(&live, suffix);
        if from.is_file() {
            let _ = std::fs::rename(&from, dir.join(format!("state.db{suffix}")));
        }
    }
    let _ = std::fs::remove_file(live_meta(shell));
    (Some(id), notices)
}

/// One finish at a time: the background finish a launch handed off, the sweep
/// and a quit can all reach the same staged folder.
static FINISHING: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Keep every finish waiting, so a test can look at a run mid-archive.
#[cfg(test)]
pub(crate) fn hold_finishes() -> std::sync::MutexGuard<'static, ()> {
    FINISHING.lock().unwrap_or_else(|e| e.into_inner())
}

/// **Finish a staged run: the slow half of archiving** (D-099).
///
/// Freezes the log to one file, writes the readable export and the manifest,
/// then moves the whole folder into `runs/` in one rename — so History never
/// holds half an archive, and an interrupted finish is simply run again.
/// A run that is not staged (already finished, or never was) is a no-op.
pub fn finish(shell: &Path, runs: &Path, id: &str) -> Vec<(NoticeLevel, String)> {
    let _one = FINISHING.lock().unwrap_or_else(|e| e.into_inner());
    let dir = staging_dir(shell).join(id);
    let db = dir.join("state.db");
    if !db.is_file() {
        let _ = std::fs::remove_dir_all(&dir);
        return Vec::new();
    }
    let meta = read_meta(&dir.join(RUN_JSON));
    let mut notices = Vec::new();

    // A log that will not freeze — corrupt, or held open — is archived as the
    // three files it is: those are the runs whose evidence is worth keeping.
    if archive::freeze(&db).is_ok() {
        for suffix in ["-wal", "-shm"] {
            let _ = std::fs::remove_file(with_suffix(&db, suffix));
        }
    }
    if !dir.join(TASKS_JSON).is_file() {
        notices.extend(stage_tasks(shell, &dir, id, meta.target.as_deref()));
    }

    let dest = match free_archive(runs, id) {
        Ok(dest) => dest,
        Err(e) => return vec![(NoticeLevel::Warn, format!("could not archive run {id}: {e}"))],
    };
    let id = dest.file_name().unwrap_or_default().to_string_lossy().to_string();
    let record = match record_for(&dir, &id, meta.target.as_deref()) {
        Ok(mut record) => {
            // The two identity fields a reopen is gated on (R4, R1), off the
            // run's own meta because neither is derivable from the log.
            record.sessions = meta.sessions.clone();
            record.parent = meta.parent.clone();
            if let Err(e) = write_agent_view(&dir, &record, &meta.panes) {
                notices.push((
                    NoticeLevel::Warn,
                    format!("archived run {id}, but its JSON export did not write: {e}"),
                ));
            }
            Some(record)
        }
        Err(e) => {
            notices.push((NoticeLevel::Warn, format!("archived run {id}, but could not read it back: {e}")));
            None
        }
    };

    if let Err(e) = std::fs::rename(&dir, &dest) {
        notices.push((
            NoticeLevel::Warn,
            format!("run {id} is staged at {} and could not be moved into History: {e}", dir.display()),
        ));
        return notices;
    }
    let _ = std::fs::remove_file(dest.join(RUN_JSON));
    if let Some(mut record) = record {
        record.bytes = dir_bytes(&dest);
        let label = record.label.clone();
        if let Err(e) = upsert(runs, record) {
            notices.push((NoticeLevel::Warn, format!("archived run {id}, but its index entry did not save: {e}")));
        } else {
            notices.push((NoticeLevel::Info, format!("previous run archived as “{label}” — see History")));
        }
    }
    notices
}

/// Finish every run a crash, a quit or a launch left staged.
pub fn sweep(shell: &Path, runs: &Path) -> Vec<(NoticeLevel, String)> {
    let Ok(staged) = std::fs::read_dir(staging_dir(shell)) else { return Vec::new() };
    let mut ids: Vec<String> =
        staged.flatten().map(|entry| entry.file_name().to_string_lossy().to_string()).collect();
    ids.sort();
    ids.iter().flat_map(|id| finish(shell, runs, id)).collect()
}

/// The `tasks.json` hook (D-099, Coordination C): written at stage, and by a
/// finish for a staged folder that lacks it.
fn stage_tasks(shell: &Path, dir: &Path, id: &str, target: Option<&str>) -> Option<(NoticeLevel, String)> {
    match target {
        Some(target) => archive_tasks(shell, dir, id, Path::new(target))
            .err()
            .map(|e| (NoticeLevel::Warn, format!("archived run {id}, but its tasks.json did not write: {e}"))),
        None => Some((NoticeLevel::Info, format!("run {id} recorded no target, so its archive has no tasks.json"))),
    }
}

/// Write the archived run's `tasks.json`: the goals and tasks this run
/// touched, each with its whole chain (D-099 Coordination C, D-100). Read from
/// the target's task store read-only, after the fleet that held it is gone.
fn archive_tasks(shell: &Path, dest: &Path, run_id: &str, target: &Path) -> Result<(), String> {
    let root = shell.parent().ok_or("the shell has no parent directory")?;
    let store = crate::placement::Layout::under(root).task_store(target);
    let events = if store.is_file() {
        archive::events(&store, 0).map_err(|e| e.to_string())?
    } else {
        Vec::new()
    };
    let touched: Vec<_> = fleetor_core::task::board(events.iter().map(|(_, event)| event))
        .into_iter()
        .filter(|record| record.chain.iter().any(|line| line.run == run_id))
        .collect();
    let json = serde_json::json!({ "run": run_id, "target": target, "tasks": touched });
    std::fs::write(dest.join(TASKS_JSON), serde_json::to_string_pretty(&json).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}

/// How many tasks an archived run touched, from its `tasks.json`. `None` for a
/// run archived before tasks left the run log, whose count is the digest's.
fn archived_task_count(dir: &Path) -> Option<i64> {
    let text = std::fs::read_to_string(dir.join(TASKS_JSON)).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    Some(json.get("tasks")?.as_array()?.len() as i64)
}

/// Take one transcript into the archive the way its harness says it may be taken,
/// and answer whether it arrived.
///
/// How a file is read is the *harness's* answer, which is all this branches on.
fn take_transcript(from: &Path, to: &Path, transport: Transport) -> bool {
    match transport {
        Transport::Rename => std::fs::copy(from, to).is_ok(),
        Transport::SqliteBackup => sqlite_backup(from, to).is_ok(),
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

/// Copy one run's seat transcripts into `dest/transcripts/<seat>/` (WP-27, R3, R4).
///
/// **Nothing moves any more, and `Take` went with the move.** D-059 moved
/// transcripts out so an archive would not accumulate its predecessors; R3 keeps
/// that goal and drops the mechanism, because R4 made a run's sessions a
/// *directory* — so an archive is scoped by which directory it walks rather than
/// by emptying the one it walked. The two verbs this function used to take
/// collapsed to one the moment rotation stopped being destructive.
///
/// **The vendor's session stays where the vendor keeps it**, which is what makes
/// reopening `--resume <id>` against a file that never left, with no restore step
/// that could half-fail.
fn walk_transcripts(shell: &Path, sessions: &SessionsId, dest: &Path) -> u32 {
    let Ok(panes) = std::fs::read_dir(crate::placement::pane_config_run(shell, sessions)) else {
        return 0;
    };
    let mut taken = 0;

    for pane in panes.flatten() {
        let name = pane.file_name();
        let into = dest.join("transcripts").join(&name);

        for at in transcript_locations() {
            for from in transcript_files_for(&pane.path().join(at.subdir), at.file_ext) {
                let Some(file_name) = from.file_name() else { continue };
                if std::fs::create_dir_all(&into).is_err() {
                    continue;
                }
                if take_transcript(&from, &into.join(file_name), at.transport) {
                    taken += 1;
                }
            }
        }
    }
    taken
}

/// Fill each pane's [`PaneRecord::session_id`] by asking its own harness
/// (WP-27, R6 — checkpoint 15's reader).
///
/// **Keyed off the harness the manifest already records**, which is the reversal
/// C33 named and this is the half of it that applies: unlike the transcript walk
/// — which runs before anything has been written down and so must look under every
/// registered harness — this runs *after* `record_pane` has said what each seat
/// ran, so it can ask that harness and no other. A seat whose harness is not
/// registered in this build, or that opened no resumable session, keeps `None`.
fn capture_session_ids(shell: &Path, sessions: &SessionsId, panes: &mut BTreeMap<String, PaneRecord>) {
    let root = crate::placement::pane_config_run(shell, sessions);
    for (seat, record) in panes.iter_mut() {
        let Some(harness) = crate::placement::harness::by_name(&record.harness) else { continue };
        record.session_id = harness.session_id(&root.join(seat));
    }
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
pub(crate) fn transcript_files_for(root: &Path, file_ext: &str) -> Vec<PathBuf> {
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
            "tasks.json": "the goals and tasks this run touched, each with its whole chain; absent on runs archived before tasks had their own store",
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

/// Claim an unused staging folder for `id`, disambiguating rather than
/// overwriting. Two runs can share a start second — a fast restart, or a clock
/// that did not move — and silently merging them would lose one.
fn reserve(shell: &Path, runs: &Path, id: &str) -> std::io::Result<PathBuf> {
    let staging = staging_dir(shell);
    std::fs::create_dir_all(&staging)?;
    for name in variants(id) {
        if runs.join(&name).exists() {
            continue;
        }
        match std::fs::create_dir(staging.join(&name)) {
            Ok(()) => return Ok(staging.join(&name)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::other(format!("{id} and 99 variants of it are all taken")))
}

/// Where a staged run lands in `runs/`: its own id, which [`reserve`] kept free.
fn free_archive(runs: &Path, id: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(runs)?;
    variants(id)
        .map(|name| runs.join(name))
        .find(|dir| !dir.exists())
        .ok_or_else(|| std::io::Error::other(format!("{id} and 99 variants of it are all taken")))
}

fn variants(id: &str) -> impl Iterator<Item = String> + '_ {
    (1..=100).map(move |n| if n == 1 { id.to_string() } else { format!("{id}-{n}") })
}

// --- the index ----------------------------------------------------------------

/// Every archived run, newest first.
///
/// The directories are authoritative: an indexed run whose directory is gone is
/// dropped, and a directory the index has never heard of is read and added. So
/// deleting `index.json` costs labels and nothing else.
pub fn list(runs: &Path) -> Vec<RunRecord> {
    ordered(archived(runs))
}

/// **History as the operator sees it** (D-099): the archived runs, every run
/// still staged, and the live one when a fleet is up — `live_tasks` is its
/// count from the live task store. One row per lineage, newest first.
pub fn list_all(shell: &Path, runs: &Path, live_tasks: Option<i64>) -> Vec<RunRecord> {
    // Staged before archived: a finish that lands between the two reads shows
    // the run twice here, never zero times, and the archived copy wins.
    let mut rows = staged(shell);
    if let Some(tasks) = live_tasks {
        rows.extend(running(shell, tasks));
    }
    let done = archived(runs);
    rows.retain(|row| !done.iter().any(|r| r.id == row.id));
    rows.extend(done);
    ordered(rows)
}

fn ordered(rows: Vec<RunRecord>) -> Vec<RunRecord> {
    let mut out = collapse_lineages(rows);
    out.sort_by(|a, b| b.started_ms.cmp(&a.started_ms).then_with(|| b.id.cmp(&a.id)));
    out
}

fn archived(runs: &Path) -> Vec<RunRecord> {
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
        let mut record = match indexed.remove(&id) {
            Some(record) => record,
            None => match record_for(&dir, &id, None) {
                Ok(record) => {
                    healed = true;
                    record
                }
                Err(_) => continue,
            },
        };
        record.bytes = dir_bytes(&dir);
        // Derived from the archive on every read, never trusted from the index:
        // the index is a cache and these gate a reopen (R8).
        let manifest = manifest_of(&dir);
        record.sessions = manifest
            .as_ref()
            .and_then(|m| m.get("run")?.get("sessions")?.as_str().map(str::to_string));
        record.parent = manifest
            .as_ref()
            .and_then(|m| m.get("run")?.get("parent")?.as_str().map(str::to_string));
        record.cannot_reopen = reopen_blocker(runs, &id);
        record.state = RunState::Archived;
        out.push(record);
    }

    if healed || !indexed.is_empty() {
        let _ = write_index(runs, &out);
    }
    out
}

/// Every run in `_shell/archiving/`. One whose log cannot be read right now —
/// a finish may be freezing it — still gets its row, without counts.
fn staged(shell: &Path) -> Vec<RunRecord> {
    let Ok(entries) = std::fs::read_dir(staging_dir(shell)) else { return Vec::new() };
    entries
        .flatten()
        .filter(|entry| entry.path().join("state.db").is_file())
        .map(|entry| {
            let dir = entry.path();
            let id = entry.file_name().to_string_lossy().to_string();
            let meta = read_meta(&dir.join(RUN_JSON));
            let mut record = record_for(&dir, &id, meta.target.as_deref())
                .unwrap_or_else(|_| bare_record(&id, &meta));
            record.sessions = meta.sessions;
            record.parent = meta.parent;
            record.state = RunState::Archiving;
            record
        })
        .collect()
}

/// The live run, read from the live slot. `None` for a run begun before runs
/// claimed their id at launch, which has no row identity to keep.
fn running(shell: &Path, tasks: i64) -> Option<RunRecord> {
    let meta = read_live_meta(shell);
    let id = meta.id.clone()?;
    let db = live_db(shell);
    let mut record = digest_record(&db, &id, meta.target.as_deref()).unwrap_or_else(|_| bare_record(&id, &meta));
    record.tasks = tasks;
    record.bytes = std::fs::metadata(&db).map(|m| m.len()).unwrap_or(0);
    record.sessions = meta.sessions;
    record.parent = meta.parent;
    record.state = RunState::Running;
    Some(record)
}

fn bare_record(id: &str, meta: &LiveMeta) -> RunRecord {
    RunRecord {
        id: id.to_string(),
        label: suggest_label(meta.target.as_deref(), None),
        target: meta.target.clone(),
        started_ms: meta.started_ms,
        ended_ms: None,
        events: 0,
        messages: 0,
        tasks: 0,
        bytes: 0,
        transcripts: 0,
        sessions: None,
        parent: None,
        reopened: 0,
        cannot_reopen: None,
        state: RunState::Archived,
    }
}

/// How many goals and tasks `run_id` touched, from a task store's events — the
/// same rule [`archive_tasks`] writes `tasks.json` by.
pub fn tasks_touched(events: &[(i64, FleetEvent)], run_id: &str) -> i64 {
    fleetor_core::task::board(events.iter().map(|(_, event)| event))
        .into_iter()
        .filter(|record| record.chain.iter().any(|line| line.run == run_id))
        .count() as i64
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

/// Delete a run and cascade to its lineage's `pane-config/` when no sibling
/// remains (R15).
pub fn delete(runs: &Path, shell: &Path, id: &str) -> Result<(), String> {
    let dir = run_dir(runs, id)?;
    let sessions = manifest_of(&dir)
        .and_then(|m| m.get("run")?.get("sessions")?.as_str().map(str::to_string));

    std::fs::remove_dir_all(&dir).map_err(|e| format!("removing {}: {e}", dir.display()))?;
    let remaining = list(runs);
    write_index(runs, &remaining).map_err(|e| e.to_string())?;

    if let Some(ref sid) = sessions {
        let still_used = remaining.iter().any(|r| r.sessions.as_deref() == Some(sid.as_str()));
        if !still_used {
            let _ = std::fs::remove_dir_all(
                crate::placement::pane_config_run(shell, &SessionsId::new(sid)),
            );
        }
    }
    Ok(())
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
    let mut record = digest_record(&dir.join("state.db"), id, target)?;
    record.tasks = archived_task_count(dir).unwrap_or(record.tasks);
    record.bytes = dir_bytes(dir);
    record.transcripts = count_transcripts(dir);
    Ok(record)
}

/// What a log says about its run, and nothing its folder would add.
fn digest_record(db: &Path, id: &str, target: Option<&str>) -> Result<RunRecord, String> {
    let digest = archive::digest(db).map_err(|e| e.to_string())?;
    Ok(RunRecord {
        label: suggest_label(target, digest.headline.as_deref()),
        id: id.to_string(),
        target: target.map(str::to_string),
        started_ms: digest.first_ts,
        ended_ms: digest.last_ts,
        events: digest.events,
        messages: digest.messages,
        tasks: digest.tasks,
        bytes: 0,
        transcripts: 0,
        sessions: None,
        parent: None,
        reopened: 0,
        cannot_reopen: None,
        state: RunState::Archived,
    })
}

/// Collapse each lineage to its newest run (WP-27, R12).
///
/// **A lineage is one conversation at several lengths, not several
/// conversations.** R1 makes a reopened run's log a superset of its parent's and
/// R3 has them share one live session, so offering the parent as its own row would
/// hand back the child's panes under the parent's label. Every archive stays on
/// disk and every one still exports; what collapses is the *row*.
///
/// The count of hidden ancestors becomes `reopened`, so the row can explain its
/// own inherited totals (R1's stated cost) rather than quietly inflating them.
fn collapse_lineages(records: Vec<RunRecord>) -> Vec<RunRecord> {
    let parents: std::collections::HashSet<String> =
        records.iter().filter_map(|r| r.parent.clone()).collect();
    let by_id: BTreeMap<&str, &RunRecord> = records.iter().map(|r| (r.id.as_str(), r)).collect();

    records
        .iter()
        .filter(|r| !parents.contains(&r.id))
        .map(|r| {
            // Walk back to the root, counting. Bounded by the number of runs, and
            // guarded anyway: a hand-edited manifest could name a cycle.
            let mut hops = 0u32;
            let mut seen = std::collections::HashSet::new();
            let mut at = r.parent.as_deref();
            while let Some(id) = at {
                if !seen.insert(id.to_string()) {
                    break;
                }
                hops += 1;
                at = by_id.get(id).and_then(|p| p.parent.as_deref());
            }
            let mut row = r.clone();
            row.reopened = hops;
            row
        })
        .collect()
}

/// The nearest record of **which seats this lineage ran**, walking back from `id`
/// (WP-27, R12).
///
/// A lineage shares one seat directory, so a member's pane record describes a seat
/// its ancestors ran too; what varies is how many seats a given member lived long
/// enough to write down. Merged per seat from `id` backwards, nearest first, so a
/// reopen quit immediately — or partway through placing its panes — still
/// resolves every seat through whichever ancestor recorded it (R20).
///
/// Bounded by the archives on disk and cycle-guarded, because a hand-edited
/// manifest could name its own ancestor.
fn lineage_panes(runs: &Path, id: &str) -> Option<serde_json::Map<String, serde_json::Value>> {
    let mut seen = std::collections::HashSet::new();
    let mut at = Some(id.to_string());
    let mut merged: Option<serde_json::Map<String, serde_json::Value>> = None;
    while let Some(current) = at {
        if !seen.insert(current.clone()) {
            break;
        }
        let Ok(dir) = run_dir(runs, &current) else { break };
        let Some(manifest) = manifest_of(&dir) else { break };
        // **Per seat, nearest record wins** (R20) — not the nearest non-empty map.
        // Panes are recorded one at a time as they spawn, so a reopen quit after
        // two of five, or one whose worker seats were refused placement, records
        // a partial map; taking it whole hid the parent's other seats, and the
        // next reopen started them on fresh sessions without a word.
        if let Some(panes) = manifest.get("panes").and_then(|p| p.as_object()) {
            let into = merged.get_or_insert_with(serde_json::Map::new);
            // A record that names a session beats a nearer one that does not
            // (D-095): a seat that came up without resuming must not hide the
            // session its ancestor left.
            let has_session = |r: &serde_json::Value| r.get("session_id").is_some_and(|v| v.is_string());
            for (seat, record) in panes {
                match into.get(seat) {
                    Some(held) if has_session(held) || !has_session(record) => {}
                    _ => {
                        into.insert(seat.clone(), record.clone());
                    }
                }
            }
        }
        at = manifest
            .get("run")
            .and_then(|r| r.get("parent"))
            .and_then(|v| v.as_str())
            .map(str::to_string);
    }
    merged
}

/// What a run's own `manifest.json` says about reopening it (WP-27).
///
/// Read from the archive rather than the index, because the index is a *cache*
/// (D-058) and this is the answer a reopen is actually gated on — a lost index
/// must cost labels, never the ability to tell a reopenable run from one that
/// would hang.
fn manifest_of(dir: &Path) -> Option<serde_json::Value> {
    serde_json::from_str(&std::fs::read_to_string(dir.join(MANIFEST_JSON)).ok()?).ok()
}

/// Why this run cannot be reopened, or `None` if it can (WP-27, R8, R10).
///
/// **Whole-run, and the order of the checks is the order the operator can act
/// on.** A run with no recorded session directory predates the feature and never
/// will open; a seat whose harness declares no resume support names the harness;
/// a seat whose recorded session has since left the disk names the seat (R19).
///
/// **A seat that recorded no session at all is not a blocker** (R24). Claude Code
/// writes a seat's transcript only once something happens in that pane, so an
/// untouched worker — the common case in a short run — has none, and refusing the
/// whole run over it made most sessions unopenable. That seat comes back fresh:
/// `ctx.resume` simply has no entry for it and the spawn path falls back to
/// `command_args`, brief and all.
fn reopen_blocker(runs: &Path, id: &str) -> Option<String> {
    use crate::placement::harness::{by_name, Resume};

    let manifest = manifest_of(&run_dir(runs, id).ok()?)?;
    // The cause only. What survives a refusal — the log and the transcripts — is
    // the same sentence for every cause, so the History row says it, once (R8).
    let Some(sessions) =
        manifest.get("run").and_then(|r| r.get("sessions")).and_then(|v| v.as_str())
    else {
        return Some("archived before sessions were recorded".into());
    };

    // **The seats come from the lineage, not from this run alone.**
    //
    // A reopened run inherits its parent's seat directory (R4) but starts with an
    // empty `panes` map, filled in one pane at a time as they spawn. Quit before
    // that finishes — which takes seconds — and the child records no panes at all,
    // while every session it would reopen is sitting in the shared directory and
    // named by an ancestor's manifest. R12 then shows the child as the lineage's
    // row, so asking *it* refuses a row whose sessions are all present.
    //
    // Measured on a real reopen rather than reasoned about: run 19-41-01Z had
    // `panes: {}` and `transcripts: 5`, and its parent named all five ids.
    let panes = lineage_panes(runs, id)?;
    if panes.is_empty() {
        return Some("no panes were placed in this run".into());
    }
    let seats = crate::placement::pane_config_run(&shell_for(runs), &SessionsId::new(sessions));
    for (seat, rec) in &panes {
        let name = rec.get("harness").and_then(|v| v.as_str()).unwrap_or("an unknown harness");
        let Some(harness) = by_name(name) else {
            return Some(format!("{seat} ran {name}, which this build does not have"));
        };
        if let Resume::NotSupported { why } = &harness.spec().resume {
            return Some(format!("{seat} ran {name}, which {why}"));
        }
        // Nothing recorded, nothing to resume, nothing to refuse: this seat comes
        // back fresh (R24). Only a seat that *had* a session is gated below.
        let Some(session) = rec.get("session_id").and_then(|v| v.as_str()) else { continue };
        // **Recorded is not present** (R19). The id says the session existed at
        // rotation; a seat directory deleted since leaves the id behind, and the
        // vendor's resume would then fail in a pane the live fleet was killed for.
        if !harness.has_session(&seats.join(seat), session) {
            return Some(format!("{seat}\u{2019}s session is no longer on disk, so there is nothing to resume"));
        }
    }
    None
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
    read_meta(&live_meta(shell))
}

fn read_meta(file: &Path) -> LiveMeta {
    std::fs::read_to_string(file)
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
pub fn timestamp_id_for(ms: i64) -> String {
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
        begin(&shell, 1_800_000_000_000, Path::new("/tmp/logstat"), &SessionsId::new("run-1"), None);
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

        begin(&shell, 1_800_000_000_000, Path::new("/tmp/logstat"), &SessionsId::new("run-1"), None);
        write_live(&shell, &["first run"]);
        rotate(&shell, &runs, 0);

        begin(&shell, 1_900_000_000_000, Path::new("/tmp/logstat"), &SessionsId::new("run-1"), None);
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
        begin(&shell, 1_800_000_000_000, Path::new("/tmp/logstat"), &SessionsId::new("run-1"), None);
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
        begin(&shell, 1_800_000_000_000, Path::new("/tmp/logstat"), &SessionsId::new("run-1"), None);
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
    fn a_run_copies_its_workers_transcripts_and_leaves_the_originals_in_place() {
        let root = scratch("transcripts");
        let shell = root.join("_shell");
        let runs = runs_dir(&root);
        // Two workers, one with two sessions — the shape a real config dir has.
        for (worker, sessions) in [("worker-1", &["a", "b"][..]), ("worker-2", &["c"][..])] {
            let project =
                shell.join("pane-config/run-1").join(worker).join("projects").join("-tmp-slug");
            std::fs::create_dir_all(&project).unwrap();
            for s in sessions {
                std::fs::write(project.join(format!("{s}.jsonl")), r#"{"type":"user"}"#).unwrap();
            }
            std::fs::write(project.join("not-a-transcript.txt"), "ignore me").unwrap();
        }
        begin(&shell, 1_800_000_000_000, Path::new("/tmp/logstat"), &SessionsId::new("run-1"), None);
        write_live(&shell, &["take the parser"]);

        rotate(&shell, &runs, 0);

        let all = list(&runs);
        assert_eq!(all[0].transcripts, 3, "two workers, three sessions");
        let dir = runs.join(&all[0].id);
        assert!(dir.join("transcripts/worker-1/a.jsonl").is_file());
        assert!(dir.join("transcripts/worker-2/c.jsonl").is_file());
        assert!(!dir.join("transcripts/worker-1/not-a-transcript.txt").exists());

        // **WP-27 R3: copied, and the original stays.** D-059 moved transcripts so
        // an archive would not accumulate its predecessors; R4's per-run directory
        // now does that job, and leaving the session where the vendor keeps it is
        // what makes reopening `--resume <id>` against a file that never moved.
        let left = shell.join("pane-config/run-1/worker-1/projects/-tmp-slug");
        assert!(left.join("a.jsonl").is_file(), "the vendor's own session must survive the archive");
        assert!(left.join("not-a-transcript.txt").is_file(), "only .jsonl is archived");
        // And the accumulation D-059 feared is answered by the directory, not the
        // move: another run's sessions are simply not in this run's walk.
        let other = shell.join("pane-config/run-2/worker-1/projects/-tmp-slug");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join("z.jsonl"), r#"{"type":"user"}"#).unwrap();
        assert!(!dir.join("transcripts/worker-1/z.jsonl").exists(), "another run's session is not this run's evidence");
    }

    /// WP-14, D-062: the pane that makes the decisions is archived like every
    /// other one. `orch`'s config dir is a sibling of the workers' under the same
    /// `pane-config/` root, so rotation files it under `transcripts/orch/` with
    /// no arm of its own — and anyone reading the run gets the reasoning
    /// behind the messages, not only the messages.
    #[test]
    fn a_run_takes_orchs_transcript_with_it_the_same_way_it_takes_a_workers() {
        let root = scratch("orch-transcript");
        let shell = root.join("_shell");
        let runs = runs_dir(&root);
        for (pane, session) in [("orch", "o1"), ("worker-1", "w1")] {
            let project =
                shell.join("pane-config/run-1").join(pane).join("projects").join("-tmp-slug");
            std::fs::create_dir_all(&project).unwrap();
            std::fs::write(project.join(format!("{session}.jsonl")), r#"{"type":"user"}"#).unwrap();
        }
        begin(&shell, 1_800_000_000_000, Path::new("/tmp/logstat"), &SessionsId::new("run-1"), None);
        write_live(&shell, &["take the parser"]);

        rotate(&shell, &runs, 0);

        let all = list(&runs);
        assert_eq!(all[0].transcripts, 2, "orch counts like any other pane");
        let dir = runs.join(&all[0].id);
        assert!(dir.join("transcripts/orch/o1.jsonl").is_file(), "the deciding pane is archived");
        assert!(dir.join("transcripts/worker-1/w1.jsonl").is_file());
        // Copied, not moved (WP-27 R3) — the vendor's session stays where the
        // vendor keeps it, which is what `--resume` reopens.
        assert!(shell.join("pane-config/run-1/orch/projects/-tmp-slug/o1.jsonl").is_file());

        let manifest: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join(MANIFEST_JSON)).unwrap()).unwrap();
        let layout = manifest["layout"]["transcripts/"].as_str().unwrap();
        assert!(layout.contains("orch"), "a cold reader is told orch is in there: {layout}");
    }

    /// **The manifest carries each seat's resumable session id** (WP-27, R6) —
    /// checkpoint 15's reader, run at rotation, keyed off the harness
    /// `record_pane` already wrote down.
    ///
    /// Claude Code's id is its transcript's own filename, so this plants a
    /// session and asserts the *stem* comes back — the read is exercised, not the
    /// shape of the field.
    #[test]
    fn a_rotated_runs_manifest_carries_each_seats_resumable_session_id() {
        let root = scratch("session-ids");
        let shell = root.join("_shell");
        let runs = runs_dir(&root);

        let session = "9f3c-a-real-looking-session";
        for seat in ["orch", "worker-1"] {
            let project =
                shell.join("pane-config/run-1").join(seat).join("projects").join("-tmp-slug");
            std::fs::create_dir_all(&project).unwrap();
            std::fs::write(project.join(format!("{session}.jsonl")), r#"{"type":"user"}"#).unwrap();
        }
        begin(&shell, 1_800_000_000_000, Path::new("/tmp/logstat"), &SessionsId::new("run-1"), None);
        record_pane(&shell, "orch", crate::placement::harness::claude_code().spec(), None, None);
        record_pane(&shell, "worker-1", crate::placement::harness::claude_code().spec(), None, None);
        write_live(&shell, &["take the parser"]);

        rotate(&shell, &runs, 0);

        let all = list(&runs);
        let dir = runs.join(&all[0].id);
        let manifest: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join(MANIFEST_JSON)).unwrap())
                .unwrap();
        for seat in ["orch", "worker-1"] {
            assert_eq!(
                manifest["panes"][seat]["session_id"], session,
                "{seat} has no resumable id in the manifest: {manifest}",
            );
        }
        // And the run says which seat directory it used, which is what makes a
        // reopen point at the right one and R15 able to count a lineage.
        assert_eq!(manifest["run"]["sessions"], "run-1");
        // With every seat answered, nothing blocks reopening it.
        assert_eq!(all[0].cannot_reopen, None, "{:?}", all[0]);
    }

    /// **A seat that recorded no session comes back fresh rather than blocking the
    /// run** (R24), while the seats that recorded one still resume it.
    ///
    /// This reverses what R8 shipped, on measured grounds: Claude Code writes a
    /// seat's transcript only when something happens in that pane, so a worker
    /// nobody messaged has none — and two real sessions were refused over exactly
    /// that.
    #[test]
    fn a_seat_with_no_recorded_session_comes_back_fresh_instead_of_blocking_the_run() {
        let root = scratch("blocked");
        let shell = root.join("_shell");
        let runs = runs_dir(&root);

        // orch wrote a session; worker-1 was placed and never wrote one.
        let project = shell.join("pane-config/run-1/orch/projects/-tmp-slug");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("abc.jsonl"), r#"{"type":"user"}"#).unwrap();
        begin(&shell, 1_800_000_000_000, Path::new("/tmp/logstat"), &SessionsId::new("run-1"), None);
        record_pane(&shell, "orch", crate::placement::harness::claude_code().spec(), None, None);
        record_pane(&shell, "worker-1", crate::placement::harness::claude_code().spec(), None, None);
        write_live(&shell, &["take the parser"]);

        rotate(&shell, &runs, 0);

        let all = list(&runs);
        assert_eq!(all[0].cannot_reopen, None, "an untouched worker is not a refusal: {:?}", all[0]);
        // And the gate a caller actually consults agrees with the row.
        assert_eq!(reopen_blocker_for(&runs, &all[0].id), None);

        // What the reopen will do with it: orch resumes its own session, worker-1
        // has no entry at all and so is spawned by `command_args`, brief and all.
        let resume = lineage_session_ids(&runs, &all[0].id);
        assert_eq!(resume.get("orch").map(String::as_str), Some("abc"), "{resume:?}");
        assert_eq!(resume.len(), 1, "a seat with nothing to resume is not invented: {resume:?}");
    }

    /// **A run archived before WP-27 says so, and says it once** (R8).
    #[test]
    fn a_run_from_before_sessions_were_recorded_is_unopenable_and_explains_itself() {
        let root = scratch("legacy");
        let shell = root.join("_shell");
        let runs = runs_dir(&root);
        // No `begin`, so no recorded seat directory — exactly a pre-WP-27 archive.
        write_live(&shell, &["take the parser"]);

        rotate(&shell, &runs, 1_800_000_000_000);

        let all = list(&runs);
        let why = all[0].cannot_reopen.as_deref().expect("a pre-WP-27 run cannot be reopened");
        assert!(why.contains("archived before"), "{why}");
        // What survives the refusal is said by the row, once, for every cause —
        // asserted where it renders, `tests/history_row_renders.rs`.
    }

    /// **A reopened run quit before its panes registered still reopens** (WP-27,
    /// R12) — found on a real reopen, not reasoned about.
    ///
    /// A reopen inherits its parent's seat directory but starts with an empty
    /// `panes` map, filled in one pane at a time as they spawn. Quit within a few
    /// seconds and the child records none — while every session it would reopen is
    /// in the shared directory and named by its parent. R12 shows the child as the
    /// lineage's row, so a gate that asked the child alone refused a row whose
    /// sessions were all present and correct.
    #[test]
    fn a_reopen_that_recorded_no_panes_of_its_own_resolves_through_its_parent() {
        let root = scratch("lineage-gate");
        let shell = root.join("_shell");
        let runs = runs_dir(&root);

        // The parent: five-seat-ish run that recorded its sessions properly.
        let project = shell.join("pane-config/root-run/orch/projects/-tmp-slug");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("sess-a.jsonl"), r#"{"type":"user"}"#).unwrap();
        begin(&shell, 1_800_000_000_000, Path::new("/tmp/logstat"), &SessionsId::new("root-run"), None);
        record_pane(&shell, "orch", crate::placement::harness::claude_code().spec(), None, None);
        write_live(&shell, &["take the parser"]);
        rotate(&shell, &runs, 0);
        let parent_id = list(&runs)[0].id.clone();

        // The child: reopened from it, inheriting the seat directory, and archived
        // before any pane got as far as `record_pane`.
        begin(
            &shell,
            1_900_000_000_000,
            Path::new("/tmp/logstat"),
            &SessionsId::new("root-run"),
            Some(&parent_id),
        );
        write_live(&shell, &["still going"]);
        rotate(&shell, &runs, 0);

        let all = list(&runs);
        // R12: one row for the lineage, and it is the child.
        assert_eq!(all.len(), 1, "a lineage is one row: {all:?}");
        assert_eq!(all[0].reopened, 1);
        assert_eq!(
            all[0].cannot_reopen, None,
            "the child recorded no panes, but its lineage's sessions are all present: {:?}",
            all[0].cannot_reopen,
        );
        assert!(reopen_blocker_for(&runs, &all[0].id).is_none());
    }

    /// A one-seat Claude Code run with its session planted and archived — the
    /// starting point for the reopen gate's refusals below. Returns the run's id.
    fn an_archived_one_seat_run(shell: &Path, runs: &Path) -> String {
        let project = shell.join("pane-config/run-1/orch/projects/-tmp-slug");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("abc.jsonl"), r#"{"type":"user"}"#).unwrap();
        begin(shell, 1_800_000_000_000, Path::new("/tmp/logstat"), &SessionsId::new("run-1"), None);
        record_pane(shell, "orch", crate::placement::harness::claude_code().spec(), None, None);
        write_live(shell, &["take the parser"]);
        rotate(shell, runs, 0);
        list(runs)[0].id.clone()
    }

    /// **A session gone from disk refuses the reopen, before the live fleet is
    /// touched** (R8, R19).
    ///
    /// The manifest recorded an id, which says the session existed at rotation —
    /// not that it still does. Clearing `pane-config/` leaves the id behind, and
    /// Claude Code's `--resume` then exits 1 ("No conversation found with session
    /// ID") in a pane the live fleet was already killed for. The gate used to let
    /// exactly that through.
    #[test]
    fn a_session_gone_from_disk_refuses_the_reopen_before_anything_is_torn_down() {
        let root = scratch("gone");
        let shell = root.join("_shell");
        let runs = runs_dir(&root);
        let id = an_archived_one_seat_run(&shell, &runs);
        assert_eq!(list(&runs)[0].cannot_reopen, None, "the control: with its session present it opens");

        std::fs::remove_dir_all(shell.join("pane-config")).unwrap();

        let why = list(&runs)[0].cannot_reopen.clone().expect("a session gone from disk blocks the row");
        assert!(why.contains("orch") && why.contains("no longer on disk"), "{why}");

        assert!(reopen_blocker_for(&runs, &id).is_some(), "and the launch's verdict is asked the same question");
    }

    /// **A seat on a harness this build does not have names the seat and the
    /// harness** (R10) — the branch no registered harness can reach, so it is
    /// reached by editing the manifest a newer build would have written.
    #[test]
    fn a_seat_on_a_harness_this_build_does_not_have_names_both() {
        let root = scratch("unknown-harness");
        let shell = root.join("_shell");
        let runs = runs_dir(&root);
        let id = an_archived_one_seat_run(&shell, &runs);

        let path = run_dir(&runs, &id).unwrap().join(MANIFEST_JSON);
        let mut manifest: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        manifest["panes"]["orch"]["harness"] = serde_json::json!("gamma-cli");
        std::fs::write(&path, manifest.to_string()).unwrap();

        let why = reopen_blocker_for(&runs, &id).expect("an unregistered harness blocks the run");
        assert!(
            why.contains("orch") && why.contains("gamma-cli") && why.contains("does not have"),
            "{why}"
        );
    }

    /// **A partly recorded reopen keeps its parent's other seats** (R20).
    ///
    /// Panes are recorded one at a time as they spawn. A reopen that placed orch
    /// and got no further records orch alone; taking that map whole hid the
    /// parent's worker-1, and the next reopen started worker-1 on a fresh session
    /// while its real one sat in the shared seat directory.
    #[test]
    fn a_partly_recorded_reopen_still_resumes_every_seat_its_lineage_recorded() {
        let root = scratch("lineage-partial");
        let shell = root.join("_shell");
        let runs = runs_dir(&root);
        let cc = crate::placement::harness::claude_code().spec();
        for (seat, session) in [("orch", "sess-orch"), ("worker-1", "sess-w1")] {
            let project = shell.join(format!("pane-config/root-run/{seat}/projects/-tmp-slug"));
            std::fs::create_dir_all(&project).unwrap();
            std::fs::write(project.join(format!("{session}.jsonl")), r#"{"type":"user"}"#).unwrap();
        }
        begin(&shell, 1_800_000_000_000, Path::new("/tmp/logstat"), &SessionsId::new("root-run"), None);
        record_pane(&shell, "orch", cc, None, None);
        record_pane(&shell, "worker-1", cc, None, None);
        write_live(&shell, &["take the parser"]);
        rotate(&shell, &runs, 0);
        let parent_id = list(&runs)[0].id.clone();

        // The child placed orch and got no further.
        begin(
            &shell,
            1_900_000_000_000,
            Path::new("/tmp/logstat"),
            &SessionsId::new("root-run"),
            Some(&parent_id),
        );
        record_pane(&shell, "orch", cc, None, None);
        write_live(&shell, &["still going"]);
        rotate(&shell, &runs, 0);

        let child_id = list(&runs)[0].id.clone();
        assert_ne!(child_id, parent_id, "the lineage's row is the child");
        let resumed = lineage_session_ids(&runs, &child_id);
        assert_eq!(resumed.get("orch").map(String::as_str), Some("sess-orch"), "{resumed:?}");
        assert_eq!(
            resumed.get("worker-1").map(String::as_str),
            Some("sess-w1"),
            "worker-1 was recorded by the parent alone, and a partial child must not hide it: {resumed:?}",
        );
        assert_eq!(list(&runs)[0].cannot_reopen, None);
    }

    /// **A reopen that came up on the wrong harness does not hide the session**
    /// (D-095). The child here ran another vendor against the parent's seats and
    /// so recorded no session; the lineage still answers with what the parent ran.
    #[test]
    fn a_seat_reopened_on_the_wrong_harness_still_resolves_to_what_it_ran() {
        let root = scratch("lineage-wrong-harness");
        let shell = root.join("_shell");
        let runs = runs_dir(&root);
        let cc = crate::placement::harness::claude_code().spec();
        let project = shell.join("pane-config/root-run/worker-1/projects/-tmp-slug");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("sess-w1.jsonl"), r#"{"type":"user"}"#).unwrap();
        begin(&shell, 1_800_000_000_000, Path::new("/tmp/logstat"), &SessionsId::new("root-run"), None);
        record_pane(&shell, "worker-1", cc, Some("a-model"), Some(CredentialChoice::FleetKey));
        write_live(&shell, &["take the parser"]);
        rotate(&shell, &runs, 0);
        let parent_id = list(&runs)[0].id.clone();

        begin(
            &shell,
            1_900_000_000_000,
            Path::new("/tmp/logstat"),
            &SessionsId::new("root-run"),
            Some(&parent_id),
        );
        record_pane(&shell, "worker-1", crate::placement::codex::codex().spec(), None, None);
        write_live(&shell, &["still going"]);
        rotate(&shell, &runs, 0);
        let child_id = list(&runs)[0].id.clone();
        assert_ne!(child_id, parent_id);

        let seat = recorded_panes(&runs, &child_id).remove("worker-1").expect("the seat is recorded");
        assert_eq!(seat.harness, cc.name);
        assert_eq!(seat.model.as_deref(), Some("a-model"));
        assert_eq!(seat.credential, Some(CredentialChoice::FleetKey));
        assert_eq!(seat.session_id.as_deref(), Some("sess-w1"));
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
        assert!(take_transcript(&from, &into, Transport::SqliteBackup));

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
        }
        // **The source survives, WAL and all** (WP-27 R3). The archive is a
        // `VACUUM INTO` snapshot; the live store stays where its vendor keeps it,
        // which is what `codex resume <id>` reopens. The next run does not
        // re-archive it because it walks its own directory (R4), not this one.
        assert!(from.exists(), "the vendor's own thread store must survive the archive");
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

        let mut found: Vec<String> = transcript_files_for(&root, "sqlite")
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        found.sort();
        assert_eq!(found, ["at-the-root.sqlite", "one-below.sqlite"]);
    }

    /// **What a reader of a mixed run cold is told** (M24, #39).
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
        begin(&shell, 1_800_000_000_000, Path::new("/tmp/logstat"), &SessionsId::new("run-1"), None);
        write_live(&shell, &["take the parser"]);

        // Two seats placed as two different harnesses — the run the old sentence
        // could not describe. Recorded through the production path, so what is
        // asserted is what a spawn writes.
        record_pane(&shell, "orch", crate::placement::harness::claude_code().spec(), None, None);
        record_pane(&shell, "worker-1", crate::placement::codex::codex().spec(), Some("a-model"), None);

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
        begin(&shell, 1_800_000_000_000, Path::new("/tmp/logstat"), &SessionsId::new("run-1"), None);
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
        begin(&shell, 1_800_000_000_000, Path::new("/tmp/logstat"), &SessionsId::new("run-1"), None);
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
        begin(&shell, 1_800_000_000_000, Path::new("/tmp/logstat"), &SessionsId::new("run-1"), None);
        write_live(&shell, &["take the parser"]);
        rotate(&shell, &runs, 0);

        let id = list(&runs)[0].id.clone();
        delete(&runs, &shell, &id).unwrap();
        assert!(list(&runs).is_empty());
        assert!(!runs.join(&id).exists());
        assert!(delete(&runs, &shell, &id).is_err(), "a second delete has nothing to remove");
    }

    /// R15: deleting a lineage's last run removes its `pane-config/` directory.
    #[test]
    fn deleting_the_last_run_in_a_lineage_removes_its_pane_config() {
        let root = scratch("cascade");
        let shell = root.join("_shell");
        let runs = runs_dir(&root);

        // Two independent runs with different sessions ids.
        begin(&shell, 1_800_000_000_000, Path::new("/tmp/a"), &SessionsId::new("sess-a"), None);
        write_live(&shell, &["alpha"]);
        rotate(&shell, &runs, 0);

        begin(&shell, 1_800_000_001_000, Path::new("/tmp/b"), &SessionsId::new("sess-b"), None);
        write_live(&shell, &["beta"]);
        rotate(&shell, &runs, 0);

        // Seed pane-config dirs the way placement does.
        let pc_a = crate::placement::pane_config_run(&shell, &SessionsId::new("sess-a"));
        let pc_b = crate::placement::pane_config_run(&shell, &SessionsId::new("sess-b"));
        std::fs::create_dir_all(&pc_a).unwrap();
        std::fs::create_dir_all(&pc_b).unwrap();

        let all = list(&runs);
        assert_eq!(all.len(), 2);
        let id_a = all.iter().find(|r| r.sessions.as_deref() == Some("sess-a")).unwrap().id.clone();

        delete(&runs, &shell, &id_a).unwrap();
        assert!(!pc_a.exists(), "the deleted lineage's pane-config must be removed");
        assert!(pc_b.exists(), "the surviving lineage's pane-config must remain");
    }

    /// R15: deleting a reopened run does not remove pane-config while the
    /// lineage has other members.
    #[test]
    fn deleting_a_reopened_run_keeps_pane_config_while_the_lineage_survives() {
        let root = scratch("cascade-lineage");
        let shell = root.join("_shell");
        let runs = runs_dir(&root);

        // Parent run.
        begin(&shell, 1_800_000_000_000, Path::new("/tmp/c"), &SessionsId::new("sess-c"), None);
        write_live(&shell, &["parent"]);
        rotate(&shell, &runs, 0);
        let parent_id = list(&runs)[0].id.clone();

        // Child (reopened) — same sessions id, parent link.
        begin(&shell, 1_800_000_001_000, Path::new("/tmp/c"), &SessionsId::new("sess-c"), Some(&parent_id));
        write_live(&shell, &["child"]);
        rotate(&shell, &runs, 0);

        let pc = crate::placement::pane_config_run(&shell, &SessionsId::new("sess-c"));
        std::fs::create_dir_all(&pc).unwrap();

        // The collapsed list shows the child only (tip of lineage).
        let visible = list(&runs);
        assert_eq!(visible.len(), 1, "collapse_lineages shows one row");
        let child_id = visible[0].id.clone();

        delete(&runs, &shell, &child_id).unwrap();
        assert!(pc.exists(), "pane-config must survive — the parent still references it");
    }

    #[test]
    fn a_run_id_is_the_utc_calendar_time_it_started() {
        // Cross-checked against `date -u -r 1786098690`.
        assert_eq!(timestamp_id_for(1_786_098_690_000), "2026-08-07T10-31-30Z");
        assert_eq!(timestamp_id_for(0), "1970-01-01T00-00-00Z");
        // Leap day, and the year boundary the civil-from-days shift exists for.
        assert_eq!(&timestamp_id_for(1_709_164_800_000)[..10], "2024-02-29");
        assert_eq!(&timestamp_id_for(1_704_067_199_000)[..10], "2023-12-31");
    }

    #[test]
    fn two_runs_that_start_in_the_same_second_do_not_overwrite_each_other() {
        let root = scratch("collide");
        let runs = runs_dir(&root);
        let shell = root.join("_shell");
        let a = reserve(&shell, &runs, "2026-08-07T10-31-30Z").unwrap();
        let b = reserve(&shell, &runs, "2026-08-07T10-31-30Z").unwrap();
        assert_ne!(a, b);
        assert!(b.file_name().unwrap().to_string_lossy().ends_with("-2"));
        // An id already in History is not handed out again either.
        std::fs::create_dir_all(runs.join("2026-08-07T10-31-30Z-3")).unwrap();
        let c = reserve(&shell, &runs, "2026-08-07T10-31-30Z").unwrap();
        assert!(c.file_name().unwrap().to_string_lossy().ends_with("-4"));
    }

    /// **A run staged and then abandoned — a crash, a quit — is finished by the
    /// next sweep** (story 34), and shows as archiving until it is.
    #[test]
    fn a_staged_run_left_by_a_crash_is_finished_by_the_sweep() {
        let root = scratch("staged-crash");
        let (shell, runs) = (root.join("_shell"), runs_dir(&root));
        write_live(&shell, &["one", "two"]);

        let (staged, _) = stage(&shell, &runs, 0);
        let id = staged.expect("a live run to stage");
        assert!(!shell.join("state.db").exists(), "the live slot is free for the next fleet");
        assert!(list(&runs).is_empty(), "nothing is in History yet");
        let rows = list_all(&shell, &runs, None);
        assert_eq!((rows.len(), rows[0].id.as_str(), rows[0].state), (1, id.as_str(), RunState::Archiving));

        let notices = sweep(&shell, &runs);
        assert!(notices.iter().any(|(_, text)| text.contains("archived as")), "{notices:?}");
        let rows = list_all(&shell, &runs, None);
        assert_eq!((rows.len(), rows[0].id.as_str(), rows[0].state), (1, id.as_str(), RunState::Archived));
        assert!(!staging_dir(&shell).join(&id).exists());
        assert!(sweep(&shell, &runs).is_empty(), "and a second sweep finds nothing");
    }

    /// A stage interrupted before the log moved left the live slot whole, so
    /// its half-made folder is discarded rather than archived.
    #[test]
    fn a_staging_folder_with_no_log_is_discarded() {
        let root = scratch("staged-empty");
        let (shell, runs) = (root.join("_shell"), runs_dir(&root));
        let half = staging_dir(&shell).join("2026-08-07T10-31-30Z");
        std::fs::create_dir_all(half.join("transcripts")).unwrap();

        assert!(sweep(&shell, &runs).is_empty());
        assert!(!half.exists());
        assert!(list_all(&shell, &runs, None).is_empty());
    }

    /// **Finish leaves one self-contained log** (story 40): no journal beside it
    /// in History, and none left behind in the shell.
    #[test]
    fn a_finished_archive_is_a_single_file_database() {
        let root = scratch("single-file");
        let (shell, runs) = (root.join("_shell"), runs_dir(&root));
        // A log abandoned mid-write: its rows are still in the journal.
        let held = a_live_thread_store(&shell.join("state.db"), "kept");
        assert!(shell.join("state.db-wal").is_file(), "the fixture is a database with a live journal");

        let (staged, _) = stage(&shell, &runs, 0);
        let id = staged.unwrap();
        let staged_dir = staging_dir(&shell).join(&id);
        assert!(staged_dir.join("state.db-wal").is_file(), "the journal is staged with its database");
        drop(held);
        finish(&shell, &runs, &id);

        let dest = runs.join(&id);
        assert!(holds(&dest.join("state.db"), "kept"), "the journal's rows are in the one file");
        for suffix in ["-wal", "-shm"] {
            assert!(!dest.join(format!("state.db{suffix}")).exists(), "no state.db{suffix} in the archive");
            assert!(!shell.join(format!("state.db{suffix}")).exists());
        }
    }

    /// **What a pane writes after staging is the next run's** (story 39): a
    /// reopen shares its lineage's seat directory, so the archive holds only
    /// what was there when the run was staged.
    #[test]
    fn a_transcript_written_after_staging_is_not_in_the_archive() {
        let root = scratch("staged-transcripts");
        let (shell, runs) = (root.join("_shell"), runs_dir(&root));
        write_live(&shell, &["one"]);
        let sessions = SessionsId::new("lineage");
        begin(&shell, 0, Path::new("/work/repo"), &sessions, None);
        let projects = crate::placement::pane_config_run(&shell, &sessions).join("orch/projects/p");
        std::fs::create_dir_all(&projects).unwrap();
        std::fs::write(projects.join("before.jsonl"), "{}").unwrap();

        let (staged, _) = stage(&shell, &runs, 0);
        let id = staged.unwrap();
        // The resumed pane of the next run, writing into the same directory.
        std::fs::write(projects.join("after.jsonl"), "{}").unwrap();
        std::fs::write(projects.join("before.jsonl"), "{}\n{}").unwrap();
        finish(&shell, &runs, &id);

        let archived = runs.join(&id).join("transcripts/orch");
        assert_eq!(std::fs::read_to_string(archived.join("before.jsonl")).unwrap(), "{}");
        assert!(!archived.join("after.jsonl").exists());
        assert_eq!(list(&runs)[0].transcripts, 1);
    }
}
