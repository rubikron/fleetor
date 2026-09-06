//! **`SeatRow` actually renders the picker it is declared to** (WP-26 redesign;
//! C63, C24, C12).
//!
//! `gate_pickers.rs` reads the source and proves `SeatRow`'s JSX *says* the harness
//! list is never filtered, that a refused option carries `harness.reason`, and that
//! every status word has a label. That file's own doc comment names the risk this
//! one closes: a source-reading tripwire cannot tell you the component it is
//! quoting is actually the one that mounts, or that a later restructuring did not
//! wrap the whole thing in a branch that never renders. #35 shipped exactly that
//! shape once, behind a fifteen-check passing tripwire with a working negative
//! control (`tests/gauge_unavailable_renders.rs`'s own doc comment). This redesign
//! touches `SeatRow`'s markup directly — the harness `<select>` and the model
//! `<input>` move from a flex row to a grid, specifically to stop the harness picker
//! collapsing to `c.` and the model field clipping `deepseek-v4-flash` to
//! `deepsee` — so it is exactly the kind of change C63 says needs a render, not a
//! read.
//!
//! ## Why a Rust test for a TypeScript component
//!
//! The same move `tests/gauge_unavailable_renders.rs` makes, for the same reason:
//! the frontend has no test runner and this arc does not add one (C24). This file
//! bundles `tests/gate_probe/render.tsx` with the esbuild already in the repo's
//! `node_modules` and runs it under `node` — no vitest, no jest, no `node:test`, no
//! new dependency.
//!
//! ## What is asserted here, and what is asserted next door
//!
//! This proves the component mounts and produces real markup: every harness is an
//! `<option>` regardless of which one is selected, a refused harness carries its
//! reason both as the option's `title` and as the row's visible facts line, the
//! orchestrator's absent model renders as a placeholder rather than a value, and a
//! long model name survives into the field uncut. It does not re-derive
//! `gate_pickers.rs`'s source-level rules (the `.filter(` check, the vendor-name
//! sweep) — those stay where they are, cheaper and already passing.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

const COMPONENT: &str = "ui/src/components/StartGate.tsx";
const PROBE: &str = "src-tauri/tests/gate_probe/render.tsx";
const BUNDLER: &str = "node_modules/.bin/esbuild";
const RUNTIME: &str = "node";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("src-tauri has a parent").to_path_buf()
}

fn announce(lines: &[String]) {
    let banner = format!(
        "\n\
         ┌──────────────────────────────────────────────────────────────────┐\n\
         {}\n\
         └──────────────────────────────────────────────────────────────────┘\n",
        lines.iter().map(|l| format!("│ {l}")).collect::<Vec<_>>().join("\n")
    );
    let stderr = std::io::stderr();
    let mut stderr = stderr.lock();
    let _ = stderr.write_all(banner.as_bytes());
    let _ = stderr.flush();
}

