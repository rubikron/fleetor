# codex spike notes

**Version stamp: `codex-cli 0.153.4`** (npm `latest` on 2026-09-05; `0.154.0-alpha.3` exists
on the alpha tag only). macOS 15.7.4, arm64, npm install.

Every arm below is **re-runnable in one command** and spends **zero tokens**:

```
python3 examples/codex-spike/probe.py
```

That is not a convenience — it is C13. The probe is suite infrastructure, not a transcript
of a session. It exits with the number of failed arms so CI can gate on it, and it skips
loudly (exit 0) when `codex` is not on PATH. It prints a drift note when the running build
differs from the recorded one, because a failure then may be drift rather than a
regression, and the right response is to re-record rather than patch around it.

**Never rewritten** (`building.md` §4). Findings append.

---

## Why a spike could be this cheap

Codex ships three instruments that exercise the real binary without a model:

| instrument | what it yields |
|---|---|
| `codex debug prompt-input` | the exact model-visible input list, as JSON |
| `codex sandbox <cmd>` | an arbitrary command under the real seatbelt, no model |
| a fabricated `[model_providers.*]` at a local capture server | the **literal request body** |

The third is the one that mattered most. Pointing a `model_provider` at
`http://127.0.0.1:<port>` with `wire_api = "responses"` and answering every POST with
`data: [DONE]` gives you the request codex would have sent — `instructions`, `input`,
`tools`, everything — for nothing. Three of the findings below were only settleable that
way, and one of them **reversed a conclusion reached from reading the config surface**.

---

## Arm 1 — the brief carrier (C3)

Three candidates. Only one works, and it was not the obvious one.

| candidate | result |
|---|---|
| `model_instructions_file` | **replaces the system prompt** |
| `base_instructions` in a custom `model_catalog_json` | **not honoured** |
| `AGENTS.md` in the pane's cwd | reaches the model as a **`user` message** |

With `-c model_instructions_file=<file>`, the request's `instructions` field went from the
built-in **17,730** characters — `"You are Codex, an agent based on GPT-5. You and the user
share one workspace…"` — to the **50**-character sentinel file. `"You are Codex"` was absent
from the whole body. That is D-043's *replace, not append* property exactly, and it is a
plain config key, so **the brief does not need to live in the pane's worktree** the way
M5's `.mdc` does. No `.git/info/exclude` line, no story about keeping `git status` honest.

`base_instructions` looked like the intended override — it is a real field on every catalog
entry, and the operator's own `models.json` carries a full 17k prompt in it. Setting it in
a custom catalog changed nothing: the built-in prompt was sent anyway. It is a
**descriptor**, not an override. This was believed to work for about ten minutes on the
strength of the config surface alone; the capture server is what disproved it.

`AGENTS.md` does reach the model, but as `role: "user"` — in-band, spending the pane's own
context window, and reading as though the operator typed it. That is precisely the failure
story 32 names. Observed input roles on a briefed turn:
`['developer', 'developer', 'developer', 'user', 'user']` — the developer messages are
codex's own skills/plugins/apps instructions, the first `user` is `AGENTS.md`, the second
is the actual prompt.

`experimental_instructions_file`, present in older builds, **does not exist** in 0.153.4.

**Not measured:** whether `model_instructions_file` survives `/clear`. It is a file the
harness reads rather than a value snapshotted at boot, which is why it is expected to.
Expected is not measured. Phase-2 spike.

## Arm 2 — the fence (C7)

`codex sandbox -c sandbox_mode=workspace-write sh -c 'echo x > $HOME/probe'`

→ `sh: /Users/…/probe: Operation not permitted`, and **no file was created**.

The sandbox is real, not advisory. Combined with `approval_policy = "never"` this is the
worker posture: commands auto-run inside the jail with no prompt, and there is no human to
answer an escalation. `on-request` was rejected for M13's reason — it lets the model decide
when to ask, and a worker parks looking perfectly healthy.

## Arm 3 — the socket, and the lever that does not exist (C5)

Under the default `workspace-write` policy, a unix-socket `connect()` is refused:

