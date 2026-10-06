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

use fleetor_core::brief::Startup;
use fleetor_core::event::{FleetEvent, NoticeLevel};
use fleetor_core::pane::{PaneEntry, PaneId, PaneState};
use fleetor_core::wire::{Op, OpResult, TaskAction};
use fleetor_core::Store;
use fleetor_db::SqliteStore;
use fleetor_ipc::UnixTransport;
use fleetor_server::{AppCommand, BroadcastStore, Hub, TaskContext};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};
use tokio::runtime::Runtime;
use tokio::sync::{mpsc, oneshot, Notify};

use crate::context_gauge::GaugeSources;
use crate::credential_source::{CredentialChoice, Gap, PlanOffer};
use crate::placement::{self, harness, Host, Layout, PaneSpec};
use crate::prompts::PaneContext;
use crate::pty::PaneRegistry;
use crate::{deliver, guardrail, prompts, runs, testbed};

/// Emitted for every appended event, in `seq` order, the moment it persists.
const EVENT_FLEET: &str = "fleet://event";
/// A launch's verdict passed: the UI drops the old run's state on this, so a
/// refused launch changes nothing on screen.
const EVENT_LAUNCHING: &str = "fleet://launching";
/// Chain entries from the fleet target's task store (D-100).
const EVENT_TASK: &str = "fleet://task";

/// One event as the webview sees it: the `seq` cursor plus the flattened
/// [`FleetEvent`] (its `#[serde(tag = "type")]` discriminator carries through, so
/// the UI matches on `type`).
#[derive(Serialize, Clone)]
pub struct WireEvent {
    seq: i64,
    #[serde(flatten)]
    event: FleetEvent,
}

/// What the UI gets back from a launch. The feed itself arrives
/// entirely over [`EVENT_FLEET`] — the follower replays history from 0 — so all
/// this carries is the cursor. It used to carry the board too; there is no board.
#[derive(Debug, Serialize)]
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
    /// What fills the worker seats by default: the model on the fleet's key,
    /// or `"plan"` when there is none and seats spend the operator's own
    /// login instead — never `"none"`. A worker seat has not required a key
    /// since a seat can authenticate on the operator's plan (C78); this field
    /// is display-only and does not gate whether worker seats exist.
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
    /// orchestrator's `default (your login)` sentinel (M2), a worker's
    /// launch-configured model on the fleet's key, or — on a plan seat — that
    /// same sentinel again, because the vendor's own login is what chooses there.
    pub model: Option<String>,
    /// **Whose usage this seat spends** (C78) — `plan` or `fleet_key`.
    ///
    /// **On the seat, because the answer is the seat's.** C75 put it in
    /// `config.json` as one fleet-wide value and C78 reversed that: the operator
    /// asked for a per-seat toggle, and a fleet-wide key beside a per-seat control
    /// would be two homes for one answer — the shape M15 exists to refuse.
    ///
    /// **Absent on the wire reads as the plan**, which is what makes a first run
    /// and a stored selection from before this field existed both land on the
    /// default C75 chose. `Seed::credential_source` still defaults the *other*
    /// way, and that asymmetry is deliberate: this is what the operator was
    /// offered, and that is what a code path nobody thought about gets.
    #[serde(default)]
    pub credential: CredentialChoice,
}

