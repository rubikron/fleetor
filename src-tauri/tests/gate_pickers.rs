//! **The start gate's pickers, asserted where they are declared** (WP-25 #35;
//! M1, M2, M3, M15, M16, C15, C23, C24, C58).
//!
//! #35 puts a harness and a model on every seat the gate offers a choice for, and
//! almost everything that can go wrong with it goes wrong *silently*. A harness
//! filtered out of a list is a supported feature that looks unimplemented. A status
//! word added on one side of the wire and not the other renders as nothing at all. A
//! re-check button wired to a cached read looks exactly like one that worked. A
//! vendor name spelled in the interface makes the third harness invisible on the day
//! it is registered. None of those fail a compile and none of them fail a run.
//!
//! ## Why a Rust test for a TypeScript property
//!
//! The same move `tests/views.rs` makes, for the same stated reason: the frontend has
//! no test runner, and this arc deliberately does not add one (C24 lists a frontend
//! test runner as out of scope, and introducing one as a side effect of adding two
//! dropdowns is how a small package becomes the polluted one). So this reads source.
//! `tests/placement_reads_nothing.rs` is the other current example of the style.
//!
//! It reads the *declarations*, not the behaviour. It cannot tell you that choosing
//! codex spawns codex — the conformance suite is what observes a placement end to
//! end. It tells you that the two halves of each seam still name the same things,
//! which is the way this class of interface has actually gone wrong.
//!
//! ## The negative control
//!
//! Every check below is a function over a `&str`, and
//! [`the_checks_fire_on_a_source_that_violates_them`] runs each one against a
//! synthetic source that breaks it. A tripwire nobody has watched fire is a tripwire
//! that might be asserting `true`; `placement_reads_nothing.rs` guards the same worry
//! from the other end, by requiring each of its needles to still match something.
//! This file does both.

use std::path::{Path, PathBuf};

/// The interface's own declarations, which are what this file reads.
const GATE: &str = "ui/src/components/StartGate.tsx";
const PICKERS: &str = "ui/src/ui/useSeatPickers.ts";

/// **The pane chrome** — the operator's rail, added by #50.
///
/// #35's vendor-name check was pointed at the gate's own two files, and caught
/// nothing when `TerminalGrid.tsx` shipped `orchestrator · claude` on every pane
/// head of a mixed fleet. The check was right and was simply not pointed here. The
/// harness, its mark and its model all arrive on the spawn event, so these three
/// files are under exactly the rule the gate's two are.
const CHROME: [&str; 3] = [
    "ui/src/components/TerminalGrid.tsx",
    "ui/src/components/TerminalPane.tsx",
    "ui/src/components/PaneHead.tsx",
];
const WIRE: &str = "ui/src/fleet/types.ts";
const API: &str = "ui/src/fleet/api.ts";

/// The backend halves each of those must agree with.
const BACKEND: &str = "src-tauri/src/fleet.rs";
const COMMANDS: &str = "src-tauri/src/lib.rs";
const HARNESS: &str = "src-tauri/src/placement/harness.rs";
const CODEX: &str = "src-tauri/src/placement/codex.rs";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("src-tauri has a parent").to_path_buf()
}

fn read(rel: &str) -> String {
    let file = repo_root().join(rel);
    std::fs::read_to_string(&file)
        .unwrap_or_else(|e| panic!("{} must exist to be checked: {e}", file.display()))
}