```
REFUSED [Errno 1] Operation not permitted
```

With **one key** — `sandbox_workspace_write.network_access = true` — the identical probe:

```
CONNECTED          # and the listener received its bytes
```

So a fenced codex pane runs the real `fleet` CLI directly. **No bridge**, one transport,
one CLI, nothing new on the message path — the same conclusion M28 reached for cursor,
reached independently here.

**The narrow lever was found, tried, and does not work.** Codex's config exposes
`network.unix_sockets` and `network.dangerously_allow_all_unix_sockets`, which read exactly
like a targeted allowance and would have been *strictly better* than M14's all-or-nothing
network switch — a codex worker could have kept a restricted network and still talked to
its peers. Three variations were run, including `--enable network_proxy` to turn on the
feature those keys belong to. All three left the refusal in place. Those keys configure the
network-**proxy** layer, not the seatbelt profile.

Recorded as *tried and rejected* rather than *unconsidered*, so a later reader does not
spend an afternoon rediscovering it.

## Arm 4 — delivery, and the timing that is load-bearing (C22)

A bracketed paste followed by CR into a live `codex` pty, with FLEETOR's own `TERM` and
`COLORTERM`, **submits the turn** — the request reached the capture server carrying a
sentinel that appeared nowhere in the pasted bytes. That is M26's property.

**It failed the first time, and the failure is the finding:**

| startup wait | post-paste delay | submitted |
|---|---|---|
| 6.0 s | 0.6 s | **no** |
| 12.0 s | 1.5 s | yes |

The probe used fixed sleeps rather than readiness detection, so *which* of the two timings
is load-bearing is not yet known — but that a timing is load-bearing now is. Codex also
ships a `disable_paste_burst` config key, which suggests the paste path is special-cased.

This is exactly the failure M6 exists to refuse: a green `accepted` for a message left
sitting unsubmitted in an input box, the one lie D-034 was built to prevent. M26 found
cursor's profile identical to Claude Code's and owed no tuning. **Codex owes tuning, and is
the first customer of a seam M6 kept on speculation.** Phase-2 spike replaces the sleeps
with readiness detection.

Both timings are asserted in the probe, including the failing one — a tier that only
records the working path would not have caught this.

---

## Findings not in the probe

These were measured once by hand and are recorded rather than automated, because each is a
property of the *installation* rather than of the binary's behaviour.

**`CODEX_HOME` carries config *and* login (C6).** A fabricated `HOME` with the real
`CODEX_HOME` reported `Logged in using an API key`; an empty `CODEX_HOME` reported
`Not logged in` regardless of `HOME`. **M8's central fact — that auth follows `HOME`, so
the Fence logs a pane out unless the credential is planted — is cursor's, not a harness
universal.** This is the single measurement that most justified investigating a second
non-Claude harness *before* writing the seam, and it is why checkpoint 6 was renamed.

**The tilde trap (C6).** A `~` inside a copied config value resolves against the pane's
private `HOME`, not the operator's. The operator's own config carries
`model_catalog_json = "~/.codex/models.json"`, and a pane seeded with it verbatim dies at
spawn with `Error loading configuration: No such file or directory (os error 2)`. Loud, but
only if the seeder rewrites such paths — hence the conformance assertion.

**`codex doctor --json` in 1.36 s (C8, C14).** Returns `schemaVersion`, `codexVersion`,
`overallStatus` and a `checks` array carrying auth mode, provider, model, filesystem
sandbox, network sandbox and approval policy. One call answers checkpoint 14 *and* the
posture tripwire. **Caveat: it reports reachability, not authorization** — its probe
returned HTTP 401 against a live provider and still passed. A revoked key clears the gate
and fails on the first turn.

**47 feature flags on by default (C21).** Four reach past the fence arm 2 measured:
`multi_agent`, `browser_use` (with `browser_use_full_cdp_access` also on), `computer_use`,
`in_app_local_automation`. The seatbelt bounds the filesystem and the network; it does not
bound a pane driving a browser, a desktop, or fanning out into threads the run manifest
never sees.

