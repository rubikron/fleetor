//! The **pane registry** — N live `claude` terminals, one pty each (D-030, Phase 3).
//!
//! This was a single-session bridge for the orchestrator. It is now the thing the
//! whole product stands on: five terminals, each with its own pty, its own pair of
//! event channels, and its own writer lock.
//!
//! Five decisions here are load-bearing, and each of them is a silent failure if
//! reversed:
//!
//!  - **Per-pane event channels** (`pty://output/orch`, `pty://output/2`). Not one
//!    channel with an id in the payload: `AppHandle::emit` wakes *every* listener
//!    on a name, so a shared channel means five JS callbacks fire for every chunk
//!    from every pane and four of them discard it. Per-id names make that
//!    impossible rather than merely unlikely — pane 2 cannot receive pane 1's
//!    bytes if it never listens on that name (L6, chokepoint #2).
//!  - **Coalescing in the reader** (~16 ms / 64 KB). `read()` returns as soon as
//!    *any* bytes are available, so a TUI's 200-byte spinner frame makes a
//!    complete trip across the IPC bridge. The per-event cost barely shrinks with
//!    payload size, which is what makes merging 50 × 200 B into 1 × 10 KB an
//!    order-of-magnitude win (L6, chokepoint #1).
//!  - **One write, one lock** ([`PaneRegistry::write_paste`]). Tauri dispatches
//!    sync commands on a thread pool, so a keystroke racing a delivery could
//!    otherwise land between a message's body and its `\r` — submitting it early
//!    or corrupting it. The writer mutex is per-pane and held across the whole
//!    injection, including the 30 ms gap.
//!  - **How to type is the pane's harness's answer, not this module's** (WP-25
//!    #21, checkpoints 9 and 10). Each pane records the `HarnessSpec` it was
//!    placed with, and the framing, the submit byte, the submit gap and a slash
//!    command's spelling are all read from it. This is a *widening* and nothing
//!    more: the spec says which bytes go out, never whether or when — no queue,
//!    no guard, no retry, no ceiling joined the message path, and checkpoint 9
//!    has no startup-wait field for one to arrive as (C26).
//!  - **A pane that cannot receive is not in the map delivery reads** (#42, C26).
//!    Codex opens on an animated splash that ends on a keypress and throws away
//!    everything written to it until then, so a pane of it is woken — and watched
//!    until it stops painting — *before* it is announced. The wake is on the
//!    bring-up path and nowhere else: `writable` did not learn a new question,
//!    because a readiness check between `fleet send` and a live pty is the Tier
//!    1.4 violation this whole arrangement exists to avoid. Which harnesses need
//!    it is [`BringUp`]'s answer, and it carries no number for anyone to tune —
//!    `wake` asks the pane whether it has stopped painting rather than waiting
//!    out a guess about how long it takes.
//!  - **`kill_all` signals the process group.** `openpty` gives each child its own
//!    session, so killing the pid alone can leave the real work orphaned. A leaked
//!    Opus after window close is a money bug.
//!
//! The registry itself knows nothing about Tauri: it emits through an [`Emit`]
//! callback, so `src-tauri/tests/panes.rs` drives five real ptys without a window.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::{engine::general_purpose::STANDARD, Engine};
use fleetor_core::pane::{PaneEntry, PaneId, PaneState};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use tauri::State;

use crate::placement::harness::{BringUp, CommandChannel, TypingProfile};
use crate::placement::HarnessSpec;

/// Merge pty reads for this long before emitting, so a repainting TUI doesn't
/// cross the IPC bridge once per spinner frame.
const COALESCE_WINDOW: Duration = Duration::from_millis(16);
/// …or until this much is buffered, whichever comes first.
const COALESCE_MAX: usize = 64 * 1024;
/// One pty read.
const READ_CHUNK: usize = 8192;

// Bracketed paste framing, the submit byte and the submit gap were `pub(crate)`
// constants here until the contract batch (WP-25 #23). They are checkpoint 9 —
// one harness's answer, not this driver's — so they live on
// [`TypingProfile`](crate::placement::harness::TypingProfile) and
// [`PaneRegistry::write_paste`] reads them off the pane's own spec (C35). The
// submit gap is still D-034's only delay between `fleet send` and a pty; the
// invariant is asserted on `TypingProfile::submit_gap_ms` now, and
// `harness_literals.rs` is the tripwire that stops it creeping back in here.

/// How long a pane gets to honor SIGTERM before it is killed outright.
const TERM_GRACE: Duration = Duration::from_millis(200);
const TERM_POLL: Duration = Duration::from_millis(20);

// --- waking a pane that opens on a splash (#42, C26) ---------------------------
//
// **These three are the bring-up path's, and none of them is a startup wait.**
// A startup wait is a number you believe instead of looking; what is below is a
// loop that looks. How long a pane takes to come up is decided by the pane —
// `wake` returns as soon as it settles and not before, and a pane that settles
// in 200 ms is announced in 200 ms.

/// The keypress a splash ends on.
///
/// On an empty composer this submits nothing, which is the whole reason it is
/// safe to press more than once — and it is pressed more than once, because
/// nothing tells us when the pane started reading. Measured on `codex-cli
/// 0.153.4`: a `\r` written before the pane reads is **discarded**, so this
/// cannot be written once at spawn and left to the pty's ordering to deliver.
const WAKE_KEY: &[u8] = b"\r";