/// What the probe printed, or `None` with a banner explaining what this machine
/// did not measure.
///
/// **A missing probe or a missing component is a failure, never a skip** — a
/// deleted render or a deleted picker, not an absent toolchain. Only a missing
/// `esbuild`/`node` skips.
fn rendered() -> Option<serde_json::Value> {
    let root = repo_root();

    for required in [PROBE, COMPONENT] {
        assert!(
            root.join(required).is_file(),
            "{required} is part of this test. It is missing, which is a deleted component or a \
             deleted probe rather than an absent toolchain (C63). Restore it; do not delete \
             this test."
        );
    }

    let bundler = root.join(BUNDLER);
    if !bundler.is_file() {
        announce(&[
            format!("SKIPPED: the seat-row render tier — {BUNDLER} is not installed."),
            "  Run `npm install`. Nothing below was measured: that the harness".into(),
            "  picker and the model field actually render, uncrushed and".into(),
            "  uncut (WP-26 redesign).".into(),
            "  This machine is running a strictly weaker suite.".into(),
        ]);
        return None;
    }

    let bundle = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("gate_probe.cjs");
    let built = Command::new(&bundler)
        .arg(root.join(PROBE))
        .args(["--bundle", "--platform=node", "--format=cjs", "--jsx=automatic"])
        .arg(format!("--outfile={}", bundle.display()))
        .arg("--log-level=warning")
        .current_dir(&root)
        .output();

    let built = match built {
        Ok(out) => out,
        Err(e) => {
            announce(&[
                format!("SKIPPED: the seat-row render tier — could not run {BUNDLER}: {e}."),
                "  Nothing about SeatRow's markup was measured.".into(),
            ]);
            return None;
        }
    };
    assert!(
        built.status.success(),
        "bundling {PROBE} failed. The probe imports the real {COMPONENT}, so this is that \
         component failing to build:\n{}\n{}",
        String::from_utf8_lossy(&built.stdout),
        String::from_utf8_lossy(&built.stderr)
    );

    let ran = match Command::new(RUNTIME).arg(&bundle).current_dir(&root).output() {
        Ok(out) => out,
        Err(e) => {
            announce(&[
                format!("SKIPPED: the seat-row render tier — no `{RUNTIME}` on PATH ({e})."),
                "  SeatRow was not rendered, so nothing about it was measured.".into(),
            ]);
            return None;
        }
    };
    assert!(
        ran.status.success(),
        "{PROBE} did not render. This is the component throwing, not a missing toolchain:\n{}",
        String::from_utf8_lossy(&ran.stderr)
    );

    let out = String::from_utf8_lossy(&ran.stdout).to_string();
    Some(serde_json::from_str(&out).unwrap_or_else(|e| {
        panic!("{PROBE} must print the renderings as JSON: {e}\n--- stdout ---\n{out}")
    }))
}

/// One rendering, by the key the probe printed it under.
fn markup(all: &serde_json::Value, key: &str) -> String {
    all.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("{PROBE} printed no `{key}`: {all}"))
        .to_string()
}

/// **Every registered harness is an `<option>`, on every row, whoever is
/// selected.**
///
/// The behavioural half of `gate_pickers.rs`'s
/// `no_harness_is_hidden_and_a_refused_one_carries_its_reason`: that check reads
/// `.filter(` out of the source, which cannot tell a filtered list from one that
/// renders nothing at all for an unrelated reason. This renders four different
/// seats — including one currently sitting on the refused harness — and checks the
/// same two options survive every one of them.
#[test]
fn every_row_offers_both_harnesses_never_just_the_one_selected() {
    let Some(all) = rendered() else { return };
    for key in [
        "orchestrator",
        "worker_named_model",
        "worker_on_refused_harness",
        "worker_on_runnable_harness",
    ] {
        let row = markup(&all, key);
        assert!(
            row.contains("<option value=\"alpha-cli\""),
            "`{key}` dropped the runnable harness from its list:\n{row}"
        );
        assert!(
            row.contains("<option value=\"beta-cli\""),
            "`{key}` dropped the refused harness from its list — exactly the failure user \
             story 9 refuses, an unsupported-looking feature instead of a disabled one:\n{row}"
        );
    }
}

/// **The refused harness's option is disabled and carries the vendor's reason**,
/// both as the hover title and — when it is the one selected — as the row's
/// visible facts line underneath. The reason travels as one string to two places
/// rather than being respelled for the tooltip.
#[test]
fn the_refused_harness_carries_its_reason_on_the_option_and_inline() {
    let Some(all) = rendered() else { return };
    let reason = "no OAuth token found in ~/.beta/auth.json — run `beta-cli login` and try again.";

    let refused_row = markup(&all, "worker_on_refused_harness");
    assert!(
        refused_row.contains(&format!("title=\"{reason}\"")),
        "the refused option's title no longer carries the vendor's reason:\n{refused_row}"
    );
    assert!(
        refused_row.contains("<option value=\"beta-cli\" disabled=\"\""),
        "the refused option is no longer disabled:\n{refused_row}"
    );
    assert!(
        refused_row.contains(&format!("<p class=\"seat-row__facts\">{reason}</p>")),
        "a row sitting on the refused harness no longer shows the reason inline, in the facts \
         line beneath the controls — the state the redesign brief says the shipped card \
         handled worst:\n{refused_row}"
    );
    assert!(
        refused_row.contains("<option value=\"beta-cli\" disabled=\"\" title=")
            && refused_row.contains("selected=\"\">beta-cli — not logged in</option>"),
        "the row on the refused harness no longer shows it as the selected option:\n{refused_row}"
    );

    // And the same option, unselected, still carries the same disabled+reason pair
    // on a row that is not sitting on it — the option's own state, not something
    // that only appears because it happens to be picked.
    let runnable_row = markup(&all, "worker_on_runnable_harness");
    assert!(
        runnable_row.contains(&format!(
            "<option value=\"beta-cli\" disabled=\"\" title=\"{reason}\">beta-cli — not logged in</option>"
        )),
        "the refused option loses its disabled state and reason when it is not the one \
         selected:\n{runnable_row}"
    );
}

