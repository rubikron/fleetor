"""Turn raw access-log lines into structured records.

The `Record` shape defined here is consumed by both `stats` and `render`, so
changing it is a cross-cutting change — coordinate before you do.
"""

from dataclasses import dataclass
from typing import Optional


@dataclass(frozen=True)
class Record:
    """One parsed log line."""

    ip: str
    method: str
    path: str
    status: int
    duration_ms: int


class ParseError(ValueError):
    """Raised when a line does not match the expected format."""


def parse_line(line: str) -> Optional[Record]:
    """Parse one log line into a `Record`.

    Expected format, space-separated:

        <ip> <method> <path> <status> <duration_ms>

    Returns None for blank lines and comments. Raises ParseError on a
    malformed line, so a caller can decide whether to skip or fail.
    """
    stripped = line.strip()
    if not stripped or stripped.startswith("#"):
        return None

    parts = stripped.split()
    if len(parts) != 5:
        raise ParseError(f"expected 5 fields, got {len(parts)}: {stripped!r}")

    ip, method, path, raw_status, raw_duration = parts
    try:
        status = int(raw_status)
        duration_ms = int(raw_duration)
    except ValueError as exc:
        raise ParseError(f"non-numeric status or duration: {stripped!r}") from exc

    return Record(ip=ip, method=method, path=path, status=status, duration_ms=duration_ms)


def parse_lines(lines):
    """Parse an iterable of lines, skipping blanks and comments."""
    for line in lines:
        record = parse_line(line)
        if record is not None:
            yield record