/// **What the operator picked, for every seat that may carry a choice** (C23).
///
/// Five seats: the orchestrator and the four workers.
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
    /// **No argument since C78.** It took the fleet's worker model and stamped it
    /// on all four seats; a fresh fleet's workers are on the operator's plan now,
    /// and a plan seat names no model at all. The launch-configured model is what
    /// a seat falls back to when it is *moved* to the key, which `seatDefault`
    /// answers per credential rather than once here.
    fn unpicked() -> Self {
        let claude = harness::claude_code().spec().name.to_string();
        let workers = fleetor_core::pane::WORKER_SLOTS
            .iter()
            .map(|_| SeatChoice {
                harness: claude.clone(),
                // **`None`, not the launch model** (C78). A fresh fleet's workers
                // are on the plan, and a plan seat names no model: the login
                // chooses. The launch-configured model is what a seat falls back
                // to when it is moved to the fleet's key, which `seat_default`
                // below answers per credential rather than once here.
                model: None,
                credential: CredentialChoice::OperatorsPlan,
            })
            .collect();
        Self {
            orch: SeatChoice {
                harness: claude,
                model: None,
                // The orchestrator has only ever run the operator's own login and
                // has no second answer to hold. Recorded rather than left to a
                // default so nothing reads its absence as the fleet's key.
                credential: CredentialChoice::OperatorsPlan,
            },
            workers,
        }
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

    /// This selection with every seat a past run recorded put back on what it ran
    /// (D-095). A seat the run never placed, or one on a harness this build lacks,
    /// keeps the gate's pick; a record older than D-095 keeps the gate's credential.
    fn as_recorded(mut self, recorded: &std::collections::BTreeMap<String, runs::PaneRecord>) -> Self {
        let slots = fleetor_core::pane::WORKER_SLOTS.iter().map(|&slot| PaneId::Worker(slot));
        let seats = std::iter::once(&mut self.orch).chain(&mut self.workers);
        for (pane, seat) in std::iter::once(PaneId::Orch).chain(slots).zip(seats) {
            let Some(rec) = recorded.get(&pane.to_string()) else { continue };
            if harness::by_name(&rec.harness).is_none() {
                continue;
            }
            seat.harness = rec.harness.clone();
            seat.model = rec.model.clone();
            if let Some(credential) = rec.credential {
                seat.credential = credential;
            }
        }
        self
    }

    /// The choice one seat carries, if it carries one.
    fn choice_for(&self, pane: PaneId) -> Option<&SeatChoice> {
        match pane {
            PaneId::Orch => Some(&self.orch),
            PaneId::Worker(slot) => self.workers.get(usize::from(slot).checked_sub(1)?),
            _ => None,
        }
    }

    /// The spec one seat places as, resolved against the registry.
    ///
    /// `None` for the operator, who carries no choice, so the caller keeps the arm
    /// it already had rather than growing a branch here.
    fn spec_for(&self, pane: PaneId) -> Option<PaneSpec> {
        let seat = match pane {
            PaneId::Orch => &self.orch,
            PaneId::Worker(slot) => self.workers.get(usize::from(slot).checked_sub(1)?)?,
            PaneId::Operator => return None,
        };
        let harness = harness::by_name(&seat.harness)?;
        let placed = match pane {
            PaneId::Orch => PaneSpec::orch(harness),
            // **Where the operator's gate pick becomes a seat's credential**
            // (C75). The flag is read here rather than stored on `SeatChoice`
            // because it is one answer for the fleet, not four: storing it per
            // seat would let a saved selection disagree with the picker the
            // operator is looking at.
            PaneId::Worker(slot) => {
                let placed = PaneSpec::worker(slot, harness);
                // **Where the operator's row-level pick becomes a seat's
                // credential** (C78). Read off `seat`, which is what the picker
                // wrote and what the gate summarised — one value, so the sentence
                // the operator read and the fleet that spawns cannot differ (M15).
                if seat.credential.is_the_operators_plan() {
                    placed.on_the_operators_plan()
                } else {
                    placed
                }
            }
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

/// **A seat whose chosen credential this machine could not honour, and the one it
/// runs on instead** (C79).
///
/// A sibling of [`ModelFallback`] and for the same reason: the operator asked for
/// something specific and got something else, which they have to be told. The
/// seat's own dropdown already shows the new answer — this is what stops that
/// from being a value that changed while nobody was looking.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct CredentialFallback {
    seat: String,
    harness: String,
    /// What the seat was set to and this machine cannot provide.
    asked: CredentialChoice,
    /// What it runs on instead — the only other answer there is.
    fell_back_to: CredentialChoice,
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
    /// warning: the interface disables the button and [`fleet_launch`] refuses.
    refusals: Vec<StartRefusal>,
    /// One sentence per harness per role, in seat order.
    cost: Vec<CostLine>,
    /// What was quietly replaced, said out loud.
    fallbacks: Vec<ModelFallback>,
    /// **Seats whose credential this machine could not honour, moved to the one
    /// it can** (C79) — the same "said out loud" as `fallbacks`, one axis over.
    credential_fallbacks: Vec<CredentialFallback>,
    /// **How many seats are about to spend the operator's plan, before Start**
    /// (#49, C75) — `None` when none are.
    ///
    /// **A count and not a flag**, because the number is the thing an operator
    /// has to weigh: FLEETOR applies no per-pane budget anywhere, so four seats on
    /// one subscription is four uncapped consumers and one seat is one. It is
    /// separate from the [`cost`](Self::cost) lines rather than folded into them
    /// because those are per-harness and this is per-fleet — a mixed fleet has two
    /// cost sentences and still exactly one answer to "how many".
    plan_seats: Option<String>,
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

/// **The fleet's half of C9's split, as one sentence** (story 12, reshaped by C75).
///
/// **The split this sentence carries is no longer orchestrator-versus-worker.** It
/// was, and #49 said so: the pairing with [`orchestrator_cost`] used to be the
/// whole content, because a worker could only ever be on the fleet's key. C73 made
/// a worker able to run the operator's plan, so the axis moved to **plan versus
/// fleet key**, and this function takes the answer rather than assuming it.
///
/// **The orchestrator's line is unchanged and still means what it did** — that
/// seat has always run the operator's own login, and nothing here touched it.
/// What changed is that a worker's line can now say the same thing, which is
/// exactly why it has to be computed and not written down.
///
/// **No plan tier is promised in either branch** (C14 as narrowed by C58): "your
/// own login" is what FLEETOR knows, and the tier is a thing only a vendor's
/// diagnostic may say.
fn worker_cost(harness: &str, seats: &str, spends_the_plan: bool) -> String {
    if spends_the_plan {
        return format!(
            "{harness} in {seats} runs your own {harness} login, copied into each pane at \
             spawn. Turns there spend your own subscription quota — not FLEETOR's metered \
             key. FLEETOR applies no per-pane budget, and a seat that exhausts your quota \
             stops responding rather than reporting an error."
        );
    }
    format!(
        "{harness} in {seats} runs FLEETOR's own provider and key. Turns there spend the \
         fleet's metered credential — never your own login or plan."
    )
}

/// **What a seat is refused with when *neither* credential is on this machine**
/// (C78, narrowed by C79).
///
/// **The only refusal left on this axis.** A seat that could run on the other
/// credential has already been moved there by [`settle_credentials`]; this
/// sentence is for the seat with nowhere to go, so it names both gaps rather than
/// offering the other answer as a way out — C78's version ended "you can also put
/// this seat on your key", which on a machine with no key was advice that led
/// nowhere.
///
/// **Kept out of the call site and beside the cost sentences**, for
/// `missing_api_key`'s reason: this is the sentence standing between the operator
/// and a pane that would look perfectly healthy while logged out, so it is
/// somewhere a test can read it.
///
/// It carries the harness's own [`LoginInstruction`] (C72) and the directory the
/// `.env` walk started from (D-075) — the two things the operator can act on, and
/// both of them, because either one alone unblocks the seat.
///
/// [`LoginInstruction`]: harness::LoginInstruction
fn credential_gap(harness: &str, gap: Gap) -> String {
    let how = match harness::by_name(harness).map(|h| &h.spec().login) {
        Some(login) => {
            let then = login.then.map(|t| format!(" and {t}")).unwrap_or_default();
            format!(" Run `{}`{then} and press Re-check logins,", login.command)
        }
        None => String::new(),
    };
    // The gap names which credential the seat was *asking* for, which is what
    // decides the order the two fixes are offered in — the operator's own choice
    // first, then the other.
    let first = match gap {
        Gap::NoLogin => "no {harness} login was readable on this machine".replace("{harness}", harness),
        Gap::NoKey => "no worker key was found in your `.env`".to_string(),
    };
    format!(
        "this seat has neither credential available — {first}, and the other is missing \
         too.{how} or add a `DEEPSEEK_API_KEY` to a `.env` under `{}`. FLEETOR moves a seat \
         to whichever credential this machine has; this one has neither.",
        api_key_search_start().display(),
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
    /// **Every worker seat is described and, if it cannot start, refused** — there
    /// is no longer a fleet-wide "do workers run at all" (C78).
    ///
    /// The `workers_run: bool` that used to gate this whole block meant *no key,
    /// no workers*, and a machine without one ran the orchestrator alone with no
    /// refusal and no cost line. Since a seat can now authenticate on the
    /// operator's plan, a key is no longer what makes a worker possible; and since
    /// each seat carries its own credential, whether it can start is the seat's
    /// question. `offer` answers it per seat, in both directions.
    fn for_seats(
        seats: &FleetSeats,
        readings: &[harness::HarnessReadiness],
        fallbacks: Vec<ModelFallback>,
        credential_fallbacks: Vec<CredentialFallback>,
        offer: &PlanOffer,
    ) -> Self {
        let orch = &seats.orch;
        let orch_reading = reading_for(readings, &orch.harness);
        let mut refusals = Vec::new();
        let mut plan_slots: Vec<u8> = Vec::new();
        let mut cost =
            vec![CostLine {
                seats: seat_label(PaneId::Orch),
                harness: orch.harness.clone(),
                sentence: orchestrator_cost(&orch.harness, orch_reading),
            }];
        for reason in refusal_for(orch_reading).into_iter().chain(posture_refusal(orch_reading)) {
            refusals.push(StartRefusal {
                seat: seat_label(PaneId::Orch),
                harness: orch.harness.clone(),
                reason,
            });
        }

        // **Grouped by harness *and* credential** (C78), in first-seat order. It
        // was harness alone, which was right while a worker could only ever run
        // the fleet's key; now claude-code on the plan and claude-code on the key
        // are two different spends and must be two sentences. A fleet where all
        // four agree still produces one.
        let mut grouped: Vec<(String, CredentialChoice, Vec<u8>)> = Vec::new();
        for (at, seat) in seats.workers.iter().enumerate() {
            let slot = u8::try_from(at + 1).unwrap_or(u8::MAX);
            match grouped
                .iter_mut()
                .find(|(name, cred, _)| name == &seat.harness && *cred == seat.credential)
            {
                Some((_, _, slots)) => slots.push(slot),
                None => grouped.push((seat.harness.clone(), seat.credential, vec![slot])),
            }
            let reading = reading_for(readings, &seat.harness);
            for reason in refusal_for(reading).into_iter().chain(posture_refusal(reading)) {
                refusals.push(StartRefusal {
                    seat: seat_label(PaneId::Worker(slot)),
                    harness: seat.harness.clone(),
                    reason,
                });
            }
            // **Neither credential is available for this seat** (C79). A seat
            // that could run on the *other* one has already been moved there by
            // `settle_credentials` and is not refused — visibly moved, named in a
            // notice, and showing its new answer in its own dropdown, which is
            // what "not silent" means. This is the case with nowhere left to go,
            // and it is the only one that stops a start.
            if let Some(gap) = offer.gap_for(&seat.harness, seat.credential) {
                refusals.push(StartRefusal {
                    seat: seat_label(PaneId::Worker(slot)),
                    harness: seat.harness.clone(),
                    reason: credential_gap(&seat.harness, gap),
                });
            }
        }
        for (name, cred, slots) in grouped {
            let phrase = worker_seat_phrase(&slots);
            let spends_the_plan = cred.is_the_operators_plan();
            if spends_the_plan {
                plan_slots.extend(slots.iter().copied());
            }
            cost.push(CostLine {
                seats: phrase.clone(),
                sentence: worker_cost(&name, &phrase, spends_the_plan),
                harness: name,
            });
        }

        // **Said once per fleet, and only when it is true.** A sentence that
        // appeared reading "0 seats" every launch is a sentence an operator stops
        // reading, which is the failure this one exists to avoid.
        plan_slots.sort_unstable();
        let plan_seats = (!plan_slots.is_empty()).then(|| {
            format!(
                "{} will spend your own plan. FLEETOR applies no per-pane budget, and an \
                 exhausted plan looks like an idle pane rather than an error.",
                match plan_slots.len() {
                    1 => format!("1 of the {} worker seats", seats.workers.len()),
                    n if n == seats.workers.len() => format!("All {n} worker seats"),
                    n => format!("{n} of the {} worker seats", seats.workers.len()),
                }
            )
        });

        Self { refusals, cost, fallbacks, credential_fallbacks, plan_seats }
    }

    /// The refusal as one sentence, or `None` when the fleet may start.
    ///
    /// Assembled here rather than at the two call sites so the operator reads the
    /// same words whether the interface stopped them or [`fleet_launch`] did.
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

/// **Why a seat on this harness would spawn under a containment nobody chose, or
/// `None`** (WP-25 #37; C8).
///
/// The posture tripwire's one call site. Everything it knows is
/// [`harness::HarnessReadiness::posture_disagreements`]'s; what this adds is the
/// decision that a disagreement is a **refusal** rather than a warning, and the
/// reason is the failure class the whole arc is shaped around: a fleet that spawns
/// clean and cannot talk, or can talk and cannot write, with nothing on screen to
/// say so. By the time a pane is up, the only evidence is silence.
///
/// **One sentence per seat, not one per key.** Every disagreeing key is named
/// inside it — that is acceptance criterion 4 and it is the actionable half — but
/// three keys times five seats would be fifteen refusals for one broken vendor
/// release, and a refusal nobody reads to the end of is a refusal that did not
/// happen.
///
/// **It goes through `StartRefusal` rather than beside it**, so the interface needs
/// no new wire: `StartGate.tsx` already renders every refusal and disables Start on
/// a non-empty list, and [`fleet_launch`] already refuses on the same list. A
/// second channel would have been a second implementation of "may this fleet start",
/// which is the disagreement M15 exists to prevent.
///
/// **A harness this build does not register produces nothing here**, because
/// [`refusal_for`] has already refused it by name and a second sentence about a
/// fleet that is already stopped tells the operator nothing new.
fn posture_refusal(reading: Option<&harness::HarnessReadiness>) -> Option<String> {
    let reading = reading?;
    let disagreements = reading.posture_disagreements();
    if disagreements.is_empty() {
        return None;
    }
    let each: Vec<String> =
        disagreements.iter().map(harness::PostureDisagreement::sentence).collect();
    Some(format!(
        "the containment FLEETOR writes is not the containment `{}` resolved. {}",
        reading.invoked,
        each.join(" "),
    ))
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

/// **Move any seat whose credential this machine cannot honour onto the one it
/// can, and say so** (C79).
///
/// **This replaces a refusal, and the difference is what "silent" meant.** C78
/// refused such a seat outright, on the rule that FLEETOR never substitutes one
/// credential for another — because a substitution bills the operator for a
/// choice they did not make. The property that actually mattered there was *no
/// **invisible** spend*, not *no substitution*: a seat that moves, shows its new
/// credential in its own dropdown, and is named in a notice above the button has
/// not spent anything the operator could not see. What C78 got wrong was the
/// cost of the strict rule — **the default is the plan on every seat, so a
/// machine where one harness has no login met a dead gate on arrival**, refusing
/// a fleet the operator had not configured at all.
///
/// **A seat with neither credential available is left alone**, and `for_seats`
/// refuses it. That is the case the operator genuinely has to fix, and it is now
/// the only one.
///
/// **Run before the seats are stored**, exactly as [`settle_models`] is and for
/// its reason: what the summary describes and what the rows show is then the
/// fleet that would spawn, rather than a selection the backend would quietly
/// reinterpret later (M15).
fn settle_credentials(
    mut seats: FleetSeats,
    offer: &PlanOffer,
) -> (FleetSeats, Vec<CredentialFallback>) {
    let mut moved = Vec::new();
    for (at, seat) in seats.workers.iter_mut().enumerate() {
        let slot = u8::try_from(at + 1).unwrap_or(u8::MAX);
        if offer.gap_for(&seat.harness, seat.credential).is_none() {
            continue;
        }
        let other = match seat.credential {
            CredentialChoice::Plan => CredentialChoice::FleetKey,
            CredentialChoice::FleetKey => CredentialChoice::Plan,
        };
        // Neither works. Left as asked so the refusal names the credential the
        // operator actually chose rather than one this function picked for them.
        if offer.gap_for(&seat.harness, other).is_some() {
            continue;
        }
        moved.push(CredentialFallback {
            seat: seat_label(PaneId::Worker(slot)),
            harness: seat.harness.clone(),
            asked: seat.credential,
            fell_back_to: other,
        });
        seat.credential = other;
        // **The model follows the credential** (C78). A name from one provider is
        // meaningless to the other, so a seat that moves drops back to the new
        // side's default rather than carrying `deepseek-v4-flash` to a login or an
        // alias to FLEETOR's endpoint.
        seat.model = None;
    }
    (seats, moved)
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
/// The gate is the screen that runs *before* [`fleet_launch`], and both of the
/// things it renders from are things a fleet that does not exist yet cannot own:
/// what each harness reports about this machine, and which harness and model the
/// operator put on each seat. #35 hung both off [`Fleet`], where [`fleet_gate`]
/// could only ever answer `no fleet is running` — the pickers were unreachable in
/// the one state they exist for, and nothing failed a compile or a run to say so.
///
/// They live here instead, managed for the app's lifetime, and [`Fleet`] holds **the
/// same `Arc`** rather than a second copy of the values. That is the idiom
/// [`Target`] already uses in this file, for the identical reason:
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
    /// opened by [`fleet_launch`]. Without this the operator's first reading of
    /// their own machine — including the caveat about what a passing check does not
    /// prove — would be discovered and then dropped.
    pending: Mutex<Vec<(NoticeLevel, String)>>,
}

impl GateHold {
    /// What is picked right now, defaulting to the fleet nobody has picked yet.
    fn seats(&self) -> FleetSeats {
        self.held().get_or_insert_with(FleetSeats::unpicked).clone()
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
    /// **What the operator can do about it** (#51) — the move out of the state
    /// `reason` describes, which until this ticket was a dead end.
    ///
    /// Computed by `HarnessReadiness::login_guidance` off `HarnessSpec::login`, so
    /// the card renders whatever the harness declared and a third one registered
    /// tomorrow needs no interface edit. `None` for a harness that is logged in and
    /// for one that is not installed, where `reason` already carries the only
    /// actionable sentence there is.
    guidance: Option<LoginHelp>,
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

/// **The way out of a harness that cannot take a seat**, in the shape the card
/// renders (#51).
///
/// Three slots and no verdict word: the interface lays out whichever are present and
/// switches on nothing. `harness::LoginGuidance` decided which of the three shapes
/// this machine is in, and it decided it once.
#[derive(Serialize, Clone)]
pub struct LoginHelp {
    /// Why the operator rather than FLEETOR is the one who has to act.
    sentence: String,
    /// What to type. Absent when no command would help.
    command: Option<&'static str>,
    /// The second step, for a harness whose login lives inside what `command`
    /// starts.
    then: Option<&'static str>,
    /// The environment variable to set, for the shape no command fixes.
    variable: Option<String>,
}

impl From<harness::LoginGuidance> for LoginHelp {
    fn from(guidance: harness::LoginGuidance) -> Self {
        Self {
            sentence: guidance.sentence,
            command: guidance.command,
            then: guidance.then,
            variable: guidance.variable,
        }
    }
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
    /// the operator reads and the rule [`fleet_launch`] enforces are one answer.
    verdict: StartVerdict,
    /// **Which harnesses this machine has a readable operator login for** (C78),
    /// so a row can say *why* `your plan` is unavailable instead of offering
    /// something Start will refuse.
    ///
    /// On this value rather than fetched separately, for the reason every other
    /// field here is: the picker and the cost sentence must be two readings of one
    /// answer, and a second round trip is a second answer that agrees only until
    /// it does not.
    harnesses_with_a_login: Vec<String>,
    /// **Whether the `.env` walk found a worker key** (C78) — the other half of
    /// the same question, since a seat on `your key` refuses individually now
    /// rather than the whole fleet quietly becoming orchestrator-only.
    has_fleet_key: bool,
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
            LoginState::NoCredential { summary, .. } => (
                "no-credential",
                None,
                Some(summary.clone()),
            ),
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
            guidance: readiness.login_guidance().map(LoginHelp::from),
            version: readiness.version.clone(),
            provider: readiness.provider.clone(),
            models,
            readings_agree: readiness.readings_agree,
            resolved: readiness.resolved.as_ref().map(|p| p.display().to_string()),
        }
    }
}

/// The live backend, built by a [`launch`] and dropped by the next one or by quit. Owns the tokio runtime the follower, hub, and delivery loop run on.
struct Fleet {
    rt: Runtime,
    store: Arc<dyn Store>,
    /// The fleet target's task store (D-100); `None` if it would not open.
    tasks: Option<Arc<dyn Store>>,
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
    /// The seats this fleet places, fixed at launch (D-099): the gate's settled
    /// picks for a fresh launch, the run's recorded ones for a reopen (D-095).
    /// Editing the gate afterwards describes the next fleet, never this one.
    seats: FleetSeats,
    /// The id this run is archived under and its lineage's seat directory, which
    /// differ on a reopen. Neither is shared with any other run or lineage.
    /// Held for the task store (D-099, Coordination D).
    #[allow(dead_code)]
    run_id: String,
    #[allow(dead_code)]
    sessions: placement::SessionsId,
    /// The briefs and launch settings every pane spawns with, from `prompts/`
    /// and the operator's `~/.fleetor/prompts/`. Resolved once for the same
    /// reason the target is: a fleet whose panes were briefed from two revisions
    /// of a file being edited is not a fleet anyone can reason about.
    context: PaneContext,
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
}

/// Managed Tauri state: at most one embedded fleet.
#[derive(Default)]
pub struct FleetState(Mutex<Option<Fleet>>);

/// The repo the fleet works on — **one value, shared by everything that reads it**
/// (WP-21, D14).
///
/// [`apply_target`] rewrites it and [`spawn_pane`] reads it, through one cell
/// rather than two copies.
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

/// **Archive the session that ended, as the app opens** (D-085).
///
/// Rotation used to run only inside [`fleet_launch`], so the session the operator
/// had just closed was still the live slot on the start gate — not a History row —
/// and "reopen the app, go back to what I was doing" found nothing to click. Called
/// from `setup` straight after `orphans::sweep`, so the panes that wrote the slot
/// are gone before it is frozen, and before anything could open it: bootstrap is
/// the slot's only opener, and it still rotates too (a reopen archives the fleet
/// being left there).
///
/// The notices wait on the gate, drained onto the feed by bootstrap — there is no
/// feed yet, and "previous run archived" is worth saying rather than dropping.
pub fn archive_previous_run(gate: &GateHold) {
    let layout = Layout::for_operator();
    archive_previous_run_under(&layout.shell(), &runs::runs_dir(layout.root()), gate);
}

fn archive_previous_run_under(shell: &Path, runs_dir: &Path, gate: &GateHold) {
    gate.remember(runs::rotate(shell, runs_dir, fleetor_core::time::now_ms()));
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
/// before bootstrap (the start gate's configuration, the History list, the orphan
/// sweep) call this and then one accessor, which is one spelling
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

// --- launch (D-099) -----------------------------------------------------------

/// What a launch brings up: a new fleet on the gate's picks, or a past run.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum LaunchSource {
    Fresh,
    Reopen { id: String },
}

/// Why a launch brought no fleet up. `torn_down` is false for a refusal at the
/// verdict, which leaves whatever was running exactly as it was.
#[derive(Debug, Clone, Serialize)]
pub struct LaunchFailure {
    pub reason: String,
    pub torn_down: bool,
}

/// What a launch tells the webview.
enum Signal {
    /// The verdict passed and the old fleet is down; the new log is not open
    /// yet, so everything after this belongs to the new run.
    Launching,
    Event(WireEvent),
    /// One chain entry from the task store, on its own pipe.
    Task(WireEvent),
}

/// How a [`Signal`] reaches the webview; `false` once nobody is listening. A
/// callback rather than an `AppHandle`, so [`launch`] runs without a window.
type FeedEmit = Arc<dyn Fn(Signal) -> bool + Send + Sync>;

/// What the verdict settled, and what the rest of the launch builds from.
struct LaunchPlan {
    seats: FleetSeats,
    /// The run to reopen and the target it recorded. `None` on a fresh launch.
    reopen: Option<(String, Option<PathBuf>)>,
    probe_notices: Vec<(NoticeLevel, String)>,
}

/// **The one way a fleet comes up** (D-099): verdict, teardown, archive, build.
/// Replaces `fleet_bootstrap` and `run_reopen`.
#[tauri::command]
pub fn fleet_launch(
    app: AppHandle,
    state: State<'_, FleetState>,
    registry: State<'_, Arc<PaneRegistry>>,
    gate: State<'_, Arc<GateHold>>,
    source: LaunchSource,
) -> Result<BootSnapshot, LaunchFailure> {
    let emit: FeedEmit = Arc::new(move |signal| match signal {
        Signal::Launching => app.emit(EVENT_LAUNCHING, ()).is_ok(),
        Signal::Event(event) => app.emit(EVENT_FLEET, event).is_ok(),
        Signal::Task(event) => app.emit(EVENT_TASK, event).is_ok(),
    });
    launch(&state, &registry, &gate, Layout::for_operator(), &plan_offer(), source, emit)
}

/// [`fleet_launch`] with no Tauri state, so a test drives it over a scratch layout.
///
/// **The order is the contract.** Nothing is touched before the verdict, so a
/// launch that was never going to work costs the running fleet nothing. A launch
/// issued while another is in flight waits on the fleet lock behind it.
fn launch(
    state: &FleetState,
    registry: &Arc<PaneRegistry>,
    gate: &GateHold,
    layout: Layout,
    offer: &PlanOffer,
    source: LaunchSource,
    emit: FeedEmit,
) -> Result<BootSnapshot, LaunchFailure> {
    let refused = |reason: String| LaunchFailure { reason, torn_down: false };
    let failed = |reason: String| LaunchFailure { reason, torn_down: true };
    // A pane mid-placement writes the live `run.json`; it finishes before the slot
    // is archived. Same order as `spawn_pane`: placing, then the fleet.
    let _placing = PLACING.lock().map_err(|e| refused(e.to_string()))?;
    let mut slot = state.0.lock().map_err(|e| refused(e.to_string()))?;

    let plan = verdict(&layout, gate, offer, &source).map_err(refused)?;
    teardown(&mut slot, registry);
    // On every launch, and after the teardown so no event of the old fleet can
    // follow it.
    emit(Signal::Launching);
    let started_ms = fleetor_core::time::now_ms();
    let archived = runs::rotate(&layout.shell(), &runs::runs_dir(layout.root()), started_ms);
    let fleet = build(layout, registry, gate, plan, started_ms, archived, emit).map_err(failed)?;
    let snap = snapshot(&fleet.store).map_err(failed)?;
    *slot = Some(fleet);
    Ok(snap)
}

/// **Whether this launch can start, decided before anything is torn down**
/// (WP-25 #36, R8). A seat on a harness this machine cannot log into would come up
/// on a login prompt; a reopen whose sessions or target are gone would kill the
/// live fleet for nothing.
fn verdict(
    layout: &Layout,
    gate: &GateHold,
    offer: &PlanOffer,
    source: &LaunchSource,
) -> Result<LaunchPlan, String> {
    let runs_dir = runs::runs_dir(layout.root());
    let cannot_open = |id: &str, why: String| format!("\u{201c}{id}\u{201d} can\u{2019}t be opened: {why}");
    let reopen = match source {
        LaunchSource::Fresh => None,
        LaunchSource::Reopen { id } => {
            if let Some(why) = runs::reopen_blocker_for(&runs_dir, id) {
                return Err(cannot_open(id, why));
            }
            let target = runs::recorded_target(&runs_dir, id);
            if let Some(gone) = target.as_ref().filter(|t| !t.is_dir()) {
                let why = format!("its target {} is no longer a folder", gone.display());
                return Err(cannot_open(id, why));
            }
            Some((id.clone(), target, runs::recorded_panes(&runs_dir, id)))
        }
    };

    // The gate has almost always probed already; a launch reached without a
    // reading pays for the probe here.
    let (readings, probe_notices) = gate.harnesses.read();
    // A reopen places what the run recorded, not what the gate holds (D-095).
    let asked = match &reopen {
        Some((_, _, recorded)) => gate.seats().as_recorded(recorded),
        None => gate.seats(),
    };
    // Settled first, so a model the vendor no longer lists has already fallen
    // back rather than reaching a `--model` flag (story 14).
    let (settled, model_fallbacks) = settle_models(asked, &readings);
    let (settled, credential_fallbacks) = settle_credentials(settled, offer);
    // A fresh launch writes what it settled back into the gate; a reopen leaves
    // the gate's selection for the next new fleet.
    let seats = if reopen.is_some() { settled } else { gate.store_seats(settled) };
    let start =
        StartVerdict::for_seats(&seats, &readings, model_fallbacks, credential_fallbacks, offer);
    if let Some(why) = start.why_it_will_not_start() {
        return Err(match source {
            LaunchSource::Fresh => why,
            LaunchSource::Reopen { id } => cannot_open(id, why),
        });
    }
    Ok(LaunchPlan {
        seats,
        reopen: reopen.map(|(id, target, _)| (id, target)),
        probe_notices,
    })
}

/// Kill every pane and drop the live fleet, which closes its store and frees
/// `state.db` for the archive. The one teardown: launch and quit both call it.
///
/// The runtime goes before the hub can unlink its socket, so the socket is
/// removed here.
fn teardown(slot: &mut Option<Fleet>, registry: &Arc<PaneRegistry>) {
    crate::pty::kill_all(registry);
    if let Some(fleet) = slot.take() {
        fleet.shutdown.notify_one();
        let socket = fleet.layout.socket();
        drop(fleet);
        let _ = std::fs::remove_file(socket);
    }
}

/// Bring the new fleet up in the empty live slot: seed it for a reopen, open the
/// store, fix the target and seats, start the hub.
fn build(
    layout: Layout,
    registry: &Arc<PaneRegistry>,
    gate: &GateHold,
    plan: LaunchPlan,
    started_ms: i64,
    archived: Vec<(NoticeLevel, String)>,
    emit: FeedEmit,
) -> Result<Fleet, String> {
    let dir = layout.shell();
    let runs_dir = runs::runs_dir(layout.root());
    std::fs::create_dir_all(&dir).map_err(|e| format!("create shell dir: {e}"))?;

    // Resolved once per fleet, for the reason the target is.
    let mut context = PaneContext::resolve(&prompts::override_dir(layout.root()));

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("start runtime: {e}"))?;

    // **A reopen is this same path with a seeded slot** (WP-27, R2): the archive
    // has just emptied it and nothing has opened it yet.
    let (reopened, recorded_target) = match plan.reopen {
        Some((id, target)) => (Some(runs::seed_reopen(&dir, &runs_dir, &id)?), target),
        None => (None, None),
    };
    let no_recorded_target = reopened.is_some() && recorded_target.is_none();

    // The observability core: real store, wrapped once so every append publishes.
    let bcast = Arc::new(BroadcastStore::new(Arc::new(
        SqliteStore::open(&dir.join("state.db")).map_err(|e| format!("open store: {e}"))?,
    )));
    let store: Arc<dyn Store> = bcast.clone();

    spawn_follower(&rt, bcast.clone(), emit.clone(), Signal::Event);

    for (level, text) in &archived {
        note(&store, *level, text);
    }

    // **Fixed here for the fleet's life** (D-099). A reopen runs against the
    // target it recorded and writes no config; a run that recorded none, like a
    // fresh launch, takes the gate's.
    let target = match recorded_target {
        Some(target) => {
            note(&store, NoticeLevel::Info, &format!("target: {}", target.display()));
            target
        }
        None => resolve_target(&layout, &store)?,
    };
    if no_recorded_target {
        note(
            &store,
            NoticeLevel::Warn,
            &format!(
                "this run recorded no target, so it reopened on the gate\u{2019}s: {}",
                target.display()
            ),
        );
    }
    let config = fleet_config_for(&target);

    // What the gate's probe said before there was a feed, and what this launch's
    // own probe said if nothing had probed.
    for (level, text) in gate.take_pending().into_iter().chain(plan.probe_notices) {
        note(&store, level, &text);
    }

    // **Which seat directory this run's panes live in** (WP-27, R4). A fresh run
    // gets its own; a reopened one inherits its lineage's, which is what lets its
    // panes resume in place with no copy.
    let run_id = runs::new_run_id(&dir, &runs_dir, started_ms);
    let sessions = match &reopened {
        Some(r) => r.sessions.clone(),
        None => placement::SessionsId::new(run_id.clone()),
    };
    let parent = reopened.as_ref().map(|r| r.parent.as_str());
    runs::begin_as(&dir, &run_id, started_ms, &target, &sessions, parent);
    context.sessions = sessions.clone();
    // A pure function of the target (R26): the spawn path renders briefs from a
    // worktree and cannot recover the target from one.
    context.branch_prefix = placement::worker_branch_prefix(&target, &sessions);
    // Which session each seat reopens (R6), through the lineage: a reopen quit
    // before its panes registered still knows its seats. Empty on a fresh launch.
    context.resume = match &reopened {
        Some(r) => runs::lineage_session_ids(&runs_dir, &r.parent),
        None => Default::default(),
    };
    // Read at each launch, so the toggle takes effect at the next session start.
    context.startup = match &reopened {
        Some(_) => Startup::Reopened,
        None if startup_tasks_at(&layout.config_file()) => Startup::Resume,
        None => Startup::Ask,
    };
    if let Some(r) = &reopened {
        note(
            &store,
            NoticeLevel::Info,
            &format!(
                "reopened from run {} — {} of 5 seats resume their own session",
                r.parent,
                context.resume.len()
            ),
        );
    }

    for (level, text) in &context.notices {
        note(&store, *level, text);
    }

    // The hub↔app seam: the hub routes, the app owns the terminals. Unbounded on
    // purpose — a bounded channel would make a busy fleet block a send (D-034).
    let (app_tx, app_rx) = mpsc::unbounded_channel();
    let gauges = Arc::new(GaugeSources::default());
    deliver::spawn_delivery(&rt, registry.clone(), app_rx, store.clone(), gauges.clone());
    // The hub takes its own clone; `fleet_roster` sends into the original (see the
    // `Fleet::app` doc).
    // The fleet target's task store, never the gate's (D-099, Coordination B).
    let tasks: Option<Arc<dyn Store>> = open_task_store(&layout, &target, &store).map(|tasks| {
        spawn_follower(&rt, tasks.clone(), emit, Signal::Task);
        tasks as Arc<dyn Store>
    });
    let ctx = TaskContext { run: run_id.clone(), lineage: sessions.to_string() };
    let (hub, shutdown) =
        spawn_hub(&rt, store.clone(), tasks.clone().map(|t| (t, ctx)), app_tx.clone(), layout.socket());

    // The write guardrail's own feed (WP-17), started with the run and emptied by it.
    spawn_guardrail_feed(&rt, store.clone(), &dir);

    Ok(Fleet {
        rt,
        store,
        tasks,
        shutdown,
        config,
        target: Target::new(target),
        seats: plan.seats,
        run_id,
        sessions,
        context,
        gauges,
        app: app_tx,
        layout,
        hub,
    })
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
/// **The operator arm is the one `spawn_pane` always had**, lifted out whole when
/// the picked seats moved above it, and matched exhaustively rather than with a
/// wildcard so a new pane kind still has to say what places it.
///
/// The other is the one this ticket added: a seat the gate *does* carry a choice
/// for, whose choice would not resolve. `FleetSeats::spec_for` refuses rather than
/// substituting — a fleet that quietly placed a different harness than the gate
/// promised is the failure M15 exists to prevent — and `fleet_set_seats` is what
/// stops such a value being stored. This arm is the belt to that pair of braces.
fn spec_without_a_choice(pane: PaneId, seats: &FleetSeats) -> Result<PaneSpec, String> {
    match pane {
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

static PLACING: Mutex<()> = Mutex::new(());

pub(crate) fn spawn_pane(
    fleet: &FleetState,
    registry: &Arc<PaneRegistry>,
    pane: PaneId,
    rows: u16,
    cols: u16,
) -> Result<(), String> {
    // Placement writes files the panes share (`run.json`, the worktrees), so it is
    // one pane at a time. The lock is dropped before the wake, which takes seconds
    // and runs for every pane at once (D-096).
    let placing = PLACING.lock().map_err(|e| e.to_string())?;
    // StrictMode's second mount, or a caller racing the first: nothing to place.
    if registry.is_up(pane)? {
        return Ok(());
    }
    let (target, layout, store, context, gauges, seats) = {
        let guard = fleet.0.lock().map_err(|e| e.to_string())?;
        let f = guard.as_ref().ok_or("fleet not bootstrapped")?;
        (
            f.target.get(),
            f.layout.clone(),
            f.store.clone(),
            f.context.clone(),
            f.gauges.clone(),
            f.seats.clone(),
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
    // gate instead. `FleetSeats::spec_for` answers `None` for the operator, who
    // carries no choice and is refused below, so the arm after it is the arm it
    // always was.
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
    // operator watching a mixed fleet come up is not poorer than a reader of the
    // same run afterwards.
    //
    // Both are the values placement *returned*, never re-derived from `spec`: a
    // fact the caller recomputes is a fact that can disagree with the one the pane
    // was actually brought up on.
    //
    // Neither is on the message path and neither can become so (Tier 1.4) — this is
    // the spawn path, several filesystem writes deep already, and a `fleet send`
    // reads none of it.
    runs::record_pane(
        &layout.shell(),
        &pane.to_string(),
        placed.harness,
        placed.model.as_deref(),
        seats.choice_for(pane).map(|seat| seat.credential),
    );
    if let Err(e) = store.append_event(&FleetEvent::PaneState {
        pane,
        // A pane with no pty is `Dead` to every reader in the fleet — the roster
        // included — so that is what it was a moment ago, rather than a fifth state
        // meaning "never existed".
        from: PaneState::Dead,
        to: PaneState::Spawning,
        harness: Some(placed.harness.name.to_string()),
        // The rail's mark, off the same spec the name came off — so the interface
        // never has to ask which harness this is (#50).
        mark: Some(placed.harness.mark.to_string()),
        model: placed.model.clone(),
    }) {
        eprintln!("fleet: could not append the spawn event: {e}");
    }

    drop(placing);
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

/// Open the target's `tasks.db` and note the target's path beside it, so a
/// moved repo can be re-linked. A store that will not open is reported and the
/// fleet comes up without one: `fleet task` then refuses and says why.
fn open_task_store(layout: &Layout, target: &Path, feed: &Arc<dyn Store>) -> Option<Arc<BroadcastStore>> {
    let path = layout.task_store(target);
    match SqliteStore::open(&path) {
        Ok(store) => {
            let canonical = target.canonicalize().unwrap_or_else(|_| target.to_path_buf());
            let _ = std::fs::write(layout.target_dir(target).join("target.txt"), canonical.to_string_lossy().as_bytes());
            Some(Arc::new(BroadcastStore::new(Arc::new(store))))
        }
        Err(e) => {
            note(feed, NoticeLevel::Error, &format!("the task store at {} would not open, so goals and tasks are unavailable this session: {e}", path.display()));
            None
        }
    }
}

/// Pump every appended event of one store to the webview, oldest-first then
/// live, for the fleet's lifetime.
fn spawn_follower(rt: &Runtime, bcast: Arc<BroadcastStore>, emit: FeedEmit, signal: fn(WireEvent) -> Signal) {
    rt.spawn(async move {
        let mut follower = match bcast.follow(0) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("fleet: follower failed to start: {e}");
                return;
            }
        };
        while let Ok(Some((seq, event))) = follower.next().await {
            if !emit(signal(WireEvent { seq, event })) {
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
    tasks: Option<(Arc<dyn Store>, TaskContext)>,
    app: mpsc::UnboundedSender<AppCommand>,
    sock: PathBuf,
) -> (Arc<Hub>, Arc<Notify>) {
    let _ = std::fs::remove_file(&sock); // clear a stale socket from a prior run
    let transport = Arc::new(UnixTransport::new(&sock));

    let shutdown = Arc::new(Notify::new());
    let gate = shutdown.clone();
    let for_note = store.clone();
    let hub = match tasks {
        Some((tasks, ctx)) => Hub::with_tasks(store, app, tasks, ctx),
        None => Hub::new(store, app),
    };
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

/// One task operation by the operator, through the hub as `operator` — the
/// path the composer's messages take. The UI never writes the store itself.
/// Resolves to the task's number; rejects with the hub's refusal.
#[tauri::command]
pub fn fleet_task(state: State<'_, FleetState>, action: TaskAction) -> Result<String, String> {
    let (hub, handle) = {
        let guard = state.0.lock().map_err(|e| e.to_string())?;
        let fleet = guard.as_ref().ok_or("start a fleet to change tasks")?;
        (fleet.hub.clone(), fleet.rt.handle().clone())
    };
    match handle.block_on(hub.handle(PaneId::Operator, Op::Task { action })) {
        OpResult::Recorded { record_id } => Ok(record_id),
        OpResult::Error { message } => Err(message),
        other => Err(format!("the hub answered a task change with something else: {other:?}")),
    }
}

/// Every chain entry for the Tasks view, and whether it can be written to.
#[derive(Clone, Serialize)]
pub struct TaskSnapshot {
    /// A fleet is running, so the operator's controls work.
    live: bool,
    /// The repo these tasks belong to.
    target: String,
    /// This run and its lineage, for "earlier run"; `None` with no fleet.
    run: Option<String>,
    lineage: Option<String>,
    events: Vec<WireEvent>,
}

/// The task store the Tasks view shows: the live fleet's, else the gate
/// target's opened read-only and never created (D-100).
#[tauri::command]
pub fn fleet_tasks(state: State<'_, FleetState>) -> Result<TaskSnapshot, String> {
    task_snapshot(&state, &layout())
}

fn task_snapshot(state: &FleetState, layout: &Layout) -> Result<TaskSnapshot, String> {
    let wire = |events: Vec<(i64, FleetEvent)>| {
        events.into_iter().map(|(seq, event)| WireEvent { seq, event }).collect()
    };
    let guard = state.0.lock().map_err(|e| e.to_string())?;
    if let Some(fleet) = guard.as_ref() {
        let events = match &fleet.tasks {
            Some(tasks) => tasks.events_since(0).map_err(|e| e.to_string())?,
            None => Vec::new(),
        };
        return Ok(TaskSnapshot {
            live: true,
            target: fleet.target.get().to_string_lossy().into_owned(),
            run: Some(fleet.run_id.clone()),
            lineage: Some(fleet.sessions.to_string()),
            events: wire(events),
        });
    }
    drop(guard);
    let target = configured_target(layout)?.unwrap_or_else(|| layout.testbed());
    let path = layout.task_store(&target);
    let events = if path.is_file() {
        fleetor_db::archive::events(&path, 0).map_err(|e| e.to_string())?
    } else {
        Vec::new()
    };
    Ok(TaskSnapshot {
        live: false,
        target: target.to_string_lossy().into_owned(),
        run: None,
        lineage: None,
        events: wire(events),
    })
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
                        "{} is not a git repository — starting the fleet will `git init` \
                         it and commit what is there, so each worker gets its own worktree.",
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
// Off the main thread: a blocking dialog there hangs the app.
#[tauri::command(async)]
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
/// **The return is the stored value read back rather than the argument echoed**:
/// the gate's summary renders from what this returns, and a summary rendered from what
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
/// prompts are resolved on the spot, which is the same read `fleet_launch` will
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
    let offer = plan_offer();
    let (settled, fallbacks) = settle_models(gate.seats(), readings);
    let (settled, credential_fallbacks) = settle_credentials(settled, &offer);
    let seats = gate.store_seats(settled);
    let verdict =
        StartVerdict::for_seats(&seats, readings, fallbacks, credential_fallbacks, &offer);
    GateState {
        harnesses: readings.iter().map(HarnessOffer::from).collect(),
        seats,
        worker_model_default,
        verdict,
        harnesses_with_a_login: offer.readable().to_vec(),
        has_fleet_key: offer.has_fleet_key(),
    }
}

// **`workers_run` is gone, and its absence is the ticket** (C78).
//
// It answered "is there a fleet key" for the whole fleet, and `false` meant no
// worker seats existed at all — the orchestrator ran alone, silently, with no
// refusal. Two things retired it. A machine with a subscription and no API
// account must run four plan workers, so a key is no longer what makes a worker
// possible; and once seats carry their own credential, "can a worker run" stops
// being a fleet-wide question at all. Every seat now answers it for itself
// through `PlanOffer::gap_for`, and a seat that cannot start **refuses** rather
// than vanishing — which is what every other unusable seat on this card already
// did.

/// **What the operator picked, paired with what this machine can honour** (C75).
///
/// Assembled here rather than passed down from `fleet_launch` because both
/// readers — the gate's summary and [`workers_run`] — are reached on paths that
/// do not share a [`Host`], and two hand-built offers is two answers to one
/// question.
///
/// [`Host::discover`] is the read, which keeps the credential's one reader rule
/// intact: this function takes the *names* off that value and drops the
/// credentials with it, so nothing outside the spawn path ever holds one.
fn plan_offer() -> PlanOffer {
    let host = Host::discover();
    PlanOffer::new(
        host.operator_logins.iter().map(|(name, _)| (*name).to_string()).collect(),
        load_api_key().is_ok(),
    )
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

#[tauri::command]
pub fn run_delete(id: String) -> Result<(), String> {
    let l = layout();
    runs::delete(&runs::runs_dir(l.root()), &l.shell(), &id)
}

/// Save a run's JSON export wherever the operator points.
///
/// The dialog lives here rather than in the webview so no npm plugin has to be
/// added for it — `fleet_pick_target` set the pattern. `Ok(None)` means the
/// operator dismissed the dialog, which is not an error and must not be shown
/// as one.
// Off the main thread: a blocking dialog there hangs the app.
#[tauri::command(async)]
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

/// The config key behind "finish remaining tasks upon startup".
const STARTUP_TASKS_KEY: &str = "startup_tasks";

/// Whether the setting is on. Only a JSON `true` is; a missing or unreadable
/// config is off, which is the state where orch asks first.
fn startup_tasks_at(file: &Path) -> bool {
    std::fs::read_to_string(file)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|config| config.get(STARTUP_TASKS_KEY).and_then(|v| v.as_bool()))
        .unwrap_or(false)
}

/// Set it and return what is now stored, which is what the switch renders.
fn set_startup_tasks_at(file: &Path, on: bool) -> Result<bool, String> {
    write_config_key_at(file, STARTUP_TASKS_KEY, on.into())?;
    Ok(startup_tasks_at(file))
}

#[tauri::command]
pub fn startup_tasks_get() -> bool {
    startup_tasks_at(&layout().config_file())
}

#[tauri::command]
pub fn startup_tasks_set(on: bool) -> Result<bool, String> {
    set_startup_tasks_at(&layout().config_file(), on)
}

/// Set one key in `~/.fleetor/config.json`, leaving every other key alone.
///
/// The one writer of that file, so a second setting cannot grow a second spelling
/// of "merge, don't clobber" that drops the first one.
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

/// **Quitting archives the session** (WP-28, D-086) — the whole of what closing
/// the app does to the fleet, in the one order that works.
///
/// Panes first, because they are the processes that cost money. Then the fleet,
/// which closes its store. Then the archive — the same `runs::rotate` launch and
/// bootstrap call, so there is still one archive path. Launch keeps archiving
/// (D-085) for the quit this never sees: a crash, a Force Quit, a `kill -9`.
///
/// Safe to call twice, and a normal window close does (`CloseRequested`, then
/// `RunEvent::Exit`): the second call finds no panes, no fleet and no live log.
pub fn quit(state: &FleetState, registry: &Arc<PaneRegistry>, gate: &GateHold) {
    let layout = layout();
    archive_after_teardown(&layout.shell(), &runs::runs_dir(layout.root()), gate, || {
        match state.0.lock() {
            Ok(mut slot) => teardown(&mut slot, registry),
            Err(_) => crate::pty::kill_all(registry),
        }
    });
}

/// Tear down, **then** archive. A function of its own so the order is something a
/// test holds rather than two lines that happen to be in sequence: `archive::freeze`
/// cannot take a database out of WAL mode while another connection has it open, and
/// rotation would then fall back to moving three files.
fn archive_after_teardown(shell: &Path, runs_dir: &Path, gate: &GateHold, teardown: impl FnOnce()) {
    teardown();
    archive_previous_run_under(shell, runs_dir, gate);
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
        // What a click will spend by default, absent a per-seat override — never
        // gates whether worker seats exist (C78): a seat with no fleet key still
        // spends the operator's own plan.
        worker_backend: match load_api_key() {
            Ok(_) => "deepseek-v4-flash".to_string(),
            Err(_) => "plan".to_string(),
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
    /// what they are about. `write_target` calls the generic writer directly.
    fn merge_target(existing: Option<&str>, target: &Path) -> Result<String, String> {
        merge_config_key(existing, "target", target.to_string_lossy().into_owned().into())
    }

    /// **Quitting closes the live log before archiving it** (WP-28, D-086), which is
    /// what makes the archive one self-contained file rather than the three-file
    /// fallback `archive::freeze` forces while anything still holds the database.
    #[test]
    fn quitting_closes_the_live_log_before_archiving_it_so_the_archive_is_one_file() {
        let root = std::env::temp_dir().join(format!("fleetor-quit-archive-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let shell = root.join("_shell");
        let runs_dir = runs::runs_dir(&root);
        std::fs::create_dir_all(&shell).unwrap();
        let store: Arc<dyn Store> = Arc::new(SqliteStore::open(&shell.join("state.db")).unwrap());
        store
            .append_event(&FleetEvent::Message {
                id: fleetor_core::ids::new_id("msg"),
                from: PaneId::Orch,
                to: PaneId::Worker(1),
                body: "take the parser".into(),
                group: None,
                accepted: true,
                detail: None,
            })
            .unwrap();
        assert!(shell.join("state.db-wal").exists(), "precondition: a live WAL-mode log, as a running fleet has");
        let mut live = Some(store);

        archive_after_teardown(&shell, &runs_dir, &GateHold::default(), || drop(live.take()));

        let rows = runs::list(&runs_dir);
        assert_eq!(rows.len(), 1, "the session is a run the moment the app has quit");
        let archive = runs_dir.join(&rows[0].id);
        assert!(archive.join("state.db").is_file());
        assert!(
            !archive.join("state.db-wal").exists(),
            "frozen into one file, which only works once the fleet's connection is closed",
        );
        assert!(!shell.join("state.db").exists(), "and nothing is left live for the next launch to archive");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// **Opening the app makes the last session a History row** (D-085), and says
    /// so on the feed the next fleet opens rather than into the void.
    #[test]
    fn the_session_left_in_the_live_slot_is_archived_at_launch_and_announced_later() {
        let root = std::env::temp_dir().join(format!("fleetor-launch-archive-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let shell = root.join("_shell");
        let runs_dir = runs::runs_dir(&root);
        std::fs::create_dir_all(&shell).unwrap();
        {
            let store = SqliteStore::open(&shell.join("state.db")).unwrap();
            store
                .append_event(&FleetEvent::Message {
                    id: fleetor_core::ids::new_id("msg"),
                    from: PaneId::Orch,
                    to: PaneId::Worker(1),
                    body: "take the parser".into(),
                    group: None,
                    accepted: true,
                    detail: None,
                })
                .unwrap();
        }
        let gate = GateHold::default();

        archive_previous_run_under(&shell, &runs_dir, &gate);

        assert!(!shell.join("state.db").exists(), "the live slot is empty for the next fleet");
        assert_eq!(runs::list(&runs_dir).len(), 1, "and the session is a History row before any fleet starts");
        let held = gate.take_pending();
        assert!(held.iter().any(|(_, text)| text.contains("see History")), "held for the feed: {held:?}");

        // A second launch with nothing live archives nothing and says nothing.
        archive_previous_run_under(&shell, &runs_dir, &gate);
        assert_eq!(runs::list(&runs_dir).len(), 1);
        assert!(gate.take_pending().is_empty());
        let _ = std::fs::remove_dir_all(&root);
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
        let (_hub, shutdown) = spawn_hub(&rt, store.clone(), None, app_tx, sock.clone());

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
        let orch = layout().pane_config(&placement::SessionsId::new("run-1"), PaneId::Orch);
        let worker = layout().pane_config(&placement::SessionsId::new("run-1"), PaneId::Worker(2));
        // WP-27 R4: one level per run, and the seats are siblings *within* it.
        assert!(orch.ends_with("pane-config/run-1/orch"), "{}", orch.display());
        assert!(worker.ends_with("pane-config/run-1/worker-2"), "{}", worker.display());
        assert_eq!(orch.parent(), worker.parent(), "the transcript walk walks the parent");
        assert_eq!(worker.parent().unwrap(), layout().shell().join("pane-config").join("run-1"));
        // And a different run is a different directory — the whole of R4.
        let other = layout().pane_config(&placement::SessionsId::new("run-2"), PaneId::Orch);
        assert_ne!(orch, other, "two runs must not share a seat directory");
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

    /// A key this build does not read — a `dev_mode` left by an older one (D-094)
    /// — is ignored, and a target write leaves it in place.
    #[test]
    fn a_stale_key_in_the_config_is_ignored() {
        let stale = r#"{"dev_mode": true, "target": "/Users/me/code/thing"}"#;
        assert_eq!(parse_target(stale).unwrap(), Some(PathBuf::from("/Users/me/code/thing")));
        assert_eq!(parse_target(r#"{"dev_mode": true}"#).unwrap(), None);

        let merged = merge_target(Some(stale), Path::new("/picked")).unwrap();
        assert_eq!(parse_target(&merged).unwrap(), Some(PathBuf::from("/picked")));
        let config: serde_json::Value = serde_json::from_str(&merged).unwrap();
        assert_eq!(config["dev_mode"], serde_json::json!(true));
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
            orch: SeatChoice {
                harness: orch.to_string(),
                model: None,
                credential: CredentialChoice::Plan,
            },
            workers: fleetor_core::pane::WORKER_SLOTS
                .iter()
                .map(|_| SeatChoice {
                    harness: worker.to_string(),
                    model: Some("m".into()),
                    // The stock seat for tests that are not about this feature
                    // spends the key, matching `PlanOffer::fleet_key`.
                    credential: CredentialChoice::FleetKey,
                })
                .collect(),
        }
    }

    // --- one launch (D-099) ---------------------------------------------------
    //
    // Driven through `launch` over a scratch layout, a real registry and a gate
    // whose harness reading is described rather than probed.

    struct Bench {
        root: PathBuf,
        layout: Layout,
        state: FleetState,
        registry: Arc<PaneRegistry>,
        gate: GateHold,
        /// How many launches told the webview their verdict passed.
        launchings: Arc<std::sync::atomic::AtomicUsize>,
        /// Every chain entry the `fleet://task` pipe carried.
        task_events: Arc<Mutex<Vec<FleetEvent>>>,
    }

    impl Bench {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!("fl-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).unwrap();
            let bench = Self {
                layout: Layout::under(&root),
                state: FleetState::default(),
                registry: Arc::new(PaneRegistry::new(Arc::new(|_, _| {}), root.join("panes.pids"))),
                gate: GateHold::default(),
                launchings: Arc::default(),
                task_events: Arc::default(),
                root,
            };
            *bench.gate.harnesses.held() = Some(vec![logged_in("claude-code", AccountShape::ApiKey)]);
            bench.pick("m");
            bench.point_at("repo-a");
            bench
        }

        /// Put every worker seat on `model`, as the gate's pickers would.
        fn pick(&self, model: &str) {
            let mut seats = seats_on("claude-code", "claude-code");
            for worker in &mut seats.workers {
                worker.model = Some(model.to_string());
            }
            self.gate.store_seats(seats);
        }

        /// Set the gate's target, as the Home input would.
        fn point_at(&self, repo: &str) -> PathBuf {
            let target = self.root.join(repo);
            std::fs::create_dir_all(&target).unwrap();
            let value = target.to_string_lossy().into_owned().into();
            write_config_key_at(&self.layout.config_file(), "target", value).unwrap();
            target
        }

        fn launch(&self, source: LaunchSource) -> Result<BootSnapshot, LaunchFailure> {
            let offer = PlanOffer::fleet_key();
            let launchings = self.launchings.clone();
            let task_events = self.task_events.clone();
            let emit: FeedEmit = Arc::new(move |signal| {
                match signal {
                    Signal::Launching => {
                        launchings.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    }
                    Signal::Task(wire) => task_events.lock().unwrap().push(wire.event),
                    Signal::Event(_) => {}
                }
                true
            });
            launch(&self.state, &self.registry, &self.gate, self.layout.clone(), &offer, source, emit)
        }

        fn fresh(&self) {
            self.launch(LaunchSource::Fresh).expect("a fresh launch on a logged-in machine");
        }

        fn reopen(&self, id: &str) -> Result<BootSnapshot, LaunchFailure> {
            self.launch(LaunchSource::Reopen { id: id.to_string() })
        }

        /// A pane of the live fleet: recorded as placement records one, with a
        /// real pty behind it. `sleep` stands in for the harness.
        fn bring_up_orch(&self, model: &str) {
            let spec = harness::claude_code().spec();
            runs::record_pane(&self.layout.shell(), "orch", spec, Some(model), Some(CredentialChoice::Plan));
            let mut cmd = portable_pty::CommandBuilder::new("sleep");
            cmd.arg("30");
            self.registry.spawn(PaneId::Orch, cmd, spec, 24, 80).expect("a pty for sleep");
        }

        /// What the live fleet places against: orch's model, the workers' model, the target.
        fn placing(&self) -> Option<(Option<String>, Option<String>, PathBuf)> {
            let slot = self.state.0.lock().unwrap();
            let fleet = slot.as_ref()?;
            Some((
                fleet.seats.orch.model.clone(),
                fleet.seats.workers[0].model.clone(),
                fleet.target.get(),
            ))
        }

        fn runs_dir(&self) -> PathBuf {
            runs::runs_dir(self.layout.root())
        }

        /// One task operation as the operator, through the live fleet's hub.
        fn task(&self, action: fleetor_core::wire::TaskAction) -> OpResult {
            let slot = self.state.0.lock().unwrap();
            let fleet = slot.as_ref().expect("a live fleet");
            fleet.rt.block_on(fleet.hub.handle(PaneId::Operator, Op::Task { action }))
        }

        fn open_goal(&self, outcome: &str) -> OpResult {
            self.task(fleetor_core::wire::TaskAction::Post {
                goal: true,
                outcome: outcome.into(),
                technical: vec![],
                vision: vec!["it reads as one thing".into()],
                owner: None,
                instructions: None,
                parent: None,
                converges_on: None,
            })
        }

        fn task_list(&self) -> Vec<fleetor_core::task::TaskRecord> {
            match self.task(fleetor_core::wire::TaskAction::List) {
                OpResult::Board { tasks, .. } => tasks,
                other => panic!("expected the task list, got {other:?}"),
            }
        }

        fn launchings(&self) -> usize {
            self.launchings.load(std::sync::atomic::Ordering::SeqCst)
        }

        /// The live fleet's run id and sessions id.
        fn ids(&self) -> (String, String) {
            let slot = self.state.0.lock().unwrap();
            let fleet = slot.as_ref().expect("a live fleet");
            (fleet.run_id.clone(), fleet.sessions.to_string())
        }

        /// Every archived run folder, lineage members included.
        fn archives(&self) -> Vec<PathBuf> {
            let Ok(entries) = std::fs::read_dir(self.runs_dir()) else { return Vec::new() };
            let mut dirs: Vec<_> =
                entries.flatten().map(|e| e.path()).filter(|p| p.join("state.db").is_file()).collect();
            dirs.sort();
            dirs
        }
    }

    impl Drop for Bench {
        fn drop(&mut self) {
            teardown(&mut self.state.0.lock().unwrap_or_else(|e| e.into_inner()), &self.registry);
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    /// **The reported bug** (D-099): New fleet while a reopened run is live starts
    /// a fleet on the gate's picks, and the reopened run goes to History.
    #[test]
    fn a_new_fleet_after_a_reopen_places_the_gates_seats_and_archives_the_reopened_run() {
        let bench = Bench::new("after-reopen");
        let repo_a = bench.root.join("repo-a");
        bench.fresh();
        bench.bring_up_orch("recorded");
        bench.fresh();
        assert!(!bench.registry.any_pane(), "the first fleet's pane went with it");
        let first = runs::list(&bench.runs_dir())[0].id.clone();

        // The gate moves on; the reopen must not follow it, and must not move it.
        bench.pick("gate");
        bench.reopen(&first).expect("the first run reopens");
        let (orch, workers, target) = bench.placing().expect("a reopened fleet");
        assert_eq!(orch.as_deref(), Some("recorded"), "a reopen places what the run recorded");
        assert_eq!(workers.as_deref(), Some("gate"), "a seat with no record keeps the gate's pick");
        assert_eq!(target, repo_a);
        assert_eq!(bench.gate.seats().orch.model, None, "and the gate is left for the next new fleet");
        bench.bring_up_orch("recorded");

        bench.fresh();

        let (orch, workers, _) = bench.placing().expect("a new fleet");
        assert_eq!((orch, workers.as_deref()), (None, Some("gate")), "the new fleet is the gate's");
        assert!(!bench.registry.any_pane(), "the reopened run's pane is gone");
        assert_eq!(bench.archives().len(), 3, "the first run, the empty second, and the reopened sitting");
    }

    /// The startup setting is one key in `config.json`, read at each launch;
    /// a reopen never triages.
    #[test]
    fn the_startup_setting_is_read_at_each_launch_and_a_reopen_ignores_it() {
        let bench = Bench::new("startup");
        let config = bench.layout.config_file();
        let startup = || bench.state.0.lock().unwrap().as_ref().unwrap().context.startup;

        bench.fresh();
        assert_eq!(startup(), Startup::Ask, "off by default");
        bench.bring_up_orch("recorded");
        let (first, _) = bench.ids();

        assert_eq!(set_startup_tasks_at(&config, true), Ok(true));
        assert!(configured_target(&bench.layout).unwrap().is_some(), "the target key survives");
        bench.fresh();
        assert_eq!(startup(), Startup::Resume);

        bench.reopen(&first).expect("the first run reopens");
        assert_eq!(startup(), Startup::Reopened);

        std::fs::write(&config, r#"{"startup_tasks":"true"}"#).unwrap();
        assert!(!startup_tasks_at(&config), "only a JSON true is on");
    }

    /// **Goals and tasks belong to the target and outlive the run** (D-100): a
    /// relaunch reads them back, another target has its own, the entries ride
    /// their own pipe, and none of them enters the run log.
    #[test]
    fn goals_and_tasks_follow_the_fleets_target_across_launches() {
        let bench = Bench::new("tasks");
        let repo_a = bench.root.join("repo-a");
        bench.fresh();
        let (first_run, _) = bench.ids();

        assert_eq!(bench.open_goal("one grammar"), OpResult::Recorded { record_id: "1".into() });
        assert!(bench.layout.task_store(&repo_a).is_file(), "the store is the target's");
        assert!(
            !bench.layout.task_store(&repo_a).starts_with(bench.layout.shell()),
            "and outside the shell the panes may write to",
        );
        let noted = std::fs::read_to_string(bench.layout.target_dir(&repo_a).join("target.txt")).unwrap();
        assert_eq!(PathBuf::from(noted), repo_a.canonicalize().unwrap());

        let piped = || bench.task_events.lock().unwrap().len();
        for _ in 0..200 {
            if piped() > 0 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(piped(), 1, "the entry reached the task pipe");
        {
            let slot = bench.state.0.lock().unwrap();
            let log = slot.as_ref().unwrap().store.events_since(0).unwrap();
            assert!(
                !log.iter().any(|(_, e)| matches!(e, FleetEvent::Chain { .. } | FleetEvent::Task { .. })),
                "no task entry is in the run log",
            );
        }

        bench.fresh();
        let (second_run, _) = bench.ids();
        assert_ne!(first_run, second_run);
        assert_eq!(bench.open_goal("one parser"), OpResult::Recorded { record_id: "2".into() });
        let list = bench.task_list();
        assert_eq!(list.len(), 2, "the first session's goal is still there");
        assert_eq!(list[0].chain[0].run, first_run);
        assert_eq!(list[1].chain[0].run, second_run);

        // The first run is archived by now, with the one goal it touched.
        let tasks_json = |run: &str| -> serde_json::Value {
            let text = std::fs::read_to_string(bench.runs_dir().join(run).join("tasks.json")).unwrap();
            serde_json::from_str(&text).unwrap()
        };
        let archived = tasks_json(&first_run);
        assert_eq!(archived["run"], first_run.as_str());
        assert_eq!(archived["tasks"].as_array().unwrap().len(), 1);
        assert_eq!(archived["tasks"][0]["number"], 1);
        assert_eq!(archived["tasks"][0]["chain"][0]["entry"], "opened");
        let history = runs::list(&bench.runs_dir());
        assert_eq!(history.iter().find(|r| r.id == first_run).unwrap().tasks, 1, "History counts from tasks.json");

        let live = task_snapshot(&bench.state, &bench.layout).unwrap();
        assert!(live.live);
        assert_eq!((live.events.len(), live.run.as_deref()), (2, Some(second_run.as_str())));

        // The gate moves to another repo; the next fleet's tasks are that repo's.
        let repo_b = bench.point_at("repo-b");
        bench.fresh();
        assert!(bench.task_list().is_empty(), "another target has its own store");
        let second = tasks_json(&second_run);
        let numbers: Vec<_> = second["tasks"].as_array().unwrap().iter().map(|t| t["number"].clone()).collect();
        assert_eq!(numbers, vec![serde_json::json!(2)], "only what that run touched, not the whole store");

        // With no fleet, the gate target's store is shown read-only and a
        // target that never had one is not given one.
        teardown(&mut bench.state.0.lock().unwrap(), &bench.registry);
        bench.point_at("repo-a");
        let gate = task_snapshot(&bench.state, &bench.layout).unwrap();
        assert!(!gate.live && gate.run.is_none());
        assert_eq!(gate.events.len(), 2, "repo-a's two goals, read without a fleet");
        let repo_c = bench.point_at("repo-c");
        assert!(task_snapshot(&bench.state, &bench.layout).unwrap().events.is_empty());
        assert!(!bench.layout.target_dir(&repo_c).exists(), "reading never creates a store");
        assert!(bench.layout.task_store(&repo_b).is_file());
    }

    /// **A refusal kills nothing** (stories 10, 27): the verdict runs before the
    /// teardown, so the running fleet and its panes survive it.
    #[test]
    fn a_launch_refused_at_the_verdict_leaves_the_running_fleet_untouched() {
        let bench = Bench::new("refused");
        bench.fresh();
        assert_eq!(bench.launchings(), 1, "a first launch, with nothing to tear down, still says so");
        bench.bring_up_orch("m");

        let unknown = bench.reopen("2020-01-01T00-00-00Z").expect_err("no such run");
        assert!(!unknown.torn_down && unknown.reason.contains("no such run"), "{unknown:?}");

        *bench.gate.harnesses.held() = Some(vec![reading(
            "claude-code",
            LoginState::NoCredential { summary: "records no login".into(), provider_key: None },
        )]);
        let logged_out = bench.launch(LaunchSource::Fresh).expect_err("a logged-out fleet may not start");
        assert!(!logged_out.torn_down && logged_out.reason.contains("records no login"), "{logged_out:?}");

        assert!(bench.registry.any_pane(), "the running fleet's pane is still up");
        assert!(bench.placing().is_some(), "and so is the fleet");
        assert!(bench.archives().is_empty(), "nothing was archived for a launch that never started");
        assert_eq!(bench.launchings(), 1, "and the webview was told nothing, so it resets nothing");
    }

    /// **A reopen runs on the target it recorded** (stories 8, 9), whatever the
    /// gate holds, and is refused before teardown once that folder is gone.
    #[test]
    fn a_reopen_uses_its_recorded_target_and_refuses_when_that_folder_is_gone() {
        let bench = Bench::new("target");
        let repo_a = bench.root.join("repo-a");
        bench.fresh();
        bench.bring_up_orch("m");
        let repo_b = bench.point_at("repo-b");
        bench.fresh();
        assert_eq!(bench.placing().unwrap().2, repo_b, "a fresh launch takes the gate's target");
        let on_a = runs::list(&bench.runs_dir())[0].id.clone();

        bench.reopen(&on_a).expect("repo A's session reopens while the gate is on repo B");
        assert_eq!(bench.placing().unwrap().2, repo_a);
        assert_eq!(configured_target(&bench.layout).unwrap(), Some(repo_b.clone()), "config is not written");

        std::fs::remove_dir_all(&repo_a).unwrap();
        let gone = bench.reopen(&on_a).expect_err("its folder is gone");
        assert!(!gone.torn_down && gone.reason.contains("no longer a folder"), "{gone:?}");
        assert_eq!(bench.placing().unwrap().2, repo_a, "the live fleet is the one that was running");
    }

    /// A run archived before targets were recorded reopens on the gate's, and
    /// says so on the feed.
    #[test]
    fn a_reopen_with_no_recorded_target_says_it_took_the_gates() {
        let bench = Bench::new("no-target");
        bench.fresh();
        bench.bring_up_orch("m");
        let repo_b = bench.point_at("repo-b");
        bench.fresh();
        let id = runs::list(&bench.runs_dir())[0].id.clone();
        let manifest_path = bench.runs_dir().join(&id).join("manifest.json");
        let mut manifest: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&manifest_path).unwrap()).unwrap();
        manifest["run"].as_object_mut().unwrap().remove("target");
        std::fs::write(&manifest_path, manifest.to_string()).unwrap();

        bench.reopen(&id).expect("it still reopens");

        assert_eq!(bench.placing().unwrap().2, repo_b);
        let slot = bench.state.0.lock().unwrap();
        let said = slot.as_ref().unwrap().store.events_since(0).unwrap().into_iter().any(|(_, e)| {
            matches!(&e, FleetEvent::Notice { text, .. } if text.contains("recorded no target"))
        });
        assert!(said, "the fallback is on the feed");
    }

    /// **Rapid relaunches each archive cleanly** (story 36): one self-contained
    /// folder per run left, frozen to a single file.
    #[test]
    fn rapid_relaunches_each_leave_one_single_file_archive() {
        let bench = Bench::new("rapid");
        let mut ids = Vec::new();
        for _ in 0..3 {
            bench.fresh();
            let (run, sessions) = bench.ids();
            assert_eq!(run, sessions, "a fresh run's seat directory is its own");
            ids.push(run);
        }
        // Launched within one second, which is one timestamp.
        assert_eq!(ids.iter().collect::<std::collections::BTreeSet<_>>().len(), 3, "{ids:?}");
        let archives = bench.archives();
        assert_eq!(archives.len(), 2, "two runs were left; the third is live");
        for (archive, id) in archives.iter().zip(&ids) {
            assert_eq!(archive.file_name().unwrap().to_string_lossy(), *id, "archived under the id it ran as");
            let manifest = std::fs::read_to_string(archive.join("manifest.json")).unwrap();
            assert!(manifest.contains(&format!("\"sessions\": \"{id}\"")), "{manifest}");
            assert!(!archive.join("state.db-wal").exists(), "{} was not frozen", archive.display());
            assert!(archive.join("manifest.json").is_file(), "{} has no manifest", archive.display());
        }
        assert!(bench.layout.shell().join("state.db").is_file(), "and the live slot holds the third");
    }

    /// **A reopen places what the run recorded** (D-095): harness, model and
    /// credential come from the record; a seat with no record, or on a harness
    /// this build lacks, keeps the gate's pick.
    #[test]
    fn recorded_seats_replace_the_gates_pick() {
        let claude = harness::claude_code().spec().name;
        let codex = crate::placement::codex::codex().spec().name;
        let record = |harness: &str, model: Option<&str>, credential| runs::PaneRecord {
            harness: harness.to_string(),
            model: model.map(str::to_string),
            credential,
            transcript_format: String::new(),
            session_id: None,
        };
        let recorded = std::collections::BTreeMap::from([
            ("orch".to_string(), record(codex, Some("gpt-x"), None)),
            ("worker-1".to_string(), record(codex, None, Some(CredentialChoice::Plan))),
            ("worker-2".to_string(), record("gamma-cli", Some("g"), None)),
        ]);

        let seats = seats_on(claude, claude).as_recorded(&recorded);

        assert_eq!((seats.orch.harness.as_str(), seats.orch.model.as_deref()), (codex, Some("gpt-x")));
        let w1 = &seats.workers[0];
        assert_eq!((w1.harness.as_str(), w1.model.as_deref(), w1.credential), (codex, None, CredentialChoice::Plan));
        for kept in &seats.workers[1..] {
            assert_eq!((kept.harness.as_str(), kept.model.as_deref()), (claude, Some("m")));
            assert_eq!(kept.credential, CredentialChoice::FleetKey);
        }
    }

    /// **Story 11.** A seat on a harness this machine cannot log into stops the
    /// fleet at the gate, rather than spawning a pane onto a login prompt.
    #[test]
    fn a_seat_that_cannot_log_in_refuses_the_start() {
        let seats = seats_on("claude-code", "claude-code");
        let out = vec![reading(
            "claude-code",
            LoginState::NoCredential { summary: "records no login in ~/.claude.json".into(), provider_key: None },
        )];

        let refused = StartVerdict::for_seats(&seats, &out, Vec::new(), Vec::new(), &PlanOffer::fleet_key());
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
            StartVerdict::for_seats(&seats, &in_, Vec::new(), Vec::new(), &PlanOffer::fleet_key())
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
            StartVerdict::for_seats(&seats, &unreadable, Vec::new(), Vec::new(), &PlanOffer::fleet_key())
                .why_it_will_not_start()
                .is_none(),
            "a report this build could not parse must not stop a fleet",
        );
    }

    /// **A seat that cannot start refuses, and the fleet no longer degrades around
    /// it** (C78 — reshaped, not deleted).
    ///
    /// This test used to assert the opposite half: that on a machine with no
    /// worker key the orchestrator ran alone and a worker seat refused nothing,
    /// because a fleet stopped by a seat that was never part of it is a refusal
    /// nobody can act on. That was right while `workers_run` existed. It does not
    /// survive per-seat credentials — a seat is now *always* part of the fleet, so
    /// "was never going to spawn" has no referent, and the refusal it used to
    /// suppress is exactly the one an operator can act on: put that seat on the
    /// other credential, or supply the one it asked for.
    ///
    /// **The property underneath is unchanged and is what this now pins:** a seat
    /// the operator cannot start is named, and no cost sentence promises spend in
    /// a seat that will not run.
    #[test]
    fn a_seat_that_cannot_start_is_named_and_promises_no_spend() {
        let seats = seats_on("claude-code", "codex");
        let readings = vec![
            logged_in("claude-code", AccountShape::ApiKey),
            reading(
                "codex",
                LoginState::NoCredential {
                    summary: "no Codex credentials".into(),
                    provider_key: None,
                },
            ),
        ];

        let verdict = StartVerdict::for_seats(&seats, &readings, Vec::new(), Vec::new(), &PlanOffer::fleet_key());
        let why = verdict.why_it_will_not_start().expect("a logged-out worker harness refuses");
        assert!(why.contains("worker 1"), "the refusal does not name the seat: {why}");

        // **A seat that can run on the other credential is moved, not refused**
        // (C79). This is the case C78 got wrong: the default is the plan on every
        // seat, so a machine where one harness has no login met a dead gate on a
        // fleet the operator had not configured at all.
        let plan_only = PlanOffer::new(vec!["claude-code".into()], false);
        let on_the_key = FleetSeats {
            orch: seats.orch.clone(),
            workers: seats
                .workers
                .iter()
                .map(|_| SeatChoice {
                    harness: "claude-code".to_string(),
                    model: Some("deepseek-v4-flash".into()),
                    credential: CredentialChoice::FleetKey,
                })
                .collect(),
        };
        let (settled, moved) = settle_credentials(on_the_key.clone(), &plan_only);
        assert_eq!(moved.len(), 4, "a seat that could run on the plan was not moved to it");
        assert!(
            settled.workers.iter().all(|s| s.credential == CredentialChoice::Plan),
            "the seats were reported as moved and not actually moved",
        );
        assert!(
            settled.workers.iter().all(|s| s.model.is_none()),
            "a moved seat kept `deepseek-v4-flash`, which its new credential's vendor has \
             never heard of",
        );
        let ran = StartVerdict::for_seats(
            &settled,
            &[logged_in("claude-code", AccountShape::SubscriptionPlan { plan: Some("Max".into()) })],
            Vec::new(),
            moved,
            &plan_only,
        );
        assert!(
            ran.why_it_will_not_start().is_none(),
            "a fleet every seat of which can run was still refused: {:?}",
            ran.refusals,
        );

        // **Neither credential available is the only refusal left** (C79).
        let nothing = PlanOffer::new(Vec::new(), false);
        let (unmoved, moved) = settle_credentials(on_the_key, &nothing);
        assert!(moved.is_empty(), "a seat was moved to a credential that is also missing");
        let refused = StartVerdict::for_seats(
            &unmoved,
            &[logged_in("claude-code", AccountShape::ApiKey)],
            Vec::new(),
            moved,
            &nothing,
        );
        let why = refused.why_it_will_not_start().expect("a seat with neither credential refuses");
        assert!(
            why.contains("neither credential available"),
            "the refusal does not say both are missing, so an operator fixes one and meets \
             the same wall: {why}",
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
        let verdict = StartVerdict::for_seats(&seats, &readings, Vec::new(), Vec::new(), &PlanOffer::fleet_key());

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
            Vec::new(),
            Vec::new(),
            &PlanOffer::fleet_key(),
        );
        assert_eq!(same.cost.len(), 2, "one orchestrator line and one worker line: {:?}", same.cost);
        assert!(same.cost[1].seats.contains("all 4 worker seats"), "{:?}", same.cost[1]);

        let mut mixed = seats_on("claude-code", "claude-code");
        mixed.workers[1].harness = "codex".to_string();
        let split = StartVerdict::for_seats(&mixed, &readings, Vec::new(), Vec::new(), &PlanOffer::fleet_key());
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

    // --- the posture tripwire (WP-25 #37; C8) ---------------------------------

    /// The posture a harness's own spec says the vendor must be seen to resolve.
    ///
    /// **Built from the spec rather than typed**, so this helper cannot drift from
    /// the thing it is describing: if `verified_as` gains a row, the `_` arm panics
    /// here rather than the test quietly asserting about two rows out of four.
    fn as_the_spec_expects(name: &str) -> harness::ResolvedPosture {
        let spec = spec_of(name);
        let mut posture = harness::ResolvedPosture {
            schema: spec.posture.verified_against_schema.map(str::to_string),
            ..Default::default()
        };
        for expectation in spec.posture.verified_as {
            let word = Some(expectation.resolved.to_string());
            match expectation.row {
                "filesystem" => posture.filesystem = word,
                "network" => posture.network = word,
                "approval" => posture.approval = word,
                other => panic!("#37's tripwire grew a `{other}` row this helper cannot set"),
            }
        }
        posture
    }

    /// A codex reading whose vendor resolved exactly what FLEETOR writes.
    fn resolving_what_fleetor_writes() -> HarnessReadiness {
        HarnessReadiness {
            posture: as_the_spec_expects("codex"),
            ..logged_in("codex", AccountShape::SubscriptionPlan { plan: None })
        }
    }

    /// **#37, and the property #35 could not see.** A vendor that resolved a
    /// containment other than the one FLEETOR wrote stops the fleet *at the gate*,
    /// naming the key — rather than spawning five panes that come up looking healthy
    /// and cannot reach the socket.
    ///
    /// **This is the test the ticket exists for.** A source tripwire proves the code
    /// says the right thing, not that it runs: #35's fifteen checks all passed on a
    /// screen that could never load. So every assertion below runs the real
    /// `StartVerdict::for_seats` against a machine a value *describes*, and the first
    /// thing it establishes is the negative control — that a machine whose vendor
    /// agrees starts. A tripwire that fires on everything is worse than none, because
    /// it reads as coverage right up until somebody deletes the rule it was guarding.
    #[test]
    fn a_containment_the_vendor_did_not_resolve_refuses_the_start() {
        let seats = seats_on("codex", "codex");

        // The negative control, first and deliberately: agreement starts a fleet.
        let agrees = vec![resolving_what_fleetor_writes()];
        assert_eq!(
            StartVerdict::for_seats(&seats, &agrees, Vec::new(), Vec::new(), &PlanOffer::fleet_key()).why_it_will_not_start(),
            None,
            "a vendor that resolved what FLEETOR wrote must not be refused",
        );

        // **One flip per key, and each must name its own.** The words below are the
        // spec's own, negated — never respelled — so this stays a test of the
        // comparison rather than of a string typed twice.
        for expectation in spec_of("codex").posture.verified_as {
            let mut broken = resolving_what_fleetor_writes();
            let elsewhere = Some(format!("not-{}", expectation.resolved));
            match expectation.row {
                "filesystem" => broken.posture.filesystem = elsewhere,
                "network" => broken.posture.network = elsewhere,
                "approval" => broken.posture.approval = elsewhere,
                other => panic!("#37's tripwire grew a `{other}` row this test cannot flip"),
            }

            let refused = StartVerdict::for_seats(&seats, &[broken], Vec::new(), Vec::new(), &PlanOffer::fleet_key());
            let why = refused
                .why_it_will_not_start()
                .unwrap_or_else(|| panic!("`{}` disagreeing must stop the fleet", expectation.row));
            assert!(
                why.contains(expectation.written_key),
                "the refusal has to name which key disagreed (criterion 4), not merely \
                 that one did: {why}",
            );
            assert!(
                why.contains(expectation.resolved) && why.contains(&format!("not-{}", expectation.resolved)),
                "and it has to say both what was expected and what the vendor said: {why}",
            );
            assert_eq!(
                refused.refusals.len(),
                1 + fleetor_core::pane::WORKER_SLOTS.len(),
                "every seat on the harness is named, exactly once — three broken keys \
                 must not become fifteen sentences nobody reads to the end of",
            );
        }

        // **A row the vendor stopped reporting is the loudest case**, because that is
        // what a retired key looks like: measured on `0.153.4`, an unknown `-c` key
        // leaves `doctor` emitting an ordinary report with the override silently
        // dropped. Nothing else in the reading changes.
        let mut retired = resolving_what_fleetor_writes();
        retired.posture.network = None;
        let why = StartVerdict::for_seats(&seats, &[retired], Vec::new(), Vec::new(), &PlanOffer::fleet_key())
            .why_it_will_not_start()
            .expect("a row the vendor no longer reports must stop the fleet");
        assert!(
            why.contains("sandbox_workspace_write.network_access") && why.contains("no such row"),
            "a retired key is named and said to be missing rather than wrong: {why}",
        );

        // **Criterion 3.** A report shape this arc has not measured stops the fleet
        // too — and produces *one* sentence naming `schemaVersion`, not three about
        // keys whose readings came out of a document this build cannot read.
        let mut moved = resolving_what_fleetor_writes();
        moved.posture.schema = Some("2".to_string());
        let refused = StartVerdict::for_seats(&seats, &[moved], Vec::new(), Vec::new(), &PlanOffer::fleet_key());
        let why = refused.why_it_will_not_start().expect("a schema this arc does not understand");
        assert!(why.contains("schemaVersion"), "the schema refusal names the stamp: {why}");
        for expectation in spec_of("codex").posture.verified_as {
            assert!(
                !why.contains(expectation.written_key),
                "a schema change must not also report three key disagreements — the one \
                 fact that explains all of them would be buried: {why}",
            );
        }

        // **The three states that must *not* be refused** (C14). `Unreadable` is a
        // working installation this build could not parse and `NotInstalled` has no
        // posture at all; refusing either here would stop a fleet on the strength of
        // something that is not evidence, which is the failure C14's narrowness
        // exists to avoid. `NoCredential` is already refused by `refusal_for`, and a
        // second sentence about a fleet that is stopped tells nobody anything.
        for quiet in [
            LoginState::Unreadable { why: "an unknown report format".into() },
            LoginState::NotInstalled,
        ] {
            let reading = reading("codex", quiet);
            assert!(
                posture_refusal(Some(&reading)).is_none(),
                "the tripwire may only speak about a machine that answered: {:?}",
                reading.login,
            );
        }

        // And a harness with nothing to read back says nothing, which is not a pass.
        assert!(
            posture_refusal(Some(&logged_in("claude-code", AccountShape::ApiKey))).is_none(),
            "a harness that publishes no resolved posture has nothing to disagree about",
        );
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
