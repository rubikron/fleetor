//! The embedded fleet: store, event bus, hub, and where the panes run (D-030).
//!
//! What this module was through Phase 2: a shell that spawned four headless
//! standby workers and bound a *dynamic* hub, so the lead's `assign` turned into a
//! supervised process. That apparatus is gone. In the TUI fleet there are no
//! headless workers — every agent is a live `claude` terminal, and the only thing
//! crossing the socket is a message.
//!
//! What is here now:
//!
//!  - the **store** and the live **event bus**, pumped to the webview by
//!    [`spawn_follower`] (unchanged, and the reason the feed still streams);
//!  - a plain [`Hub`] bound to the unix socket, whose pane ops are
//!    served by [`crate::deliver`] out of the real pty registry;
//!  - the **target** the fleet works on: the operator's repo if
//!    `~/.fleetor/config.json` names one, else a seeded [`testbed`];
//!  - [`spawn_pane`] — the one place the app asks [`crate::placement`] to bring a
//!    pane up, and hands what came back to the registry, the feed and the gauge.
//!
//! **What is deliberately not here any more (WP-21, D-075):** how a pane is
//! brought up, and where anything lives on disk. The bring-up was a long untested
//! match in this file with three shapes in it, beside a guardrail helper and a
//! brief helper only one of those shapes used; the locations were six free
//! functions each deriving the `~/.fleetor` tree from `$HOME` on its own. The first
//! is `placement::place`, reached once; the second is a [`Layout`] resolved at
//! bootstrap and held on [`Fleet`]. This module keeps the store, the bus, the hub,
//! the target, the operator's composer and the run commands.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use fleetor_core::event::{FleetEvent, NoticeLevel};
use fleetor_core::pane::{PaneEntry, PaneId, PaneState};
use fleetor_core::wire::{Op, OpResult};
use fleetor_core::Store;
use fleetor_db::SqliteStore;
use fleetor_ipc::UnixTransport;
use fleetor_server::{AppCommand, BroadcastStore, Hub, Interview};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};
use tokio::runtime::Runtime;
use tokio::sync::{mpsc, oneshot, Notify};

use crate::context_gauge::GaugeSources;
use crate::placement::{self, harness, Host, Layout, PaneSpec, RunSource};
use crate::prompts::PaneContext;
use crate::pty::PaneRegistry;
use crate::{deliver, dev, evaluator, guardrail, prompts, runs, testbed};

/// Emitted for every appended event, in `seq` order, the moment it persists.
const EVENT_FLEET: &str = "fleet://event";

/// Emitted once per handoff that clears [`evaluator::readiness`] — the signal
/// that replaced creating a second window (D-073).
///
/// It carries the Rust-side readiness decision and nothing else, which is why the
/// webview cannot derive it from the `FleetEvent::Handoff` it already receives: a
/// handoff with no mission, or with the mode off, must wake nothing at all.
const EVENT_EVALUATOR_WAKE: &str = "evaluator://wake";

/// One event as the webview sees it: the `seq` cursor plus the flattened
/// [`FleetEvent`] (its `#[serde(tag = "type")]` discriminator carries through, so
/// the UI matches on `type`).
#[derive(Serialize, Clone)]
pub struct WireEvent {
    seq: i64,
    #[serde(flatten)]
    event: FleetEvent,
}

/// What a freshly-mounted UI gets back from bootstrap. The feed itself arrives
/// entirely over [`EVENT_FLEET`] — the follower replays history from 0 — so all
/// this carries is the cursor. It used to carry the board too; there is no board.
#[derive(Serialize)]
pub struct BootSnapshot {
    latest_seq: i64,
}

/// The fleet's live configuration, surfaced to the top bar so it shows *facts*
/// rather than placeholders.
#[derive(Serialize, Clone)]
pub struct FleetConfig {
    /// The repo the fleet operates on (display name — the target's directory).
    target: String,
    /// Its full path, for the spend gate and the target picker.
    target_path: String,
    /// That repo's current git branch (best-effort).
    branch: String,
    /// What fills the worker seats: the model, or `"none"` when no key is
    /// available and the fleet can only run its orchestrator.
    worker_backend: String,
    /// The model in the orchestrator seat — the operator's own Opus.
    lead_model: String,
    /// A short, honest label for the quality gate workers pass through.
    gate: String,
}

// --- the start gate's seats (WP-25 #35; M1, M2, M3, M15, C23) -----------------

/// **One seat's choice: a harness and the model it starts with** (M1).
///
/// The selection unit is a *pair*, which is M1 unamended — not a harness alone and
/// not a named profile. Both halves travel together because a model name only means
/// something relative to the harness that would accept it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SeatChoice {
    /// The harness's [`harness::HarnessSpec`] name — the same string `manifest.json`
    /// records (M24) and the same one [`harness::by_name`] resolves. One spelling,
    /// three readers.
    pub harness: String,
    /// The model this seat starts with, or `None` for the seat's own default: the
    /// orchestrator's `default (your login)` sentinel (M2), or a worker's
    /// launch-configured model.
    pub model: Option<String>,
}

/// **What the operator picked, for every seat that may carry a choice** (C23).
///
/// Five seats: the orchestrator and the four workers. **The two judges are absent
/// and there is no field for them**, which is C15 made structural one layer up from
/// [`PaneSpec`] — offering a judge a harness later is a type change here as well as
/// there, rather than a value somebody sets.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct FleetSeats {
    /// The operator's own seat.
    pub orch: SeatChoice,
    /// The four fenced workers, in slot order — the row C23 says a disclosure
    /// expands into four.
    pub workers: Vec<SeatChoice>,
}

impl FleetSeats {
    /// The fleet nobody has picked yet: Claude Code everywhere, the orchestrator on
    /// its sentinel, the workers on the launch configuration's model.
    ///
    /// **Byte for byte the fleet that spawned before the pickers existed**, which is
    /// the property that makes shipping them safe: an operator who never opens a
    /// dropdown gets exactly what they got yesterday.
    fn unpicked(worker_model: &str) -> Self {
        let claude = harness::claude_code().spec().name.to_string();
        let workers = fleetor_core::pane::WORKER_SLOTS
            .iter()
            .map(|_| SeatChoice {
                harness: claude.clone(),
                model: Some(worker_model.to_string()),
            })
            .collect();
        Self { orch: SeatChoice { harness: claude, model: None }, workers }
    }

    /// The same selection, refused if it names something that cannot be placed.
    ///
    /// **Two checks, and neither is about models.** A harness name that no longer
    /// resolves is a fleet that cannot spawn, and a wrong number of workers is a
    /// fleet with a seat nobody chose for. A *model* that no longer exists is the
    /// other kind of problem entirely — it is real, it is #36's, and refusing it
    /// here would refuse it silently at the wrong moment.
    fn validated(self) -> Result<Self, String> {
        let expected = fleetor_core::pane::WORKER_SLOTS.len();
        if self.workers.len() != expected {
            return Err(format!("a fleet has {expected} worker seats, not {}", self.workers.len()));
        }
        for seat in std::iter::once(&self.orch).chain(&self.workers) {
            if harness::by_name(&seat.harness).is_none() {
                let registered: Vec<&str> =
                    harness::registered().iter().map(|h| h.spec().name).collect();
                return Err(format!(
                    "`{}` is not a harness this build can place — the registered ones are {}",
                    seat.harness,
                    registered.join(", "),
                ));
            }
        }
        Ok(self)
    }

    /// The spec one seat places as, resolved against the registry.
    ///
    /// `None` for a pane that carries no choice — the two judges and the operator —
    /// so the caller keeps the arms it already had rather than growing a branch
    /// here.
    fn spec_for(&self, pane: PaneId) -> Option<PaneSpec> {
        let seat = match pane {
            PaneId::Orch => &self.orch,
            PaneId::Worker(slot) => self.workers.get(usize::from(slot).checked_sub(1)?)?,
            PaneId::Evaluator | PaneId::Critic | PaneId::Operator => return None,
        };
        let harness = harness::by_name(&seat.harness)?;
        let placed = match pane {
            PaneId::Orch => PaneSpec::orch(harness),
            PaneId::Worker(slot) => PaneSpec::worker(slot, harness),
            _ => return None,
        };
        Some(match &seat.model {
            Some(model) => placed.with_model(model),
            None => placed,
        })
    }
}

// --- what a click will cost, and what would refuse it (WP-25 #36) -------------
//
// Three properties live in this section, and they are together because they are
// three readings of one value: the [`FleetSeats`] the pickers write and
// [`spawn_pane`] places against (M15). None of them can be computed from a seat
// alone — each needs what the harnesses reported about *this machine* — and all
// three fail silently if they are wrong, which is why the spec singles them out:
//
//  1. a seat on a harness that is not logged in must **refuse the start**, so the
//     operator never watches a pane spawn into a login prompt (story 11);
//  2. the cost line must say what **each** harness will actually spend, with C9's
//     orchestrator/worker split intact rather than flattened to "it will spend
//     tokens" (story 12);
//  3. a model the harness's own catalog no longer lists must **fall back with a
//     visible notice rather than spawn** (story 14).
//
// **The model half is here and only here.** `FleetSeats::validated` deliberately
// checks harness names and seat count and not models (#35), because a model needs
// the live catalog and `validated` has no reading to check one against. Splitting
// the model rule across the two would have put its quiet half in the function that
// looks like it handles validation.

/// The orchestrator's sentinel, in the one place both sides read it (M2).
///
/// A *seat* default rather than a model name — `None` on the wire — so it keeps
/// reachable exactly the command the orchestrator ran before the pickers existed:
/// the operator's own login, naming no model at all. `ui/src/fleet/types.ts` spells
/// the same string and `tests/gate_refusal.rs` fails if the two drift, because a
/// fallback that named a different default on each side would be invisible.
pub const DEFAULT_YOUR_LOGIN: &str = "default (your login)";

/// **One seat the fleet will not start with, and why** (story 11).
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct StartRefusal {
    /// The seat, by the label its picker row wears.
    seat: String,
    /// The harness that seat is on.
    harness: String,
    /// The vendor's own sentence about this machine — the operator's only
    /// actionable line, so it is carried verbatim rather than summarised.
    reason: String,
}

/// **What one harness will actually spend, in one role** (story 12, C9).
///
/// One line per *role*, not per seat: four workers on one harness spend one
/// credential in one way, and four identical sentences would be the gate padding a
/// single-screen cost statement into something nobody reads to the end of.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct CostLine {
    /// Which seats this sentence is about — `orchestrator`, `worker seats 1 and 3`.
    seats: String,
    /// The harness those seats are on.
    harness: String,
    /// The whole sentence, assembled in [`orchestrator_cost`] or [`worker_cost`].
    sentence: String,
}

/// **A model the harness's own catalog does not list, and what the seat fell back
/// to** (story 14).
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct ModelFallback {
    seat: String,
    harness: String,
    /// The model that was asked for and is not there any more.
    asked: String,
    /// The words the row now shows — the seat's own default, spelled the way the
    /// row spells it rather than as `null`.
    fell_back_to: String,
}

/// **What a click will do, read off the seats that will place** (M15).
///
/// Computed in Rust and rendered by the interface rather than derived on both
/// sides. The gate's summary and the rule that refuses a start are then the same
/// answer to the same question: an interface that re-derived "can this start" from
/// the same inputs would be a second implementation of the rule, and the one that is
/// wrong is the one somebody reads.
#[derive(Serialize, Clone, Debug, PartialEq, Eq, Default)]
pub struct StartVerdict {
    /// Empty exactly when the fleet may start. Non-empty is a refusal, not a
    /// warning: the interface disables the button and [`fleet_bootstrap`] refuses.
    refusals: Vec<StartRefusal>,
    /// One sentence per harness per role, in seat order.
    cost: Vec<CostLine>,
    /// What was quietly replaced, said out loud.
    fallbacks: Vec<ModelFallback>,
}

/// The reading for one harness by name, or `None` when this build has no such
/// harness at all.
fn reading_for<'a>(
    readings: &'a [harness::HarnessReadiness],
    name: &str,
) -> Option<&'a harness::HarnessReadiness> {
    readings.iter().find(|reading| reading.harness.name == name)
}

/// **The operator's half of C9's split, as one sentence** (story 12).
///
/// The orchestrator runs the operator's *own* login and the provider that login
/// resolved — inherited and displayed, never picked (C2 as amended by C9) — so a
/// turn in this seat spends whatever that credential bills. On a subscription that
/// is the operator's own quota, which is precisely the warning "it will spend
/// tokens" does not carry.
///
/// **No plan tier is promised anywhere in it.** The shape comes from
/// `AccountShape::display`, which reports what the vendor's diagnostic said and
/// nothing more (C14 as narrowed by C58) — so a tier can only appear here if a
/// vendor reported one, and none does today.
fn orchestrator_cost(harness: &str, reading: Option<&harness::HarnessReadiness>) -> String {
    use harness::{AccountShape, LoginState};
    let (shape, bills) = match reading.map(|r| &r.login) {
        Some(LoginState::LoggedIn(account)) => (
            account.display(),
            match account {
                AccountShape::SubscriptionPlan { .. } => "your own subscription quota".to_string(),
                AccountShape::ApiKey => "the key that login stores".to_string(),
                AccountShape::CustomProvider { name, .. } => format!("whatever {name} bills"),
            },
        ),
        // Not logged in, unreadable, or a harness this build does not have. The row
        // above and — for the two that refuse — the refusal say which; this sentence
        // still has to be true, so it names no shape it did not read.
        _ => ("credential unread".to_string(), "whatever that credential bills".to_string()),
    };
    let provider = match reading.and_then(|r| r.provider.as_deref()) {
        Some(provider) => format!(", on {provider}"),
        None => String::new(),
    };
    format!(
        "{harness} in the orchestrator seat runs your own login ({shape}){provider}. \
         Turns there spend {bills} — never FLEETOR's metered key."
    )
}

/// **The fleet's half of C9's split, as one sentence** (story 12).
///
/// A worker is fenced on FLEETOR's own provider and key (D-030, D-052, D-062), so
/// its turns cannot reach the operator's plan at all. Saying so is the other half of
/// what makes the orchestrator's line meaningful: a cost statement that warned about
/// spend without saying *whose* leaves the operator to assume the wrong one.
fn worker_cost(harness: &str, seats: &str) -> String {
    format!(
        "{harness} in {seats} runs FLEETOR's own provider and key. Turns there spend the \
         fleet's metered credential — never your own login or plan."
    )
}

