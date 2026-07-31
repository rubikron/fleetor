//! The throwaway toy repo the probe drives workers against.
//!
//! Ships GREEN under `python3 -m unittest` (stdlib only — no pytest dependency,
//! so verification never fails for environment reasons). It carries a few latent
//! bugs in functions no base test covers, which the bugfix tickets target; the
//! suite stays green until a ticket touches them.

use anyhow::Result;
use std::fs;
use std::path::Path;

/// (relative path, contents) for every file in the toy repo.
const FILES: &[(&str, &str)] = &[
    (
        "mathx/__init__.py",
        "from .ops import add, mul, average, clamp\nfrom .strings import slugify, titlecase\n",
    ),
    (
        "mathx/ops.py",
        r#"def add(a, b):
    return a + b


def mul(a, b):
    return a * b


def average(xs):
    # BUG: off-by-one — the +1 makes this not a real mean.
    return sum(xs) / len(xs) + 1


def clamp(x, lo, hi):
    # BUG: upper bound is ignored.
    return max(lo, x)
"#,
    ),
    (
        "mathx/strings.py",
        r#"def slugify(s):
    return s.strip().lower().replace(" ", "-")


def titlecase(s):
    return " ".join(w.capitalize() for w in s.split())
"#,
    ),
    (
        "tests/test_ops.py",
        r#"import unittest

from mathx.ops import add, mul


class TestOps(unittest.TestCase):
    def test_add(self):
        self.assertEqual(add(2, 3), 5)

    def test_mul(self):
        self.assertEqual(mul(2, 3), 6)


if __name__ == "__main__":
    unittest.main()
"#,
    ),
    (
        "tests/test_strings.py",
        r#"import unittest

from mathx.strings import slugify, titlecase


class TestStrings(unittest.TestCase):
    def test_slugify(self):
        self.assertEqual(slugify("Hello World"), "hello-world")

    def test_titlecase(self):
        self.assertEqual(titlecase("hello world"), "Hello World")


if __name__ == "__main__":
    unittest.main()
"#,
    ),
    (
        "data/config.json",
        "{\n  \"name\": \"mathx\",\n  \"version\": \"0.1.0\",\n  \"features\": [\"ops\", \"strings\"]\n}\n",
    ),
    (
        "README.md",
        r#"# mathx

A tiny math + strings toy library used by the FLEETOR Phase 0 probe.

## Modules

- `mathx/ops.py` — arithmetic helpers
- `mathx/strings.py` — string helpers

Run the tests with `python3 -m unittest discover -s tests`.
"#,
    ),
];

/// Materialize a fresh copy of the toy repo at `root` (created if missing,
/// wiped if present).
pub fn materialize(root: &Path) -> Result<()> {
    if root.exists() {
        fs::remove_dir_all(root)?;
    }
    for (rel, contents) in FILES {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, contents)?;
    }
    Ok(())
}