/// After a press, how long its own repaint is allowed to land before the sample
/// below starts. Without it the loop measures the echo of its own keypress and
/// never converges — that is a real failure, met and fixed on the way here.
const PRESS_SETTLE: Duration = Duration::from_millis(600);

/// The window a pane has to stay silent in to count as settled.
///
/// **Measured rather than picked:** a codex pane on its splash paints every 72 ms
/// on average and its largest gap over 279 consecutive frames was **152 ms**, so
/// this sits at roughly 2.5× the widest gap an animating pane has shown. Once
/// settled the same pane painted 75 bytes in 12 s. It is a threshold on the
/// pane's own silence, not a duration anyone waits out: the loop below exits on
/// the first quiet sample.
const QUIET_SAMPLE: Duration = Duration::from_millis(400);

/// One pane's writer, shared by every path that types into it. Named because
/// three signatures here hand it around and the shape is noise at each of them.
type PaneWriter = Arc<Mutex<Box<dyn Write + Send>>>;

/// What [`wake`] needs from a pane, taken while the registry locks are held and
/// used after they are released.
///
/// Three `Arc` clones rather than a borrow of the entry, because the entry stays
/// in the `waking` map the whole time — so `kill` can still reach the process,
/// and so a bring-up cannot outlive a pane the operator stopped.
struct Waking {
    writer: PaneWriter,
    painted: Arc<AtomicU64>,
    state: Arc<AtomicU8>,
}

/// How the registry talks to the outside world: `(channel, base64 payload)`.
/// A callback rather than an `AppHandle` so the registry is testable headless.
pub type Emit = Arc<dyn Fn(&str, String) + Send + Sync>;

/// One live terminal.
struct Pane {
    master: Box<dyn MasterPty + Send>,
    /// Which harness this pane runs, recorded from the same [`Placed`] the
    /// command came off (WP-25 #21, checkpoints 9 and 10).
    ///
    /// **Carried on the pane rather than resolved into a value**, the shape C31
    /// chose for the gauge and for the same reason: the typing profile and the
    /// command spellings are two checkpoints, and a pane that answered them from
    /// two places could answer them differently. Recorded at spawn so it cannot
    /// disagree with the process that is actually running — a respawn replaces
    /// the entry, so a pane relaunched under another harness types that
    /// harness's bytes.
    ///
    /// [`Placed`]: crate::placement::Placed
    harness: &'static HarnessSpec,
    /// Shared with every writer — keystrokes *and* deliveries — so an injection
    /// and a keypress can never interleave.
    writer: PaneWriter,
    child: Box<dyn Child + Send + Sync>,
    /// Written by the reader thread, read by the roster. A [`PaneState`] as an
    /// atomic, because the reader learns of an exit and the roster asks about it.
    state: Arc<AtomicU8>,
    /// Bytes this pane has painted, ever. Written by the coalescer, read only by
    /// [`wake`] — which asks whether it has *stopped* growing.
    ///
    /// A counter rather than a timestamp because that is the question being
    /// asked: two reads a fixed window apart, equal, is a pane that painted
    /// nothing in that window. A pane that has painted nothing *at all* is a
    /// process that is not yet a terminal, and the zero tells that apart from
    /// silence.
    painted: Arc<AtomicU64>,
}

const SPAWNING: u8 = 0;
const LIVE: u8 = 1;
const DEAD: u8 = 2;

fn decode_state(raw: u8) -> PaneState {
    match raw {
        LIVE => PaneState::Live,
        DEAD => PaneState::Dead,
        _ => PaneState::Spawning,
    }
}

/// Managed Tauri state: every pane the shell is running.
pub struct PaneRegistry {
    panes: Mutex<HashMap<PaneId, Pane>>,
    /// Panes that have a pty and cannot yet receive (#42).
    ///
    /// **A second map rather than a flag on the first, and that is the whole
    /// point.** A pane is addressable because it is in `panes`; every path that
    /// can reach a pty — [`Self::write`], [`Self::write_paste`],
    /// [`Self::write_command`], and `writable` underneath all three — looks a
    /// name up there and is untouched by this. Holding an unready pane *out* of
    /// that map is not a gate on the message path, because there is nothing new
    /// on the message path to gate with: a `fleet send` into this window is
    /// refused for want of a pane, exactly as it is for a pane nobody has
    /// spawned, and comes back `accepted: false` carrying "… is not running" —
    /// which is the sentence the sending model reads on stderr (Tier 1.5).
    ///
    /// The alternative — one map plus a readiness check in `writable` — is the
    /// same behaviour and a Tier 1.4 violation, because the check would sit
    /// between `fleet send` and a live pty and could only grow.
    ///
    /// Killing and pid-recording read both maps; the roster and delivery read
    /// only `panes`. Locks are always taken `panes` first.
    waking: Mutex<HashMap<PaneId, Pane>>,
    emit: Emit,
    /// Where live pane pids are durably recorded, for `orphans::sweep` to find
    /// on the next launch. Explicit rather than resolved internally, so a test
    /// can point it at a scratch file instead of the operator's real
    /// `~/.fleetor` — the same reason `orphans::write_registry` takes a path.
    registry_path: std::path::PathBuf,
}

