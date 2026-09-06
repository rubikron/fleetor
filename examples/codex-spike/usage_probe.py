#!/usr/bin/env python3
"""Does codex persist per-turn token usage in its thread store? (#38, C12's open spike)

    python3 examples/codex-spike/usage_probe.py --spend

*** THIS PROBE SPENDS REAL MONEY. Every other probe in this directory spends
*** zero tokens (C13). This one cannot: the question is what codex writes to
*** disk *after a completed assistant turn*, and no capture server completes
*** one. It runs exactly ONE tiny turn against the operator's configured real
*** endpoint. Without `--spend` (or FLEETOR_SPEND_OK=1) it prints the banner
*** and exits 0 without contacting anything, so CI cannot bill anyone.

Measured against `codex-cli 0.153.4`.

Eight arms, PASS/FAIL against what C61 recorded:

  1. window-published    the catalog publishes `context_window` per model (C12)
  2. schema-no-usage     no migration adds a usage/token column to `thread_items`
                         or `thread_turns` — read out of the vendor binary, free
  3. turn-completes      one real `codex exec` turn completes with a reply
  4. store-written       `thread_history_1.sqlite` appears in the scratch home
  5. no-usage-in-store   NO persisted thread-store row carries usage — the answer
  6. usage-in-state      `state_5.sqlite` `threads.tokens_used` carries the total
  7. usage-in-rollout    the rollout JSONL carries a per-turn `token_usage_record`
  8. window-is-derated   codex's own `token_count` reports a window 5% under the
                         catalog's, so the two denominators disagree

Arms 1-2 are free. Only arm 3 talks to the endpoint; 4-8 read what it left on
disk. `--reuse` re-reads a previous run's scratch home and spends nothing.

Per C47 the vendor binary is resolved **absolutely** — the `codex` on PATH is a
cmux shim that injects flags. Per every earlier arm, the operator's real
`~/.codex` is never written to: config, catalog and credential are *copied* into
a fabricated `CODEX_HOME` and the turn runs entirely inside it.

Exit code is the number of failed arms. Skips cleanly (exit 0, loud message)
when the vendor binary is absent — C13's stated rule.
"""

import glob, json, os, re, shutil, sqlite3, subprocess, sys, time

BUILD = "codex-cli 0.153.4"
ROOT = "/tmp/codex-usage-spike"
OPERATOR_HOME = os.path.expanduser("~/.codex")

# C47: the PATH `codex` is a cmux shim. Resolve the real binary absolutely.
VENDOR = ("/opt/homebrew/lib/node_modules/@openai/codex/node_modules/"
          "@openai/codex-darwin-arm64/vendor/aarch64-apple-darwin/bin/codex")

PROMPT = "Reply with exactly: ok"

FAILURES = []


def check(arm, ok, detail=""):
    print(f"  {'PASS' if ok else 'FAIL'}  {arm}{'  — ' + detail if detail else ''}")
    if not ok:
        FAILURES.append(arm)


BANNER = """
================================================================================
  THIS PROBE SPENDS REAL MONEY.

  It runs ONE assistant turn against the endpoint configured in the operator's
  ~/.codex, using the operator's own credential. Every other probe in
  examples/codex-spike/ spends zero tokens; this one is the documented
  exception (#38), because the question is what codex writes to its thread
  store after a turn *completes*, and a capture server never completes one.

  One turn, smallest prompt, lowest reasoning effort. Recorded cost for the
  reference run is in docs/notes/codex-usage-notes.md.
================================================================================
"""


# --- arm 1: the window the harness states rather than asserts -----------------


def arm_window():
    """codex publishes `context_window` per model, so checkpoint 11's constant is
    a fact the harness reads out of the vendor, not one FLEETOR asserts (C12)."""
    out = subprocess.run([VENDOR, "debug", "models"], capture_output=True, text=True,
                         env={**os.environ, "CODEX_HOME": OPERATOR_HOME}).stdout
    try:
        models = json.loads(out)["models"]
    except Exception:
        return check("window-published", False, "catalog did not parse")
    windows = {m.get("id") or m.get("slug"): m.get("context_window") for m in models}
    ok = bool(windows) and all(isinstance(v, int) and v > 0 for v in windows.values())
    check("window-published", ok, ", ".join(f"{k}={v}" for k, v in list(windows.items())[:3]))


# --- arm 2: the schema, read out of the binary rather than out of a run -------


USAGE_WORD = re.compile(r"(usage|token)", re.I)


def arm_schema():
    """Every migration the vendor ships, grepped for a usage column on the two
    tables C12 named. Free: `strings` on the binary, no run, no endpoint."""
    if not os.path.exists(VENDOR):
        return check("schema-no-usage", False, "vendor binary absent")
    strings = subprocess.run(["strings", "-n", "4", VENDOR],
                             capture_output=True, text=True).stdout
    ddl = [ln for ln in strings.splitlines()
           if re.search(r"(CREATE TABLE|ALTER TABLE) thread_(items|turns)", ln)]
    offenders = [ln for ln in ddl if USAGE_WORD.search(ln)]
    check("schema-no-usage", not offenders,
          f"{len(ddl)} statements touch thread_items/thread_turns, "
          f"{len(offenders)} name a usage column")


