# logstat

A tiny log-analysis CLI. Reads newline-delimited access-log records, aggregates them, and
prints a summary.

This is FLEETOR's **testbed project** — a small but real codebase for the fleet to work on.
It is deliberately shaped so that four workers can hold different files and still have to
talk to each other: `parser` produces the record shape that `stats` and `render` both consume,
so a change to it ripples.

## Layout

| File | Owns |
|---|---|
| `src/parser.py` | Turning a raw log line into a `Record` |
| `src/stats.py` | Aggregating records into counts and percentiles |
| `src/render.py` | Formatting an aggregate as text |
| `src/cli.py` | Argument handling and wiring the three together |

## Run

```sh
python3 -m src.cli sample.log
./run-tests.sh
```

## Known gaps

- `parser.parse_line` doesn't handle quoted request paths containing spaces.
- `stats.percentile` uses nearest-rank; no interpolation.
- `render` has no column alignment for wide status codes.
- There are no tests for `render` at all.