/// `worker seat 3`, `worker seats 1 and 2`, `all 4 worker seats`.
fn worker_seat_phrase(slots: &[u8]) -> String {
    let all = fleetor_core::pane::WORKER_SLOTS.len();
    if slots.len() == all {
        return format!("all {all} worker seats");
    }
    let named: Vec<String> = slots.iter().map(|slot| slot.to_string()).collect();
    match named.split_last() {
        None => "no worker seat".to_string(),
        Some((last, [])) => format!("worker seat {last}"),
        Some((last, rest)) => format!("worker seats {} and {last}", rest.join(", ")),
    }
}

/// The label a seat wears on its picker row, which is the label a refusal, a cost
/// line and a fallback all name it by. One spelling, four readers.
fn seat_label(pane: PaneId) -> String {
    match pane {
        PaneId::Orch => "orchestrator".to_string(),
        PaneId::Worker(slot) => format!("worker {slot}"),
        other => other.to_string(),
    }
}

impl StartVerdict {
    /// **What this fleet would do if the operator clicked now.**
    ///
    /// `workers_run` is `false` on a machine with no worker key, where the
    /// orchestrator runs alone (`FleetConfig::worker_backend == "none"`). It is not
    /// a display detail: a worker seat that cannot spawn must not refuse a start it
    /// was never going to be part of, and must not appear in a cost statement
    /// promising spend that will not happen.
    fn for_seats(
        seats: &FleetSeats,
        readings: &[harness::HarnessReadiness],
        workers_run: bool,
        fallbacks: Vec<ModelFallback>,
    ) -> Self {
        let orch = &seats.orch;
        let orch_reading = reading_for(readings, &orch.harness);
        let mut refusals = Vec::new();
        let mut cost =
            vec![CostLine {
                seats: seat_label(PaneId::Orch),
                harness: orch.harness.clone(),
                sentence: orchestrator_cost(&orch.harness, orch_reading),
            }];
        if let Some(reason) = refusal_for(orch_reading) {
            refusals.push(StartRefusal {
                seat: seat_label(PaneId::Orch),
                harness: orch.harness.clone(),
                reason,
            });
        }

        if workers_run {
            // Grouped by harness, in first-seat order, so a fleet where all four
            // agree produces one sentence and a mixed one produces as many as it
            // really has — never four identical lines, and never one that speaks for
            // seats it is not about.
            let mut grouped: Vec<(String, Vec<u8>)> = Vec::new();
            for (at, seat) in seats.workers.iter().enumerate() {
                let slot = u8::try_from(at + 1).unwrap_or(u8::MAX);
                match grouped.iter_mut().find(|(name, _)| name == &seat.harness) {
                    Some((_, slots)) => slots.push(slot),
                    None => grouped.push((seat.harness.clone(), vec![slot])),
                }
                let reading = reading_for(readings, &seat.harness);
                if let Some(reason) = refusal_for(reading) {
                    refusals.push(StartRefusal {
                        seat: seat_label(PaneId::Worker(slot)),
                        harness: seat.harness.clone(),
                        reason,
                    });
                }
            }
            for (name, slots) in grouped {
                let phrase = worker_seat_phrase(&slots);
                cost.push(CostLine {
                    seats: phrase.clone(),
                    sentence: worker_cost(&name, &phrase),
                    harness: name,
                });
            }
        }

        Self { refusals, cost, fallbacks }
    }

    /// The refusal as one sentence, or `None` when the fleet may start.
    ///
    /// Assembled here rather than at the two call sites so the operator reads the
    /// same words whether the interface stopped them or [`fleet_bootstrap`] did.
    fn why_it_will_not_start(&self) -> Option<String> {
        if self.refusals.is_empty() {
            return None;
        }
        let each: Vec<String> = self
            .refusals
            .iter()
            .map(|refused| {
                // An em dash rather than "which": the vendor's own line is a whole
                // sentence ("~/.claude.json records no completed `claude` login"),
                // and a conjunction in front of one reads as a grammar bug in the
                // gate rather than as a quotation of the harness.
                format!("{} is on `{}` — {}", refused.seat, refused.harness, refused.reason)
            })
            .collect();
        Some(format!(
            "the fleet will not start: {}. Put those seats on a harness that can take one, or \
             log in and press Re-check logins.",
            each.join("; "),
        ))
    }
}

/// Why a seat on `name` cannot spawn, or `None`.
///
/// The two arms are one rule read from two ends: a harness this build does not
/// register at all, and one it registers that this machine cannot log into. The
/// first is what [`FleetSeats::validated`] already refuses to *store*; it is
/// repeated here because a stored selection can outlive the build that stored it.
fn refusal_for(reading: Option<&harness::HarnessReadiness>) -> Option<String> {
    match reading {
        None => Some("no harness by that name is registered in this build".to_string()),
        Some(reading) => reading.refusal(),
    }
}

/// **A model the harness's own catalog does not list, replaced by the seat's own
/// default and named out loud** (story 14).
///
/// The gate is the last moment a retired model id can be a sentence instead of a
/// dead pane: `--model` reaches the vendor at spawn, and by then the operator has
/// been told the fleet started. So the selection that will place is settled here,
/// where the live catalog is, and the substitution is reported rather than done
/// quietly — a gate that silently ran a different model than the row showed is the
/// same lie M15 refuses about harnesses.
///
/// **Only the orchestrator seat is checked, and that is C9 rather than laziness.**
/// The catalog is the vendor's own resolution of the *operator's* configuration —
/// what the login in the orchestrator seat can ask for. A worker runs on FLEETOR's
/// provider and key, whose model names that catalog knows nothing about; checking a
/// worker against it would rewrite the launch configuration's own worker model to a
/// default on the day somebody put a worker on a harness that publishes a list,
/// which is the gate choosing a fleet nobody picked.
///
/// **A harness that publishes no catalog has no opinion.** An empty list is a real
/// answer (Claude Code has no catalog command), not evidence that every model is
/// retired, so nothing is checked against it and a name typed by hand stands.
fn settle_models(
    mut seats: FleetSeats,
    readings: &[harness::HarnessReadiness],
) -> (FleetSeats, Vec<ModelFallback>) {
    let Some(asked) = seats.orch.model.clone() else { return (seats, Vec::new()) };
    let Some(reading) = reading_for(readings, &seats.orch.harness) else {
        return (seats, Vec::new());
    };
    if reading.models.is_empty() || reading.models.iter().any(|model| model.slug == asked) {
        return (seats, Vec::new());
    }
    let fallback = ModelFallback {
        seat: seat_label(PaneId::Orch),
        harness: seats.orch.harness.clone(),
        asked,
        fell_back_to: DEFAULT_YOUR_LOGIN.to_string(),
    };
    seats.orch.model = None;
    (seats, vec![fallback])
}

/// **What each harness reports about this machine, held for the gate** (C58).
///
/// **Held rather than re-probed, and the mutex is the whole of the waiting.** The
/// probe costs a subprocess per harness and, for codex, a live provider reachability
/// request measured at about 1.4 s — so it may run once, it may not run on the path
/// between a click and a pty, and two callers arriving together must produce one
/// probe rather than two. A `Mutex` held *across* the probe gives all three: the
/// second caller blocks until the first has finished, then finds the answer already
/// there. That is deliberate, not a lock held too long.
///
/// **This is not [`Host`] kept at bootstrap**, and the distinction is D13's. The
/// spawn path still discovers the machine at every spawn and never reads this; what
/// is cached here is the *gate's* reading, which exists because the operator is
/// waiting on one screen and not because anything wanted a snapshot.
#[derive(Default)]
struct HarnessGate(Mutex<Option<Vec<harness::HarnessReadiness>>>);

/// One reading of the harnesses, and the sentences it produced for the feed.
type HarnessReading = (Vec<harness::HarnessReadiness>, Vec<(NoticeLevel, String)>);

impl HarnessGate {
    /// What the harnesses said, probing once if nobody has yet.
    ///
    /// Returns the notices too, and empty ones when the answer was already there:
    /// the operator is told what a probe found at the moment it finds it, and a
    /// cached read has found nothing new to say.
    fn read(&self) -> HarnessReading {
        let mut held = self.held();
        match held.as_ref() {
            Some(known) => (known.clone(), Vec::new()),
            None => {
                let (fresh, notices) = probe_the_harnesses();
                *held = Some(fresh.clone());
                (fresh, notices)
            }
        }
    }

    /// **Ask again, whatever is held** — the re-check button (M17, user story 10).
    ///
    /// The operator pressing it has just logged in from another terminal, and that
    /// is a fact no cached answer can contain. Re-rendering a held snapshot here
    /// would be the control that looks like it worked and did nothing.
    fn refresh(&self) -> HarnessReading {
        let mut held = self.held();
        let (fresh, notices) = probe_the_harnesses();
        *held = Some(fresh.clone());
        (fresh, notices)
    }

    /// Forget what is held, without paying for a new reading.
    ///
    /// Called when the target changes: codex resolves trust and project identity
    /// against a directory (C34), so a fleet pointed somewhere else has been told
    /// about a machine in a state it is no longer in. Invalidating costs nothing and
    /// the next [`Self::read`] pays for the truth, which is the right way round —
    /// the operator who just retyped a path is not the one waiting on a probe.
    fn invalidate(&self) {
        *self.held() = None;
    }

    /// The cell, recovered from a poisoned lock rather than propagated.
    ///
    /// A panic inside the probe must not turn every later gate render into an error:
    /// what is behind this mutex is a cache, and the worst a poisoned one can hold
    /// is a stale reading the re-check button already exists to replace.
    fn held(&self) -> std::sync::MutexGuard<'_, Option<Vec<harness::HarnessReadiness>>> {
        self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// **Where the start gate's two answers live — before there is a fleet to hold
/// them** (WP-25 #36).
///
/// The gate is the screen that runs *before* [`fleet_bootstrap`], and both of the
/// things it renders from are things a fleet that does not exist yet cannot own:
/// what each harness reports about this machine, and which harness and model the
/// operator put on each seat. #35 hung both off [`Fleet`], where [`fleet_gate`]
/// could only ever answer `no fleet is running` — the pickers were unreachable in
/// the one state they exist for, and nothing failed a compile or a run to say so.
///
/// They live here instead, managed for the app's lifetime, and [`Fleet`] holds **the
/// same `Arc`** rather than a second copy of the values. That is the idiom
/// [`Target`] and `Interview` already use in this file, for the identical reason:
/// two copies of one fact means the copy that is wrong is the one somebody reads.
#[derive(Default)]
pub struct GateHold {
    /// What each harness reports about this machine, probed once (C58).
    harnesses: HarnessGate,
    /// What the operator picked, which is what a click will spawn.
    ///
    /// `None` until something asks. The fleet nobody has picked yet is a function of
    /// the launch configuration's worker model, which is resolved from `prompts/`
    /// rather than known here — so the default is built by the first reader that has
    /// one to hand, and never guessed.
    seats: Mutex<Option<FleetSeats>>,
    /// **Sentences from a probe that ran before there was a feed to put them on.**
    ///
    /// The gate probes at app launch, when no store exists; the Activity feed is
    /// opened by [`fleet_bootstrap`]. Without this the operator's first reading of
    /// their own machine — including the caveat about what a passing check does not
    /// prove — would be discovered and then dropped.
    pending: Mutex<Vec<(NoticeLevel, String)>>,
}

impl GateHold {
    /// What is picked right now, defaulting to the fleet nobody has picked yet.
    fn seats(&self, worker_model: &str) -> FleetSeats {
        self.held().get_or_insert_with(|| FleetSeats::unpicked(worker_model)).clone()
    }

    /// Record a selection and hand back what is now stored.
    fn store_seats(&self, seats: FleetSeats) -> FleetSeats {
        let mut held = self.held();
        *held = Some(seats.clone());
        seats
    }

    /// Keep what a probe said until there is a feed for it.
    fn remember(&self, notices: Vec<(NoticeLevel, String)>) {
        self.pending().extend(notices);
    }

    /// Everything kept, handed over once.
    fn take_pending(&self) -> Vec<(NoticeLevel, String)> {
        std::mem::take(&mut *self.pending())
    }