# --- the fabricated CODEX_HOME ------------------------------------------------


def build_scratch():
    """The operator's provider, catalog and credential, copied into a scratch home.

    Copied, never referenced in place, and the operator's ~/.codex is opened
    read-only — every earlier arm held this line and so does this one.
    """
    shutil.rmtree(ROOT, ignore_errors=True)
    home, wt = os.path.join(ROOT, "home"), os.path.join(ROOT, "wt")
    os.makedirs(home)
    os.makedirs(wt)

    for name in ("auth.json", "models.json"):
        src = os.path.join(OPERATOR_HOME, name)
        if os.path.exists(src):
            shutil.copy(src, os.path.join(home, name))

    config = open(os.path.join(OPERATOR_HOME, "config.toml")).read()
    # Point the catalog at our copy, and trust only the scratch worktree.
    config = config.replace('model_catalog_json = "~/.codex/models.json"',
                            f'model_catalog_json = "{home}/models.json"')
    config = re.sub(r'\[projects\."[^"]*"\]\ntrust_level = "trusted"\n', "", config)
    config += f'\n[projects."{wt}"]\ntrust_level = "trusted"\n'
    open(os.path.join(home, "config.toml"), "w").write(config)
    return home, wt


# --- arm 3: the one turn that costs money ------------------------------------


def arm_turn(home, wt):
    started = time.time()
    proc = subprocess.run(
        [VENDOR, "exec", "--cd", wt, "--skip-git-repo-check",
         "-c", 'sandbox_mode="read-only"',
         "-c", 'approval_policy="never"',
         "-c", 'model_reasoning_effort="low"',
         PROMPT],
        capture_output=True, text=True,
        env={**os.environ, "CODEX_HOME": home},
        timeout=180,
    )
    elapsed = time.time() - started
    body = proc.stdout + proc.stderr
    open(os.path.join(ROOT, "exec.log"), "w").write(body)
    ok = proc.returncode == 0 and "ok" in proc.stdout.lower()
    check("turn-completes", ok, f"exit {proc.returncode} in {elapsed:.1f}s")
    if not ok:
        print("     --- last lines of the run ---")
        for ln in body.strip().splitlines()[-12:]:
            print(f"     {ln}")
    return ok


# --- arms 4-5: what landed on disk -------------------------------------------


def arm_store(home):
    stores = sorted(glob.glob(os.path.join(home, "thread_history_*.sqlite")))
    check("store-written", bool(stores),
          ", ".join(os.path.basename(s) for s in stores) or "no thread_history_*.sqlite")
    if not stores:
        return None
    return stores[-1]


def arm_no_usage_in_store(store):
    """The answer, with the literal rows as evidence: every column of both tables,
    and every persisted `item_json` walked for a usage payload."""
    con = sqlite3.connect(f"file:{store}?mode=ro", uri=True)

    for table in ("thread_turns", "thread_items"):
        cols = [r[1] for r in con.execute(f"PRAGMA table_info({table})")]
        print(f"     {table}: {', '.join(cols)}")

    items = con.execute(
        "SELECT item_type, item_json FROM thread_items ORDER BY rollout_ordinal").fetchall()
    kinds = {}
    for t, _ in items:
        kinds[t] = kinds.get(t, 0) + 1
    print(f"     item_type counts: {kinds or '{}'}")

    carriers = []
    for item_type, item_json in items:
        try:
            doc = json.loads(item_json)
        except Exception:
            continue
        found = {}

        def walk(node, path):
            if isinstance(node, dict):
                for k, v in node.items():
                    if USAGE_WORD.search(k) and isinstance(v, (int, dict)):
                        found[f"{path}.{k}".lstrip(".")] = v
                    walk(v, f"{path}.{k}".lstrip("."))
            elif isinstance(node, list):
                for i, v in enumerate(node):
                    walk(v, f"{path}[{i}]")

        walk(doc, "")
        if found:
            carriers.append((item_type, found))

    for item_type, found in carriers:
        print(f"     usage payload on item_type={item_type!r}:")
        print(f"       {json.dumps(found, sort_keys=True)[:600]}")
    if not carriers:
        print("     no persisted thread-store row carries a token/usage field")
    con.close()

    # C61 recorded the answer as NO. The arm fails if that flips, in either
    # direction — a flip is the whole reason this file is re-runnable.
    check("no-usage-in-store", not carriers,
          "absent, as recorded" if not carriers
          else f"PRESENT on {', '.join(t for t, _ in carriers)} — C61 and #41 must be re-read")


