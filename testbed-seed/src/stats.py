"""Aggregate parsed records into a summary.

Consumes `parser.Record`. If that shape changes, this breaks.
"""

from collections import Counter
from dataclasses import dataclass, field
from typing import Dict, Iterable, List

from .parser import Record


@dataclass
class Aggregate:
    """Everything `render` needs to print a summary."""

    total: int = 0
    by_status: Dict[int, int] = field(default_factory=dict)
    by_path: Dict[str, int] = field(default_factory=dict)
    p50_ms: int = 0
    p95_ms: int = 0


def percentile(values: List[int], pct: float) -> int:
    """Nearest-rank percentile. `pct` is 0..100. Empty input returns 0."""
    if not values:
        return 0
    ordered = sorted(values)
    rank = max(1, int(round(pct / 100.0 * len(ordered))))
    return ordered[min(rank, len(ordered)) - 1]


def aggregate(records: Iterable[Record]) -> Aggregate:
    """Fold records into an `Aggregate`."""
    durations: List[int] = []
    status_counts: Counter = Counter()
    path_counts: Counter = Counter()
    total = 0

    for record in records:
        total += 1
        status_counts[record.status] += 1
        path_counts[record.path] += 1
        durations.append(record.duration_ms)

    return Aggregate(
        total=total,
        by_status=dict(sorted(status_counts.items())),
        by_path=dict(path_counts.most_common()),
        p50_ms=percentile(durations, 50),
        p95_ms=percentile(durations, 95),
    )