/// **The orchestrator's absent model is a placeholder, never a printed value.**
///
/// `DEFAULT_YOUR_LOGIN` is a seat *default*, not a name (M2) — if it rendered as the
/// field's value rather than its placeholder, an operator editing the field would
/// have to clear real-looking text first, and the row would look like it was
/// already naming something nobody chose.
#[test]
fn the_orchestrators_sentinel_is_a_placeholder_not_a_value() {
    let Some(all) = rendered() else { return };
    let row = markup(&all, "orchestrator");
    assert!(
        row.contains("placeholder=\"default (your login)\""),
        "the sentinel is no longer offered as the model field's placeholder:\n{row}"
    );
    assert!(
        row.contains("aria-label=\"orchestrator model\" value=\"\""),
        "the orchestrator's model field now carries a printed value instead of staying empty \
         with the sentinel as its placeholder:\n{row}"
    );
}

/// **A long model name survives into the field whole** — the behavioural half of
/// the `deepsee` clip: `gate_pickers.rs` cannot see a CSS `max-width`, so this
/// renders a seat with a model name long enough to have been cut and asserts the
/// full string is what the `<input>`'s `value` attribute carries.
#[test]
fn a_long_model_name_reaches_the_field_uncut() {
    let Some(all) = rendered() else { return };
    let row = markup(&all, "worker_named_model");
    assert!(
        row.contains("value=\"deepseek-v4-flash-preview\""),
        "the model field no longer carries the full model string in its value attribute — a \
         truncated one would still contain `deepseek-v4-flash` as a prefix, which is why this \
         checks the whole string:\n{row}"
    );
}

// --- the way out of a harness that cannot take a seat (#51) -------------------

/// The instruction paragraph out of one rendered row, or `None` when the row
/// rendered none.
///
/// **The class name is the component's, never the probe's** — a row that stopped
/// rendering the paragraph makes every assertion below fail rather than pass
/// vacuously, which is the whole reason these are read out of markup and not out of
/// the fixture.
fn instruction(row: &str) -> Option<String> {
    let open = "<p class=\"seat-row__howto\">";
    let start = row.find(open)? + open.len();
    let rest = &row[start..];
    let end = rest.find("</p>").expect("an opened paragraph closes");
    Some(rest[..end].to_string())
}

/// Every `<span class="mono">…</span>` in one paragraph, in order — the things the
/// operator is being told to type.
fn typed(paragraph: &str) -> Vec<String> {
    let open = "<span class=\"mono\">";
    let mut out = Vec::new();
    let mut rest = paragraph;
    while let Some(at) = rest.find(open) {
        rest = &rest[at + open.len()..];
        let end = rest.find("</span>").expect("an opened span closes");
        out.push(rest[..end].to_string());
        rest = &rest[end..];
    }
    out
}