    /// Recovered from a poisoned lock rather than propagated, for the reason
    /// [`HarnessGate::held`] gives: a panic in a probe must not wedge the gate.
    fn held(&self) -> std::sync::MutexGuard<'_, Option<FleetSeats>> {
        self.seats.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn pending(&self) -> std::sync::MutexGuard<'_, Vec<(NoticeLevel, String)>> {
        self.pending.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// The gate's one reading of the machine, with the sentences it produced.
///
/// The sentences and the conditions are placement's (`Host::harness_notices`); every
/// caller here does the emit and nothing more, exactly as `machine_notices` is used
/// on the spawn path.
fn probe_the_harnesses() -> HarnessReading {
    let host = Host::discover_for_the_gate();
    let notices = host.harness_notices();
    (host.harnesses, notices)
}

/// One harness as the start gate offers it — the facts beside the model (M17, C14).
#[derive(Serialize, Clone)]
pub struct HarnessOffer {
    /// The harness's name, which is also the value a [`SeatChoice`] carries.
    name: String,
    /// The program name as invoked, resolved on the operator's own `PATH`.
    invoked: String,
    /// **Which of the four states this machine is in**, as one word the interface
    /// switches on. The spellings are mirrored in `ui/src/fleet/types.ts` and the
    /// two lists are pinned against each other by `tests/gate_pickers.rs` — a fifth
    /// state added on one side and not the other renders as nothing at all.
    status: &'static str,
    /// The account shape, where this machine is logged in (C14 as narrowed by C58 —
    /// the *shape*, never a plan tier).
    account: Option<String>,
    /// **Why this harness cannot take a seat, in the vendor's own words.** Present
    /// for every state but `logged-in`, and rendered *inline beside the disabled
    /// option* rather than in place of it: a supported feature must never look
    /// unimplemented (user story 9).
    reason: Option<String>,
    /// **What a passing check does not prove** (#34's caveat, M17's twin). Present
    /// exactly when this machine looks logged in, because that is the only state
    /// where an unqualified green would mislead anybody.
    caveat: Option<&'static str>,
    /// The vendor's own version string.
    version: Option<String>,
    /// The provider the vendor resolved — a fact, never a control (C2, C9).
    provider: Option<String>,
    /// What the vendor's own catalog resolution offers, in the vendor's order. Empty
    /// is an answer: the gate offers no list and the operator types a name.
    models: Vec<ModelOffer>,
    /// Whether the `PATH` name and the vendor binary behind it agreed (C47). `false`
    /// with a resolved binary means a wrapper is injecting flags, and what a pane
    /// runs under is not what this row describes.
    readings_agree: bool,
    /// The vendor binary the `PATH` name resolved to, absolutely (C47).
    resolved: Option<String>,
}

/// One model a harness would accept, as the vendor names it.
#[derive(Serialize, Clone)]
pub struct ModelOffer {
    slug: String,
    display_name: String,
}

/// **The whole of what the start gate renders from** (M15).
///
/// One value, so the pickers and the summary cannot be reading two things. The
/// summary is generated from `seats`, and `seats` is what [`spawn_pane`] places
/// against — that is the chain M15 asks for, and it has no branch in it.
#[derive(Serialize, Clone)]
pub struct GateState {
    /// Every registered harness, offered or refused, **never filtered**.
    harnesses: Vec<HarnessOffer>,
    /// What is currently picked, which is what a click will spawn.
    seats: FleetSeats,
    /// The model a worker runs when the operator names none, from `prompts/`.
    worker_model_default: String,
    /// **What a click will cost and whether it is allowed at all** (#36), computed
    /// from `seats` above and the same readings `harnesses` was built from.
    ///
    /// On the gate's own value rather than derived by the interface, so the summary
    /// the operator reads and the rule [`fleet_bootstrap`] enforces are one answer.
    verdict: StartVerdict,
}

impl HarnessOffer {
    /// One harness's readiness, in the shape the interface reads.
    ///
    /// **The four status words are decided here and nowhere else.** `unreadable` is
    /// deliberately not a refusal (C14): a vendor that changed its report format is
    /// a working installation the gate could not parse, and treating that as "logged
    /// out" would refuse a seat on the strength of a parse error.
    fn from(readiness: &harness::HarnessReadiness) -> Self {
        use harness::LoginState;
        let (status, account, reason) = match &readiness.login {
            LoginState::LoggedIn(shape) => ("logged-in", Some(shape.display()), None),
            LoginState::NoCredential { summary } => ("no-credential", None, Some(summary.clone())),
            // The sentence is `harness::not_on_the_path`'s rather than this file's,
            // because the start refusal says the same thing about the same machine
            // (#36) and an operator told two different things about one installation
            // trusts neither.
            LoginState::NotInstalled => (
                "not-installed",
                None,
                Some(harness::not_on_the_path(&readiness.invoked)),
            ),
            LoginState::Unreadable { why } => ("unreadable", None, Some(why.clone())),
        };
        let models = readiness
            .models
            .iter()
            .map(|m| ModelOffer { slug: m.slug.clone(), display_name: m.display_name.clone() })
            .collect();
        Self {
            name: readiness.harness.name.to_string(),
            invoked: readiness.invoked.clone(),
            status,
            account,
            reason,
            caveat: readiness.login.caveat(),
            version: readiness.version.clone(),
            provider: readiness.provider.clone(),
            models,
            readings_agree: readiness.readings_agree,
            resolved: readiness.resolved.as_ref().map(|p| p.display().to_string()),
        }
    }
}

/// The live backend, created once by [`fleet_bootstrap`] and kept for the app's
/// lifetime. Owns the tokio runtime the follower, hub, and delivery loop run on.
struct Fleet {
    rt: Runtime,
    store: Arc<dyn Store>,
    /// Fired on window close so the hub stops serving and unlinks its socket.
    shutdown: Arc<Notify>,
    config: FleetConfig,
    /// Resolved at bootstrap, updated in place by `fleet_set_target` /
    /// `fleet_pick_target`. Panes spawn against *this*, not a re-read of
    /// config.json. Safe to update before panes exist (the start gate); once any
    /// pane exists both commands refuse (see [`ensure_target_settable`]). The UI
    /// also stops offering the control at that point, which is now belt and
    /// braces rather than the only guard (D-071).
    ///
    /// **The handoff watch holds a clone of this same cell, not a copy of its
    /// value** (D14) — see [`Target`] for what that fixed.
    target: Target,
    /// The briefs and launch settings every pane spawns with, from `prompts/`
    /// and the operator's `~/.fleetor/prompts/`. Resolved once for the same
    /// reason the target is: a fleet whose panes were briefed from two revisions
    /// of a file being edited is not a fleet anyone can reason about.
    context: PaneContext,
    /// **What the operator picked at the start gate, and what the harnesses said**
    /// (#35, #36; M1, M15, C23, C58).
    ///
    /// **A handle on the app's own cell, not a copy of its values.** The gate writes
    /// it before this struct exists — that is the whole point of [`GateHold`] — and
    /// [`spawn_pane`] reads it afterwards, which is M15's chain with nothing in the
    /// middle: the summary renders what this cell holds, and this cell is what
    /// places. Snapshotting it at bootstrap would put the selection and the thing
    /// that spawns one `Arc::clone` apart and the divergence would be silent.
    ///
    /// Held here rather than passed at spawn because panes come up one at a time,
    /// lazily, as their terminals mount — so a selection travelling on the spawn
    /// call would be one choice spread across five invocations of the pty path.
    gate: Arc<GateHold>,
    /// Where each worker's own transcript lives, recorded at spawn — the WP-04
    /// live gauge's source of truth. Empty until a worker has actually spawned.
    gauges: Arc<GaugeSources>,
    /// A clone of the sender [`Hub`] was built with. `fleet_roster` sends
    /// [`AppCommand::Roster`] into it directly — the identical op the CLI's
    /// `fleet roster` reaches over the socket — so the UI's poll and the CLI
    /// converge on the one place ([`deliver::spawn_delivery`]'s `Roster` arm)
    /// that samples gauges and guards the once-per-session Notice.
    app: mpsc::UnboundedSender<AppCommand>,
    /// Where this fleet lives on disk (D1, D-075). **Resolved once, at bootstrap,
    /// and held**: the state root cannot move under a running fleet, and every
    /// command that reaches for a path while the fleet is up now reads the value
    /// the panes were placed against rather than re-deriving one from `$HOME`.
    ///
    /// **[`Host`] is deliberately not here beside it (D13).** The layout is where
    /// this installation writes and that is fixed for the session; the host is what
    /// the machine has, and that changes while the session runs — a rustup
    /// installed mid-session, an `.env` saved, a `fleet` binary built. Caching it
    /// would make those invisible until relaunch, which is the exact staleness D13
    /// exists to avoid, and it would do so silently. [`spawn_pane`] discovers it per
    /// spawn instead: a handful of filesystem checks, six times a session.
    layout: Layout,
    /// The routing hub, held so the operator's composer can call it (WP-07).
    ///
    /// The human has no pane and therefore no socket to dial, but their
    /// messages must take the same route a pane's do or "operator → pane rides
    /// the existing path unmodified" is a claim rather than a fact.
    /// [`fleet_send`] calls `Hub::handle` with `from: PaneId::Operator` — the
    /// identical function `Hub::serve_conn` calls after reading a `Hello`, with
    /// the socket the only thing missing.
    hub: Arc<Hub>,
    /// Whether the operator has opened the Critic's interview (WP-21, D-079).
    /// Closed on a fresh fleet, and a property of the *run* rather than of a
    /// pane's session — a `/clear` or a respawn inside the Critic does not
    /// close it, because nothing about a restarted terminal changes what the
    /// operator decided to allow.
    ///
    /// **A handle on the hub's own cell, taken from [`Hub::interview`], not a
    /// second copy of the value** — the mistake [`Target`] exists to have
    /// fixed. The hub is what enforces the switch; these commands only move it,
    /// and two `bool`s would let the UI report open while the hub still refused.
    interview: Interview,
}

/// Managed Tauri state: at most one embedded fleet.
#[derive(Default)]
pub struct FleetState(Mutex<Option<Fleet>>);

/// The repo the fleet works on — **one value, shared by everything that reads it**
/// (WP-21, D14).
///
/// Before this type there were two: the field on [`Fleet`], which
/// [`apply_target`] rewrites, and a `PathBuf` the handoff watch was handed at
/// bootstrap and kept forever. [`wake_evaluator`]'s doc comment claimed the two
/// were "the same one", and they were not — a target set at the start gate moved
/// the spawn path and left the watch on whatever bootstrap had resolved. The
/// comment was right about what should be true; this makes it true, by giving the
/// two readers one cell instead of two copies.
///
/// **No behaviour change, and there is a reason to expect none rather than a hope:**
/// D-071 fixed the target the moment a pane exists, and a handoff cannot happen
/// before `orch` exists to send one. So the value the watch used to snapshot and
/// the value it now reads can only differ in a fleet that never reaches a handoff.
/// The behaviour-change half of D14 was that freeze, and it is already in.
///
/// A `Mutex` rather than an `RwLock` because there is one writer, at most a
/// handful of times, before any pane exists; a poisoned lock hands back the value
/// anyway, since a target nobody can read is a fleet that cannot spawn.
#[derive(Clone)]
struct Target(Arc<Mutex<PathBuf>>);

impl Target {
    fn new(path: PathBuf) -> Self {
        Self(Arc::new(Mutex::new(path)))
    }

    /// What the fleet is pointed at right now.
    fn get(&self) -> PathBuf {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Point it somewhere else. Only ever reached through [`adopt_target`], which
    /// refuses once a pane exists (D-071).
    fn set(&self, path: &Path) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = path.to_path_buf();
    }
}

// --- locations ----------------------------------------------------------------

/// The operator's own layout: `~/.fleetor` and everything under it.
///
/// **The tree itself is [`Layout`], and since D-075 this is the only free function
/// left that names it.** There were six — `fleetor_dir`, `shell_dir`,
/// `testbed_dir`, `config_path`, `socket_path` and this one — each an accessor on
/// `Layout::for_operator()` wearing a different name, kept while the improvised
/// bring-up sequence still called them. The sequence is gone, so they are: a
/// running fleet reads [`Fleet::layout`], and the handful of commands that run
/// before bootstrap (the start gate's configuration, the History list, the dev-mode
/// flag, the orphan sweep) call this and then one accessor, which is one spelling
/// of where a thing lives rather than six.
pub(crate) fn layout() -> Layout {
    Layout::for_operator()
}

/// The target named in `~/.fleetor/config.json`, if there is a usable one.
/// `Ok(None)` means nothing is configured (the ordinary first-run case); `Err`
/// means something *is* configured and can't be used, which the operator has to
/// be told about rather than silently working somewhere else.
fn configured_target(layout: &Layout) -> Result<Option<PathBuf>, String> {
    let path = layout.config_file();
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("could not read {}: {e}", path.display())),
    };
    let Some(target) = parse_target(&text).map_err(|e| format!("{}: {e}", path.display()))? else {
        return Ok(None);
    };
    if !target.is_dir() {
        return Err(format!("target {} is not a directory", target.display()));
    }
    Ok(Some(target))
}

/// Pull the `target` out of config text. `Ok(None)` for a config that simply
/// doesn't set one; `Err` only for text that isn't JSON at all — a typo'd config
/// must not read as "no target configured". Kept pure so it is unit-tested
/// without touching the filesystem.
fn parse_target(text: &str) -> Result<Option<PathBuf>, String> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("not valid JSON ({e})"))?;
    let Some(target) = value.get("target") else { return Ok(None) };
    let Some(s) = target.as_str() else {
        return Err("\"target\" must be a path string".to_string());
    };
    let s = s.trim();
    Ok((!s.is_empty()).then(|| PathBuf::from(s)))
}

// --- bootstrap ----------------------------------------------------------------

/// Start the embedded fleet (idempotent) and return the boot snapshot.
///
/// First call: opens the store, wraps it in the live bus, spawns the follower
/// pump, resolves the target, and binds the hub. Later calls (e.g. React
/// StrictMode's double-mount) find it already running and just return a fresh
/// snapshot.
///
/// **This is where the fleet refuses to start** (WP-25 #36, story 11). A seat on a
/// harness this machine cannot log into would come up on a login prompt, and the
/// operator would be watching a pane that is never going to answer. The refusal is
/// here rather than in [`spawn_pane`] because a pane-by-pane failure arrives *after*
/// the operator has been told the fleet started; and it comes before the run
/// boundary is cut, because a refusal that has already archived the last run and
/// opened an empty database is not a refusal, it is a start that failed.
///
/// The interface disables the button on the same verdict, so this is the belt to
/// that brace — and the only one of the two a caller cannot skip.
#[tauri::command]
pub fn fleet_bootstrap(
    app: AppHandle,
    state: State<'_, FleetState>,
    registry: State<'_, Arc<PaneRegistry>>,
    gate: State<'_, Arc<GateHold>>,
) -> Result<BootSnapshot, String> {
    let mut guard = state.0.lock().map_err(|e| e.to_string())?;
    if let Some(fleet) = guard.as_ref() {
        return snapshot(&fleet.store);
    }

    // Where this installation lives, resolved once and then held on `Fleet` — the
    // one process-global read the whole spawn path performs (D1, D-075).
    let layout = Layout::for_operator();
    let dir = layout.shell();
    std::fs::create_dir_all(&dir).map_err(|e| format!("create shell dir: {e}"))?;

    // Prompts and launch settings, resolved before anything else is touched: the
    // refusal below needs the launch configuration's worker model to know what the
    // unpicked fleet is. Its notices wait for a feed to exist (below) — an override
    // that silently did nothing is the one failure the whole override path is built
    // to avoid, and it is the *emit* that has to wait, not the read.
    let context = PaneContext::resolve(&prompts::override_dir(layout.root()));

    // **What each harness makes of this machine** (WP-25 #34; C8, C14) — held on the
    // gate, which has almost always probed already: the start gate renders at app
    // launch and reads it there, off every path the operator is waiting on. A start
    // reached without one pays for the probe here, and that is the right end to pay
    // it at: the operator has just committed to spending money, and 1.4 s is less
    // than a pane takes to come up.
    let (readings, probe_notices) = gate.harnesses.read();

    // **What a click will do, decided once and enforced here** (#36). The seats are
    // settled first, so a model the vendor no longer lists has already fallen back
    // to the seat's own default rather than reaching a `--model` flag (story 14).
    let (settled, model_fallbacks) = settle_models(gate.seats(&context.launch.worker_model), &readings);
    let seats = gate.store_seats(settled);
    let verdict = StartVerdict::for_seats(&seats, &readings, workers_run(), model_fallbacks);
    if let Some(why) = verdict.why_it_will_not_start() {
        return Err(why);
    }

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("start runtime: {e}"))?;

    // Cut the run boundary before anything opens the log (WP-11, D-058). This is
    // the whole of run isolation: the previous run becomes a frozen file under
    // `runs/` and this one opens an empty database. Held rather than emitted —
    // there is no store to append a notice to yet.
    let started_ms = fleetor_core::time::now_ms();
    let rotation = runs::rotate(&dir, &runs::runs_dir(layout.root()), started_ms);

    // The observability core: real store, wrapped once so every append publishes.
    let bcast = Arc::new(BroadcastStore::new(Arc::new(
        SqliteStore::open(&dir.join("state.db")).map_err(|e| format!("open store: {e}"))?,
    )));
    let store: Arc<dyn Store> = bcast.clone();

    spawn_follower(&rt, bcast.clone(), app.clone());

    for (level, text) in &rotation {
        note(&store, *level, text);
    }

    let target = Target::new(resolve_target(&layout, &store)?);
    let config = fleet_config_for(&target.get());

    // **What each harness makes of this machine, on the feed** (WP-25 #34; C8, C14).
    // The reading itself was taken above, before this fleet was allowed to exist;
    // these are its sentences, and they reach the operator here because this is the
    // first moment there is a feed to put them on.
    //
    // Two sources, one drain. `take_pending` is everything the *gate's* probe said
    // at app launch, when no store existed to say it to — the ordinary case, since
    // the start gate renders before anything can be started. `probe_notices` is
    // non-empty only when nothing had probed and this call paid for it. The
    // sentences and the conditions are placement's (`Host::harness_notices`); this
    // is the emit and nothing more, exactly as `machine_notices` is above.
    for (level, text) in gate.take_pending().into_iter().chain(probe_notices) {
        note(&store, level, &text);
    }

    // Stamp what this run is, for the History row it becomes at the next start.
    // The target is only ever prose inside a notice in the log, so a run that
    // ended without this marker lists with an unknown target rather than a guess.
    runs::begin(&dir, started_ms, &target.get());

    // Every notice the prompt resolver produced, from the read at the top of this
    // function — an override that silently did nothing is the one failure the whole
    // override path is built to avoid.
    for (level, text) in &context.notices {
        note(&store, *level, text);
    }

    // The hub↔app seam: the hub routes, the app owns the terminals. Unbounded on
    // purpose — a bounded channel would make a busy fleet block a send (D-034).
    let (app_tx, app_rx) = mpsc::unbounded_channel();
    let gauges = Arc::new(GaugeSources::default());
    deliver::spawn_delivery(&rt, registry.inner().clone(), app_rx, store.clone(), gauges.clone());
    // The hub takes its own clone; `fleet_roster` sends into the original so
    // the UI's poll reaches the identical `AppCommand::Roster` arm the CLI's
    // `fleet roster` does over the socket (see the `Fleet::app` doc).
    let (hub, shutdown) = spawn_hub(&rt, store.clone(), app_tx.clone(), layout.socket());

    // The write guardrail's own feed (WP-17). Started with the run and emptied
    // by it, so what the operator reads is this run's refusals and not the last
    // one's — those went to `runs/` with the rest of that log (D-058).
    spawn_guardrail_feed(&rt, store.clone(), &dir);

    // The wake (WP-15), a second subscriber on the bus D-020 built for exactly
    // this: downstream of `append_event`, outside the hub, sending no
    // `AppCommand` and holding nothing up. See `spawn_evaluator_wake`.
    spawn_evaluator_wake(&rt, bcast.clone(), store.clone(), app, target.clone());

    let snap = snapshot(&store)?;
    *guard = Some(Fleet {
        rt,
        store,
        shutdown,
        config,
        target,
        // The same cell the gate wrote the selection into, not a copy of what it
        // held a moment ago (M15). What places is what the operator picked, and
        // there is no second value that could disagree.
        gate: Arc::clone(&gate),
        context,
        gauges,
        app: app_tx,
        layout,
        interview: hub.interview(),
        hub,
    });
    Ok(snap)
}

