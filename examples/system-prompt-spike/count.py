#!/usr/bin/env python3
"""Measure a rendered brief — the WP-09 budget baseline.

Renders `prompts/*.md` the way `fleetor_core::brief` does (plain `{name}`
substitution, fragments composed in first) and reports size. With a key
available it also asks the worker endpoint's own tokenizer, so the number for a
worker brief is the number that model will actually be charged for.

The substitution here is a deliberate 6-line mirror of `brief.rs::render`
rather than a call into it: this is a measuring tape in `examples/`, and
`building.md` §3 says crates never import from here — so the dependency cannot
run the other way either.

Usage:
    python3 count.py
    python3 count.py --json
"""

import argparse
import json
import os
import sys
import urllib.error
import urllib.request
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
PROMPTS = REPO / "prompts"

WORKER_SLOTS = [1, 2, 3, 4]
ROSTER = ["orch"] + [f"worker-{n}" for n in WORKER_SLOTS]
DEEPSEEK_COUNT_URL = "https://api.deepseek.com/anthropic/v1/messages/count_tokens"
MODEL_FLASH = "deepseek-v4-flash"


def read_env_file(start: Path, key: str):
    for directory in [start, *start.parents]:
        candidate = directory / ".env"
        if not candidate.is_file():
            continue
        for line in candidate.read_text(encoding="utf-8", errors="replace").splitlines():
            if line.strip().startswith(f"{key}="):
                value = line.split("=", 1)[1].strip().strip('"').strip("'")
                if value:
                    return value
    return None


def peer_list(me: str) -> str:
    names = [p for p in ROSTER if p != me]
    if not names:
        return "nobody else, yet"
    if len(names) == 1:
        return names[0]
    return ", ".join(names[:-1]) + f" and {names[-1]}"


def fragment(name: str) -> str:
    return (PROMPTS / name).read_text().rstrip()


def render(template: str, variables: dict) -> str:
    text = template
    for name, value in variables.items():
        text = text.replace("{" + name + "}", value)
    return text


def shared(cwd: str) -> dict:
    """Every slot filled the same way for both roles, fragments first."""
    variables = {"cwd": cwd}
    for name, file in (
        ("delivery_contract", "delivery-contract.md"),
        ("broadcast_rule", "broadcast-rule.md"),
        ("scaffolding", "scaffolding.md"),
        ("vision_tenets", "vision-tenets.md"),
    ):
        if (PROMPTS / file).is_file():
            variables[name] = fragment(file)
    return variables


def briefs() -> dict:
    orch = render(
        (PROMPTS / "orch.md").read_text(),
        {**shared("/Users/you/your-repo"), "workers": peer_list("orch")},
    )
    worker = render(
        (PROMPTS / "worker.md").read_text(),
        {
            **shared("~/.fleetor/worktrees/worker-1"),
            "me": "worker-1",
            "peers": peer_list("worker-1"),
        },
    )
    return {"orch": orch, "worker": worker}


def count_tokens(text: str, key: str):
    body = json.dumps(
        {"model": MODEL_FLASH, "system": text, "messages": [{"role": "user", "content": "x"}]}
    ).encode()
    request = urllib.request.Request(
        DEEPSEEK_COUNT_URL,
        data=body,
        headers={
            "content-type": "application/json",
            "x-api-key": key,
            "authorization": f"Bearer {key}",
            "anthropic-version": "2023-06-01",
        },
    )
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            return json.loads(response.read()).get("input_tokens")
    except (urllib.error.URLError, urllib.error.HTTPError, TimeoutError) as exc:
        print(f"[count] tokenizer unavailable ({exc}) — reporting size only", file=sys.stderr)
        return None


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--json", action="store_true")
    args = p.parse_args()

    key = os.environ.get("DEEPSEEK_API_KEY") or read_env_file(REPO, "DEEPSEEK_API_KEY")
    report = {}
    for role, text in briefs().items():
        left = [line for line in text.splitlines() if "{" in line and "}" in line]
        report[role] = {
            "chars": len(text),
            "words": len(text.split()),
            "lines": len(text.splitlines()),
            "tokens_deepseek": count_tokens(text, key) if key else None,
            "unrendered_lines": left,
        }

    if args.json:
        print(json.dumps(report, indent=2))
        return
    for role, stats in report.items():
        tokens = stats["tokens_deepseek"]
        print(
            f"{role:7} {stats['chars']:6} chars  {stats['words']:5} words  "
            f"{stats['lines']:3} lines  {tokens if tokens is not None else '?':>6} tokens"
        )
        for line in stats["unrendered_lines"]:
            print(f"        !! looks unrendered: {line.strip()[:90]}")


if __name__ == "__main__":
    main()