/// **A harness that cannot take a seat shows what to run, not only what is wrong**
/// — the whole of this ticket, asserted on markup React actually produced.
///
/// The row still carries the vendor's reason in its facts line (the test above);
/// this is the line beneath it, and the command in it is a thing to copy rather
/// than a sentence to read.
#[test]
fn a_harness_that_cannot_take_a_seat_says_what_to_run() {
    let Some(all) = rendered() else { return };
    let row = markup(&all, "guidance_login_command");
    let told = instruction(&row).unwrap_or_else(|| {
        panic!(
            "a row on a harness that cannot take a seat renders no instruction at all — the \
             dead end #51 exists to close:\n{row}"
        )
    });
    assert!(
        told.contains("In a terminal of your own, run:"),
        "the instruction lost the sentence that says who runs the command:\n{told}"
    );
    assert_eq!(
        typed(&told),
        vec!["beta-cli login".to_string()],
        "the command is no longer rendered as something to type. A sentence with a command \
         buried in it is a command nobody can copy:\n{told}"
    );
    // The reason is still there and is still the vendor's, unchanged by this row
    // gaining a second line: diagnosis and instruction are two sentences.
    assert!(
        row.contains("<p class=\"seat-row__facts\">no OAuth token found"),
        "the instruction replaced the vendor's own reason instead of following it:\n{row}"
    );
}

/// **A login that lives inside the harness's own session renders both steps.**
///
/// One registered harness logs in with a subcommand and the other with a command
/// inside its own prompt; the row states whichever the harness declared, and knows
/// which is which for neither.
#[test]
fn a_two_step_login_renders_both_of_its_steps() {
    let Some(all) = rendered() else { return };
    let told = instruction(&markup(&all, "guidance_two_step"))
        .expect("a two-step login still renders an instruction");
    assert_eq!(
        typed(&told),
        vec!["gamma-cli".to_string(), "/signin".to_string()],
        "a harness whose login is a command inside its own session lost one of its two \
         steps, so the instruction is one an operator cannot follow:\n{told}"
    );
    assert!(
        told.contains("then"),
        "the two steps render with nothing between them saying they are ordered:\n{told}"
    );
}

/// **A custom provider whose key is missing names the variable and no command**
/// (C14's third shape, #29).
///
/// The distinction is the point: no login command writes an environment variable,
/// so a row that offered one here would be sending the operator to do something
/// that cannot work.
#[test]
fn a_custom_provider_missing_its_key_names_a_variable_and_no_command() {
    let Some(all) = rendered() else { return };
    let told = instruction(&markup(&all, "guidance_provider_key"))
        .expect("a provider missing its key still renders an instruction");
    assert_eq!(
        typed(&told),
        vec!["OPERATORS_SHELL_KEY".to_string()],
        "the row no longer names exactly the variable the provider reads its key from — \
         either it has stopped naming it, or it has offered a login command beside it, and \
         no login command sets an environment variable:\n{told}"
    );
    assert!(
        told.contains("No login command will help here"),
        "the row stopped saying that this is the shape no login fixes:\n{told}"
    );
}

/// **An unreadable harness is told it is not logged out** (C14, #36).
///
/// A working installation whose report this build could not parse. It is not
/// refused — the option stays selectable — and it is given nothing to type, because
/// there is nothing wrong with its login.
#[test]
fn an_unreadable_harness_is_not_told_to_log_in() {
    let Some(all) = rendered() else { return };
    let row = markup(&all, "guidance_unreadable");
    let told = instruction(&row).expect("an unreadable harness still renders an instruction");
    assert!(
        typed(&told).is_empty(),
        "an operator whose harness works is being told to type something. `Unreadable` is a \
         parse failure on this side of the wire, and #36 deliberately does not refuse it:\n{told}"
    );
    assert!(
        told.contains("Nothing to log in to"),
        "the row no longer distinguishes a report this build could not read from a machine \
         with no credential:\n{told}"
    );
    assert!(
        row.contains("<option value=\"epsilon-cli\"") && !row.contains("epsilon-cli\" disabled"),
        "the unreadable harness's option was disabled — refusing a working installation on \
         the strength of a parse error, which is the failure C14's narrowness exists to \
         avoid:\n{row}"
    );
}