// --- panes --------------------------------------------------------------------

/// Bring one pane up and hand it to the registry.
///
/// **Every pane kind that can exist comes up through [`crate::placement`]**
/// (WP-21). That module owns the order — cwd, config seed, guardrail, notices,
/// command — against a [`Layout`] and a [`Host`] handed to it, so the identical
/// code path runs in a test against a scratch directory.
///
/// **What is left here is four lines of wiring and one refusal (D-075):** the
/// `PaneId` this pane's `PaneSpec` is, the name with nothing to spawn, and putting
/// what placement returned where it goes — the notices on the feed, the gauge
/// source in the map, the command in the registry. It knows no step of the
/// sequence and no path under `~/.fleetor`. Adding a pane kind is a variant in
/// `PaneSpec` and an arm in `place`, not a new branch here.
///
/// **The L1 re-seed requirement is met structurally, and now entirely inside
/// [`placement::place`].** `hasTrustDialogAccepted` is keyed by absolute project
/// path, so a fleet pointed at a new target needs its seed re-applied for every
/// pane cwd — otherwise all four workers sit on a trust dialog while every `fleet
/// send` reports success. Seeding inside the placement that also chooses the cwd,
/// rather than at the target picker, means that can only be got wrong by deleting
/// a line, not by forgetting a code path.
/// The spec for a pane the gate offers no choice for.
///
/// **Three of the four arms are the arms `spawn_pane` always had**, lifted out
/// whole when the picked seats moved above them, and matched exhaustively rather
/// than with a wildcard so a new pane kind still has to say what places it.
///
/// The fourth is the one this ticket added: a seat the gate *does* carry a choice
/// for, whose choice would not resolve. `FleetSeats::spec_for` refuses rather than
/// substituting — a fleet that quietly placed a different harness than the gate
/// promised is the failure M15 exists to prevent — and `fleet_set_seats` is what
/// stops such a value being stored. This arm is the belt to that pair of braces.
fn spec_without_a_choice(pane: PaneId, seats: &FleetSeats) -> Result<PaneSpec, String> {
    match pane {
        PaneId::Evaluator => Ok(PaneSpec::Evaluator),
        // **The live run, because that is the only run this name can mean here**
        // (D-076). `pty_spawn` carries a pane and nothing else, so a Critic opened
        // from the rail is a Critic on the run in progress; pointing one at a
        // History row is a different gesture with a different argument, and it does
        // not exist yet (`placement::ARCHIVED_NOT_BUILT`).
        PaneId::Critic => Ok(PaneSpec::Critic { run: RunSource::Live }),
        // Never spawnable, and refused here rather than left to fail somewhere
        // deeper: there is no command to run for a human, no cwd that is theirs, and
        // no config dir to seed. WP-07's "the operator is never spawnable or
        // killable" is this arm plus the fact that nothing in the UI offers the
        // button (`pty_kill` on a name with no pty already answers "operator is not
        // running").
        PaneId::Operator => {
            let why = "the operator is a participant, not a pane — there is nothing to spawn";
            Err(why.to_string())
        }
        PaneId::Orch | PaneId::Worker(_) => {
            let named = match pane {
                PaneId::Orch => seats.orch.harness.clone(),
                _ => seats.workers.iter().map(|s| s.harness.as_str()).collect::<Vec<_>>().join("/"),
            };
            Err(format!(
                "no registered harness answers to `{named}`, so this seat cannot be placed",
            ))
        }
    }
}

pub(crate) fn spawn_pane(
    fleet: &FleetState,
    registry: &Arc<PaneRegistry>,
    pane: PaneId,
    rows: u16,
    cols: u16,
) -> Result<(), String> {
    let (target, layout, store, context, gauges, seats) = {
        let guard = fleet.0.lock().map_err(|e| e.to_string())?;
        let f = guard.as_ref().ok_or("fleet not bootstrapped")?;
        (
            f.target.get(),
            f.layout.clone(),
            f.store.clone(),
            f.context.clone(),
            f.gauges.clone(),
            // The gate's own cell, read at the moment this pane comes up — the far
            // end of M15's chain. The worker default is only reached on a fleet
            // nothing ever picked for, which is the unpicked fleet #35 preserved.
            f.gate.seats(&f.context.launch.worker_model),
        )
    };

    // The seam. The layout comes off `Fleet`, resolved once at bootstrap; the host
    // is discovered *here*, at every spawn, and is deliberately not held beside it
    // (D13). That is what the code did before placement existed, so a toolchain
    // installed mid-session still takes effect on the next pane restart. Between
    // them they are the whole of what placement would otherwise have had to read
    // from the process.
    //
    // Every kind but one is placed. Matched exhaustively rather than with a
    // wildcard, so a new pane kind has to say what places it instead of silently
    // getting somebody else's arm.
    //
    // **Which harness and which model each seat runs is the operator's answer, read
    // here** (WP-25 #35; M1, M15, C23). #33 carried the choice on `PaneSpec` with
    // one call site that said Claude Code twice; this is that call site asking the
    // gate instead. `FleetSeats::spec_for` answers `None` for every pane that
    // carries no choice — the two judges, whose specs have no field to hold one
    // (C15), and the operator, who is refused below — so the arms after it are the
    // arms they always were.
    //
    // **The whole of M15 is this line.** The gate renders its summary from the same
    // `seats` value, so a fleet the gate described and a fleet that spawns cannot be
    // two different fleets without one of them being read from somewhere else.
    let spec = match seats.spec_for(pane) {
        Some(picked) => picked,
        None => spec_without_a_choice(pane, &seats)?,
    };

    // One discovery, read twice: the warnings below and the placement itself see
    // the same machine (D-075). They were two independent reads of `fleet_bin_path`
    // and `operator_toolchain` — a third and fourth spelling of "what does this
    // machine have" beside `Host`'s, which is what `Host` exists to end.
    let host = Host::discover();

    // What this machine is missing, said before placement is asked to do anything:
    // these have to land even when the placement that follows fails, so a machine
    // with no worker key still tells the operator what *else* is wrong instead of
    // only naming the key. The sentences and the conditions are placement's
    // (D-075) — this is the emit, and nothing more.
    for (level, text) in placement::machine_notices(&host, pane) {
        note(&store, level, &text);
    }
    let placed = placement::place(spec, &layout, &host, &target, &context)?;
    for (level, text) in &placed.notices {
        note(&store, *level, text);
    }
    if let Some(source) = placed.gauge {
        gauges.record(pane, source);
    }

    // **What this pane was placed as, written down twice — once for the archive
    // and once for the feed** (M24, #39).
    //
    // The archive's copy goes into the live run's own `run.json`, because that is
    // the only place it can survive to reach `manifest.json`: rotation archives the
    // *previous* run, whose panes and whose fleet are both gone, and a
    // configuration directory is named for its seat and says nothing about which
    // vendor was pointed at it (C33). The feed's copy is the spawn event, so an
    // operator watching a mixed fleet come up is not poorer than a Critic reading
    // the same run afterwards.
    //
    // Both are the values placement *returned*, never re-derived from `spec`: a
    // fact the caller recomputes is a fact that can disagree with the one the pane
    // was actually brought up on.
    //
    // Neither is on the message path and neither can become so (Tier 1.4) — this is
    // the spawn path, several filesystem writes deep already, and a `fleet send`
    // reads none of it.
    runs::record_pane(&layout.shell(), &pane.to_string(), placed.harness, placed.model.as_deref());
    if let Err(e) = store.append_event(&FleetEvent::PaneState {
        pane,
        // A pane with no pty is `Dead` to every reader in the fleet — the roster
        // included — so that is what it was a moment ago, rather than a fifth state
        // meaning "never existed".
        from: PaneState::Dead,
        to: PaneState::Spawning,
        harness: Some(placed.harness.name.to_string()),
        model: placed.model.clone(),
    }) {
        eprintln!("fleet: could not append the spawn event: {e}");
    }

    registry.spawn(pane, placed.command, placed.harness, rows, cols)
}

/// Poll the guardrail's refusal journal onto the Activity feed.
///
/// **A silent denial is the wrong answer.** A pane that cannot tell a guardrail
/// from a broken path retries forever or reports confident nonsense, and an
/// operator who never learns a pane was refused cannot tell a mission that is
/// going badly from one that is fenced badly.
///
/// A task of its own on the fleet's runtime, deliberately not a hook into
/// anything the message path touches (Tier 1.4): a refusal is at most a second
/// late reaching the feed, and no delivery ever waits on one.
fn spawn_guardrail_feed(rt: &Runtime, store: Arc<dyn Store>, shell: &Path) {
    let mut journal = guardrail::Journal::fresh(guardrail::journal_path(shell));
    rt.spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(1));
        loop {
            tick.tick().await;
            for (level, text) in journal.drain() {
                note(&store, level, &text);
            }
        }
    });
}

// --- the wake (WP-15) ----------------------------------------------------------

/// Watch the run's own event stream for a handoff, and wake the evaluator on one.
///
/// ## Why this is not on the message path, and why that is structural
///
/// Tier 1.4 forbids anything between `fleet send` and a pty that can delay,
/// refuse, reorder, drop or alter a message.
/// `crates/fleetor-server/tests/handoff.rs` counts the `AppCommand`s a handoff
/// causes and expects **zero**. This task sends none: `Hub::handoff` is still not
/// `async`, still never touches `self.app`, and still answers `Recorded` from its
/// own append alone. The wake reaches the registry the way `pty_spawn` does —
/// through [`spawn_pane`] — which is not a road any message travels.
///
/// It rides D-020's bus, which is **persist-then-publish**: by the time an event
/// is on the channel it is already durable, and `broadcast::Sender::send` never
/// blocks its publisher. So the handoff op is answered whether or not this task
/// ever runs, a crash between the two loses the wake and never the record, and
/// a lagging follower recovers from the database rather than from the ring.
/// Nothing waits on any of it.
///
/// ## What it does mean, said plainly
///
/// **WP-15 is the first thing that reads a handoff back**, and `Hub::handoff`'s
/// own doc had to be corrected to say so (it claimed *nothing anywhere* did).
/// What `task.rs`'s tripwire list actually bars is a read that goes on to
/// **permit, order or refuse** something — a board that became a dispatcher.
/// This permits nothing, orders nothing and refuses nothing: it changes what
/// *exists* (a window, an address), which is the allowed shape WP-16 named and
/// WP-19 is held to. A `fleet send` is byte-identical before and after, and the
/// test that says so is untouched.
///
/// Idempotent for a second handoff: `PaneRegistry::spawn` is a no-op for a pane
/// that is still running, so `orch` finding more work and handing back again
/// does not get a second evaluator — it gets the one that is already there.
fn spawn_evaluator_wake(
    rt: &Runtime,
    bcast: Arc<BroadcastStore>,
    store: Arc<dyn Store>,
    app: AppHandle,
    target: Target,
) {
    rt.spawn(async move {
        // A follower of its own rather than an arm inside `spawn_follower`, so a
        // slow wake can never hold up the feed the operator is watching — and so
        // the file that pushes events to the webview stays a file about that.
        let mut follower = match bcast.follow(0) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("fleet: the handoff watch failed to start: {e}");
                return;
            }
        };
        while let Ok(Some((_, event))) = follower.next().await {
            if !matches!(event, FleetEvent::Handoff { .. }) {
                continue;
            }
            for (level, text) in wake_evaluator(&app, &target.get()) {
                note(&store, level, &text);
            }
        }
    });
}

