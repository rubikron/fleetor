# WP-25 (#49) — a plan-backed worker cannot authenticate from inside the Fence

**Verdict: the ticket is blocked on a Tier 1.7 decision, not on implementation.**

Issue #49 asks that a worker seat be able to run on the operator's own plan instead
of the fleet's metered key, opt-in per seat, defaulting to the fleet's key. Its stated
mechanism was the orchestrator's, which is known to work: `CLAUDE_SECURESTORAGE_CONFIG_DIR`
**defined and empty** selects the unsuffixed Keychain service name — the entry the
operator's own `/login` wrote (`orch-config-dir-notes.md` §2) — and the plan-backed
worker was to keep the Fence (a private `HOME`, a PATH with no operator rung) while
no longer having the credential names scrubbed.

The ticket said to measure that rather than infer it. Measured, against a real
interactive `claude` **2.1.263** at `~/.local/bin/claude`, on macOS 24.6.0. It does
not work, and the reason is not Claude Code's.

**A private `HOME` removes the operator's login keychain from the process's keychain
search list.** The credential is still there; the pane can no longer see it.

Zero tokens were spent. Nothing was ever submitted to any pane: every arm spawns an
interactive `claude` in a pty, watches the screen for twelve seconds and SIGKILLs.
Reaching the input box and reading the mode line is the whole answer.

---

## 1. The arms

`examples/plan-worker-spike/probe.py`, run in `~/.fleetor/testbed`. Each arm gets a
fresh fleet-owned `CLAUDE_CONFIG_DIR` seeded with exactly the two keys
`spawn::seed_config_dir` writes (L1), and a fresh private `HOME` seeded with exactly
the `.gitconfig` `spawn::seed_worker_home` writes (D-052).

| Arm | `HOME` | PATH | `CLAUDE_SECURESTORAGE_CONFIG_DIR` | Reached a prompt? | Mode line |
|---|---|---|---|---|---|
| `unfenced-plan` | operator's | operator's | defined, empty | yes | **`Opus 5 (1M context) · Claude Max`** |
| `fenced-plan` | **private** | worker's | defined, empty | yes | `API Usage Billing` + **`Not logged in · Run /login`** |
| `iso-homeonly` | **private** | operator's | defined, empty | yes | `API Usage Billing` + **`Not logged in`** |
| `iso-pathonly` | operator's | worker's | defined, empty | yes | **`Claude Max`** |

`unfenced-plan` is the known-good control: it reproduces WP-14's `securestorage` arm
on today's binary, so the operator's keychain entry is present and readable *right
now*. It is what makes the other three arms mean something.

The two isolation arms are the whole finding. **`HOME` alone flips it**; the worker's
PATH is irrelevant. Every arm reaches the input box — this is the D-062 failure mode
verbatim, a pane that looks perfectly healthy while being logged out, and every
`fleet send` into it would report `accepted`.

## 2. Why — the mechanism, and it is macOS's rather than Claude Code's

The macOS default keychain search list is derived from `$HOME`:

```
$ security list-keychains
    "/Users/<operator>/Library/Keychains/login.keychain-db"
    "/Library/Keychains/System.keychain"

$ env HOME=<private home> security list-keychains
    "/Library/Keychains/System.keychain"
```

The login keychain is simply gone. And the entry `claude` wants lives in it:

```
$ security dump-keychain | grep '"svce".*Claude Code'
    "svce"<blob>="Claude Code-credentials"

$ env HOME=<private home> security dump-keychain | grep '"svce".*Claude Code'
    (nothing)
```

Confirmed positively rather than only by absence — a private `HOME` whose
`Library/Keychains` is a symlink to the operator's restores both the search list and
the entry:

```
$ env HOME=<private home with symlinked Library/Keychains> security list-keychains
    "/Users/<operator>/Library/Keychains/login.keychain-db"
    "/Library/Keychains/System.keychain"
    "svce"<blob>="Claude Code-credentials"
```

So the lever is exact and it is not a Claude Code variable at all. **`CLAUDE_SECURESTORAGE_CONFIG_DIR`
selects *which* service name is read; `HOME` decides *whether any keychain containing
it is searched*.** Setting the first correctly, as `fenced-plan` does, buys nothing
once the second has been replaced.

## 3. The spec field this makes wrong, in the C17 direction

`CLAUDE_CODE_SPEC`'s checkpoint 6 carries `credentials_follow_home: false`
(`src-tauri/src/placement/harness.rs:1155`), documented as:

> Whether this harness's login is reachable only through `HOME`. `false` here, and it
> is the fact C17 renamed this checkpoint over: a fenced Claude Code pane keeps a
> private `HOME` and still authenticates, because its credential arrives through
> `Credentials::token_env` and its keychain entry through `credential_env`.