def arm_usage_in_state(home):
    """`state_5.sqlite`'s `threads` row: a cumulative `tokens_used`, and the
    `rollout_path` that links a thread to the file carrying its per-turn rows."""
    state = os.path.join(home, "state_5.sqlite")
    if not os.path.exists(state):
        return check("usage-in-state", False, "no state_5.sqlite")
    con = sqlite3.connect(f"file:{state}?mode=ro", uri=True)
    cols = [r[1] for r in con.execute("PRAGMA table_info(threads)")]
    if "tokens_used" not in cols:
        con.close()
        return check("usage-in-state", False, "threads has no tokens_used column")
    rows = con.execute("SELECT id, tokens_used, rollout_path FROM threads").fetchall()
    con.close()
    for tid, used, path in rows:
        print(f"     threads.tokens_used = {used}  (thread {tid})")
        print(f"     threads.rollout_path = {path}")
    check("usage-in-state", bool(rows) and all(r[1] for r in rows),
          f"{len(rows)} thread row(s), tokens_used populated")
    return rows[0][2] if rows else None


def arm_usage_in_rollout(rollout):
    """The rollout JSONL beside the store: codex's own per-turn record. This is
    the vendor's number verbatim — reading it is not a reconstruction."""
    if not rollout or not os.path.exists(rollout):
        check("usage-in-rollout", False, "no rollout file")
        check("window-is-derated", False, "no rollout file")
        return
    records, token_counts = [], []
    for line in open(rollout):
        try:
            doc = json.loads(line)
        except Exception:
            continue
        if doc.get("type") == "token_usage_record":
            records.append(doc["payload"])
        payload = doc.get("payload") or {}
        if doc.get("type") == "event_msg" and payload.get("type") == "token_count":
            token_counts.append(payload["info"])

    for rec in records:
        print(f"     token_usage_record turn={rec.get('turn_id')} "
              f"usage={json.dumps(rec.get('usage'), sort_keys=True)}")
    check("usage-in-rollout", bool(records),
          f"{len(records)} token_usage_record row(s), "
          f"{len(token_counts)} token_count event(s)")

    if not token_counts:
        return check("window-is-derated", False, "no token_count event")
    reported = token_counts[-1].get("model_context_window")
    catalog = 1048576
    print(f"     token_count.model_context_window = {reported} "
          f"vs catalog context_window = {catalog}")
    check("window-is-derated", reported is not None and reported < catalog,
          f"reported is {reported / catalog:.1%} of the catalog window"
          if reported else "absent")


def provenance():
    """What the binary says it is, read the way #34 reads it. C47's rule says an
    arm that could depend on the binary resolves it absolutely and records which
    one it spent through; this is that record, and it costs nothing."""
    out = subprocess.run([VENDOR, "doctor", "--json"], capture_output=True, text=True,
                         env={**os.environ, "CODEX_HOME": OPERATOR_HOME}).stdout

    def walk(node):
        if isinstance(node, dict):
            if node.get("id") == "runtime.provenance":
                return (node.get("details") or {}).get("current executable")
            for v in node.values():
                if (hit := walk(v)):
                    return hit
        elif isinstance(node, list):
            for v in node:
                if (hit := walk(v)):
                    return hit
        return None

    try:
        return walk(json.loads(out)) or "unreported"
    except Exception:
        return "unreported"


def main():
    reuse = "--reuse" in sys.argv
    print(BANNER)
    if reuse:
        print(f"--reuse: re-reading {ROOT} from a previous run. NOTHING IS BILLED.\n")
    elif not (("--spend" in sys.argv) or os.environ.get("FLEETOR_SPEND_OK") == "1"):
        print("REFUSING: re-run with --spend (or FLEETOR_SPEND_OK=1) to authorize the turn,\n"
              "or with --reuse to re-read a previous run's scratch home for free.\n"
              "Nothing was contacted and nothing was billed.")
        return 0
    if not os.path.exists(VENDOR):
        print(f"SKIP: the vendor binary is not at {VENDOR}.\n"
              f"      This tier is measured against {BUILD}.")
        return 0
    if not os.path.exists(os.path.join(OPERATOR_HOME, "config.toml")):
        print(f"SKIP: no {OPERATOR_HOME}/config.toml to copy a provider out of.")
        return 0

    actual = subprocess.run([VENDOR, "--version"], capture_output=True, text=True).stdout.strip()
    print(f"codex usage spike — recorded against {BUILD}, running against {actual}\n")
    if actual != BUILD:
        print(f"  NOTE: build differs from the recorded one. A failure below may be drift,\n"
              f"        not a regression — re-record rather than patching around it.\n")

    print(f"  binary: {VENDOR}")
    print(f"  self-reported `runtime.provenance.current executable`: {provenance()}\n")

    arm_window()
    arm_schema()

    if reuse:
        home = os.path.join(ROOT, "home")
        if not os.path.isdir(home):
            print(f"\nSKIP: --reuse needs a previous run's {home}; none is there.")
            return len(FAILURES)
    else:
        home, wt = build_scratch()
        arm_turn(home, wt)

    store = arm_store(home)
    if store:
        arm_no_usage_in_store(store)
    else:
        check("no-usage-in-store", False, "no store to read")
    rollout = arm_usage_in_state(home)
    arm_usage_in_rollout(rollout)

    print(f"\n{len(FAILURES)} failed" + (f": {', '.join(FAILURES)}" if FAILURES else ""))
    return len(FAILURES)


if __name__ == "__main__":
    sys.exit(main())