/// One wake. Returns what the operator should be told — never nothing, unless
/// the mode is simply off, because a wake that silently did nothing is
/// indistinguishable from a wake that is broken.
/// `target` is read out of the fleet's own [`Target`] cell — **literally the same
/// value [`spawn_pane`] places the evaluator against** (D14), rather than a copy
/// taken at bootstrap that a target set at the start gate would leave behind.
/// Re-reading `config.json` here instead would let a mid-run target change send
/// the two at different repos.
fn wake_evaluator(app: &AppHandle, target: &Path) -> Vec<(NoticeLevel, String)> {
    let mut notices = Vec::new();
    match evaluator::readiness(dev::is_enabled(), target, &evaluator::MissionRoots::discover()) {
        // A build with no grader in it, and the ordinary non-dev case. Both are
        // silent: the first is what every release build is, and the second is
        // what the operator asked for by leaving the mode off.
        evaluator::Readiness::NotBuilt | evaluator::Readiness::ModeOff => return notices,
        evaluator::Readiness::NoMission { target, workspaces } => {
            notices.push((
                NoticeLevel::Warn,
                format!(
                    "dev mode is on, but {} is not a prepared mission workspace under {} — \
                     so this handoff has no ground truth to be judged against and nothing \
                     was started. Point the fleet at a prepared workspace first.",
                    target.display(),
                    workspaces.display(),
                ),
            ));
            return notices;
        }
        evaluator::Readiness::Ready(mission) => {
            // The answer key lands **before** the pane exists: the brief has the
            // evaluator seal a verdict against it before it speaks to `orch`, so
            // a key that arrived mid-conversation would be read after its
            // position had already formed from the fleet's own account.
            match evaluator::reveal_answer_key(&mission) {
                Ok(Some(text)) => notices.push((NoticeLevel::Info, text)),
                Ok(None) => {}
                Err(why) => notices.push((NoticeLevel::Warn, why)),
            }
        }
    }
    // **The wake is an event, not a window** (D-073). Creating a second OS window
    // was never what made the evaluator exist — the pane is spawned by the React
    // root's own `pty_spawn`, exactly like every other terminal, and the window
    // was only how that root got mounted. So the wake says *the evaluator is
    // awake* and the one webview turns its always-mounted view on. Idempotent for
    // a second handoff for the same reason `PaneRegistry::spawn` is: the view is
    // already showing a pane that is already running.
    match app.emit(EVENT_EVALUATOR_WAKE, ()) {
        Ok(()) => notices.push((
            NoticeLevel::Info,
            "the mission was handed back — the review view is live".to_string(),
        )),
        Err(why) => notices.push((NoticeLevel::Warn, format!("the review view: {why}"))),
    }
    notices
}

/// Decide where the fleet works, announcing the choice on the feed so it is never
/// a silent surprise. A configured target wins; anything else falls back to the
/// seeded testbed, which is the only case that has to create anything.
fn resolve_target(layout: &Layout, store: &Arc<dyn Store>) -> Result<PathBuf, String> {
    match configured_target(layout) {
        Ok(Some(target)) => {
            note(store, NoticeLevel::Info, &format!("target: {}", target.display()));
            Ok(target)
        }
        Ok(None) => fall_back_to_testbed(layout, store, None),
        Err(e) => fall_back_to_testbed(layout, store, Some(e)),
    }
}

fn fall_back_to_testbed(
    layout: &Layout,
    store: &Arc<dyn Store>,
    problem: Option<String>,
) -> Result<PathBuf, String> {
    let testbed = testbed::ensure(&layout.testbed())?;
    let (level, why) = match problem {
        Some(e) => (NoticeLevel::Warn, format!("{e} — ")),
        None => (NoticeLevel::Info, String::new()),
    };
    note(
        store,
        level,
        &format!(
            "{why}working in the seeded testbed at {}. Set \"target\" in {} to point the fleet at your own repo.",
            testbed.display(),
            layout.config_file().display()
        ),
    );
    Ok(testbed)
}

/// Pump every appended event to the webview, oldest-first then live. Started once;
/// runs for the app's lifetime.
fn spawn_follower(rt: &Runtime, bcast: Arc<BroadcastStore>, app: AppHandle) {
    rt.spawn(async move {
        let mut follower = match bcast.follow(0) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("fleet: follower failed to start: {e}");
                return;
            }
        };
        while let Ok(Some((seq, event))) = follower.next().await {
            if app.emit(EVENT_FLEET, WireEvent { seq, event }).is_err() {
                break; // webview gone
            }
        }
    });
}

/// Bind the hub on `sock` and serve until the app closes. Returns the shutdown
/// gate. (The socket is a parameter, not [`socket_path`], so this is exercisable
/// over a real unix socket in a test.)
///
/// A bind failure is the one error that takes messaging down completely, so it is
/// reported on the feed rather than only to stderr — a shell that looks fine while
/// every `fleet send` fails is the worst version of this failure.
/// Returns the hub itself alongside the gate, because the operator's composer
/// calls it in-process (WP-07) — it is built here rather than inside the served
/// task so there is exactly one, shared by the socket and the UI.
fn spawn_hub(
    rt: &Runtime,
    store: Arc<dyn Store>,
    app: mpsc::UnboundedSender<AppCommand>,
    sock: PathBuf,
) -> (Arc<Hub>, Arc<Notify>) {
    let _ = std::fs::remove_file(&sock); // clear a stale socket from a prior run
    let transport = Arc::new(UnixTransport::new(&sock));

    let shutdown = Arc::new(Notify::new());
    let gate = shutdown.clone();
    let for_note = store.clone();
    let hub = Hub::new(store, app);
    let serving = hub.clone();
    rt.spawn(async move {
        let hub = serving;
        tokio::select! {
            result = hub.run(transport) => {
                if let Err(e) = result {
                    eprintln!("fleet: hub stopped: {e}");
                    note(&for_note, NoticeLevel::Error, &format!("fleet socket unavailable — messaging is down: {e}"));
                }
            }
            _ = gate.notified() => {}
        }
        let _ = std::fs::remove_file(&sock);
    });
    (hub, shutdown)
}

// --- commands -----------------------------------------------------------------

/// The fleet configuration for the top bar and the spend gate. Works before
/// bootstrap (reads config.json directly) so the start gate can show what a
/// click will launch without actually launching it.
#[tauri::command]
pub fn fleet_config(state: State<'_, FleetState>) -> Result<FleetConfig, String> {
    let guard = state.0.lock().map_err(|e| e.to_string())?;
    if let Some(fleet) = guard.as_ref() {
        return Ok(fleet.config.clone());
    }
    drop(guard);
    // The same layout, the same accessor, the same question the bootstrap path
    // asks — one spelling of "what is the target", not a third (D-075). This path
    // still *propagates* a broken `target` where `resolve_target` falls back to the
    // testbed with a warning, and that difference is deliberate: the start gate is
    // showing the operator what a click will launch, and showing them the testbed
    // when their config names a directory that is not there would be a lie the
    // click then makes true.
    let layout = layout();
    let target = configured_target(&layout)?.unwrap_or_else(|| layout.testbed());
    Ok(fleet_config_for(&target))
}

/// The full path of the repo the running fleet is working in.
#[tauri::command]
pub fn fleet_target(state: State<'_, FleetState>) -> Result<String, String> {
    let guard = state.0.lock().map_err(|e| e.to_string())?;
    let fleet = guard.as_ref().ok_or("fleet not bootstrapped")?;
    Ok(fleet.target.get().to_string_lossy().into_owned())
}

/// Every pane and its state, with each worker's context gauge if one could be
/// sampled (WP-04). The UI's slow band poll and the `fleet` CLI's `fleet
/// roster` are two callers of the *same* op: this sends `AppCommand::Roster`
/// into the identical channel [`spawn_hub`] gave the [`Hub`], so both paths
/// converge on `deliver::spawn_delivery`'s one roster-answering arm — the one
/// place gauges are sampled and the once-per-session Notice is guarded.
///
/// A plain sync command, like [`fleet_pick_target`]'s blocking dialog call:
/// `oneshot::Receiver::blocking_recv` parks this call's own thread (Tauri
/// runs sync commands off its own pool), not the tokio runtime the hub and
/// the delivery loop run on, so there is nothing to deadlock.
#[tauri::command]
pub fn fleet_roster(state: State<'_, FleetState>) -> Result<Vec<PaneEntry>, String> {
    let app = {
        let guard = state.0.lock().map_err(|e| e.to_string())?;
        let fleet = guard.as_ref().ok_or("fleet not bootstrapped")?;
        fleet.app.clone()
    };
    let (ack, rx) = oneshot::channel();
    app.send(AppCommand::Roster { ack })
        .map_err(|_| "the fleet app is not accepting commands — its window may have closed".to_string())?;
    rx.blocking_recv()
        .map_err(|_| "the fleet app took the roster request but never answered".to_string())
}

// --- the operator's own messages (WP-07) --------------------------------------

/// What the composer's target select can be set to, beyond a pane's own name.
/// `all` and `reply` are spellings of a *verb*, not of a participant, which is
/// why they are resolved here into `Op::Broadcast`/`Op::Reply` rather than being
/// added to `PaneId` — a `PaneId` that meant "everyone" or "whoever spoke last"
/// would be a different participant on each side of the socket.
const TARGET_ALL: &str = "all";
const TARGET_REPLY: &str = "reply";

/// The outcome of one message the operator sent, in the fleet's own three
/// words. There is no fourth, and none of them is `delivered` (Tier 1.5).
#[derive(Serialize, Debug)]
pub struct OperatorSend {
    /// `accepted` — the bytes reached a live pty, which is not a claim the
    /// agent read them (L3). `undelivered` — they did not, and `detail` says
    /// why. `recorded` — it entered the log and no pty exists; unreachable from
    /// this composer today, since the only pty-less name is the sender.
    outcome: &'static str,
    /// The message id, or a broadcast's shared group id.
    id: String,
    /// The reason, when there is one — the pane that refused, or the legs of a
    /// fan-out that missed.
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

/// Send one message **as the operator**, through the hub the panes use.
///
/// The composer is the operator's `fleet send` / `fleet broadcast` / `fleet
/// reply`, and it is those verbs rather than a fourth thing: what crosses into
/// [`Hub::handle`] is an ordinary [`Op`] with `from: PaneId::Operator`. Nothing
/// about the delivery path knows the difference, which is the requirement —
/// "operator → pane rides the existing path unmodified" — held as a property of
/// the code rather than as a promise about it.
///
/// Sync rather than `async` for [`fleet_roster`]'s reason: Tauri runs sync
/// commands on its own pool, off the tokio runtime, so blocking this call's own
/// thread on the hub parks nothing the hub needs to finish.
#[tauri::command]
pub fn fleet_send(
    state: State<'_, FleetState>,
    target: String,
    text: String,
) -> Result<OperatorSend, String> {
    let op = operator_op(&target, &text)?;
    let (hub, handle) = {
        let guard = state.0.lock().map_err(|e| e.to_string())?;
        let fleet = guard.as_ref().ok_or("fleet not bootstrapped")?;
        (fleet.hub.clone(), fleet.rt.handle().clone())
    };
    Ok(operator_result(handle.block_on(hub.handle(PaneId::Operator, op)))?)
}

/// The composer's target and text as a wire op. Pure, so the mapping is tested
/// without a fleet — and refused here, before the hub, for the same reason the
/// CLI parses a pane name locally: a mistake should cost a sentence, not a
/// round trip.
fn operator_op(target: &str, text: &str) -> Result<Op, String> {
    let text = text.trim();
    if text.is_empty() {
        // An empty body would be typed into a live terminal as a bare newline —
        // a submitted empty turn. Clap refuses this for a pane; the composer is
        // the same boundary and owes the same refusal.
        return Err("write something first".to_string());
    }
    let text = text.to_string();
    match target.trim() {
        TARGET_ALL => Ok(Op::Broadcast { text }),
        TARGET_REPLY => Ok(Op::Reply { text }),
        name => Ok(Op::Send { to: name.parse::<PaneId>().map_err(|e| e.to_string())?, text }),
    }
}

/// The hub's answer in the operator's words. Pure, and separate from
/// [`fleet_send`], so the vocabulary is pinned by a test rather than by reading
/// the UI.
fn operator_result(result: OpResult) -> Result<OperatorSend, String> {
    match result {
        OpResult::Delivered { msg_id, accepted: true, .. } => {
            Ok(OperatorSend { outcome: "accepted", id: msg_id, detail: None })
        }
        // "undelivered", never "failed to send" — the send happened; it is the
        // arrival that did not. The same word the message feed uses.
        OpResult::Delivered { msg_id, accepted: false, detail } => {
            Ok(OperatorSend { outcome: "undelivered", id: msg_id, detail })
        }
        OpResult::Recorded { record_id } => {
            Ok(OperatorSend { outcome: "recorded", id: record_id, detail: None })
        }
        OpResult::Error { message } => Err(message),
        // The composer sends messages; a board or a roster coming back would be
        // a wiring mistake, and saying so beats rendering a blank success.
        other => Err(format!("the hub answered a message with something else: {other:?}")),
    }
}

/// What the operator is told when they try to move the target under a fleet
/// that is already up.
///
/// A constant rather than an inline literal because the test asserts on the
/// remedy, and a refusal whose remedy drifts out of the sentence is a refusal
/// the operator cannot act on.
const TARGET_FIXED: &str = "the target is fixed for a running fleet — every pane was configured \
                            against the current one when it spawned. Stop the fleet (close the \
                            window) and set the target again at the start gate.";

/// The rule: the target may be set until the first pane exists, and not after.
///
/// It was already written down and already true in practice — the start gate is
/// the only screen that offers the control, and it is gone the moment the fleet
/// starts. But it lived entirely in the interface, and the two commands behind
/// it were exposed unconditionally, so the thing that owns the state did not
/// enforce the one rule about changing it. Half a fleet in one repository and
/// half in another is incoherent, which is the same reasoning that makes the
/// target resolved once at bootstrap rather than re-read.
fn ensure_target_settable(registry: &PaneRegistry) -> Result<(), String> {
    if registry.any_pane() {
        return Err(TARGET_FIXED.into());
    }
    Ok(())
}

/// Record `target` as the fleet's, or refuse because a pane already exists.
///
/// **The one funnel both target-setting commands pass through.** Writing the
/// guard twice would make the picker and the typed box two independent chances
/// to get it right, and a rule enforced in two places is a rule that will
/// eventually hold in one of them. The refusal comes before [`write_target`] on
/// purpose: a config file recording a target no pane will ever be spawned
/// against is worse than no change at all.
fn adopt_target(
    registry: &PaneRegistry,
    state: &FleetState,
    gate: &GateHold,
    target: &Path,
) -> Result<(), String> {
    ensure_target_settable(registry)?;
    write_target(target)?;
    // **The harness reading is about a machine *in a directory*** (C34): codex
    // resolves trust and project identity against one, so a reading taken somewhere
    // else describes a state this machine is no longer in. Forgotten here rather
    // than inside `apply_target`'s `if let Some(fleet)`, because the target is
    // retyped almost exclusively at the start gate — where there is no fleet, and
    // where the reading that would go stale is the one the pickers are rendering.
    gate.harnesses.invalidate();
    apply_target(state, target);
    Ok(())
}

fn apply_target(state: &FleetState, target: &Path) {
    let is_git = placement::git(target, &["rev-parse", "--git-dir"]);
    if let Ok(mut guard) = state.0.lock() {
        if let Some(fleet) = guard.as_mut() {
            // The one write. Everything that reads the target — the spawn path
            // and the handoff watch alike — reads this cell (D14).
            fleet.target.set(target);
            fleet.config = fleet_config_for(target);
            note(
                &fleet.store,
                NoticeLevel::Info,
                &format!("target set to {}", target.display()),
            );
            if !is_git {
                note(
                    &fleet.store,
                    NoticeLevel::Warn,
                    &format!(
                        "{} is not a git repository — workers will share a single \
                         checkout with no worktrees and no per-worker branches.",
                        target.display()
                    ),
                );
            }
        }
    }
}

/// Ask the operator for a repo and record it in `~/.fleetor/config.json`.
///
/// Updates `Fleet.target` and `Fleet.config` in place so the change takes
/// effect immediately — the start gate is the only caller, and no panes exist
/// yet. Refused by [`adopt_target`] once one does. Returns `Ok(None)` when the
/// picker was dismissed.
///
/// The guard is checked *before* the dialog opens: making the operator browse
/// to a folder and only then telling them it cannot be used is a worse way to
/// deliver the same refusal.
#[tauri::command]
pub fn fleet_pick_target(
    app: AppHandle,
    state: State<'_, FleetState>,
    registry: State<'_, Arc<PaneRegistry>>,
    gate: State<'_, Arc<GateHold>>,
) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;

