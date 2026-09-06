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