**The transcript store (C12).** A pane writes `thread_history_1.sqlite` inside its
`CODEX_HOME`: `thread_items(thread_id, turn_id, item_id, rollout_ordinal, created_at_ms,
item_type, item_json)` and `thread_turns(…status, started_at, completed_at, duration_ms…)`.
An `_sqlx_migrations`-managed schema in **generation-numbered filenames** (`state_5`,
`logs_2`, `thread_history_1`) — that generation number is codex's own compatibility signal
and is what the manifest's `transcript_format` should record. Rows verified against a real
run. **It is WAL-mode, so archiving it is `VACUUM INTO` or the backup API, never `cp`** —
`run-rotation-notes.md` measured the torn-copy failure for the fleet's own store and the
same physics applies here.

**Not measured, and it is the real open question:** whether per-turn token usage is
persisted in that store at all. The capture server never completes an assistant turn, so no
usage row was ever written, and the token-free tier **cannot** answer this one — it needs a
real turn against a real endpoint. If the answer is no, M22's fallback stands unchanged:
the rail says `unavailable`, never a number synthesized from what FLEETOR sent.

**Trust is keyed differently (C16).** `seed_config_dir` writes `hasTrustDialogAccepted`
under a `project_key` that canonicalizes and matches exactly. The operator's own codex
config carries `[projects."/Users/…/harness"]` — a **parent** of the repository — which
suggests codex resolves trust by root rather than by exact path. **Observed, not verified.**
It is the concrete form of M23's "it can no longer be one function", and a phase-2 spike.

**The operator's installation, for the record.** `auth_mode: "apikey"`, but
`model_provider` points at a DeepSeek base URL with its own bearer token and a local
`model_catalog_json`. So the credential is not a first-party key on a subscription plan —
it is a third-party provider behind codex's provider mechanism, and it is a **different
key** from `.env`'s `DEEPSEEK_API_KEY` that FLEETOR already gives its Claude Code workers
(`spawn.rs:331`). Two keys, same vendor. That distinction is what C9 turns on.

---

## Findings appended by #26 — the private `CODEX_HOME`, seeded (C6, C39)

Measured on the same build, `codex-cli 0.153.4`. Two are now gated in
`src-tauri/tests/vendor_binary_tier.rs` rather than only written down.

**The tilde trap, reproduced against the binary and gated.** With a fabricated
operator installation carrying `model_instructions_file = "~/.codex/brief.md"` and a
fabricated `HOME` the way the Fence gives one, `codex sandbox /bin/echo ok` refuses:

```
Error: failed to read model instructions file <PANE HOME>/.codex/brief.md:
No such file or directory (os error 2)
```

The path it names is inside the **pane's** private `HOME`, which is C6's measurement
exactly. Seeding the identical installation through `Harness::seed_config_dir` and
re-running the same command prints `ok` and exits 0. Both arms are the new
`the_seeded_codex_home_loads_in_the_real_binary_and_the_trap_reproduces` test —
the negative control is the load-bearing half, because without it a seeder that
accidentally wrote nothing would also go green.

**`codex sandbox <cmd>` is a config-load instrument.** It reads `config.toml`,
fails loudly on a bad one, runs the command under the real seatbelt, and touches no
model and no network. ~0.2 s. `--strict-config` is *not* accepted by it (`` `--strict-config`
is not supported for `codex sandbox` ``) — that flag reaches only the interactive
TUI and `exec`, so unknown-key detection is not available at zero cost.

**The `-c` value rule is the vendor's own, and it is documented.** From
`codex --help`: *"Use a dotted path (`foo.bar.baz`) to override nested values. The
`value` portion is parsed as TOML. If it fails to parse as TOML, the raw string is
used as a literal."* This is what the three checkpoint key lists' single reader
implements, so a key written into a spec list is spelled exactly as it would be
typed after `-c`.

**There is no top-level onboarding flag.** The binary's own `ConfigToml` serde field
list carries nothing shaped like Claude Code's `hasCompletedOnboarding`. Codex's
first-run gates are the trust dialog (`[projects."<path>"] trust_level`, and the
binary confirms `ProjectTrustConfigToml` has exactly one element) and the animated
splash C37 already named as a readiness question. Checkpoint 4's `seed_keys` for
codex are therefore `sqlite_home` and `log_dir` — see C39(c).