    ensure_target_settable(&registry)?;

    let Some(picked) = app.dialog().file().blocking_pick_folder() else { return Ok(None) };
    let path = picked
        .into_path()
        .map_err(|e| format!("that folder has no usable path: {e}"))?;
    if !path.is_dir() {
        return Err(format!("{} is not a directory", path.display()));
    }

    adopt_target(&registry, &state, &gate, &path)?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

/// Record a target the operator **typed** rather than picked.
///
/// Same contract as `fleet_pick_target`: writes the config and updates
/// `Fleet.target` in place so the change takes effect immediately.
///
/// A typed path is untrusted in a way a picked one is not — the folder picker
/// can only hand back a directory that exists, whereas this accepts whatever
/// was in the box. So it is trimmed, `~` is expanded, and it has to resolve to
/// a real directory before anything is written. Returning the canonical form
/// matters: the operator should see what was actually recorded, not the
/// shorthand they typed, or they cannot tell a typo from a working path.
#[tauri::command]
pub fn fleet_set_target(
    path: String,
    state: State<'_, FleetState>,
    registry: State<'_, Arc<PaneRegistry>>,
    gate: State<'_, Arc<GateHold>>,
) -> Result<String, String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("enter a folder path".into());
    }
    let expanded = expand_home(trimmed)?;
    if !expanded.exists() {
        return Err(format!("{} does not exist", expanded.display()));
    }
    if !expanded.is_dir() {
        return Err(format!("{} is not a directory", expanded.display()));
    }
    // Resolves `..`, symlinks and relative segments, so the config records one
    // canonical spelling of a directory rather than however it was reached.
    let canonical = expanded
        .canonicalize()
        .map_err(|e| format!("resolve {}: {e}", expanded.display()))?;

    adopt_target(&registry, &state, &gate, &canonical)?;
    Ok(canonical.to_string_lossy().into_owned())
}

// --- the start gate's two commands (WP-25 #35) --------------------------------

/// **Everything the start gate renders, in one call** (M15).
///
/// One command rather than three — the readiness, the selection and the worker
/// default arrive together — because the gate is one screen and a summary assembled
/// from three round trips can render a fleet that is half of one answer and half of
/// another.
///
/// **`refresh` is the re-check button, and it is a different question** (M17, user
/// story 10). `false` answers from whatever reading is held, probing only if there
/// is none; `true` runs the probe again whatever is held, because the operator
/// pressing it has just logged in from another terminal and no cached answer can
/// contain that. It is not the default for the reason C8 gives: the probe costs a
/// subprocess per harness and about 1.4 s, and putting that on every render would
/// put it on an interaction the operator repeats.
///
/// **It answers before there is a fleet, because that is when the gate runs**
/// (#36). Everything it reads lives on [`GateHold`]; the running fleet is consulted
/// only for the two things it knows better — the launch configuration it resolved
/// and the feed to put a probe's sentences on. Requiring a bootstrap here made the
/// pickers unreachable in the one state they exist for.
#[tauri::command]
pub fn fleet_gate(
    refresh: bool,
    state: State<'_, FleetState>,
    gate: State<'_, Arc<GateHold>>,
) -> Result<GateState, String> {
    let (worker_model_default, store) = launch_and_feed(&state)?;

    let (readings, notices) =
        if refresh { gate.harnesses.refresh() } else { gate.harnesses.read() };
    // What a probe found goes on the feed at the moment it finds it, which is what
    // makes the re-check button legible after the fact: the row above changed and
    // the Activity feed says what changed it. A cached read produces none — and
    // before there is a feed, the sentences wait on the gate for one.
    match &store {
        Some(store) => {
            for (level, text) in &notices {
                note(store, *level, text);
            }
        }
        None => gate.remember(notices),
    }

    Ok(answer(&gate, &readings, worker_model_default))
}

/// Record what the operator picked, and answer with **the whole gate** read back.
///
/// **The return is the stored value read back rather than the argument echoed**,
/// which is `critic_interview_open`'s property and is here for a sharper reason: the
/// gate's summary renders from what this returns, and a summary rendered from what
/// the interface *asked for* rather than from what took effect is precisely the
/// fleet-that-is-not-what-spawns M15 refuses.
///
/// **It answers with the whole [`GateState`] rather than the seats alone** (#36).
/// The refusal, the cost lines and the model fallbacks are all functions of the
/// selection *and* the live readings, so a pick that changed the seats and left the
/// interface to re-derive the rest would be two implementations of one rule with
/// nothing pinning them together. One value, written in one direction.
///
/// **A refusal is a refusal, not a silent substitution.** A harness name this build
/// cannot place would otherwise become a `place` that fails at spawn, one pane at a
/// time, after the operator has already been told the fleet started.
#[tauri::command]
pub fn fleet_set_seats(
    seats: FleetSeats,
    state: State<'_, FleetState>,
    gate: State<'_, Arc<GateHold>>,
) -> Result<GateState, String> {
    let (worker_model_default, _) = launch_and_feed(&state)?;
    let picked = seats.validated()?;
    gate.store_seats(picked);
    // Never a fresh probe: a keystroke in a model box is not a reason to spend 1.4 s
    // per harness. `read` finds what the first render already paid for.
    let (readings, _) = gate.harnesses.read();
    Ok(answer(&gate, &readings, worker_model_default))
}

/// The launch configuration's worker model, and the feed if there is one yet.
///
/// A running fleet answers from what it resolved at bootstrap — the value its panes
/// were placed against, never a re-read. Before one exists there is no feed and the
/// prompts are resolved on the spot, which is the same read `fleet_bootstrap` will
/// do and cannot disagree with: `PaneContext::resolve` is a pure function of a
/// directory.
fn launch_and_feed(state: &FleetState) -> Result<(String, Option<Arc<dyn Store>>), String> {
    let guard = state.0.lock().map_err(|e| e.to_string())?;
    match guard.as_ref() {
        Some(fleet) => {
            Ok((fleet.context.launch.worker_model.clone(), Some(fleet.store.clone())))
        }
        None => {
            drop(guard);
            let context = PaneContext::resolve(&prompts::override_dir(layout().root()));
            Ok((context.launch.worker_model, None))
        }
    }
}

/// **The one assembly of what the gate renders** (M15, #36).
///
/// Both commands end here, so the screen after a pick and the screen after a
/// re-check are the same function of the same two inputs. The seats are settled
/// before anything is built out of them — a model the vendor no longer lists falls
/// back *and is stored back*, so what the summary describes is what would spawn.
fn answer(
    gate: &GateHold,
    readings: &[harness::HarnessReadiness],
    worker_model_default: String,
) -> GateState {
    let (settled, fallbacks) = settle_models(gate.seats(&worker_model_default), readings);
    let seats = gate.store_seats(settled);
    let verdict = StartVerdict::for_seats(&seats, readings, workers_run(), fallbacks);
    GateState {
        harnesses: readings.iter().map(HarnessOffer::from).collect(),
        seats,
        worker_model_default,
        verdict,
    }
}

/// Whether the worker seats will spawn at all.
///
/// The same question `FleetConfig::worker_backend` answers with a model name or
/// `"none"`, asked here as the `bool` the cost statement and the refusal both need:
/// a seat that is not going to exist must neither promise spend nor refuse a start.
fn workers_run() -> bool {
    load_api_key().is_ok()
}

// --- the Critic's interview (WP-21, D-079) ------------------------------------
//
// Two commands over one `bool`, and the asymmetry between them is the point: the
// setter writes a `Notice` and the reader writes nothing. Opening and closing
// each change *what is possible* in the run — one of them makes a pane that was
// unable to reach the fleet able to interrupt it — and the log records outcomes
// (Tier 1.6). Reading the switch is not an outcome.
//
// Neither command is on the message path and neither can be: they move a cell
// the hub reads *before* it resolves anything. See `Hub::handle`'s doc comment
// for why that is the only legal shape here, and `decisions.md` D-079 for the
// argument in full.

/// Open or close the Critic's interview, and answer with what is now stored.
///
/// The return is deliberately the **stored** state read back rather than the
/// argument echoed: the operator's control renders from this, and a control that
/// reported what it asked for rather than what took effect is the class of lie
/// this codebase spends most of its doc comments avoiding.
///
/// Both edges write a `Notice` naming which it was, so an operator reading the
/// Activity feed later can see the decision beside the turns it spent.
#[tauri::command]
pub fn critic_interview_open(open: bool, state: State<'_, FleetState>) -> Result<bool, String> {
    let guard = state.0.lock().map_err(|e| e.to_string())?;
    let fleet = guard.as_ref().ok_or("no fleet is running")?;
    let stored = fleet.interview.set(open);
    note(
        &fleet.store,
        NoticeLevel::Info,
        if stored {
            "the Critic's interview is open — it can now message the fleet, and doing so \
             spends the fleet's turns"
        } else {
            "the Critic's interview is closed — it can read the run and reach no pane"
        },
    );
    Ok(stored)
}

/// Whether the interview is open. Read-only, and writes nothing to the log.
#[tauri::command]
pub fn critic_interview_is_open(state: State<'_, FleetState>) -> Result<bool, String> {
    let guard = state.0.lock().map_err(|e| e.to_string())?;
    let fleet = guard.as_ref().ok_or("no fleet is running")?;
    Ok(fleet.interview.is_open())
}

// --- past runs (WP-11) --------------------------------------------------------
//
// Four commands, all read-or-relabel. There is deliberately no command that
// resumes, re-runs or replays a past run into a live one: the panes that made it
// are gone and their context died with them, so anything shaped like "continue
// this run" would be inventing a fleet that never existed (D-030's regrowth
// warning). History is readable and nothing else.
//
// None of these touch `FleetState`, so they work before the fleet is started —
// which is the case that matters, since the History view is most useful on the
// start gate, deciding what to do next.

/// Every archived run, newest first.
#[tauri::command]
pub fn runs_list() -> Result<Vec<runs::RunRecord>, String> {
    Ok(runs::list(&runs::runs_dir(layout().root())))
}

/// Replay one archived run's log, for the read-only History views.
#[tauri::command]
pub fn run_events(id: String, after: i64) -> Result<Vec<WireEvent>, String> {
    Ok(runs::events(&runs::runs_dir(layout().root()), &id, after)?
        .into_iter()
        .map(|(seq, event)| WireEvent { seq, event })
        .collect())
}

/// Give a run a name that means something to the operator.
#[tauri::command]
pub fn run_rename(id: String, label: String) -> Result<(), String> {
    runs::rename(&runs::runs_dir(layout().root()), &id, &label)
}

/// Delete a run and its directory. Nothing else in the app refers to a run by
/// id, so this needs no cascade — the index is rebuilt from what is left.
#[tauri::command]
pub fn run_delete(id: String) -> Result<(), String> {
    runs::delete(&runs::runs_dir(layout().root()), &id)
}

/// Save a run's JSON export wherever the operator points.
///
/// The dialog lives here rather than in the webview so no npm plugin has to be
/// added for it — `fleet_pick_target` set the pattern. `Ok(None)` means the
/// operator dismissed the dialog, which is not an error and must not be shown
/// as one.
#[tauri::command]
pub fn run_export(app: AppHandle, id: String) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;

    let suggested = format!("{id}-events.json");
    let Some(chosen) = app
        .dialog()
        .file()
        .set_file_name(&suggested)
        .add_filter("JSON", &["json"])
        .blocking_save_file()
    else {
        return Ok(None);
    };
    let path = chosen.into_path().map_err(|e| format!("that location has no usable path: {e}"))?;
    runs::export(&runs::runs_dir(layout().root()), &id, &path)?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

/// `~` and `~/…` are what an operator types; `std::path` treats them as literal
/// directory names, so a typed home-relative path would silently miss.
fn expand_home(input: &str) -> Result<PathBuf, String> {
    if input != "~" && !input.starts_with("~/") {
        return Ok(PathBuf::from(input));
    }
    let home = std::env::var_os("HOME").ok_or("HOME is not set, so ~ cannot be expanded")?;
    let home = PathBuf::from(home);
    Ok(if input == "~" { home } else { home.join(&input[2..]) })
}

/// Set `target` in the config without disturbing anything else the operator has
/// put there. Merge-not-clobber for the same reason the config seed is.
fn write_target(target: &Path) -> Result<(), String> {
    write_config_key("target", target.to_string_lossy().into_owned().into())
}

/// Set one key in `~/.fleetor/config.json`, leaving every other key alone.
///
/// The one writer of that file, so a second setting (WP-16's `dev_mode`) cannot
/// grow a second spelling of "merge, don't clobber" that drops the first one.
pub(crate) fn write_config_key(key: &str, value: serde_json::Value) -> Result<(), String> {
    write_config_key_at(&layout().config_file(), key, value)
}