/// **A refused harness the machine has nothing to suggest about renders no
/// instruction**, rather than an empty one.
///
/// The same rule the pane head keeps about a mark nobody supplied (#50): an absent
/// fact is absent, and a row with a blank instruction under it reads as a broken
/// screen.
#[test]
fn a_harness_with_nothing_to_suggest_renders_no_instruction() {
    let Some(all) = rendered() else { return };
    let row = markup(&all, "guidance_absent");
    assert!(
        instruction(&row).is_none(),
        "a harness the backend offered no guidance for rendered an instruction anyway:\n{row}"
    );
    assert!(
        row.contains("<p class=\"seat-row__facts\">`zeta-cli` is not on the PATH"),
        "the row lost the reason as well, so it now says nothing at all:\n{row}"
    );
}

/// **Registering a third harness needs no `ui/` change** — asserted rather than
/// asserted about, the way `pane_head_renders.rs` asserts the same criterion.
///
/// Every harness rendered above is registered in **no build**: `beta-cli`,
/// `gamma-cli`, `delta-cli`, `epsilon-cli` and `zeta-cli` appear in no
/// `HarnessSpec` and in no file under `ui/`. The card renders each one's own
/// instruction without ever having been edited for any of them, because the
/// sentence, the command and the variable all arrive on the wire from
/// `HarnessSpec::login`. A component that chose its wording by asking which harness
/// this is would render all five blank — and would fail
/// `gate_pickers.rs::the_gate_spells_no_vendors_name` by construction.
///
/// The shape is compared and not merely the strings: whatever wrapper one
/// unregistered harness's instruction gets, the others get too.
#[test]
fn a_harness_this_build_never_registered_gets_its_own_guidance() {
    let Some(all) = rendered() else { return };
    let shapes: Vec<(&str, String)> = ["guidance_login_command", "guidance_two_step",
        "guidance_provider_key", "guidance_unreadable"]
        .into_iter()
        .map(|key| (key, markup(&all, key)))
        .collect();
    for (key, row) in &shapes {
        assert_eq!(
            row.matches("seat-row__howto").count(),
            1,
            "`{key}` renders its instruction a different number of times than once — the \
             wrapper is the component's and every harness must get the same one:\n{row}"
        );
        assert_eq!(
            row.matches("seat-row__facts").count(),
            1,
            "`{key}` renders a different number of facts lines than the others:\n{row}"
        );
    }
    // The five names are the probe's own, and none of them is registered anywhere.
    let component =
        std::fs::read_to_string(repo_root().join(COMPONENT)).expect("component reads");
    for invented in ["beta-cli", "gamma-cli", "delta-cli", "epsilon-cli", "zeta-cli"] {
        assert!(
            !component.contains(invented),
            "{COMPONENT} names `{invented}`, so the guidance above is not evidence that an \
             unregistered harness renders — the component was edited for it"
        );
    }
}

/// **There is no credential field on this card** (#51's fourth criterion), asserted
/// on what actually renders rather than on what the source says.
///
/// FLEETOR names the command and writes no credential: each vendor's login writes
/// where that vendor reads, so a token pasted here would land nowhere. The one text
/// input a seat row has is the model, and it says so.
#[test]
fn no_row_renders_anywhere_to_put_a_credential() {
    let Some(all) = rendered() else { return };
    for key in [
        "orchestrator",
        "worker_on_refused_harness",
        "guidance_login_command",
        "guidance_provider_key",
        "guidance_unreadable",
    ] {
        let row = markup(&all, key);
        assert_eq!(
            row.matches("<input").count(),
            1,
            "`{key}` renders more than the one model field. The second input on a row that \
             has just been told to log in is the credential field this ticket refuses:\n{row}"
        );
        assert!(
            row.contains("aria-label=\"worker 1 model\"")
                || row.contains("aria-label=\"worker 3 model\"")
                || row.contains("aria-label=\"orchestrator model\""),
            "`{key}`'s one input is no longer the model field:\n{row}"
        );
        for forbidden in ["type=\"password\"", "autocomplete=\"", "name=\"token"] {
            assert!(
                !row.contains(forbidden),
                "`{key}` renders `{forbidden}` — a credential surface on the one screen that \
                 already spends real money:\n{row}"
            );
        }
    }
}