impl PaneRegistry {
    pub fn new(emit: Emit, registry_path: std::path::PathBuf) -> Self {
        Self {
            panes: Mutex::new(HashMap::new()),
            waking: Mutex::new(HashMap::new()),
            emit,
            registry_path,
        }
    }

    /// Spawn `cmd` under a fresh pty as `pane`.
    ///
    /// Idempotent for a pane that is still running (React StrictMode double-mounts
    /// every terminal), and a **respawn** for one that has died — which is what
    /// makes per-tab restart a two-line command rather than a special case.
    ///
    /// `harness` is the spec `place` returned beside `cmd` — the same placement,
    /// so what the pane runs and how the registry types into it cannot come from
    /// two different answers.
    ///
    /// **A pane whose harness answers [`BringUp::AfterWaking`] is woken before it
    /// is announced** (#42, C26). It gets its pty, its pump and its channels
    /// immediately — the operator watches it come up like any other pane — but it
    /// does not enter the map delivery reads until it has stopped painting, and
    /// this call does not return until then. Roughly 2 s for a codex pane; the
    /// pane decides, not a constant. Nothing is queued or retried on its behalf:
    /// a message aimed at it in that window is refused for want of a pane and
    /// answered `accepted: false`, which is a thing the sender can act on —
    /// unlike the green `accepted` a swallowed message gets today.
    ///
    /// [`BringUp::AtOnce`] is every pane FLEETOR has ever run, and takes the same
    /// path it always did.
    pub fn spawn(
        &self,
        pane: PaneId,
        cmd: CommandBuilder,
        harness: &'static HarnessSpec,
        rows: u16,
        cols: u16,
    ) -> Result<(), String> {
        let Some(waking) = self.open(pane, cmd, harness, rows, cols)? else {
            return Ok(());
        };
        wake(pane, &waking)?;
        self.announce(pane)
    }

    /// Give `pane` a pty, and either announce it or hand back what waking it
    /// needs. `None` means there is nothing left to do — the pane was already
    /// running, or its harness is ready the moment it has a pty.
    ///
    /// Split from [`Self::spawn`] so the registry locks are provably released
    /// before anything sleeps: [`wake`] runs for seconds, and a bring-up holding
    /// the map would stall delivery to the other four panes, which is the one
    /// thing this whole design exists to avoid.
    fn open(
        &self,
        pane: PaneId,
        cmd: CommandBuilder,
        harness: &'static HarnessSpec,
        rows: u16,
        cols: u16,
    ) -> Result<Option<Waking>, String> {
        let mut panes = self.lock()?;
        let mut waking = self.waking()?;
        // Already coming up. React StrictMode double-mounts every terminal, and a
        // second process for one pane is worse than a slow first one.
        if waking.contains_key(&pane) {
            return Ok(None);
        }
        if let Some(existing) = panes.get(&pane) {
            if decode_state(existing.state.load(Ordering::Relaxed)).accepts_input() {
                return Ok(None);
            }
            panes.remove(&pane); // dead: drop it and start a new one
        }

        let pair = native_pty_system()
            .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
            .map_err(|e| format!("openpty for {pane}: {e}"))?;

        let child = pair.slave.spawn_command(cmd).map_err(|e| format!("spawn {pane}: {e}"))?;
        // The child holds the slave fd now; drop ours so EOF propagates on exit.
        drop(pair.slave);

        let reader =
            pair.master.try_clone_reader().map_err(|e| format!("clone reader for {pane}: {e}"))?;
        let writer =
            pair.master.take_writer().map_err(|e| format!("take writer for {pane}: {e}"))?;

        let state = Arc::new(AtomicU8::new(SPAWNING));
        let painted = Arc::new(AtomicU64::new(0));
        // The channel names are computed once, here, and moved into the pump —
        // the reader thread must not `format!` per read.
        spawn_pump(
            reader,
            self.emit.clone(),
            out_channel(pane),
            exit_channel(pane),
            state.clone(),
            painted.clone(),
        );

        let entry = Pane {
            master: pair.master,
            harness,
            writer: Arc::new(Mutex::new(writer)),
            child,
            state,
            painted,
        };

        match harness.bring_up {
            BringUp::AtOnce => {
                panes.insert(pane, entry);
                record_live_pids(&panes, &waking, &self.registry_path);
                Ok(None)
            }
            BringUp::AfterWaking => {
                let handle = Waking {
                    writer: entry.writer.clone(),
                    painted: entry.painted.clone(),
                    state: entry.state.clone(),
                };
                waking.insert(pane, entry);
                // Recorded now rather than at announcement: a pane being woken is
                // a real process, and a crash before it settles must still leave
                // `orphans::sweep` something to reap.
                record_live_pids(&panes, &waking, &self.registry_path);
                Ok(Some(handle))
            }
        }
    }