/// [`write_config_key`] against a named file, so the read-write round trip is
/// exercisable in a temp directory instead of in the operator's real home.
pub(crate) fn write_config_key_at(
    file: &Path,
    key: &str,
    value: serde_json::Value,
) -> Result<(), String> {
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    let existing = std::fs::read_to_string(file).ok();
    let text = merge_config_key(existing.as_deref(), key, value)?;
    std::fs::write(file, text).map_err(|e| format!("write {}: {e}", file.display()))
}

/// The merge, kept pure so it is tested without writing to the operator's real
/// home directory. Unreadable or non-object config text is replaced rather than
/// treated as fatal — refusing to record a target the operator just picked would
/// leave the picker looking broken.
pub(crate) fn merge_config_key(
    existing: Option<&str>,
    key: &str,
    value: serde_json::Value,
) -> Result<String, String> {
    let mut root = existing
        .and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok())
        .filter(|v| v.is_object())
        .unwrap_or_else(|| serde_json::Value::Object(serde_json::Map::new()));
    root.as_object_mut().expect("just filtered to an object").insert(key.into(), value);
    serde_json::to_string_pretty(&root).map_err(|e| format!("encode config: {e}"))
}

/// Stop the hub and unlink its socket. Best-effort, called on window close
/// alongside the pty teardown.
pub fn shutdown(state: &FleetState) {
    if let Ok(guard) = state.0.lock() {
        if let Some(fleet) = guard.as_ref() {
            fleet.shutdown.notify_one();
        }
    }
}

fn snapshot(store: &Arc<dyn Store>) -> Result<BootSnapshot, String> {
    Ok(BootSnapshot { latest_seq: store.latest_seq().map_err(|e| e.to_string())? })
}

// --- worker credentials -------------------------------------------------------

/// Read `DEEPSEEK_API_KEY` from the env or the nearest `.env` on the path from the
/// cwd up to the filesystem root. Under `tauri dev` the cwd is `src-tauri/`, so a
/// repo-root `.env` is found by walking up (not just `<cwd>/.env`) — otherwise a
/// key sitting at the repo root silently goes unseen. Never logged.
///
/// This is what a worker pane sets `ANTHROPIC_AUTH_TOKEN` from — and only that.
/// Never `ANTHROPIC_API_KEY`: with it set, the interactive TUI blocks on api-key
/// approval and never reaches its prompt (L2).
/// The worker API key, or `None` — a [`Host`] field (D12). The orchestrator runs
/// without one; worker panes cannot, which is why the failing path keeps the
/// sentence in [`load_api_key`] rather than losing it to an `Option`.
pub(crate) fn deepseek_api_key() -> Option<String> {
    load_api_key().ok()
}

/// Where the `.env` walk begins: the application's own working directory.
///
/// Named once, read by both the walk below and by
/// [`Host::discover`](crate::placement::Host::discover), so the sentence a worker
/// fails with names the directory that was actually searched rather than a second
/// guess at it (D-075).
pub(crate) fn api_key_search_start() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn load_api_key() -> Result<String, String> {
    if let Ok(k) = std::env::var("DEEPSEEK_API_KEY") {
        if !k.is_empty() {
            return Ok(k);
        }
    }
    let start = api_key_search_start();
    for dir in start.ancestors() {
        let Ok(text) = std::fs::read_to_string(dir.join(".env")) else { continue };
        if let Some(key) = parse_deepseek_key(&text) {
            return Ok(key);
        }
    }
    // One sentence for this failure, and it lives with the pane kind that cannot
    // start without a key (D-075).
    Err(placement::missing_api_key(Some(&start)))
}

/// Pull the `DEEPSEEK_API_KEY` value out of `.env` text, tolerating surrounding
/// quotes; `None` if absent or empty. Kept separate so the parse is unit-tested
/// without touching the filesystem.
fn parse_deepseek_key(text: &str) -> Option<String> {
    for line in text.lines() {
        if let Some(rest) = line.trim().strip_prefix("DEEPSEEK_API_KEY=") {
            let val = rest.trim().trim_matches('"').trim_matches('\'');
            if !val.is_empty() {
                return Some(val.to_string());
            }
        }
    }
    None
}

// --- display ------------------------------------------------------------------

/// The current git branch of `repo`, or a sensible default when git is silent.
fn git_branch(repo: &Path) -> String {
    std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(repo)
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "main".to_string())
}

fn fleet_config_for(target: &Path) -> FleetConfig {
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| target.to_string_lossy().into_owned());
    FleetConfig {
        target: name,
        target_path: target.to_string_lossy().into_owned(),
        branch: git_branch(target),
        // What a click will actually spend. Without a key the worker panes cannot
        // start at all, and the gate must say so rather than offering four seats
        // that fail on spawn.
        worker_backend: match load_api_key() {
            Ok(_) => "deepseek-v4-flash".to_string(),
            Err(_) => "none".to_string(),
        },
        lead_model: "opus (operator)".to_string(),
        gate: "shell gate + peer review".to_string(),
    }
}