The first clause is true and the second is false. A fenced pane authenticates today
**only** through `token_env` — the fleet's bearer token, which is an environment
variable and cares nothing for `HOME`. The claim that its *keychain entry* is
reachable through `credential_env` from inside the Fence has never been exercised,
because a fenced pane has `CLAUDE_SECURESTORAGE_CONFIG_DIR` scrubbed and never
attempts that read. It is measured false above.

This is C17's own error in mirror image. C17 renamed checkpoint 6 because "auth
follows `HOME`" had been one vendor's fact treated as universal; the field that
replaced it now records "auth does not follow `HOME`" as a Claude Code fact, and on
macOS the OAuth half of Claude Code's auth does follow `HOME`, transitively, through
a keychain search list nobody was looking at. The token half does not. **One boolean
is answering for two credential channels that behave differently**, which is the same
shape of mistake at one remove.

## 4. What this leaves #49 needing a decision on

Every route to a plan-backed worker gives up something the Fence exists to hold, so
none of them is this ticket's to pick (Tier 1.7):

- **Drop the Fence for plan seats.** Give a plan-backed worker the operator's real
  `HOME`. That is not a narrowing of the Fence, it is its removal on that seat:
  `~/.ssh`, shell profiles, the operator's `~/.claude`, and their tool rungs all
  become reachable by name again on an unattended `--permission-mode auto` pane.
- **Symlink `Library/Keychains` into the private `HOME`.** Measured to work in §2.
  It hands the pane the operator's **entire login keychain**, not the Claude Code
  entry — every credential every application on the machine has stored.
- **Plant a credential file in the pane's config dir.** Claude Code will read a
  `.credentials.json` out of `CLAUDE_CONFIG_DIR` when the keychain is unavailable, so
  this is the narrowest of the three. It also means FLEETOR copying the operator's
  OAuth token out of the keychain and writing it to disk inside a fenced pane's
  directory — which is precisely what D-062 forbids and what #29's byte-scan test
  exists to catch. The scan would go red by design rather than by accident.

The two consequences #49 already names — no per-pane budget anywhere in this codebase,
and quota exhaustion being undetectable because an exhausted pane stops responding
rather than erroring — are unchanged by any of the three and still stand behind
whichever is chosen.

## 5. Reproducing

```bash
python3 examples/plan-worker-spike/probe.py --bin "$HOME/.local/bin/claude"
python3 examples/plan-worker-spike/probe.py --bin "$HOME/.local/bin/claude" --arm fenced-plan
```

All arms are free. Point `--bin` at the real binary rather than the `claude` on PATH:
this machine has a cmux wrapper shim in front of it (C47), and `worker_augmented_path_from`
puts the fleet's rungs first, so the wrapper is not what a FLEETOR pane resolves.

The keychain half needs no probe:

```bash
security list-keychains
env HOME=/tmp/somewhere-else security list-keychains
```

---

# WP-25 (#49), part 2 — the credential cannot be handed in either, for two independent reasons

**Verdict: the env-var route is not viable, and it fails worse than the route it
was meant to replace.** §4's three routes stand unchanged; this is not a fourth.

§4 listed planting a credential *file* in the pane's config dir as the narrowest
way out, and flagged that it puts a token on disk inside a fenced worktree —
exactly what D-062 forbids and what #29's byte-scan exists to catch. The operator
asked the better question: **why copy it at all — can it not be read once, before
the sandbox exists, and handed in?** Nothing written, nothing surviving the
process, no token in a worktree or a backup. And it reuses plumbing that already
exists: `Credentials::token_env` is `ANTHROPIC_AUTH_TOKEN`
(`harness.rs:1162`), which is how a fenced worker receives the fleet's metered key
today (`spawn::worker_command_with`).

Measured, not inferred, against the same interactive `claude` **2.1.263** at
`~/.local/bin/claude` on macOS 24.6.0 that §1 used — the same build, so §1's
table is a live comparison set rather than a stale one.

It does not work. **Two blockers, and they are independent**: either one alone
kills the route, so removing one buys nothing.

Zero tokens were spent. The pty arms never submit — spawn, watch the screen for
twelve seconds, SIGKILL, as `probe.py` does. The one network arm sends a
deliberately malformed body and reads only the HTTP status, so no inference is
ever requested.

## 6. The arms

`examples/plan-worker-spike/probe_env_token.py`, a sibling of `probe.py` and not
an edit to it. Same conventions: fresh seeded `CLAUDE_CONFIG_DIR` per arm (L1),
fresh private `HOME` (D-052), loud PASS/FAIL, exit code is the failure count,
loud skip when the binary or the keychain entry is absent.