    /// Move a woken pane into the map delivery reads. **This is the moment a pane
    /// becomes addressable**, and nothing before it can be sent to.
    ///
    /// A pane that is no longer in `waking` was killed while it was coming up.
    /// It is not announced: putting it back would resurrect a pane the operator
    /// stopped, and `kill` already terminated the process.
    fn announce(&self, pane: PaneId) -> Result<(), String> {
        let mut panes = self.lock()?;
        let mut waking = self.waking()?;
        if let Some(entry) = waking.remove(&pane) {
            panes.insert(pane, entry);
            record_live_pids(&panes, &waking, &self.registry_path);
        }
        Ok(())
    }

    /// Relay operator keystrokes. Takes the same per-pane writer lock a delivery
    /// does, which is what stops the two from interleaving.
    pub fn write(&self, pane: PaneId, data: &[u8]) -> Result<(), String> {
        let (writer, _) = self.writable(pane)?;
        let mut guard = writer.lock().map_err(|e| e.to_string())?;
        guard.write_all(data).map_err(|e| format!("write to {pane}: {e}"))?;
        guard.flush().map_err(|e| format!("flush {pane}: {e}"))
    }

    /// Type one framed message into `pane` as a bracketed paste, then submit it.
    ///
    /// The whole injection happens under a single hold of that pane's writer lock,
    /// so nothing — not a keystroke, not another delivery — can land between the
    /// body and its `\r`. Blocking, by design: [`crate::deliver`] runs it off the
    /// command loop so one slow pane cannot stall the other four.
    /// The framing, the submit byte and the gap are **this pane's harness's**
    /// answers to checkpoint 9, read off the spec recorded at spawn rather than
    /// off a constant here (WP-25 #21). Nothing else about the write changed: the
    /// profile says what bytes to send, never whether or when to send them.
    pub fn write_paste(&self, pane: PaneId, text: &str) -> Result<(), String> {
        let (writer, harness) = self.writable(pane)?;
        self.type_framed(pane, &writer, &harness.typing, text)
    }

    /// Type one slash command into `pane`, spelled the way **this pane's harness**
    /// spells it (checkpoint 10).
    ///
    /// A sibling of [`Self::write_paste`] rather than a flag on it, because the
    /// two differ in exactly one thing: a command's word is looked up in the
    /// harness's table first. The lookup happens under the same map entry that
    /// yields the writer, so a spelling and a writer can never come from
    /// different harnesses, and the command path takes the same single lock it
    /// always did.
    ///
    /// **A command the table does not name is typed as it arrived, never
    /// refused.** It was allowlisted at accept time (`fleetor_core::command`), and
    /// after acceptance nothing may delay, drop or alter it (D-034/D-045); a
    /// second refusal here would be exactly the post-accept gate Tier 1.4 bans.
    /// The missing row is a conformance failure, caught by checkpoint 10 before a
    /// harness can be registered.
    pub fn write_command(&self, pane: PaneId, command: &str) -> Result<(), String> {
        let (writer, harness) = self.writable(pane)?;
        let spelled = spell(&harness.commands, command);
        self.type_framed(pane, &writer, &harness.typing, &spelled)
    }

    /// The one write both of the above make.
    ///
    /// The whole injection happens under a single hold of that pane's writer lock,
    /// so nothing — not a keystroke, not another delivery — can land between the
    /// body and its submit byte.
    fn type_framed(
        &self,
        pane: PaneId,
        writer: &PaneWriter,
        typing: &TypingProfile,
        text: &str,
    ) -> Result<(), String> {
        let (start, end): (&[u8], &[u8]) = if typing.bracketed_paste {
            (typing.paste_start, typing.paste_end)
        } else {
            (&[], &[])
        };

        let mut body = Vec::with_capacity(text.len() + start.len() + end.len());
        body.extend_from_slice(start);
        body.extend_from_slice(text.as_bytes());
        body.extend_from_slice(end);

        let mut guard = writer.lock().map_err(|e| e.to_string())?;
        guard.write_all(&body).map_err(|e| format!("write to {pane}: {e}"))?;
        guard.flush().map_err(|e| format!("flush {pane}: {e}"))?;
        std::thread::sleep(Duration::from_millis(typing.submit_gap_ms));
        guard.write_all(typing.submit_bytes).map_err(|e| format!("submit to {pane}: {e}"))?;
        guard.flush().map_err(|e| format!("flush {pane}: {e}"))
    }

    pub fn resize(&self, pane: PaneId, rows: u16, cols: u16) -> Result<(), String> {
        let panes = self.lock()?;
        let entry = panes.get(&pane).ok_or_else(|| format!("{pane} is not running"))?;
        entry
            .master
            .resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
            .map_err(|e| format!("resize {pane}: {e}"))
    }

    /// Stop one pane. Its tab stays; a later [`PaneRegistry::spawn`] restarts it.
    ///
    /// The exit event is left to the pump, which fires it when the pty EOFs —
    /// both dropping the master and killing the child guarantee that. Emitting
    /// one here too would give the tab two deaths for one process.
    /// A pane still being woken is killable too — it is a real process, and the
    /// operator watching it animate is exactly who would want to stop it. Its
    /// bring-up notices the entry is gone and announces nothing.
    pub fn kill(&self, pane: PaneId) -> Result<(), String> {
        let mut panes = self.lock()?;
        let mut waking = self.waking()?;
        let mut entry = panes
            .remove(&pane)
            .or_else(|| waking.remove(&pane))
            .ok_or_else(|| format!("{pane} is not running"))?;
        terminate(&mut entry);
        record_live_pids(&panes, &waking, &self.registry_path);
        Ok(())
    }

