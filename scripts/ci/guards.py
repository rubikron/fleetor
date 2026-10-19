#!/usr/bin/env python3
"""PR guards that need the base branch (D-124): G3, the decision lock, the CI surface.

usage: guards.py <base-ref> [--reviewed]   (--reviewed: the PR has the ci-reviewed label)
"""
import re
import subprocess
import sys

MIGRATIONS = "crates/fleetor-db/src/migrations.rs"
LABEL = "ci-reviewed"
SURFACE = (".github/", "scripts/ci/", "rust-toolchain.toml", "rustfmt.toml", ".git-blame-ignore-revs")


def git(*args):
    return subprocess.run(["git", *args], check=True, capture_output=True, text=True).stdout


def at(ref, path):
    r = subprocess.run(["git", "show", f"{ref}:{path}"], capture_output=True, text=True)
    return r.stdout if r.returncode == 0 else None


def migrations_body(src):
    m = re.search(r"pub const MIGRATIONS: &\[&str\] = &\[(.*)\n\];", src, re.S)
    return m and m[1]


def without_status(text):
    return re.sub(r"(?m)^status:.*$", "", text)


def guards(text):
    m = re.search(r"(?m)^guard:\s*(.+)$", text.split("\n---", 1)[0])
    return [p.strip() for p in m[1].split(",")] if m else []


def main():
    base, reviewed = sys.argv[1], "--reviewed" in sys.argv
    merge_base = git("merge-base", base, "HEAD").strip()
    changed = git("diff", "--name-only", "--no-renames", f"{merge_base}...HEAD").split()
    hard, soft = [], []

    old, new = at(merge_base, MIGRATIONS), at("HEAD", MIGRATIONS)
    if old is not None:
        old_body, new_body = migrations_body(old), new and migrations_body(new)
        if old_body is None or new_body is None or not new_body.startswith(old_body):
            soft.append(f"G3: an existing migration in {MIGRATIONS} changed; append a new one instead")

    decisions = [p for p in git("ls-tree", "--name-only", merge_base, "decisions/").split()
                 if re.fullmatch(r"decisions/D-\d+\.md", p)]
    guard_paths = set()
    for path in decisions:
        before, after = at(merge_base, path), at("HEAD", path)
        guard_paths.update(guards(before))
        if after is None:
            hard.append(f"{path} was deleted; supersede it instead (CLAUDE.md)")
        elif without_status(before) != without_status(after):
            soft.append(f"{path}: changed beyond its status line")
    for path in git("ls-files", "decisions/").split():
        guard_paths.update(guards(at("HEAD", path) or ""))

    touched = [p for p in changed if p.startswith(SURFACE) or p in guard_paths]
    if touched:
        decided = [p for p in changed if re.fullmatch(r"decisions/D-\d+\.md", p) and at("HEAD", p)]
        if not decided:
            hard.append("the CI surface changed without a decision: " + ", ".join(touched))
        soft.append("the CI surface changed: " + ", ".join(touched))

    for p in hard:
        print(f"FAIL  {p}")
    for p in soft:
        print(f"{'OK  ' if reviewed else 'FAIL'}  {p}" + ("" if reviewed else f"  (needs the {LABEL} label)"))
    if not (hard or soft):
        print("OK    nothing guarded changed")
    sys.exit(1 if hard or (soft and not reviewed) else 0)


if __name__ == "__main__":
    main()
