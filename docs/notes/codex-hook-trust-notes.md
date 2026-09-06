# codex hook-trust notes

**Decision: C48.** Issue #45, the measurement C46 gated the guardrail fix on.

**Version stamp: `codex-cli 0.153.4`** (npm `@openai/codex`, macOS 15.7.4, arm64) — the same
build `codex-spike-notes.md`, `codex-trust-key-notes.md` and `codex-clear-notes.md` were
recorded against.

Re-runnable in one command, **zero tokens**:

```
python3 examples/codex-spike/hook_probe.py
```

A **sibling** of `probe.py` and `trust_probe.py` (C13). Eleven arms, loud PASS/FAIL, exit code
equal to the number of failures, a clean skip when `codex` is absent and a separate clean
skip for the shim arms when cmux is not running. **Never rewritten** (`building.md` §4);
findings append.

---

## The question

C46 shipped codex's write guardrail installed-but-not-firing and named one measurement as
the thing that would decide the fix:

> **does a hook delivered via `-c hooks.PreToolUse=[…]` fire without a trust record?**

There was a strong hint that it does. The cmux PATH shim ships its own six hooks that way
and plainly expects them to run, so `-c` looked like a channel that sidestepped trust. If it
were, our hook could be composed into a single `-c` alongside the wrapper's and both of
C46's causes would close at once, with no `dangerously` flag.

## The answer

**No. A `-c`-delivered hook is skipped exactly as silently as a `config.toml` one.**

Hook trust in this build is a property of **the hook**, not of **how the hook arrived**. The
two arms differ only in delivery channel and behave identically:

| arm | hook delivered by | trust | hook ran | write outside landed |
|---|---|---|---|---|
| `config-toml-untrusted-is-skipped` | `config.toml` | none | **no** | **yes** |
| `c-flag-untrusted-is-skipped-too` | `-c hooks.PreToolUse=[…]` | none | **no** | **yes** |
| `control-bypass-config-toml` | `config.toml` | bypass flag | yes | no |
| `control-bypass-c-flag` | `-c hooks.PreToolUse=[…]` | bypass flag | yes | no |

The two controls are why the two `no`s can be believed: the same hook script, the same
canned tool call, the same everything except the trust gate, and it fires and denies. A
`False` in the untrusted arms cannot quietly mean "the hook was broken".

**The hint was real and the attribution was wrong.** cmux's hooks run, but not because `-c`
bypasses trust — the same shim injects `--dangerously-bypass-hook-trust` alongside them.
Reading its own arg stream settles it rather than arguing it:

```
$ cmux --socket "$CMUX_SOCKET_PATH" hooks codex inject-args | tr '\0' '\n'
--enable
hooks
--dangerously-bypass-hook-trust
-c
hooks.SessionStart=[{hooks=[{type="command",command='''…cmux-codex-hook-session-start.sh''',timeout=10000}]}]
-c
hooks.PreToolUse=[{hooks=[{type="command",command='''…cmux-codex-hook-pre-tool-use.sh''',timeout=120000}]}]
…
```

Two injections, and #45's hypothesis credited the wrong one. `arm the-shim-injects-the-bypass-flag`
pins it so a cmux update that drops the flag shows up as a failure rather than as a mystery.

## The second finding: `-c` does not clobber a `config.toml` hook

C46's second cause is **falsified as stated**. The claim was that the shim's
`-c hooks.PreToolUse=[…]` replaces the array and so erases a guardrail written into
`config.toml`. It does not.

| arm | `config.toml` | `-c` | ours ran |
|---|---|---|---|
| `c-flag-does-not-clobber-config-toml` | ours | a decoy | **yes** |
| `a-later-c-flag-does-replace-an-earlier-one` | — | ours, then a decoy | **no** |
| `config-toml-fires-under-the-shim` | ours | the shim's six | **yes** |

`config.toml` and `-c` **compose**; what replaces is one `-c` by a *later* `-c` for the same
key. #31 saw the second and generalised it one step too far. Three consequences:

- **The cmux shim never erased anything.** The guardrail was skipped for exactly one
  reason, trust, and #31's "our hook fires zero times under the shim" was a trust reading
  wearing a clobber's clothes.
- **`install_guardrail`'s placement needs no change.** Writing into the pane's own
  `config.toml`, merged with the operator's own entries, is the right shape and survives a
  wrapper that injects hooks of its own.
- **The composition #45 planned is unnecessary as well as insufficient.** Composing our
  hook into the wrapper's `-c` would have solved a problem that does not exist while
  leaving the one that does.

## What this leaves, and why the probe stops here

**The whole of the remaining gap is hook trust, and every way to close it is Tier 1.7**, so
#45's own instruction is to report rather than pick:

- the `--dangerously-bypass-hook-trust` flag, which C46 refused on two grounds — and the
  decisive one, that it does not fix the shim-clobber, is now itself falsified, because
  there is no shim-clobber. The *first* ground stands untouched: it trusts **every** hook in
  the document, including any an operator or a wrapper injected, which is a widening of what
  a pane may execute and therefore not a builder's call;