    /// Whether this session has brought any pane up at all.
    ///
    /// **Existence, not membership and not liveness.** [`Self::roster`] filters
    /// to fleet members, and `writable` asks whether a pane accepts input; both
    /// are the wrong question here. A pane that has exited still had its
    /// `CLAUDE_CONFIG_DIR`, its worktree and its brief seeded against whatever
    /// the target was when it spawned, and [`Self::spawn`] respawns it in place
    /// — so an exited pane is every bit as committed to the old target as a live
    /// one, and the evaluator being no member of the fleet does not make it
    /// indifferent to which repository it is reading. The map is emptied only by
    /// [`Self::kill`] and [`Self::kill_all`], which is exactly the point at
    /// which nothing is holding the old target any more.
    ///
    /// A poisoned registry answers `true`: the one caller is a guard that must
    /// fail closed, and "I cannot tell" is not "there are none."
    /// A pane still being woken counts: its config dir, its worktree and its
    /// brief were all seeded against the target as it was when it spawned, which
    /// is the whole of what this guard is asking about.
    pub fn any_pane(&self) -> bool {
        let live = self.panes.lock().map(|panes| !panes.is_empty()).unwrap_or(true);
        live || self.waking.lock().map(|waking| !waking.is_empty()).unwrap_or(true)
    }

    /// Reap every pane. Called on window close — best-effort and deliberately
    /// silent, because there is nowhere left to report to.
    pub fn kill_all(&self) {
        let Ok(mut panes) = self.panes.lock() else { return };
        for (_, mut entry) in panes.drain() {
            terminate(&mut entry);
        }
        // Panes still coming up are reaped here too. A window closed during a
        // bring-up would otherwise leave the one thing this module's `kill_all`
        // exists to prevent: a live agent process with nobody watching it.
        if let Ok(mut waking) = self.waking.lock() {
            for (_, mut entry) in waking.drain() {
                terminate(&mut entry);
            }
        }
        crate::orphans::write_registry(&self.registry_path, &[]);
    }

    /// Every **fleet** pane the shell is running, orch first. The hub's single
    /// source of truth for fleet membership — a pane that was never spawned must
    /// not appear here, or `fleet broadcast` fans out to somewhere that cannot
    /// receive.
    ///
    /// **Membership, not liveness, and the two are different questions.** A
    /// terminal this registry is running is not automatically a member of the
    /// fleet: `PaneId::is_fleet_member` is what decides, and it is filtered here
    /// because this one answer feeds both places the fleet gets enumerated — the
    /// `fleet roster` listing and a `fleet broadcast`'s target list (`Hub::roster`
    /// and `Hub::broadcast`, both through `app_roster`). Filtering once, at the
    /// source, is what stops those two from ever disagreeing.
    ///
    /// Nothing about *delivery* consults this. `writable` looks a name up in the
    /// map directly, so a non-member with a live terminal is still addressable
    /// by name in both directions — which is the whole shape: not enumerated,
    /// and not unreachable.
    pub fn roster(&self) -> Vec<PaneEntry> {
        let Ok(panes) = self.panes.lock() else { return Vec::new() };
        let mut entries: Vec<PaneEntry> = panes
            .iter()
            .filter(|(pane, _)| pane.is_fleet_member())
            .map(|(pane, p)| PaneEntry::new(*pane, decode_state(p.state.load(Ordering::Relaxed))))
            .collect();
        entries.sort_by_key(|e| e.pane);
        entries
    }

    /// The writer for a pane that can take input, or the sentence explaining why
    /// not — which becomes the `detail` the sending model reads on stderr.
    ///
    /// **The predicate is [`PaneState::accepts_input`], never `is_live()`.**
    /// Nothing tells us when `claude` reaches its prompt; `Spawning` is a guess
    /// about a running process. Refusing on it means a healthy pane silently drops
    /// every message until the guess catches up — which is L1's shape exactly. A
    /// write to a still-booting pty is buffered by the kernel and read when the
    /// TUI starts reading; that is the failure we can live with.
    /// The harness comes back beside the writer, from the same entry, so a write
    /// is framed by the spec of the process it is actually going to.
    fn writable(
        &self,
        pane: PaneId,
    ) -> Result<(PaneWriter, &'static HarnessSpec), String> {
        let panes = self.lock()?;
        let entry = panes.get(&pane).ok_or_else(|| format!("{pane} is not running"))?;
        let state = decode_state(entry.state.load(Ordering::Relaxed));
        if !state.accepts_input() {
            return Err(format!("{pane} has exited"));
        }
        Ok((entry.writer.clone(), entry.harness))
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, HashMap<PaneId, Pane>>, String> {
        self.panes.lock().map_err(|e| format!("pane registry poisoned: {e}"))
    }

    /// Always taken *after* [`Self::lock`], never before — the only two places
    /// that hold both are `open`/`announce` and `kill`/`kill_all`.
    fn waking(&self) -> Result<std::sync::MutexGuard<'_, HashMap<PaneId, Pane>>, String> {
        self.waking.lock().map_err(|e| format!("pane registry poisoned: {e}"))
    }
}