/// **The probe cannot fake a pass.** The strings the assertions above look for must
/// come out of `SeatRow`/`optionLabel`/`factsFor`, not out of the probe's own
/// source — otherwise a component that stopped rendering any of them would still
/// leave this suite green.
#[test]
fn the_probe_supplies_data_and_never_the_markup() {
    let probe = std::fs::read_to_string(repo_root().join(PROBE)).expect("probe reads");
    for phrase in [
        "— not logged in",
        "seat-row__facts",
        "seat-row__harness",
        "seat-row__howto",
        "<option",
        "disabled=",
        "<span class=",
    ] {
        assert!(
            !probe.contains(phrase),
            "{PROBE} contains `{phrase}`, which is markup `optionLabel`/`SeatRow` are supposed \
             to produce — a test asserting on it would pass on the probe's own text rather \
             than on what the component rendered"
        );
    }
}

/// **The declaration this file reads is still there.**
#[test]
fn the_declarations_this_file_reads_still_exist() {
    for rel in [COMPONENT, PROBE] {
        let file = repo_root().join(rel);
        assert!(file.is_file(), "{} is read by this file and is gone", file.display());
    }
    let component = std::fs::read_to_string(repo_root().join(COMPONENT)).expect("component reads");
    assert!(
        component.contains("export function SeatRow("),
        "`SeatRow` is no longer an exported function in {COMPONENT} — the probe imports it by \
         that name"
    );
}

// --- the credential picker (WP-25 #49; C73, C75, C78) --------------------------