- a correct `trusted_hash`, whose key is known byte-for-byte and whose hash did not
  reproduce across ~200 renderings (`C46`). Not reopened here — #31 already paid for that
  and the budget bought nothing;
- codex's own `hooks/list`, which reports `currentHash` at runtime and which `placement`
  may not call, because nothing in placement reads the process.

**The probe uses the bypass flag and the product does not.** `hook_probe.py` is the only
file that *runs* it, as the positive control; elsewhere it appears solely as prose in
`placement/codex.rs` explaining why it was not reached for. Proving an instrument with a
flag is the opposite of shipping one.

## The instrument, and why it had to be new

`trust_probe.py` could answer its question by watching the first-run gate render into a pty,
with no turn ever completing. This question cannot be answered that way: a `PreToolUse` hook
only fires when a tool call actually happens, and a tool call only happens when a model
emits one. A capture server answering `data: [DONE]` — the instrument every earlier codex
arm used — never gets that far.

So the probe **is** the model. A fabricated `[model_providers.probe]` points at a loopback
server that answers the first request of a turn with a canned `response.output_item.done`
carrying an `exec_command` function call whose `cmd` writes a file outside the pane's cwd,
and answers later requests with a plain assistant message so the turn ends. Codex executes
it for real. Nothing reaches any endpoint and no model is consulted.

Three details are load-bearing:

- **The witness is the filesystem, not prose.** Every arm reports two independent facts —
  whether the hook process ran at all, and whether the out-of-worktree file exists
  afterwards. "Installed but silent" and "fired and denied" cannot be confused, which is
  precisely the confusion this whole arc keeps producing.
- **`sandbox_mode = "danger-full-access"`, on purpose.** C7's seatbelt would refuse the
  write on its own (#28 measured that), and then a *skipped* hook would look exactly like a
  firing one. The primary fence is deliberately moved out of the way so the arms measure the
  second fence alone.
- **The catalog entry is cloned.** Against this build's *default* model (`gpt-6-astra`) the
  shell tool is a custom JavaScript `exec` in a `functions` namespace; against a clone of the
  operator's first catalog entry (`shell_type = "unified_exec"`,
  `apply_patch_tool_type = "freeform"`) the tools are `exec_command`, `write_stdin`,
  `apply_patch`, `view_image` — the world #31 measured and the world
  `HarnessSpec::guardrail.write_tools` is written against. `probe.py` clones the entry for an
  unrelated reason; this one clones it so the arms are about the right tools.

The captured `PreToolUse` payload confirms #31's normalization finding unchanged: a call the
model side named `exec_command` arrives as `"tool_name": "Bash"`, with
`cwd`, `hook_event_name`, `model`, `permission_mode`, `session_id`, `tool_input`,
`tool_name`, `tool_use_id`, `transcript_path` and `turn_id`.

## One unverified lead, named rather than chased  — **settled negative by #46 (C50)**

`strings` over the native binary
(`node_modules/@openai/codex-darwin-arm64/vendor/aarch64-apple-darwin/bin/codex`) turns up a
**fourth trust state**. The enum serializes as the concatenated run
`manageduntrustedtrustedmodified` — so `managed`, `untrusted`, `trusted`, `modified` — and
sits beside a managed-config field `allowManagedHooksOnly`. A hook arriving through a
managed/enterprise config layer plausibly carries `trustStatus = managed` and so is trusted
by **provenance** rather than by hash.

**This is string-level evidence and an inference.** The enforcement path was not read. It is
recorded because it is an option nobody had listed, not because it works: the probe that
would settle it is a hook placed in a managed-config layer, run with no trust record, and
observed. Two other literals from the same survey, for whoever takes that decision:
`bypass_hook_trust` (the config twin of the CLI flag, validated as a boolean, i.e.
`-c bypass_hook_trust=true`) and `HookStateToml { state, matcher, hooks, enabled,
trusted_hash }`, which says the record is a **TOML** structure rather than the sqlite row
project trust uses. No `CODEX_*` environment variable and no `features.<name>` flag relates
to hook trust.

## An honest gap: this contradicts C47's shim reading

C47 records #31 as measuring our hook firing **zero** times under the shim and twice under
the real binary. This probe measures the opposite polarity under the shim: it **fires**
there, because the shim injects the bypass flag, and it is the *real binary* that skips it
when untrusted.

The likeliest reconciliation is that #31's shim run fell through the wrapper's own
passthrough — `cmux-codex-wrapper` execs the real binary untouched when `IN_CMUX` is 0, when
the cmux socket is unavailable, or when the subcommand is not a session entrypoint — and so
injected neither the hooks nor the bypass. That would produce zero firings and look exactly
like a clobber. **Not verified, and not reproducible from here**, because #31's environment
is gone. It is written down rather than smoothed over: the shim arms in `hook_probe.py` print
the resolved shim path and assert on the injected arg stream, so the next reader can tell
which of the two situations they are in.