/// One harness's spelling of an allowlisted slash command, arguments intact
/// (checkpoint 10).
///
/// Matched on the command **word**, the same way `fleetor_core::command`
/// allowlists it, so `/compact keep the parser` is one command and its argument
/// survives the translation. A word with no row comes back untouched — see
/// [`PaneRegistry::write_command`] for why that is a pass-through and not a
/// refusal.
fn spell(channel: &CommandChannel, command: &str) -> String {
    let word = command.split_whitespace().next().unwrap_or("");
    let Some((_, spelling)) = channel.spellings.iter().find(|(canonical, _)| *canonical == word)
    else {
        return command.to_string();
    };
    format!("{spelling}{}", &command[word.len()..])
}

/// `pty://output/orch`, `pty://output/2`.
pub fn out_channel(pane: PaneId) -> String {
    format!("pty://output/{}", channel_key(pane))
}

/// `pty://exit/orch`, `pty://exit/2`.
pub fn exit_channel(pane: PaneId) -> String {
    format!("pty://exit/{}", channel_key(pane))
}

/// **Exhaustive on the identity, never on `slot()`.** This used to be
/// `match pane.slot() { Some(n) => n, None => "orch" }`, which quietly gave
/// *every* slotless name orch's channel: a second slotless pane would have had
/// its bytes rendered in the orchestrator's terminal, and there is no failure
/// signal for listening on the wrong name (which is why the pair is pinned by a
/// test at all). A `match` on the variant makes the next one a compile error.
///
/// `ui/src/fleet/types.ts::paneKey` is the other half of each of these strings.
fn channel_key(pane: PaneId) -> String {
    match pane {
        PaneId::Orch => "orch".to_string(),
        PaneId::Worker(n) => n.to_string(),
        PaneId::Evaluator => "evaluator".to_string(),
        PaneId::Critic => "critic".to_string(),
        // No pty, so no channel — this name is refused before a spawn is
        // attempted (`fleet::spawn_pane`) and nothing ever listens here.
        PaneId::Operator => "operator".to_string(),
    }
}