/// `text` with its comments removed.
///
/// **Comments have to be able to name their own subject**, which is the exemption
/// `placement_reads_nothing.rs` states for the same reason: the prose explaining why
/// a vendor's name may not be spelled here has to be able to spell it. Block
/// comments go first, so a `{/* ... */}` in JSX is one removal and not two.
fn production_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find("/*") {
        out.push_str(&rest[..open]);
        let after = &rest[open + 2..];
        match after.find("*/") {
            Some(close) => rest = &after[close + 2..],
            None => {
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out.lines()
        .map(|line| match line.find("//") {
            // `https://` inside a string is not a comment, and neither is a `//`
            // that follows a colon for the same reason.
            Some(at) if !line[..at].ends_with(':') => &line[..at],
            _ => line,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Every double-quoted literal in `text`, in order.
fn quoted(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('"') {
        rest = &rest[open + 1..];
        let Some(close) = rest.find('"') else { break };
        out.push(rest[..close].to_string());
        rest = &rest[close + 1..];
    }
    out
}

/// The section of `text` between `after` and the next `until`, panicking with the
/// declaration's name if the shape it depends on is gone — a renamed declaration
/// must fail this test rather than quietly match nothing and pass.
fn between(text: &str, after: &str, until: &str, what: &str) -> String {
    let tail = text
        .split(after)
        .nth(1)
        .unwrap_or_else(|| panic!("{what} no longer starts with `{after}` — this file reads it"));
    tail.split(until)
        .next()
        .unwrap_or_else(|| panic!("{what} has no closing `{until}`"))
        .to_string()
}

// --- the seam's two lists -----------------------------------------------------

/// The four status words as `HarnessOffer::from` decides them, in Rust.
fn statuses_the_backend_sends(backend: &str) -> Vec<String> {
    let body = between(backend, "match &readiness.login {", "\n        };", "HarnessOffer::from");
    // The first literal in each `=>` arm is the status word; the rest of an arm is a
    // reason built from the vendor's own sentence, which is not a fixed spelling.
    body.split("=> (").skip(1).filter_map(|arm| quoted(arm).into_iter().next()).collect()
}

/// The four status words the interface switches on, in TypeScript.
fn statuses_the_interface_reads(wire: &str) -> Vec<String> {
    quoted(&between(wire, "export type HarnessStatus =", ";", "HarnessStatus"))
}

/// **The status words agree across the wire.**
///
/// `views.rs`'s property, one seam over. A fifth state added to `LoginState` and
/// mapped in `HarnessOffer::from` but not added to `HarnessStatus` type-checks on
/// both sides and renders as an option with no label and no reason.
#[test]
fn the_status_words_are_the_same_four_on_both_sides() {
    let backend = statuses_the_backend_sends(&read(BACKEND));
    let interface = statuses_the_interface_reads(&read(WIRE));
    assert_eq!(
        backend.len(),
        4,
        "`HarnessOffer::from` no longer maps four login states; this file reads that match",
    );
    let mut sent = backend.clone();
    let mut read_back = interface.clone();
    sent.sort();
    read_back.sort();
    assert_eq!(
        sent, read_back,
        "the backend sends {backend:?} and `HarnessStatus` names {interface:?}. A status word \
         on one side and not the other is an option that renders with no label and no reason \
         — the same silent failure a view missing from the restore list had.",
    );
}

/// **Every status word has an arm in the label the operator reads.**
///
/// A status with no arm falls through to nothing, which is a harness in the list
/// with a blank name.
#[test]
fn every_status_word_is_labelled_for_the_operator() {
    let gate = read(GATE);
    let labels = between(&gate, "function optionLabel(", "\n}", "optionLabel");
    for status in statuses_the_interface_reads(&read(WIRE)) {
        assert!(
            labels.contains(&format!("\"{status}\"")),
            "`optionLabel` has no arm for `{status}`, so a harness in that state renders \
             with no label at all:\n{labels}",
        );
    }
}

// --- never hidden -------------------------------------------------------------

/// The harness `<select>` as the gate declares it — from the list it maps to the
/// end of the element.
fn the_harness_picker(gate: &str) -> String {
    between(gate, "gate.harnesses", "</select>", "the harness picker")
}

/// Whether a harness list is narrowed before it is offered.
fn picker_hides_a_harness(picker: &str) -> bool {
    picker.contains(".filter(")
}

/// **The list is never filtered, and what cannot run is disabled with its reason.**
///
/// User story 9's rule at the gate: a supported feature must never look
/// unimplemented. A harness dropped from the list because this machine cannot run it
/// is indistinguishable, to the operator, from a harness FLEETOR does not support.
#[test]
fn no_harness_is_hidden_and_a_refused_one_carries_its_reason() {
    let gate = read(GATE);
    let picker = the_harness_picker(&production_text(&gate));
    assert!(
        !picker_hides_a_harness(&picker),
        "the harness list is filtered before it is offered. A harness this machine cannot \
         run must appear disabled with its reason, never be dropped:\n{picker}",
    );
    assert!(
        picker.contains("disabled={!canTakeASeat(harness)}"),
        "the harness options no longer disable what cannot take a seat:\n{picker}",
    );
    assert!(
        picker.contains("harness.reason"),
        "a disabled harness option no longer carries the vendor's reason:\n{picker}",
    );
    let refusals = quoted(&between(
        &read(WIRE),
        "export const CANNOT_TAKE_A_SEAT: readonly HarnessStatus[] =",
        ";",
        "CANNOT_TAKE_A_SEAT",
    ));
    assert_eq!(
        refusals,
        vec!["no-credential".to_string(), "not-installed".to_string()],
        "the two refusals changed. `unreadable` is deliberately not one of them (C14): a \
         vendor that changed its report format is a working installation the gate could not \
         parse, and disabling a seat on the strength of a parse error is what the narrow \
         refusal exists to avoid.",
    );
}

// --- the re-check button ------------------------------------------------------

/// **The re-check button reaches a fresh probe**, link by link.
///
/// The failure it guards is the worst-looking-good one on this card: a control the
/// operator presses after logging in from another terminal, which re-renders a held
/// reading and reports the same refusal. It looks exactly like a button that worked.
#[test]
fn the_recheck_button_forces_a_probe_rather_than_re_rendering_one() {
    let gate = read(GATE);
    assert!(
        gate.contains("onClick={pickers.recheck}"),
        "the gate no longer has a control wired to `recheck`",
    );
    let pickers = read(PICKERS);
    let recheck = between(&pickers, "const recheck = useCallback(", "}, []);", "recheck");
    assert!(
        recheck.contains("fetchGate(true)"),
        "`recheck` no longer asks for a fresh reading — `fetchGate()` without the argument \
         answers from whatever the backend already holds:\n{recheck}",
    );
    let api = read(API);
    assert!(
        api.contains("invoke<GateState>(\"fleet_gate\", { refresh })"),
        "`fetchGate` no longer passes `refresh` through to the backend",
    );
    let backend = read(BACKEND);
    assert!(
        backend.contains("if refresh { gate.harnesses.refresh() } else { gate.harnesses.read() }"),
        "`fleet_gate` no longer branches on `refresh`, so the button and the render make the \
         same request",
    );
    let refresh = between(&backend, "fn refresh(&self) -> HarnessReading {", "\n    }", "refresh");
    assert!(
        refresh.contains("probe_the_harnesses()"),
        "`HarnessGate::refresh` stopped probing, so the re-check button now re-renders a \
         held answer:\n{refresh}",
    );
}

// --- one vendor, or any vendor ------------------------------------------------

/// Every word a registered harness spells itself with — its name, and the program
/// it is invoked as.
///
/// **The bin is here because the name alone would not have caught the bug #50
/// fixed.** The spec is named `claude-code` and the pane head said `claude`, which
/// is checkpoint 1's `Program::bin` — a vendor's name by any reading, and not a
/// substring of the spec's. A check that knew only the spec name would have gone on
/// passing over the literal it was written to find.
fn registered_vendor_words() -> Vec<String> {
    let mut words = registered_names();
    let harness = read(HARNESS);
    let codex = read(CODEX);
    for (text, spec, what) in [
        (&harness, "pub const CLAUDE_CODE_SPEC: HarnessSpec = HarnessSpec {", "CLAUDE_CODE_SPEC"),
        (&codex, "pub const CODEX_SPEC: HarnessSpec = HarnessSpec {", "CODEX_SPEC"),
    ] {
        let tail = text
            .split(spec)
            .nth(1)
            .unwrap_or_else(|| panic!("{what} is gone — this file reads it"))
            .to_string();
        let bin_body = between(&tail, "bin:", ",", &format!("{what}'s `Program::bin`"));
        let bin = quoted(&bin_body).into_iter().next().unwrap_or_else(|| {
            panic!("{what}'s `Program::bin` is no longer a literal — this file reads it")
        });
        if !words.contains(&bin) {
            words.push(bin);
        }
    }
    words
}

/// Every registered harness's name, read out of the spec that declares it.
fn registered_names() -> Vec<String> {
    let harness = read(HARNESS);
    let codex = read(CODEX);
    let mut names = Vec::new();
    for (text, spec, what) in [
        (&harness, "pub const CLAUDE_CODE_SPEC: HarnessSpec = HarnessSpec {", "CLAUDE_CODE_SPEC"),
        (&codex, "pub const CODEX_SPEC: HarnessSpec = HarnessSpec {", "CODEX_SPEC"),
    ] {
        let body = between(text, spec, "program:", what);
        let name = quoted(&body).into_iter().next().unwrap_or_else(|| {
            panic!("{what} no longer opens with its `name:` field — this file reads it")
        });
        names.push(name);
    }
    names
}

/// `word` with its first character upper-cased — the other spelling of the literal
/// #50 removed, used by the negative control below.
fn upper(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// Where a vendor's own name is spelled in a file that must not know one.
///
/// Case-insensitively, since #50: a head reading `· Claude` is the same wrong
/// sentence as one reading `· claude`, and an exact-case check would have let the
/// capitalised half of the bug through.
fn vendor_names_spelled_in(source: &str, names: &[String]) -> Vec<String> {
    let lowered = source.to_lowercase();
    names.iter().filter(|name| lowered.contains(&name.to_lowercase())).cloned().collect()
}

/// **The interface knows no vendor's name.**
///
/// The list of harnesses, their labels, their models and their reasons all arrive on
/// the wire. A literal here is the drift that makes a third harness invisible on the
/// day it is registered — the gate would offer it and one branch somewhere would
/// still be asking whether the name is the one it was written against. #33 found six
/// checkpoints shaped around one vendor and #34 named two more; this is the check
/// that stops the gate becoming the ninth.
#[test]
fn the_gate_spells_no_vendors_name() {
    let names = registered_vendor_words();
    for file in [GATE, PICKERS] {
        let found = vendor_names_spelled_in(&production_text(&read(file)), &names);
        assert!(
            found.is_empty(),
            "{file} spells {found:?}. Every harness's name, label, model list and reason \
             arrives on the wire from `HarnessOffer`; a literal here is a third harness \
             that is offered and then handled by a branch written for the first two.",
        );
    }
}

/// **The pane chrome knows no vendor's name either** (#50, closing C56).
///
/// The same rule as the gate's, pointed at the operator's rail — which is where it
/// was needed and was not applied. `TerminalGrid.tsx` labelled every pane
/// `orchestrator · claude` and `worker-N · claude`, so a codex worker announced the
/// wrong vendor beside the right model, and the archive of that run knew what it
/// ran while the live screen did not. The check above passed throughout, because its
/// file list was the gate's.
///
/// **The harness, its mark and its model now all arrive on the spawn event**
/// (`FleetEvent::PaneState`, M24), so there is nothing left here for a literal to
/// stand in for. A per-harness mark chosen with `harness === "…" ? … : …` would fail
/// this test by construction, which is the point: registering a third harness must
/// not require an edit under `ui/`.
#[test]
fn the_pane_chrome_spells_no_vendors_name() {
    let names = registered_vendor_words();
    for file in CHROME {
        let found = vendor_names_spelled_in(&production_text(&read(file)), &names);
        assert!(
            found.is_empty(),
            "{file} spells {found:?}. This is the bug #50 fixed: the pane head stated a \
             vendor the pane was not running, which is worse than stating none. The harness, \
             its mark and its model all arrive on `FleetEvent::PaneState` — read them, and \
             never name one here.",
        );
    }
}

// --- what is not offered ------------------------------------------------------

/// **No provider picker, anywhere** (C2 as amended by C9, C24).
///
/// The orchestrator inherits its provider and the gate displays it; a worker gets
/// FLEETOR's. Per-seat provider choice is on WP-25's out-of-scope list, and per-seat
/// providers mean per-seat credentials.
#[test]
fn the_gate_offers_no_provider_picker() {
    let gate = production_text(&read(GATE));
    let facts = between(&gate, "function factsFor(", "\n}", "factsFor");
    assert!(facts.contains("provider"), "the provider stopped being displayed as a fact");
    // A *control* is what is refused, not the word: the provider is displayed, so it
    // has to be nameable. A line that mentions it and is part of an input is the
    // thing that must not exist.
    let controls: Vec<&str> = gate
        .lines()
        .filter(|line| line.contains("provider"))
        .filter(|line| {
            ["<select", "<option", "<input", "onChange", "onClick", "datalist"]
                .iter()
                .any(|control| line.contains(control))
        })
        .collect();
    assert!(
        controls.is_empty(),
        "the provider is wired to a control. It is a fact beside the model and never a \
         choice — the orchestrator inherits it and a worker gets FLEETOR's (C2, C9, \
         C24):\n{controls:#?}",
    );
    for spelling in ["chooseProvider", "setProvider", "chooseWorkersProvider"] {
        assert!(
            !gate.contains(spelling) && !read(PICKERS).contains(spelling),
            "`{spelling}` appeared. There is no provider picker anywhere.",
        );
    }
}

/// **No harness for the judges** (C15).
///
/// #33 made it structural in `PaneSpec`; #35 keeps it structural on the wire and in
/// the interface. The seats the gate offers a choice for are the orchestrator and
/// the four workers, and there is no key for anything else.
#[test]
fn the_judges_are_offered_no_harness() {
    let pickers = read(PICKERS);
    let declared = "export const PICKABLE_SEATS: readonly PaneId[] =";
    let seats = between(&pickers, declared, ";", "PICKABLE_SEATS");
    assert!(
        seats.contains("ORCH") && seats.contains("workerPane"),
        "`PICKABLE_SEATS` no longer names the orchestrator and the workers:\n{seats}",
    );
    for judge in ["EVALUATOR", "CRITIC"] {
        assert!(
            !pickers.contains(judge) && !read(GATE).contains(judge),
            "{judge} appears in the gate's pickers. Both judges judge the fleet's work, and a \
             judge on the same harness as the judged is a variable this arc does not \
             introduce (C15).",
        );
    }
    let seats_type = between(&read(WIRE), "export interface FleetSeats {", "}", "FleetSeats");
    assert!(
        !seats_type.contains("evaluator") && !seats_type.contains("critic"),
        "`FleetSeats` grew a judge's seat:\n{seats_type}",
    );
}

/// **No plan tier is promised** (C14 as narrowed by C58).
///
/// `doctor` reports the account *shape*; the tier lives inside the stored id token
/// and reading it means parsing a credential file this arc reaches only through the
/// vendor. So the wire carries a shape and has nowhere to put a tier.
#[test]
fn the_gate_promises_no_plan_tier() {
    let offer = between(&read(BACKEND), "pub struct HarnessOffer {", "\n}", "HarnessOffer");
    let fields: Vec<&str> = offer
        .lines()
        .map(str::trim)
        .filter(|line| !line.starts_with("///") && line.ends_with(','))
        .collect();
    for field in &fields {
        assert!(
            !field.starts_with("plan") && !field.starts_with("tier"),
            "`HarnessOffer` grew `{field}`. The gate names the account shape and does not \
             promise a tier — C14 said \"ChatGPT <plan>\" and #34 measured that the vendor \
             does not report one.",
        );
    }
    assert!(
        fields.iter().any(|field| field.starts_with("account")),
        "`HarnessOffer` no longer carries the account shape at all:\n{offer}",
    );
}

// --- the summary and the fleet that spawns ------------------------------------

/// **The gate's pickers write what the spawn path reads** (M15).
///
/// The chain has three links and no branch: the pickers write `FleetSeats` through
/// `fleet_set_seats`, `spawn_pane` places `seats.spec_for(pane)`, and the summary the
/// operator reads renders from the same value. The failure it refuses is the card
/// promising a fleet that is not the one that spawns — which is what the hard-coded
/// call site #33 left behind would now be.
#[test]
fn the_seats_the_gate_writes_are_the_seats_that_spawn() {
    let backend = read(BACKEND);
    assert!(
        backend.contains("gate.store_seats(picked);"),
        "`fleet_set_seats` no longer stores what the operator picked",
    );
    let spawn = between(
        &backend,
        "pub(crate) fn spawn_pane(",
        "\n    let host = Host::discover();",
        "spawn_pane",
    );
    assert!(
        spawn.contains("seats.spec_for(pane)"),
        "`spawn_pane` no longer places against the gate's selection:\n{spawn}",
    );
    assert!(
        !spawn.contains("harness::claude_code()"),
        "`spawn_pane` names a harness of its own again. Every seat the gate offers a choice \
         for must be placed on the choice, or the card is describing a fleet that will not \
         spawn (M15):\n{spawn}",
    );
    let gate = read(GATE);
    assert!(
        gate.contains("gate.seats.orch") && gate.contains("gate.seats.workers"),
        "the gate's rows stopped rendering from the stored selection",
    );
    assert!(
        !gate.contains("config?.lead_model") && !gate.contains("config.lead_model"),
        "the gate reads `lead_model` again — a second source for the orchestrator's model, \
         beside the one the pickers write. That is exactly the two-sources shape M15 refuses.",
    );
}

/// **The sentinel survives, and it is an absent model rather than a name** (M2).
///
/// `default (your login)` is what keeps today's behaviour reachable now that the
/// orchestrator seat can name a model: the operator's own login, running whatever
/// their account defaults to, with nothing on the argv and nothing in the
/// environment.
#[test]
fn the_orchestrator_keeps_its_default_your_login_sentinel() {
    let wire = read(WIRE);
    assert!(
        wire.contains("export const DEFAULT_YOUR_LOGIN = \"default (your login)\""),
        "the sentinel's label changed or went away (M2)",
    );
    assert!(
        read(GATE).contains("sentinel={DEFAULT_YOUR_LOGIN}"),
        "the orchestrator row no longer offers the sentinel",
    );
    let default = between(
        &read(PICKERS),
        "function seatDefault(seat: PaneId, workerModelDefault: string): string | null {",
        "\n}",
        "seatDefault",
    );
    assert!(
        default.contains("seat === ORCH ? null"),
        "the orchestrator's sentinel stopped being an absent model. A named default would \
         put a model on the operator's own seat that they never chose:\n{default}",
    );
    let spawn = read("src-tauri/src/placement/spawn.rs");
    let orch = between(&spawn, "pub(super) fn orch_command_with(", "\n}", "orch_command_with");
    assert!(
        orch.contains("None => seat,"),
        "the orchestrator's command no longer has a no-model branch, so the sentinel now \
         names something:\n{orch}",
    );
}

/// **What a remembered model is, said on the card** (M16).
///
/// Two things the interface must not paper over: the remembered choice goes stale
/// the moment somebody uses the harness's own model command, and it is remembering
/// what was last *launched* rather than what a pane is *running*.
#[test]
fn the_card_says_what_a_remembered_model_is_and_is_not() {
    let gate = read(GATE);
    let said = between(&gate, "const WHAT_A_REMEMBERED_MODEL_IS =", ";", "the staleness note");
    for clause in ["starts with", "harness's own command", "last launched", "is running"] {
        assert!(
            said.contains(clause),
            "the note dropped `{clause}`. Both halves of M16 have to survive an edit: what \
             FLEETOR decides, and what the remembered value is not.\n{said}",
        );
    }
    assert!(
        gate.contains("{WHAT_A_REMEMBERED_MODEL_IS}"),
        "the note is declared and no longer rendered",
    );
}

/// **The caveat reaches the operator, not only the feed** (#34, M17).
///
/// A passing check proves a credential resolved and its provider answered — not that
/// the provider accepted it. #34 put the sentence on the Activity feed; the gate must
/// not silently contradict it with an unqualified green.
#[test]
fn a_logged_in_harness_still_carries_what_the_check_did_not_prove() {
    let gate = read(GATE);
    assert!(
        gate.contains("offer.caveat"),
        "the gate stopped rendering the caveat, so a row that says `logged in` now says it \
         without qualification",
    );
    let harness = read(HARNESS);
    assert!(
        harness.contains("pub const REACHABILITY_NOT_AUTHORIZATION"),
        "the sentence the gate renders is no longer declared where it was",
    );
    assert!(
        !gate.contains("revoked"),
        "the caveat is spelled in the interface as well as in Rust. It travels on \
         `HarnessOffer::caveat` so there is one wording, in one place.",
    );
}

// --- the commands exist -------------------------------------------------------

/// **Every command the interface invokes is registered.**
///
/// An unregistered command rejects at runtime with a message about an unknown
/// command, in a promise nothing on the gate awaits loudly. Cheap to check, and it
/// covers every command rather than only this ticket's two.
#[test]
fn every_invoked_command_is_registered() {
    let api = read(API);
    let handlers = read(COMMANDS);
    // `invoke<T>("name", …)` and `invoke("name", …)`, and neither the import above
    // them nor the `listen` channel names beside them.
    let mut invoked: Vec<String> = Vec::new();
    for piece in api.split("invoke").skip(1) {
        let call = match piece.chars().next() {
            Some('<') => piece.split_once('(').map(|(_, rest)| rest),
            Some('(') => Some(&piece[1..]),
            _ => None,
        };
        let Some(call) = call else { continue };
        let Some(name) = quoted(call).into_iter().next() else { continue };
        if name.chars().all(|c| c.is_ascii_lowercase() || c == '_') {
            invoked.push(name);
        }
    }
    assert!(invoked.len() >= 8, "the invoke scan found {} commands, which is too few to be \
         reading `api.ts` correctly: {invoked:?}", invoked.len());
    for command in &invoked {
        assert!(
            handlers.contains(&format!("::{command},")),
            "`{command}` is invoked from the interface and is not in `lib.rs`'s handler list. \
             It rejects at runtime with an unknown-command error, inside a promise.",
        );
    }
    for command in ["fleet_gate", "fleet_set_seats"] {
        assert!(
            invoked.contains(&command.to_string()),
            "`{command}` is no longer invoked from `api.ts` — this ticket's two commands are \
             what the gate is made of",
        );
    }
}

// --- the negative control -----------------------------------------------------

/// **Each check fires on a source that violates it.**
///
/// A tripwire nobody has watched trip might be asserting `true`. Each function below
/// is run against a synthetic source that breaks exactly the property the test above
/// asserts, and must report the violation.
///
/// It is the counterpart of `placement_reads_nothing.rs`'s
/// `the_needles_still_describe_a_process_read`, which guards the other end: that the
/// spellings this kind of file greps for still match something real.
#[test]
fn the_checks_fire_on_a_source_that_violates_them() {
    // A harness list that drops what this machine cannot run, rather than disabling
    // it — user story 9's failure, written out.
    let filtered = "{gate.harnesses.filter(canTakeASeat).map((h) => (<option/>))}</select>";
    assert!(
        picker_hides_a_harness(&the_harness_picker(filtered)),
        "the hidden-harness check would not notice a filtered list",
    );
    assert!(
        !picker_hides_a_harness(&the_harness_picker(&production_text(&read(GATE)))),
        "the hidden-harness check reports the gate itself, so it says nothing about a gate \
         that does hide one",
    );

    // A comment naming a vendor is not a vendor spelled in the interface.
    let names = registered_names();
    let commented = format!("// a pane switched to {} and back\nreturn offer.name;", names[1]);
    assert!(
        vendor_names_spelled_in(&production_text(&commented), &names).is_empty(),
        "the vendor-name check reads comments, which have to be able to name their subject",
    );

    // A vendor's name spelled in the interface.
    let leaked = format!("if (harness.name === \"{}\") return \"the first one\";", names[0]);
    assert_eq!(
        vendor_names_spelled_in(&leaked, &names),
        vec![names[0].clone()],
        "the vendor-name check would not notice a harness named in the interface",
    );
    assert!(
        vendor_names_spelled_in("if (canTakeASeat(harness)) offer(harness);", &names).is_empty(),
        "the vendor-name check reports a file that names no vendor, so it says nothing about \
         the ones that do",
    );

    // #50's literal, in the shape it actually shipped in: the *program* name, in a
    // pane label, in whichever case somebody typed it. The spec-name list alone
    // would not contain it, which is why `registered_vendor_words` reads the bin.
    let words = registered_vendor_words();
    let bins: Vec<&String> = words.iter().filter(|w| !names.contains(w)).collect();
    assert!(
        !bins.is_empty(),
        "no registered harness's `Program::bin` differs from its spec name, so the pane-chrome \
         check is only reading the names again — the literal #50 removed was a bin",
    );
    for bin in bins {
        for shipped in [format!("label=\"orchestrator · {bin}\""), format!("· {}", upper(bin))] {
            assert_eq!(
                vendor_names_spelled_in(&production_text(&shipped), &words),
                vec![bin.clone()],
                "the vendor-name check would not notice `{shipped}` — which is the literal \
                 this ticket found on every pane head",
            );
        }
    }

    // A fifth status word on one side of the wire only.
    let five = "export type HarnessStatus = \"logged-in\" | \"no-credential\" | \
                \"not-installed\" | \"unreadable\" | \"expired\";";
    let interface = statuses_the_interface_reads(five);
    assert_eq!(interface.len(), 5, "the status scan does not read the union it is pointed at");
    assert_ne!(
        interface,
        statuses_the_interface_reads(&read(WIRE)),
        "the status scan returns the same answer for a union with an extra word in it",
    );

    // A backend that stopped mapping a login state.
    let three = "match &readiness.login {\n  A => (\"logged-in\", None, None),\n  \
                 B => (\"no-credential\", None, Some(s)),\n  \
                 C => (\"unreadable\", None, None),\n        };";
    assert_eq!(
        statuses_the_backend_sends(three).len(),
        3,
        "the backend status scan does not count the arms it is pointed at",
    );
}

/// **The declarations this file reads are still there**, so a rename fails loudly
/// rather than leaving every test above matching nothing.
///
/// `between` already panics when a declaration it is pointed at is gone; this is the
/// same guarantee for the files themselves, which would otherwise be a path that
/// quietly stopped existing.
#[test]
fn the_declarations_this_file_reads_still_exist() {
    for rel in [GATE, PICKERS, WIRE, API, BACKEND, COMMANDS, HARNESS, CODEX]
        .into_iter()
        .chain(CHROME)
    {
        let file = repo_root().join(rel);
        assert!(file.is_file(), "{} is read by this file and is gone", file.display());
    }
    let names = registered_names();
    assert_eq!(
        names.len(),
        2,
        "the registry no longer holds two harnesses ({names:?}) — a third is welcome, and \
         `registered_names` has to learn where its spec is declared",
    );
}