| Arm | `HOME` | `ANTHROPIC_AUTH_TOKEN` | Prompt? | `Not logged in`? | Mode line |
|---|---|---|---|---|---|
| `control-unfenced` | operator's | — | yes | no | **`Opus 5 (1M context) · Claude Max`** |
| `control-fenced` | private | — | yes | **yes** | `Opus 5 (1M context) · API Usage Billing` |
| `fenced-oauth-access` | private | access token alone | yes | **no** | `Opus 5 (1M context) · API Usage Billing` |
| `fenced-oauth-blob` | private | `claudeAiOauth` as JSON | yes | **no** | `Opus 5 (1M context) · API Usage Billing` |

The two controls reproduce §1's `unfenced-plan` and `fenced-plan` readings
exactly, which is what makes the other two mean anything.

## 7. Blocker 1 — `ANTHROPIC_AUTH_TOKEN` selects the metered path, and an OAuth token is the wrong credential for it

**The answer to the operator's question is in the banner of the arms that carry
the credential: `API Usage Billing`.** Handing the value in does not put the pane
on the plan; the variable's *presence* is what chooses the metered path, and the
pane then presents an OAuth access token on the API-key path. Those are the two
different paths §1 already warned the boolean was conflating — measured here from
the other side.

**The mode line cannot see the difference, and that is the finding.**
`fenced-oauth-access` and `fenced-oauth-blob` produced **byte-identical output —
1537 bytes each**. A JSON document is not a bearer token by any reading, so a
screen that renders it the same as a real access token is not reporting on the
credential at all; it is reporting that the variable is set. Reaching the input
box with no login warning is therefore not evidence of anything here, which is
why this probe does not rest on it.

What does settle it is a zero-token request to the credential's own issuer, with
a body malformed on purpose so that nothing is billable:

```
POST https://api.anthropic.com/v1/messages   {"probe":"malformed-on-purpose"}
  Authorization: Bearer <access token>   →  HTTP 401
  x-api-key: <access token>              →  HTTP 401
```

401 is the credential being rejected before the body is ever looked at. Both
spellings, with `anthropic-beta: oauth-2025-04-20` set. **Stated as a limit
rather than glossed:** this arm has no known-good credential to prove its other
branch with, so "a working credential would have returned 400" is reasoning from
the vendor's documented status codes, not a reading taken here. It corroborates
the banner; it is not independent of it.

## 8. Blocker 2 — the lifetime, which would kill the route even if blocker 1 vanished

The keychain item is **an OAuth credential blob, not a bare key**. Under
`claudeAiOauth`: `accessToken`, `refreshToken`, `expiresAt`,
`refreshTokenExpiresAt`, `scopes`, `subscriptionType`, `rateLimitTier`. Both
tokens are 108 chars, `sk-ant-oat…`/`sk-ant-ort…` family. Measured at the time of
writing:

| | remaining |
|---|---|
| access token | **0.87 h** |
| refresh token | **430.4 h (17.9 d)** |

**A credential read at spawn is good for under an hour, and a fenced pane cannot
reach the keychain to refresh it** — that is §2's finding, unchanged and now
load-bearing in a second place. A refresh token exists and has an 18-day window,
so a rotating design is *conceivable*, but it would need something outside the
Fence holding the keychain and pushing new values into a running pane, which is
not an environment variable and not this ticket.

**This is D-062's failure mode delayed, not fixed, and delay is the expensive
part.** §1 recorded that every arm reaches the input box and every `fleet send`
reports `accepted` into a logged-out pane. Handing the token in makes that
strictly worse: the `Not logged in · Run /login` string that made the failure
*legible* on `control-fenced` is **absent** on both token arms. The route removes
the only warning the previous route left behind, and would have removed it an
hour into a run rather than at spawn.

One sample gives the *remaining* lifetime, which is what the decision needs. It
does not give the issue-to-expiry interval, so the access token's full TTL is
**not measured here** and should not be quoted from this note.

## 9. What was and was not touched

The operator's `~/.claude` and their keychain were read only —
`security find-generic-password -w` does not modify the item, and the fenced panes
were verified to have written **no credential to disk** in their config dirs. No
token material appears in this note, in the probe's output, or anywhere in the
repository; `probe_env_token.py` enforces that through one `redact` function that
every print path goes through, and the pty transcripts are filtered before they
land. The probe's `work/` directory is gitignored, as `probe.py`'s already was.

One thing worth carrying forward: **the keychain item holds more than Claude
Code's own credential.** On this machine it also carries third-party MCP OAuth
tokens (`mcpOAuth`, two servers, with their own access tokens, refresh tokens and
a client secret). Any future design that hands "the keychain value" to a pane
must hand the `claudeAiOauth` object and not the item — `fenced-oauth-blob`
deliberately narrows to the former for exactly this reason.

## 10. Reproducing

```bash
python3 examples/plan-worker-spike/probe_env_token.py --arm expiry            # instant
python3 examples/plan-worker-spike/probe_env_token.py --arm http-401-or-400   # instant
python3 examples/plan-worker-spike/probe_env_token.py --bin "$HOME/.local/bin/claude"
```

All arms are free. `--bin` for the same reason §5 gives.
