//! The 10 varied probe tickets (BUILDING Phase 0 exit test).
//!
//! Each ticket is a self-contained `-p` prompt — the message a human would type
//! — plus a machine-checkable `verify` shell command run against the mutated
//! repo afterwards. The verify gate is independent of whatever the worker
//! *claims*: it re-checks the acceptance criterion directly. Tickets are sized
//! small (Phase 0 discipline) and chosen to spread across Claude Code's tool
//! surface: Read, Write, Edit, MultiEdit, Bash, Grep, Glob, JSON, multi-file.

#[derive(Debug, Clone)]
pub struct Ticket {
    pub id: &'static str,
    /// Coarse category, for the report's per-category breakdown.
    pub category: &'static str,
    /// Primary tools the ticket is designed to exercise.
    pub targets: &'static str,
    /// The user message written to the worker.
    pub prompt: &'static str,
    /// Shell command (run via `bash -c` in the repo root) that exits 0 iff the
    /// acceptance criterion is met.
    pub verify: &'static str,
}

pub fn all() -> Vec<Ticket> {
    vec![
        Ticket {
            id: "T-01-bugfix-average",
            category: "bugfix",
            targets: "Read, Edit, Bash",
            prompt: "The function `average(xs)` in mathx/ops.py is wrong: it returns \
                sum/len + 1 instead of the true mean. Fix it so it returns the correct \
                arithmetic mean. Verify your fix by running python3.",
            verify: "python3 -c \"from mathx.ops import average; assert abs(average([2,4,6]) - 4.0) < 1e-9; assert abs(average([10]) - 10.0) < 1e-9; print('ok')\"",
        },
        Ticket {
            id: "T-02-add-sub",
            category: "add-feature",
            targets: "Edit, Write, Bash",
            prompt: "Add a new function `sub(a, b)` that returns a - b to mathx/ops.py, \
                export it from mathx/__init__.py, and add a unittest for it in \
                tests/test_ops.py. Keep the existing tests passing.",
            verify: "python3 -c \"from mathx.ops import sub; assert sub(5,3)==2; assert sub(0,4)==-4; print('ok')\" && python3 -m unittest discover -s tests -q",
        },
        Ticket {
            id: "T-03-grep-slugify",
            category: "grep-driven",
            targets: "Grep, Read, Edit",
            prompt: "Find where `slugify` is defined and improve it so that runs of \
                multiple spaces collapse into a single dash (e.g. 'a   b' -> 'a-b'). \
                Do not break the existing behavior.",
            verify: "python3 -c \"from mathx.strings import slugify; assert slugify('a   b')=='a-b'; assert slugify('Hello World')=='hello-world'; print('ok')\"",
        },
        Ticket {
            id: "T-04-rename-mul",
            category: "refactor-rename",
            targets: "Grep, MultiEdit, Edit, Bash",
            prompt: "Rename the function `mul` to `multiply` everywhere it appears in \
                the codebase — its definition, all imports/exports, and all tests — so \
                the whole test suite still passes. Leave no reference to the old name.",
            verify: "python3 -c \"from mathx.ops import multiply; assert multiply(3,4)==12\" && python3 -m unittest discover -s tests -q && ! grep -rInw 'mul' mathx tests",
        },
        Ticket {
            id: "T-05-new-module",
            category: "new-module",
            targets: "Write, Edit, Bash",
            prompt: "Create a new module mathx/geometry.py with a function \
                `area_rectangle(w, h)` returning w*h, and export it from \
                mathx/__init__.py.",
            verify: "python3 -c \"from mathx.geometry import area_rectangle; assert area_rectangle(3,4)==12\" && python3 -c \"import mathx; assert hasattr(mathx,'area_rectangle')\"",
        },
        Ticket {
            id: "T-06-json-edit",
            category: "json-edit",
            targets: "Read, Edit, Bash",
            prompt: "Add the string \"geometry\" to the `features` array in \
                data/config.json. Keep the file valid JSON.",
            verify: "python3 -c \"import json; d=json.load(open('data/config.json')); assert 'geometry' in d['features']; assert d['name']=='mathx'; print('ok')\"",
        },
        Ticket {
            id: "T-07-glob-count",
            category: "glob-count",
            targets: "Glob, Edit",
            prompt: "Count how many .py files exist under the mathx/ directory, then \
                add a line to README.md of the exact form 'Modules: N' (where N is that \
                count) in the Modules section.",
            verify: "python3 -c \"import glob,re; n=len(glob.glob('mathx/*.py')); t=open('README.md').read(); m=re.search(r'Modules:\\s*(\\d+)', t); assert m and int(m.group(1))==n, (m and m.group(1), n); print('ok')\"",
        },
        Ticket {
            id: "T-08-bugfix-clamp",
            category: "bugfix",
            targets: "Read, Edit, Bash",
            prompt: "The function `clamp(x, lo, hi)` in mathx/ops.py ignores its upper \
                bound. Fix it so the result is constrained to the range [lo, hi].",
            verify: "python3 -c \"from mathx.ops import clamp; assert clamp(10,0,5)==5; assert clamp(-1,0,5)==0; assert clamp(3,0,5)==3; print('ok')\"",
        },
        Ticket {
            id: "T-09-write-tests",
            category: "test-authoring",
            targets: "Write, Bash",
            prompt: "Add a new unittest file tests/test_ops_extra.py that tests both \
                `add` and `mul` with at least two assertions each, then run it to \
                confirm it passes.",
            verify: "test -f tests/test_ops_extra.py && python3 -m unittest tests.test_ops_extra -q",
        },
        Ticket {
            id: "T-10-refactor-validate",
            category: "refactor-multi",
            targets: "Read, Grep, Edit, Bash",
            prompt: "Add a private helper `_check_num(x)` to mathx/ops.py that raises \
                TypeError if x is not an int or float, and call it on both arguments at \
                the start of `add` and `mul`. Keep all existing tests passing.",
            verify: "python3 -c \"from mathx.ops import add, mul\nfor f in (add, mul):\n    try:\n        f('x', 1); raise SystemExit('no TypeError')\n    except TypeError:\n        pass\nassert add(2,3)==5 and mul(2,3)==6\nprint('ok')\" && python3 -m unittest discover -s tests -q",
        },
    ]
}