/// Append a notice on the feed (best-effort; a store error is only logged).
fn note(store: &Arc<dyn Store>, level: NoticeLevel, text: &str) {
    if let Err(e) = store.append_event(&FleetEvent::Notice { level, text: text.into() }) {
        eprintln!("fleet: could not append notice: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fleetor_core::pane::PaneId;
    use fleetor_core::wire::{Hello, Op, OpResult};
    use fleetor_ipc::Client;
    use std::time::Duration;

    /// The target-shaped view of [`merge_config_key`], so these tests read as
    /// what they are about. `write_target` calls the generic writer directly —
    /// there is one merge, and WP-16's `dev_mode` goes through the same one.
    fn merge_target(existing: Option<&str>, target: &Path) -> Result<String, String> {
        merge_config_key(existing, "target", target.to_string_lossy().into_owned().into())
    }

    /// The wiring end to end over a **real unix socket**: bootstrap's hub and the
    /// real delivery loop over a real (empty) pane registry, dialled by a real
    /// client exactly the way the `fleet` CLI does.
    ///
    /// The pane here is genuinely not running, so the honest answer is a refusal
    /// that names it — not a park, and not a success the feed would have to walk
    /// back. `src-tauri/tests/panes.rs` runs the same path with live ptys.
    #[test]
    fn a_pane_op_crosses_the_real_socket_and_is_answered() {
        let dir = std::env::temp_dir().join(format!("fleetor-hub-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let sock = dir.join("fleet.sock");

        let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
        let store: Arc<dyn Store> =
            Arc::new(SqliteStore::open(&dir.join("state.db")).unwrap());

        let (app_tx, app_rx) = mpsc::unbounded_channel();
        let registry = Arc::new(PaneRegistry::new(Arc::new(|_, _| {}), dir.join("panes.pids")));
        deliver::spawn_delivery(&rt, registry, app_rx, store.clone(), Arc::new(GaugeSources::default()));
        let (_hub, shutdown) = spawn_hub(&rt, store.clone(), app_tx, sock.clone());

        let result = rt.block_on(async {
            let transport = UnixTransport::new(&sock);
            let mut client = connect(&transport, PaneId::Orch).await.expect("hub never came up");
            client
                .call(Op::Send { to: PaneId::Worker(1), text: "take T-4".into() })
                .await
                .expect("the hub answered")
        });

        let OpResult::Delivered { accepted, detail, .. } = result else {
            panic!("expected a delivery result, got {result:?}");
        };
        assert!(!accepted, "a pane that is not running cannot accept anything");
        assert!(
            detail.as_deref().unwrap_or_default().contains("worker-1"),
            "the refusal must name the pane: {detail:?}"
        );

        // And the attempt is on the record, body included.
        let logged = store
            .events_since(0)
            .unwrap()
            .into_iter()
            .any(|(_, e)| matches!(&e, FleetEvent::Message { body, accepted, .. } if body == "take T-4" && !accepted));
        assert!(logged, "a refused send must still reach the feed with its body");

        shutdown.notify_one();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Both sides of the one rule the target has (D-071): settable until a pane
    /// exists, refused after — and refused with a sentence naming the remedy,
    /// because a refusal an operator cannot act on is a dead end rather than a
    /// guard.
    ///
    /// Driven against a **real** [`PaneRegistry`] with a **real** pty on the far
    /// end, for the same reason `tests/panes.rs` does: the guard's whole job is
    /// to read the registry's actual state, so a stub of that state would only
    /// prove the stub. `sleep` stands in for `claude` — this asks whether a pane
    /// exists, and nothing about what it is.
    #[test]
    fn the_target_is_settable_until_a_pane_exists() {
        let dir = std::env::temp_dir().join(format!("fleetor-target-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let registry = PaneRegistry::new(Arc::new(|_, _| {}), dir.join("panes.pids"));

        // Before the first spawn — the start gate. Unchanged behaviour.
        assert!(
            ensure_target_settable(&registry).is_ok(),
            "an empty registry is the start gate, where setting the target is the whole point"
        );

        let mut cmd = portable_pty::CommandBuilder::new("sleep");
        cmd.arg("30");
        registry.spawn(PaneId::Orch, cmd, crate::placement::harness::claude_code().spec(), 24, 80)
            .expect("a pty for sleep");

        // After it. One pane is enough — the fleet is now committed to a repo.
        let refusal = ensure_target_settable(&registry)
            .expect_err("a running fleet must not have its target moved under it");
        assert!(
            refusal.contains("fixed for a running fleet"),
            "the refusal has to say the target is fixed, not merely fail: {refusal}"
        );
        assert!(
            refusal.contains("Stop the fleet"),
            "and it has to name the remedy, or the operator is stuck: {refusal}"
        );

        // Killing the last pane puts the operator back at the start gate: nothing
        // is holding the old target any more, so nothing has a claim on it.
        registry.kill_all();
        assert!(
            ensure_target_settable(&registry).is_ok(),
            "with every pane reaped the refusal has nothing left to protect"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Every pane's config dir hangs off one root under `_shell`, `orch`
    /// included since WP-14 — which is the whole reason rotation archives its
    /// transcript without an arm of its own. A worker's path must not have moved
    /// while that was arranged, or four panes lose their trust flags at once.
    #[test]
    fn every_panes_config_dir_is_a_sibling_under_one_root() {
        let orch = layout().pane_config(PaneId::Orch);
        let worker = layout().pane_config(PaneId::Worker(2));
        assert!(orch.ends_with("pane-config/orch"), "{}", orch.display());
        assert!(worker.ends_with("pane-config/worker-2"), "{}", worker.display());
        assert_eq!(orch.parent(), worker.parent(), "harvest_transcripts walks the parent");
        assert_eq!(worker.parent().unwrap(), layout().shell().join("pane-config"));
    }

    /// A picked target must land in the config without costing the operator
    /// whatever else they had put there by hand.
    #[test]
    fn recording_a_target_replaces_only_the_target() {
        let merged = merge_target(Some(r#"{"theme":"warm","target":"/old"}"#), Path::new("/new"));
        let config: serde_json::Value = serde_json::from_str(&merged.unwrap()).unwrap();
        assert_eq!(config["target"], serde_json::json!("/new"));
        assert_eq!(config["theme"], serde_json::json!("warm"), "an unrelated setting survived");
    }

    /// The picker must still work on a first run, and on a config someone has
    /// broken — refusing to record the folder they just chose would read as a
    /// broken picker rather than as a broken file.
    #[test]
    fn a_missing_or_unreadable_config_still_records_the_target() {
        for existing in [None, Some("not json at all"), Some("[]")] {
            let merged = merge_target(existing, Path::new("/picked")).unwrap();
            let config: serde_json::Value = serde_json::from_str(&merged).unwrap();
            assert_eq!(config["target"], serde_json::json!("/picked"), "{existing:?}");
        }
    }

    /// The round trip that matters: what the picker writes is what the next boot
    /// reads. Two functions on opposite ends of a restart, pinned together.
    #[test]
    fn what_the_picker_writes_is_what_bootstrap_reads_back() {
        let merged = merge_target(None, Path::new("/Users/me/code/thing")).unwrap();
        assert_eq!(parse_target(&merged).unwrap(), Some(PathBuf::from("/Users/me/code/thing")));
    }

    /// The hub binds a beat after it is spawned, so a client that dials once loses
    /// a race the real CLI doesn't (it is started by hand, long after boot).
    async fn connect(transport: &UnixTransport, pane: PaneId) -> Option<Client> {
        for _ in 0..100 {
            if let Ok(c) = Client::connect(transport, Hello::for_pane(pane)).await {
                return Some(c);
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        None
    }

    #[test]
    fn reads_the_configured_target_from_config_text() {
        assert_eq!(
            parse_target(r#"{"target": "/Users/me/code/thing"}"#).unwrap(),
            Some(PathBuf::from("/Users/me/code/thing")),
        );
    }

    /// A config that simply doesn't name a target is the ordinary case, not an
    /// error — it falls back to the testbed with a notice.
    #[test]
    fn a_config_without_a_target_is_not_an_error() {
        assert_eq!(parse_target("{}").unwrap(), None);
        assert_eq!(parse_target(r#"{"target": ""}"#).unwrap(), None);
        assert_eq!(parse_target(r#"{"target": "   "}"#).unwrap(), None);
    }

    /// A broken config must be loud. Reading it as "nothing configured" would put
    /// the fleet in the testbed while the operator believes it is in their repo.
    #[test]
    fn a_malformed_config_is_reported_rather_than_ignored() {
        assert!(parse_target("not json at all").is_err());
        assert!(parse_target(r#"{"target": 7}"#).is_err(), "a non-string target is a mistake, not an absence");
    }

    // --- the operator's composer (WP-07) ---------------------------------------

    /// The composer's three targets are the fleet's three message verbs. A pane
    /// name is a `send`, `all` is the ordinary broadcast, `reply` is the
    /// ordinary reply — nothing here is a fourth kind of message.
    #[test]
    fn the_composers_target_picks_the_verb_and_never_invents_one() {
        assert_eq!(
            operator_op("worker-2", "ship it").unwrap(),
            Op::Send { to: PaneId::Worker(2), text: "ship it".into() },
        );
        assert_eq!(
            operator_op("2", "ship it").unwrap(),
            Op::Send { to: PaneId::Worker(2), text: "ship it".into() },
            "every spelling the CLI takes, the composer takes",
        );
        assert_eq!(operator_op("orch", "hi").unwrap(), Op::Send { to: PaneId::Orch, text: "hi".into() });
        assert_eq!(operator_op("all", "stop").unwrap(), Op::Broadcast { text: "stop".into() });
        assert_eq!(operator_op("reply", "yes").unwrap(), Op::Reply { text: "yes".into() });
    }

    /// An empty body would be typed into a live terminal as a bare newline — a
    /// submitted empty turn in somebody's `claude`. Refused at the boundary,
    /// exactly as clap refuses it for a pane.
    #[test]
    fn the_composer_refuses_an_empty_message_and_an_unknown_target() {
        assert!(operator_op("worker-2", "   ").is_err());
        assert!(operator_op("all", "").is_err());
        let why = operator_op("sidebar", "hi").expect_err("not a participant");
        assert!(why.contains("sidebar"), "the refusal names what it was handed: {why}");
    }

    /// The vocabulary, at the seam where the UI reads it. Three words, and the
    /// one that does not exist is `delivered`.
    #[test]
    fn the_composer_reports_the_fleets_three_words_and_no_others() {
        let accepted =
            operator_result(OpResult::Delivered { msg_id: "msg-1".into(), accepted: true, detail: None })
                .unwrap();
        assert_eq!(accepted.outcome, "accepted");
        assert_eq!(accepted.detail, None);

        let refused = operator_result(OpResult::Delivered {
            msg_id: "grp-1".into(),
            accepted: false,
            detail: Some("worker-3: pane worker-3 is dead".into()),
        })
        .unwrap();
        assert_eq!(refused.outcome, "undelivered", "the send happened; the arrival did not");
        assert!(refused.detail.unwrap().contains("worker-3"));

        assert_eq!(operator_result(OpResult::Recorded { record_id: "msg-2".into() }).unwrap().outcome, "recorded");

        let error = operator_result(OpResult::Error { message: "nobody has messaged operator yet".into() })
            .expect_err("an error is an error");
        assert!(error.contains("nobody has messaged operator"), "{error}");
    }

    // --- the three properties that fail silently (WP-25 #36) ------------------
    //
    // Each is exercised against a machine a test *describes* rather than one it
    // arranges to be in: `HarnessReadiness`'s fields are all public and all owned,
    // which is what makes "codex installed but logged out" a value here instead of
    // a fixture. Zero tokens and no subprocess — nothing below probes anything.

    use harness::{AccountShape, HarnessReadiness, LoginState, ModelChoice};

    fn spec_of(name: &str) -> &'static harness::HarnessSpec {
        harness::by_name(name).expect("a registered harness").spec()
    }

    /// One harness's reading, with everything the three properties do not look at
    /// left in its "nothing to report" state.
    fn reading(name: &str, login: LoginState) -> HarnessReadiness {
        HarnessReadiness {
            harness: spec_of(name),
            invoked: name.to_string(),
            resolved: None,
            readings_agree: true,
            version: None,
            login,
            provider: None,
            models: Vec::new(),
            posture: harness::ResolvedPosture::default(),
        }
    }

    fn logged_in(name: &str, account: AccountShape) -> HarnessReadiness {
        reading(name, LoginState::LoggedIn(account))
    }

    fn seats_on(orch: &str, worker: &str) -> FleetSeats {
        FleetSeats {
            orch: SeatChoice { harness: orch.to_string(), model: None },
            workers: fleetor_core::pane::WORKER_SLOTS
                .iter()
                .map(|_| SeatChoice { harness: worker.to_string(), model: Some("m".into()) })
                .collect(),
        }
    }

    /// **Story 11.** A seat on a harness this machine cannot log into stops the
    /// fleet at the gate, rather than spawning a pane onto a login prompt.
    #[test]
    fn a_seat_that_cannot_log_in_refuses_the_start() {
        let seats = seats_on("claude-code", "claude-code");
        let out = vec![reading(
            "claude-code",
            LoginState::NoCredential { summary: "records no login in ~/.claude.json".into() },
        )];

        let refused = StartVerdict::for_seats(&seats, &out, true, Vec::new());
        assert_eq!(
            refused.refusals.len(),
            1 + fleetor_core::pane::WORKER_SLOTS.len(),
            "every seat on the logged-out harness is named, not just the first",
        );
        let why = refused.why_it_will_not_start().expect("a logged-out fleet may not start");
        assert!(why.contains("orchestrator") && why.contains("worker 4"), "{why}");
        assert!(why.contains("records no login"), "the vendor's own sentence survives: {why}");
        assert!(why.contains("Re-check logins"), "and it says what to do next: {why}");

        let in_ = vec![logged_in("claude-code", AccountShape::ApiKey)];
        assert!(
            StartVerdict::for_seats(&seats, &in_, true, Vec::new())
                .why_it_will_not_start()
                .is_none(),
            "a logged-in fleet starts",
        );

        // **`Unreadable` is not a refusal** (C14). A vendor that changed its report
        // format is a working installation, and stopping a fleet on a parse error is
        // the failure the narrow refusal exists to avoid.
        let unreadable =
            vec![reading("claude-code", LoginState::Unreadable { why: "unknown format".into() })];
        assert!(
            StartVerdict::for_seats(&seats, &unreadable, true, Vec::new())
                .why_it_will_not_start()
                .is_none(),
            "a report this build could not parse must not stop a fleet",
        );
    }

    /// A worker seat that is not going to spawn refuses nothing: on a machine with
    /// no worker key the orchestrator runs alone, and a fleet stopped by a seat that
    /// was never part of it is a refusal the operator cannot act on.
    #[test]
    fn a_worker_seat_that_will_not_spawn_does_not_refuse_the_start() {
        let seats = seats_on("claude-code", "codex");
        let readings = vec![
            logged_in("claude-code", AccountShape::ApiKey),
            reading("codex", LoginState::NoCredential { summary: "no Codex credentials".into() }),
        ];

        assert!(
            StartVerdict::for_seats(&seats, &readings, true, Vec::new())
                .why_it_will_not_start()
                .is_some(),
            "with workers running, a logged-out worker harness stops the fleet",
        );
        let alone = StartVerdict::for_seats(&seats, &readings, false, Vec::new());
        assert!(alone.why_it_will_not_start().is_none(), "without workers, it does not");
        assert!(
            alone.cost.iter().all(|line| line.seats == "orchestrator"),
            "and nothing promises spend in a seat that will not exist: {:?}",
            alone.cost,
        );
    }

    /// **Story 12, and C9 is the whole of it.** The orchestrator spends the
    /// operator's own login; a worker spends FLEETOR's metered key. One sentence
    /// each, and neither may claim the other's credential.
    #[test]
    fn the_cost_line_says_whose_credential_each_seat_spends() {
        let seats = seats_on("codex", "claude-code");
        let readings = vec![
            HarnessReadiness {
                provider: Some("openai".into()),
                ..logged_in("codex", AccountShape::SubscriptionPlan { plan: None })
            },
            logged_in("claude-code", AccountShape::ApiKey),
        ];
        let verdict = StartVerdict::for_seats(&seats, &readings, true, Vec::new());

        let orch = verdict.cost.iter().find(|line| line.seats == "orchestrator").expect("a line");
        assert_eq!(orch.harness, "codex");
        assert!(orch.sentence.contains("your own login"), "{}", orch.sentence);
        assert!(orch.sentence.contains("subscription plan"), "the shape is named: {}", orch.sentence);
        assert!(orch.sentence.contains("on openai"), "and the provider it resolved: {}", orch.sentence);
        assert!(
            orch.sentence.contains("your own subscription quota"),
            "\"it will spend tokens\" is not the warning when it spends a plan: {}",
            orch.sentence,
        );
        assert!(
            !orch.sentence.contains("FLEETOR's own provider and key"),
            "the orchestrator does not run the fleet's key: {}",
            orch.sentence,
        );

        let workers =
            verdict.cost.iter().find(|line| line.seats.contains("worker")).expect("a line");
        assert_eq!(workers.harness, "claude-code");
        assert!(workers.sentence.contains("FLEETOR's own provider and key"), "{}", workers.sentence);
        assert!(
            workers.sentence.contains("never your own login or plan"),
            "a worker cannot reach the operator's plan, and says so: {}",
            workers.sentence,
        );
        assert!(
            !workers.sentence.contains("your own subscription quota"),
            "and never claims it does: {}",
            workers.sentence,
        );

        // **No plan tier, anywhere** (C14 as narrowed by C58). The shape comes from
        // `AccountShape::display`, so a tier could only appear if a vendor reported
        // one — and this cost statement invents none.
        for line in &verdict.cost {
            for tier in ["Pro", "Max", "Plus", "Team", "Enterprise"] {
                assert!(!line.sentence.contains(tier), "no tier is promised: {}", line.sentence);
            }
        }
    }

    /// Each credential shape bills differently, so each gets its own words — a
    /// single "it will spend your account" would be the flattening story 12 refuses.
    #[test]
    fn each_account_shape_names_what_it_bills() {
        let plan = orchestrator_cost(
            "codex",
            Some(&logged_in("codex", AccountShape::SubscriptionPlan { plan: None })),
        );
        assert!(plan.contains("your own subscription quota"), "{plan}");

        let key = orchestrator_cost("codex", Some(&logged_in("codex", AccountShape::ApiKey)));
        assert!(key.contains("the key that login stores"), "{key}");

        let mine = orchestrator_cost(
            "codex",
            Some(&logged_in(
                "codex",
                AccountShape::CustomProvider { name: "mine".into(), env_var: None },
            )),
        );
        assert!(mine.contains("whatever mine bills"), "{mine}");

        // A harness this build does not have claims to read a shape it never read.
        let unknown = orchestrator_cost("nobody", None);
        assert!(unknown.contains("credential unread"), "{unknown}");
        assert!(unknown.contains("runs your own login"), "the seat is still the operator's: {unknown}");
    }

    /// The four workers on one harness are one sentence; a mixed fleet is as many as
    /// it really has, and neither speaks for a seat it is not about.
    #[test]
    fn the_worker_cost_lines_group_by_harness_and_never_overreach() {
        let readings = vec![
            logged_in("claude-code", AccountShape::ApiKey),
            logged_in("codex", AccountShape::ApiKey),
        ];
        let same = StartVerdict::for_seats(
            &seats_on("claude-code", "claude-code"),
            &readings,
            true,
            Vec::new(),
        );
        assert_eq!(same.cost.len(), 2, "one orchestrator line and one worker line: {:?}", same.cost);
        assert!(same.cost[1].seats.contains("all 4 worker seats"), "{:?}", same.cost[1]);

        let mut mixed = seats_on("claude-code", "claude-code");
        mixed.workers[1].harness = "codex".to_string();
        let split = StartVerdict::for_seats(&mixed, &readings, true, Vec::new());
        assert_eq!(split.cost.len(), 3, "a mixed fleet says so: {:?}", split.cost);
        assert_eq!(split.cost[1].seats, "worker seats 1, 3 and 4");
        assert_eq!(split.cost[2].seats, "worker seat 2");
    }

    /// **Story 14.** A model the vendor's own catalog no longer lists never reaches
    /// a `--model` flag: the seat falls back to its own default and the substitution
    /// is reported rather than done quietly.
    #[test]
    fn a_model_the_catalog_no_longer_lists_falls_back_instead_of_spawning() {
        let listed = |slug: &str| ModelChoice {
            slug: slug.to_string(),
            display_name: slug.to_uppercase(),
        };
        let catalog = vec![HarnessReadiness {
            models: vec![listed("gpt-6-astra"), listed("gpt-6-sol")],
            ..logged_in("codex", AccountShape::ApiKey)
        }];

        let mut retired = seats_on("codex", "claude-code");
        retired.orch.model = Some("gpt-5.1-gone".to_string());
        let (settled, fallbacks) = settle_models(retired, &catalog);
        assert_eq!(settled.orch.model, None, "the retired id must not reach a --model flag");
        assert_eq!(fallbacks.len(), 1);
        assert_eq!(fallbacks[0].asked, "gpt-5.1-gone");
        assert_eq!(fallbacks[0].fell_back_to, DEFAULT_YOUR_LOGIN);
        assert_eq!(fallbacks[0].seat, "orchestrator");
        // And the seat that places carries the fallback, not the retired name — this
        // is the "does not spawn" half, read where `spawn_pane` reads it.
        assert!(
            matches!(settled.spec_for(PaneId::Orch), Some(PaneSpec::Orch { model: None, .. })),
            "the spec placed for this seat names no model at all",
        );

        let mut current = seats_on("codex", "claude-code");
        current.orch.model = Some("gpt-6-sol".to_string());
        let (kept, none) = settle_models(current, &catalog);
        assert_eq!(kept.orch.model.as_deref(), Some("gpt-6-sol"), "a listed model stands");
        assert!(none.is_empty());
    }

    /// A harness that publishes no catalog has no opinion, and a worker's model is
    /// not the catalog's business at all (C9) — the two ways this check could have
    /// rewritten a fleet nobody asked it to.
    #[test]
    fn the_fallback_keeps_out_of_what_the_catalog_does_not_describe() {
        let mut typed = seats_on("claude-code", "claude-code");
        typed.orch.model = Some("something-only-i-know".to_string());
        let (kept, none) = settle_models(typed, &[logged_in("claude-code", AccountShape::ApiKey)]);
        assert_eq!(
            kept.orch.model.as_deref(),
            Some("something-only-i-know"),
            "an empty catalog is an answer, not evidence that every model is retired",
        );
        assert!(none.is_empty());

        // A worker runs FLEETOR's provider and key, whose model names the vendor's
        // catalog knows nothing about. Checking one against it would rewrite the
        // launch configuration's own worker model the day somebody picked this
        // harness — the gate choosing a fleet nobody picked.
        let catalog = vec![HarnessReadiness {
            models: vec![ModelChoice { slug: "gpt-6-sol".into(), display_name: "Sol".into() }],
            ..logged_in("codex", AccountShape::ApiKey)
        }];
        let (workers, quiet) = settle_models(seats_on("codex", "codex"), &catalog);
        assert!(
            workers.workers.iter().all(|seat| seat.model.as_deref() == Some("m")),
            "the fleet's own worker model is untouched: {:?}",
            workers.workers,
        );
        assert!(quiet.is_empty());
    }

    /// The `.env` parse tolerates quotes/comments and ignores an empty value, so a
    /// repo-root key is picked up (via the walk-up in `load_api_key`) not skipped.
    #[test]
    fn parses_deepseek_key_from_env_text() {
        assert_eq!(parse_deepseek_key("DEEPSEEK_API_KEY=sk-abc\n").as_deref(), Some("sk-abc"));
        assert_eq!(
            parse_deepseek_key("# comment\nOTHER=1\nDEEPSEEK_API_KEY=\"sk-xyz\"\n").as_deref(),
            Some("sk-xyz"),
        );
        assert_eq!(parse_deepseek_key("OTHER=1\n"), None);
        assert_eq!(parse_deepseek_key("DEEPSEEK_API_KEY=\n"), None);
    }
}