## What would reverse this

A codex build that gates hooks by document rather than by entry, or that grows a
configuration-level trust surface — the note to re-run is the command at the top of this
file, and the version stamp is what makes the drift visible.


---

# Appended by #46 (C50): the lead is settled, and so is the config twin

**Same build (`codex-cli 0.153.4`), same instrument, zero tokens.** Findings append rather
than replace, per `building.md` §4 — nothing above is edited except the heading that called
the `managed` lead unverified.

## `managed` does not reach a fleet-spawned pane

The trust state is real; the way in is not. `managed` hooks are named by **`hooks.managed_dir`**,
and that field lives in a **requirements** layer, not a config layer. `HookSource` in the binary
enumerates `system | project | mdm | session_flags | plugin | cloud_requirements |
cloud_managed_config | legacy_managed_config_file | legacy_managed_config_mdm`, and the
requirements layers this build reads are `/etc/codex/requirements.toml` (absent on this
machine, root-owned), macOS MDM managed preferences, and cloud. All three are **machine-global
and not writable by FLEETOR**, which places panes as the operator with no `sudo`.

That disqualifies it before the measurement even matters: the guardrail's command line carries
the **pane id and that pane's own roots**, so one machine-global managed hooks directory cannot
serve five workers with five different root sets, and writing outside the pane's `CODEX_HOME`
breaks `placement`'s rule that every byte a seed writes lands under the pane's own directory.

Measured anyway, because a bounded probe is cheaper than an argument: a `requirements.toml`
inside `CODEX_HOME` naming a managed hooks dir, with `hooks.json`, `hooks.toml` and
`config.toml` spellings inside it, left the hook **unrun** and the out-of-worktree write on
disk. The control in the same run reproduced C48's `config.toml`-untrusted reading exactly.

## `bypass_hook_trust` — the config twin — does not exist as a config key

C48 read the literal out of the binary and inferred `-c bypass_hook_trust=true`. **Four
spellings, all accepted without error, all ignored:**

| delivery | fired | wrote outside |
|---|---|---|
| `bypass_hook_trust = true` in `config.toml` | no | yes |
| `-c bypass_hook_trust=true` | no | yes |
| `-c hooks.bypass_hook_trust=true` | no | yes |
| `-c features.bypass_hook_trust=true` | no | yes |

The attribution was wrong rather than the literal. The binary's own message is
`` `bypass_hook_trust` override must be a boolean `` and it sits in **`app-server/`** — it is a
**newThread override for the app-server protocol**, not a config key the TUI a fleet pane runs
reads. Accepted-and-ignored, which is this arc's recurring failure mode.

## And neither does a document-written trust state

Two more spellings, same run, both skipped and both let the write land:

```toml
[hooks.state."<CODEX_HOME>/config.toml:pre_tool_use:0:0"]
state = "managed"   # and, separately, state = "trusted", with no trusted_hash
```

## The positive control is what makes those six readings measurements

`--dangerously-bypass-hook-trust` fired and denied in the same run against the same scratch
installation. Six negatives beside a live positive is a measurement; six negatives alone would
have been a broken instrument.

## What ships

`--dangerously-bypass-hook-trust`, in argv, **on a seat FLEETOR drives only**, and only because
`install_guardrail` replaces a fenced pane's **whole `hooks` table** first — every event, since
the flag does not distinguish between them, plus the inherited `hooks.state` record. The
vendor's own help for the flag reads *"Intended only for automation that already vets hook
sources."* Owning the table is what vetting means here.

## The instrument grew one mode

`hook_probe.py --fleet-seeded --home H --cwd W --outside O --binary B [--arg A]...` drives a
`CODEX_HOME` that **FLEETOR seeded**, with the argv `Harness::command_args` composed, and prints
one JSON reading. The seeded `config.toml` is used **byte for byte**: the loopback provider and
the sandbox relaxation are `-c` session flags, a higher layer than the document under test.
`src-tauri/tests/vendor_binary_tier.rs::a_fleet_seeded_codex_worker_is_refused_a_write_outside_its_worktree`
is the caller.

## Both readings, per C47 — and the negative control reproduced C47 in passing

A fleet-seeded worker's out-of-worktree write is **refused under both binaries**, with two
witnesses each: the absent file, and the journal line naming `pane="worker-1"`.

With the bypass removed from the argv and nothing else changed:

| binary | refuses with our argv | negative control |
|---|---|---|
| `/opt/homebrew/bin/codex` | yes | **the same write lands** — the refusal is attributable |
| the PATH `codex` (cmux shim) | yes | the write still does not land — **inconclusive**, because the shim injects the flag itself (C48) |

So the arm requires the control on the resolved vendor binary and reports it as inconclusive on
a wrapper, instead of pretending both readings say the same thing. This is C47's rule earning
itself a second time.
