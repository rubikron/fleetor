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