**The operator's installation, measured for size.** `packages/` is **275 MB** of
downloaded release binaries; `.tmp/plugins/` is **88 MB** and is what
`codex plugin marketplace list` reports as its marketplace root; `models.json` is
112 KB and `skills/` 508 KB. That ratio is why the snapshot is an allowlist and why
the plugin cache does not travel (C39(d)).

**Provider credential fields, by name.** `ModelProviderInfo` carries `name`,
`base_url`, `env_key`, `env_key_instructions`, `experimental_bearer_token`, `aws`,
`wire_api`, `query_params` and retry/timeout settings. The seeder strikes
`experimental_bearer_token` (the operator's key in plaintext — the real installation
has one) and `env_key` (it names a variable the vendor reads the key *out of*, so a
surviving value points a fenced pane at the operator's shell profile — L2's leak).

**Not measured, and stated as such:** that setting `sqlite_home` and `log_dir`
actually relocates the stores. Both are real `ConfigToml` fields and a seed carrying
them is accepted, but no zero-token instrument was found that opens the thread
store — `codex sandbox`, `codex features list` and `codex plugin marketplace list`
all leave the directory empty. #40 reads that store for the gauge and will observe
it either way.

---

## Appended by #27 — which wire slot the system prompt occupies is *model*-dependent

**The carrier's behaviour is unchanged. What is model-dependent is where the prompt
lands in the request body**, and it is worth recording because C3 and C37 both quote a
field name that the build's own default model does not use.

Both of those measurements drove a **cloned catalog entry** — `probe.py` copies
`codex debug models`' first entry and renames it `probe-model` — and against that entry
the prompt is the top-level `instructions` field: 17,730 characters of "You are Codex"
without the carrier, the brief file's contents with it.

Against the build's **default** model (`gpt-6-astra`, resolved with no `models.json` and
no `model` key in `CODEX_HOME`), there is **no `instructions` field in the body at all**.
The prompt travels as the first `developer` message in `input`, ahead of the vendor's
own `<skills_instructions>` and `<multi_agent_role>` blocks and ahead of both `user`
messages. Measured on the same instrument: a fabricated `[model_providers.probe]`
pointed at a loopback capture server, one `codex exec` turn, zero tokens.

With `model_instructions_file` set, `input[1]` is the brief verbatim and `You are Codex`
is absent from the whole 31 KB body. With it unset, `input[1]` is the built-in prompt and
`You are Codex` is present at offset 21,822 of a 52 KB body. **Replace, not append,
either way** — which is the property D-043 needs and the only one the fleet depends on.

Two consequences:

- `src-tauri/tests/vendor_binary_tier.rs`'s brief-carrier arm asserts on the **body**,
  not on a field name: the brief arrives whole and ahead of the first `user` message (so
  it is not the in-band `AGENTS.md` shape), and the vendor's own prompt is gone. A
  negative control against an un-seeded `CODEX_HOME` inverts both.
- The vendor drops the brief file's **trailing newline**. The needle is `trim_end`ed;
  nothing else about the document changes.

**Not measured:** whether any model the fleet would actually run a worker on uses the
`instructions` field, and whether the two slots differ in caching or truncation
behaviour. Neither changes the carrier decision.

## Appended by #28 — the fence measured from both sides, and the instrument that reads the file (C5, C7, C21)

Arm 2 above measured that a write **outside** the workspace is refused. It never measured
that a write **inside** it is permitted, and those are not the same claim: a sandbox that
refuses everything passes the first assertion perfectly and ships a worker that cannot do
any work. That is precisely the *"derived permission profile cannot be represented as a
legacy sandbox policy; falling back to read-only"* path C7 rejected the newer generation
over, and it looks perfectly healthy at spawn. Both directions are now asserted, in
`src-tauri/tests/vendor_binary_tier.rs`.

