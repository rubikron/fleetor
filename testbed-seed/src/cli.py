"""Entrypoint: read a log file, aggregate it, print the summary."""

import sys

from .parser import ParseError, parse_line
from .render import render
from .stats import aggregate


def main(argv=None) -> int:
    args = sys.argv[1:] if argv is None else argv
    if len(args) != 1:
        print("usage: python3 -m src.cli <logfile>", file=sys.stderr)
        return 2

    path = args[0]
    records = []
    try:
        with open(path, "r", encoding="utf-8") as handle:
            for lineno, line in enumerate(handle, start=1):
                try:
                    record = parse_line(line)
                except ParseError as exc:
                    print(f"{path}:{lineno}: {exc}", file=sys.stderr)
                    return 1
                if record is not None:
                    records.append(record)
    except OSError as exc:
        print(f"could not read {path}: {exc}", file=sys.stderr)
        return 1

    sys.stdout.write(render(aggregate(records)))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
