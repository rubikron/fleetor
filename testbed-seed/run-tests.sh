#!/usr/bin/env bash
# The testbed's exit gate: run the unit tests from the project root.
set -euo pipefail
cd "$(dirname "$0")"
python3 -m unittest discover -s tests -t . -v