**`codex sandbox` takes its sandbox from `-c` and ignores `sandbox_mode` in
`config.toml`.** Measured three ways: a `CODEX_HOME/config.toml` saying
`danger-full-access` still ran the command read-only, one saying `workspace-write` did
too, and `-c sandbox_mode=danger-full-access` on the same directory wrote freely. Adding
the project's `trust_level = "trusted"` row changed nothing, so it is not a trust gate.

So the subcommand can prove **what the trio's values do** and can prove **nothing about
the file** — which matters, because the file is how a pane actually gets them. The second
instrument closes it:

**`codex doctor --json` resolves the configuration in a `CODEX_HOME` and reports the
posture it arrived at**, in 1.3 s, with no completion requested:

- `checks["sandbox.helpers"].details` — `"filesystem sandbox"` (`restricted` under
  FLEETOR's seed, `unrestricted` under an operator's `danger-full-access`) and
  `"approval policy"` (`Never` vs `OnRequest`). A real discriminator in both fields.
- `checks["config.load"].details["enabled feature flags"]` — the **resolved** feature
  list. This is C21's instrument: the four names are absent from a worker's seed and
  present in the orchestrator's, read off the vendor's own resolution rather than off
  what FLEETOR wrote.

**`workspace-write`'s default writable roots include `$TMPDIR` and `/tmp`.** Found the
hard way: a scratch root under `std::env::temp_dir()` is *inside* the fence, so an
"outside the worktree" probe pointed there passes for the wrong reason. Production is not
arranged that way — the Fence's private `HOME` is `~/.fleetor/_shell/homes/worker-N` and
the socket is under `~/.fleetor` — and the test hands the child a `TMPDIR` inside its own
worktree to reproduce the real layout. The vendor exposes `exclude_tmpdir_env_var` and
`exclude_slash_tmp` for this; FLEETOR sets neither, and an operator who does keeps it.

**The feature key spelling is the vendor's own**, quoted from `codex features --help`:
`--disable <FEATURE>` is documented as "Equivalent to `-c features.<name>=false`".

**Noted, and deliberately not acted on: `codex sandbox --allow-unix-socket <PATH>`
exists.** It is a flag on the `sandbox` *subcommand* — a targeted allowance of exactly the
shape C5 went looking for and did not find. It has **no configuration-key equivalent**, so
a codex TUI pane cannot be given one, and a pane is what the fleet runs. C5 stands
unchanged: `sandbox_workspace_write.network_access` is the only lever a pane can be
configured with, and it is all-or-nothing. If a later build exposes this as a config key,
that is the measurement that would reopen it.

Also observed while reading the socket flags: `codex sandbox -C <dir>` **requires**
`--permission-profile <NAME>`, which is the newer generation. Another reason the legacy
keys are what ship (C7).

---

## Appended by #29 — the credential channel, and the two names codex calls auth

Measured on the same build, `codex-cli 0.153.4`, with `codex doctor --json` and
nothing else. All four are gated in `src-tauri/tests/vendor_binary_tier.rs`'s
credential arm rather than only written down. Zero tokens: `doctor` requests no
completion, and its provider *reachability* probe is read from nowhere.

**`checks["auth.credentials"]` is the instrument, and it names the variable.** With
a `[model_providers.*]` carrying `env_key = "X"` selected by `model_provider`, the
check reports `auth is provided by the active model provider` and
`details["provider auth env var"] = "X (present)"` or `"X (missing)"`. A provider
with `experimental_bearer_token` instead reports `ok` and names no variable. Both
mechanisms work; the first is the one that keeps the secret out of a file.

**The fleet's entry cannot be satisfied by the operator's own key.** With
`env_key = "FLEETOR_CODEX_KEY"` selected and that variable **absent**, `doctor`
fails with `active model provider auth env var is missing` — *even with
`CODEX_API_KEY` present in the environment*, which the same report lists under
`auth env vars present`. There is no fallback from a named `env_key` to an ambient
auth variable. This is the property that makes checkpoint 5's scrub the second line
of defence rather than the only one, and it is the middle arm of the tier's
credential test for exactly that reason: without it, the positive arm passes just as
well on a seeder that quietly authenticated off the environment.