/// Which option the segmented control renders as the live answer, or `<none>`.
///
/// **`aria-pressed`, not `checked`** — C78 moved this control from a radio
/// fieldset to a segmented button group, and the assertions moved with it rather
/// than being deleted. The property they pin is unchanged: exactly one option is
/// shown as the answer, and it is the one the seats actually hold.
fn pressed_option(html: &str) -> String {
    html.split("<button")
        .find(|chunk| chunk.contains(r#"aria-pressed="true""#))
        .and_then(|chunk| chunk.split(r#"data-source=""#).nth(1))
        .and_then(|rest| rest.split('"').next())
        .unwrap_or("<none>")
        .to_string()
}

/// **The control that decides what a fleet spends actually renders** (C63).
///
/// It is here rather than in a file of its own because it shares this file's whole
/// apparatus — the same probe, the same bundle, the same `node` — and a second
/// copy of that machinery would be a second thing to keep working.
///
/// **Both options, always.** A picker that hid the option you are not on would
/// make the choice invisible, which is the failure mode this control exists to
/// prevent: an operator whose workers spend their subscription must be able to see
/// that they do, and see the other answer sitting beside it.
#[test]
fn the_credential_picker_renders_both_options_whichever_is_chosen() {
    let Some(all) = rendered() else { return };

    for (key, selected, other) in [
        ("credential_source_plan", "plan", "fleet_key"),
        ("credential_source_fleet_key", "fleet_key", "plan"),
    ] {
        let html = markup(&all, key);
        assert!(
            html.contains(&format!(r#"data-source="{selected}""#)),
            "the {selected} option did not render at all:\n{html}",
        );
        assert!(
            html.contains(&format!(r#"data-source="{other}""#)),
            "only the chosen option rendered, so the operator cannot see the answer they are \
             not on — or switch to it:\n{html}",
        );
        assert!(
            html.contains("your plan") && html.contains("your key"),
            "an option rendered with no label. A control with no words on it is a choice \
             nobody can make:\n{html}",
        );
    }
}

/// **The chosen option is the one that renders as pressed** (C75, C78).
///
/// The whole control is one attribute away from lying about what a click will
/// spend, and that attribute is exactly what a source-reading test cannot see.
#[test]
fn the_credential_picker_marks_the_source_the_seats_hold() {
    let Some(all) = rendered() else { return };

    let plan = markup(&all, "credential_source_plan");
    let key = markup(&all, "credential_source_fleet_key");

    assert_eq!(
        pressed_option(&plan),
        "plan",
        "the picker rendered with the seats on the plan and did not mark it, so the gate \
         shows a fleet that is not the one that will spawn:\n{plan}",
    );
    assert_eq!(
        pressed_option(&key),
        "fleet_key",
        "the picker rendered with the seats on the key and still marked the plan. That is \
         the wrong direction to be wrong in — it spends a subscription:\n{key}",
    );
}

/// **`mixed` is reported, never offered** (C78).
///
/// It is what the four seats say, not an answer the operator can give. Two
/// properties, and both matter: it must not appear when the seats agree — a third
/// option that is always there is one the operator wonders how to pick — and when
/// it does appear it must be unclickable, or it is a control that silently
/// rewrites three seats to match a fourth.
#[test]
fn the_mixed_state_appears_only_when_true_and_cannot_be_chosen() {
    let Some(all) = rendered() else { return };

    let agreed = markup(&all, "credential_source_plan");
    assert!(
        !agreed.contains(r#"data-source="mixed""#),
        "`mixed` renders on a fleet whose four seats agree, so it reads as a third answer \
         the operator could pick:\n{agreed}",
    );

    let mixed = markup(&all, "credential_source_mixed");
    assert!(
        mixed.contains(r#"data-source="mixed""#),
        "four seats disagree and the control claims one of them:\n{mixed}",
    );
    assert_eq!(pressed_option(&mixed), "mixed", "the mixed state is not the live answer:\n{mixed}");
    let chunk = mixed
        .split("<button")
        .find(|c| c.contains(r#"data-source="mixed""#))
        .expect("just asserted it renders");
    assert!(
        chunk.contains("disabled"),
        "`mixed` is clickable. Picking it would have to rewrite three seats to match a \
         fourth, chosen by nobody:\n{mixed}",
    );
}

/// **A harness with no readable login is said before Start, not discovered at it**
/// (C75, C78).
///
/// The backend refuses a seat whose credential this machine cannot honour — it
/// substitutes neither for the other — so this is the sentence that turns that
/// refusal into something the operator can act on while they still can.
#[test]
fn the_credential_picker_names_a_harness_it_has_no_login_for() {
    let Some(all) = rendered() else { return };

    let fine = markup(&all, "credential_source_plan");
    let missing = markup(&all, "credential_source_no_login");

    assert!(
        missing.contains("No login was readable"),
        "the plan option is offered for a harness with no readable login and says nothing \
         about it, so the operator meets the refusal at the Start button instead:\n{missing}",
    );
    assert!(
        missing.contains("Re-check logins"),
        "the note names the problem and not the fix. Re-check logins is the button that \
         resolves it, and it is on this same card:\n{missing}",
    );
    assert!(
        !fine.contains("No login was readable"),
        "the missing-login warning renders on a machine where every harness this fleet \
         spends has one, which is a warning that means nothing:\n{fine}",
    );
}

/// **A worker row carries three controls and the orchestrator carries two**
/// (C78).
///
/// The absence on the orchestrator is the assertion. That seat has only ever run
/// the operator's own login, so a credential dropdown there would offer a choice
/// that does not exist — and rendering it disabled would be worse, since a
/// disabled control implies a state you could reach.
#[test]
fn only_a_worker_row_offers_a_credential() {
    let Some(all) = rendered() else { return };

    let worker = markup(&all, "worker_with_a_credential");
    assert!(
        worker.contains("seat-row__credential"),
        "a worker row renders no credential dropdown, so the per-seat choice is \
         unreachable:\n{worker}",
    );
    assert!(
        worker.contains("your plan") && worker.contains("your key"),
        "the credential dropdown renders without both answers:\n{worker}",
    );

    let orch = markup(&all, "orchestrator");
    assert!(
        !orch.contains("seat-row__credential"),
        "the orchestrator row offers a credential choice. It has always run the operator's \
         own login and has no second answer to hold:\n{orch}",
    );
}