/// Read the pty on one thread and coalesce on another.
///
/// Two threads, not one, and the reason is a correctness bug rather than taste: a
/// single thread can only check its window *after* the next blocking `read`
/// returns, so the tail of a burst would sit unflushed until the pane happened to
/// print again — the last line of a finished turn, invisible for as long as the
/// pane stays quiet. The coalescer's `recv_timeout` has no such blind spot.
fn spawn_pump(
    mut reader: Box<dyn Read + Send>,
    emit: Emit,
    out_channel: String,
    exit_channel: String,
    state: Arc<AtomicU8>,
    painted: Arc<AtomicU64>,
) {
    let (tx, rx) = mpsc::channel::<Vec<u8>>();

    std::thread::spawn(move || {
        let mut buf = [0u8; READ_CHUNK];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
        // Dropping `tx` here is what tells the coalescer the pane is gone.
    });

    std::thread::spawn(move || {
        loop {
            let Ok(first) = rx.recv() else { break };
            let mut batch = first;
            let deadline = Instant::now() + COALESCE_WINDOW;
            let mut disconnected = false;
            while batch.len() < COALESCE_MAX {
                let remaining = deadline.saturating_duration_since(Instant::now());
                match rx.recv_timeout(remaining) {
                    Ok(more) => batch.extend_from_slice(&more),
                    Err(RecvTimeoutError::Timeout) => break,
                    Err(RecvTimeoutError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }
            // The pane painted something, so it is past `claude`'s startup. The
            // closest honest signal we have that it is a terminal and not a
            // process; delivery does not depend on it (see `writable`).
            state.store(LIVE, Ordering::Relaxed);
            // How much it has painted, for `wake` to ask whether it has stopped
            // (#42). Counted here rather than in the reader thread because this
            // is where a *frame* is: the reader returns on any byte available, so
            // its own idea of "a paint" is an artifact of scheduling.
            painted.fetch_add(batch.len() as u64, Ordering::Relaxed);
            emit(&out_channel, STANDARD.encode(&batch));
            if disconnected {
                break;
            }
        }
        state.store(DEAD, Ordering::Relaxed);
        emit(&exit_channel, String::new());
    });
}

/// Press the key a splash ends on until the pane stops painting (#42, C26).
///
/// **This detects readiness; it does not wait out a guess.** The question asked
/// is one the pane answers itself — *are you still painting?* — and the answer
/// comes off the same byte stream the terminal is rendering, counted by the
/// coalescer that was already counting it. A pane that settles quickly is
/// announced quickly. Measured against `codex-cli 0.153.4` with a freshly seeded
/// `CODEX_HOME`: **ready in about 2 s, after 2 presses.**
///
/// **Why it presses rather than only watching.** A fresh codex pane's splash does
/// not end on its own — #24 left one animating for 75 s — so watching alone
/// waits forever. `\r` on an empty composer submits nothing, which is what makes
/// pressing it repeatedly safe, and it is pressed repeatedly because a press
/// written before the pane starts reading is *discarded*: measured, and the
/// reason this could not be one blind write at spawn with the pty's own ordering
/// left to deliver it.
///
/// **The two halves of the ready test are both load-bearing.** `painted > 0`
/// separates a process that has not yet become a terminal from one that has
/// gone quiet — without it every pane is "settled" the instant it is spawned.
/// Equal counts across [`QUIET_SAMPLE`] is the silence itself.
///
/// **A pane that never settles is never announced, and that is the honest
/// failure** (D-034). The alternative is a deadline, which would announce a pane
/// that is still swallowing and hand back the green `accepted` this whole ticket
/// exists to stop. The operator is not left guessing: the pty is pumping to the
/// terminal from the moment it exists, so an unsettled pane is one they are
/// watching animate, and [`PaneRegistry::kill`] reaches it.
///
/// Returns early if the pane dies on the way up; [`PaneRegistry::announce`] then
/// files the corpse, so a harness that cannot start looks like one that started
/// and exited rather than like a pane that never existed.
fn wake(pane: PaneId, waking: &Waking) -> Result<(), String> {
    loop {
        if !decode_state(waking.state.load(Ordering::Relaxed)).accepts_input() {
            return Ok(());
        }
        {
            let mut guard = waking.writer.lock().map_err(|e| e.to_string())?;
            guard.write_all(WAKE_KEY).map_err(|e| format!("wake {pane}: {e}"))?;
            guard.flush().map_err(|e| format!("wake {pane}: {e}"))?;
        }
        std::thread::sleep(PRESS_SETTLE);
        let before = waking.painted.load(Ordering::Relaxed);
        std::thread::sleep(QUIET_SAMPLE);
        if before > 0 && waking.painted.load(Ordering::Relaxed) == before {
            return Ok(());
        }
    }
}

/// Durably record which pids should still be running, so a launch after a
/// crash or Force Quit — which skips [`PaneRegistry::kill_all`] entirely — has
/// something to reap them from (`orphans::sweep`).
///
/// Only called from `spawn`/`kill`/`kill_all` — a pane that exits on its own
/// (the pump thread notices EOF, `spawn_pump`) does not trigger a rewrite, so
/// the registry can briefly lag one dead pid behind reality. Harmless: the
/// sweep's own liveness+identity check already treats "gone" as a no-op, so a
/// stale entry costs nothing on the next launch — it just isn't worth a write
/// on every pty EOF to keep it byte-exact between explicit mutations.
fn record_live_pids(
    panes: &HashMap<PaneId, Pane>,
    waking: &HashMap<PaneId, Pane>,
    registry_path: &std::path::Path,
) {
    let pids: Vec<u32> =
        panes.values().chain(waking.values()).filter_map(|p| p.child.process_id()).collect();
    crate::orphans::write_registry(registry_path, &pids);
}

/// SIGTERM the child's **process group**, then SIGKILL what is left.
///
/// `openpty` puts the child in its own session, so it is a process-group leader
/// and its own children (a `claude` runs plenty) share its pgid. Killing the pid
/// alone leaves them running with no terminal and nobody watching — which for an
/// Opus session is a bill that keeps growing after the window is closed.
fn terminate(pane: &mut Pane) {
    pane.state.store(DEAD, Ordering::Relaxed);
    let pid = pane.child.process_id();

    #[cfg(unix)]
    if let Some(pid) = pid {
        unsafe { libc::killpg(pid as i32, libc::SIGTERM) };
    }

    let deadline = Instant::now() + TERM_GRACE;
    while Instant::now() < deadline {
        if matches!(pane.child.try_wait(), Ok(Some(_))) {
            return;
        }
        std::thread::sleep(TERM_POLL);
    }

    #[cfg(unix)]
    if let Some(pid) = pid {
        unsafe { libc::killpg(pid as i32, libc::SIGKILL) };
    }
    let _ = pane.child.kill();
    let _ = pane.child.wait();
}

// --- Tauri commands (thin wrappers) -------------------------------------------

#[tauri::command]
pub fn pty_spawn(
    registry: State<'_, Arc<PaneRegistry>>,
    fleet: State<'_, crate::fleet::FleetState>,
    pane: PaneId,
    rows: u16,
    cols: u16,
) -> Result<(), String> {
    crate::fleet::spawn_pane(&fleet, &registry, pane, rows, cols)
}

#[tauri::command]
pub fn pty_write(
    state: State<'_, Arc<PaneRegistry>>,
    pane: PaneId,
    data: String,
) -> Result<(), String> {
    state.write(pane, data.as_bytes())
}

#[tauri::command]
pub fn pty_resize(
    state: State<'_, Arc<PaneRegistry>>,
    pane: PaneId,
    rows: u16,
    cols: u16,
) -> Result<(), String> {
    state.resize(pane, rows, cols)
}

#[tauri::command]
pub fn pty_kill(state: State<'_, Arc<PaneRegistry>>, pane: PaneId) -> Result<(), String> {
    state.kill(pane)
}

/// Best-effort teardown on window close.
pub fn kill_all(state: &Arc<PaneRegistry>) {
    state.kill_all();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The channel names the UI listens on. Pinned because a rename that only
    /// lands on one side produces a pane that renders nothing and reports no
    /// error — there is no failure signal for listening on the wrong name.
    #[test]
    fn each_pane_owns_a_distinct_pair_of_channel_names() {
        assert_eq!(out_channel(PaneId::Orch), "pty://output/orch");
        assert_eq!(out_channel(PaneId::Worker(2)), "pty://output/2");
        assert_eq!(exit_channel(PaneId::Orch), "pty://exit/orch");
        assert_eq!(exit_channel(PaneId::Worker(4)), "pty://exit/4");

        // **Every name, not just the roster** (WP-15). The roster is the fleet,
        // and a terminal that is not in the fleet still has a pty and still
        // needs a channel nobody else is listening on. The old version of
        // `channel_key` gave `orch`'s name to every slotless identity, so this
        // list is what would have caught it.
        let all: Vec<String> = PaneId::roster(&fleetor_core::pane::WORKER_SLOTS)
            .into_iter()
            .chain([PaneId::Evaluator, PaneId::Critic, PaneId::Operator])
            .flat_map(|p| [out_channel(p), exit_channel(p)])
            .collect();
        let unique: std::collections::HashSet<&String> = all.iter().collect();
        assert_eq!(all.len(), unique.len(), "two panes share a channel: {all:?}");
        assert_eq!(out_channel(PaneId::Evaluator), "pty://output/evaluator");
        assert_eq!(out_channel(PaneId::Critic), "pty://output/critic");
    }

    /// **The veil at the registry** (WP-15). A terminal this registry runs is
    /// not automatically a member of the fleet, and this one answer feeds both
    /// places the fleet gets enumerated — `fleet roster`'s listing and a
    /// `fleet broadcast`'s target list, which both go through
    /// `AppCommand::Roster`. Filtering once here is what stops them disagreeing.
    ///
    /// Driven through a real spawn rather than asserted on the predicate, so it
    /// fails if the filter is ever dropped from `roster()` itself.
    /// **The Critic joined this test rather than getting one of its own**
    /// (WP-20, D-076), because the claim is identical and the filter is one
    /// line: a running terminal is on the roster only if it is a fleet member.
    /// `fleet roster` and `fleet broadcast` both read this, so a Critic that
    /// appeared here would be a Critic every broadcast wrote into.
    #[test]
    fn a_running_evaluator_is_not_on_the_fleets_roster() {
        let dir = std::env::temp_dir().join(format!("fleetor-roster-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let registry = PaneRegistry::new(Arc::new(|_: &str, _: String| {}), dir.join("panes.pids"));

        for pane in [PaneId::Orch, PaneId::Worker(1), PaneId::Evaluator, PaneId::Critic] {
            let mut cmd = CommandBuilder::new("/bin/cat");
            cmd.env("TERM", "dumb");
            registry.spawn(pane, cmd, crate::placement::harness::claude_code().spec(), 24, 80)
                .expect("spawn");
        }

        let roster: Vec<PaneId> = registry.roster().into_iter().map(|e| e.pane).collect();
        assert_eq!(
            roster,
            vec![PaneId::Orch, PaneId::Worker(1)],
            "neither the evaluator nor the Critic is the fleet",
        );
        // …and both are still addressable, which is the whole shape: a name that
        // is in no enumeration and is not unreachable.
        for outsider in [PaneId::Evaluator, PaneId::Critic] {
            assert!(registry.writable(outsider).is_ok(), "still a live pty to write to");
        }
        registry.kill_all();
    }

    /// **Checkpoint 10's translation, against a table that is not Claude Code's**
    /// (WP-25 #21). Driven with a fabricated channel on purpose: Claude Code's
    /// spellings are the identity, so a test using them would pass against a
    /// `spell` that returned its argument, which is the shape of assertion the
    /// conformance suite refuses. What has to hold is that the *word* is
    /// translated, the arguments survive untouched, and a word with no row is
    /// passed through rather than refused — a post-accept refusal is the thing
    /// D-045 and Tier 1.4 both ban.
    #[test]
    fn a_commands_word_is_respelled_and_its_arguments_are_not() {
        let channel = CommandChannel { spellings: &[("/clear", "/reset"), ("/compact", "/squash")] };
        assert_eq!(spell(&channel, "/clear"), "/reset");
        assert_eq!(spell(&channel, "/compact keep the parser"), "/squash keep the parser");
        assert_eq!(
            spell(&channel, "/compact  two  spaces"),
            "/squash  two  spaces",
            "the tail is copied, never re-joined",
        );
        assert_eq!(
            spell(&channel, "/model opus"),
            "/model opus",
            "a word with no row is typed as it arrived — refusing here would be a second gate \
             after accept time",
        );
        assert_eq!(spell(&channel, ""), "");
    }

    /// Only a dead pane refuses input — asserted on the decoder the registry
    /// actually consults, so a state-encoding change can't quietly flip it.
    #[test]
    fn a_spawning_pane_still_accepts_input() {
        assert!(decode_state(SPAWNING).accepts_input(), "refusing here is L1's shape");
        assert!(decode_state(LIVE).accepts_input());
        assert!(!decode_state(DEAD).accepts_input());
    }
}