**Codex recognises two ambient auth variables, by name: `OPENAI_API_KEY` and
`CODEX_API_KEY`.** Probed by setting eight candidates at once against a
`CODEX_HOME` on the default provider; `doctor` listed exactly those two under
`auth env vars present` and ignored `AZURE_OPENAI_API_KEY`, `OPENAI_TOKEN`,
`CHATGPT_API_KEY`, `OPENAI_ACCESS_TOKEN`, `CODEX_TOKEN` and `OPENAI_BASE_URL`.
Those two are `Credentials::scrubbed_env`.

**An unselected provider table is inert.** A `CODEX_HOME` defining
`[model_providers.fleetor]` with `env_key` naming an absent variable, while
`model_provider` names something else, resolves clean — no failure attributable to
the unused table. That is what lets the FLEETOR entry be a spec key list written on
**every** codex pane (C41(a)) while only a fenced seat's `model_provider` selects it.

**Caveat, restated because it now matters twice:** `doctor` reports *reachability*,
not authorization — its probe returned HTTP 401 against a live provider and still
passed, and a provider table with **no** credential field at all reports
`OpenAI auth is not required for the active model provider` and passes. So a seat
whose inherited provider had its credential struck out looks healthy here. That is
the shape of the orchestrator question #33 inherits, written down rather than
discovered later.

---

## Appended by #34 — the diagnostic as the gate's instrument (C8, C14, C47, C58)

Measured on the same build, `codex-cli 0.153.4`, with `codex doctor --json` and
`codex debug models` and nothing else. Gated in `src-tauri/tests/vendor_binary_tier.rs`'s
gate-probe arm rather than only written down. Zero tokens; every fabricated provider is
`127.0.0.1`.

**`codex debug models` is the catalog resolution, and it is free.** `18 ms`, returning
`{"models":[…]}` with `slug`, `display_name`, `visibility` and `priority` per entry —
11 entries on a default `CODEX_HOME`, of which 6 are `visibility = "list"`. The `hide`
entries are ones codex does not offer in its own picker. **This is why the model list is
not read out of `models.json`:** the operator's own config points `model_catalog_json` at
a file of theirs, that value may carry a `~` which resolves against whichever `HOME` the
process runs under (the tilde trap above), and a reader would have to reimplement both
rules to get the same answer.

**`doctor` reports the auth *shape* and not the plan tier.** A fabricated `auth.json` in
the ChatGPT shape, with a syntactically real id token carrying `chatgpt_plan_type: "pro"`,
reads back `stored auth mode = chatgpt` and `stored ChatGPT tokens = true`. There is no
`stored ChatGPT plan` field and the tier appears nowhere in the report. C14 said the gate
would show "ChatGPT &lt;plan&gt;"; what it can honestly show is the shape, and C58 narrows
it on that evidence rather than parsing the credential file.

**The checks the gate reads, by name.** `auth.credentials.details` — `stored auth mode`,
`provider auth env var` (with its `(present)`/`(missing)` suffix), `auth env vars present`.
`sandbox.helpers.details` — `filesystem sandbox`, `network sandbox`, `approval policy`.
`config.load.details` — `model`, `model provider`. `network.websocket_reachability.details`
— `provider name`, which is the operator's own label for a custom provider.
`runtime.provenance.details["current executable"]` — the vendor binary behind whatever was
invoked.

**`doctor` exits non-zero whenever any check fails**, including an unreachable provider or
an update warning, so the exit status is ignored and the report is read. A gate that
checked the status would discard exactly the reports it exists to show.

**Both readings agree, on everything the gate displays (C47).** Run through the cmux shim
and through the resolved vendor binary against the same `CODEX_HOME`: identical auth mode,
model, provider, filesystem sandbox, network sandbox, approval policy and version. The one
field that differs is `sandbox.helpers.details["execve wrapper helper"]`, a per-invocation
temporary path that differs between two runs of the *same* binary — which is why the
comparison is field-by-field over what the gate shows rather than document equality. The
shim is also **3× slower** (0.44 s against 0.13 s on a loopback provider), being a node
wrapper in front of a native binary.
