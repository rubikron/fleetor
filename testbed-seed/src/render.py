"""Format an `Aggregate` as plain text.

Consumes `stats.Aggregate`. There are currently no tests for this module.
"""

from .stats import Aggregate

TOP_PATHS = 5


def render(agg: Aggregate) -> str:
    """Render a summary block."""
    if agg.total == 0:
        return "no records\n"

    lines = [
        f"records   {agg.total}",
        f"p50       {agg.p50_ms}ms",
        f"p95       {agg.p95_ms}ms",
        "",
        "status",
    ]

    for status, count in agg.by_status.items():
        share = 100.0 * count / agg.total
        lines.append(f"  {status}  {count}  ({share:.1f}%)")

    lines.append("")
    lines.append(f"top {TOP_PATHS} paths")
    for path, count in list(agg.by_path.items())[:TOP_PATHS]:
        lines.append(f"  {count:>4}  {path}")

    return "\n".join(lines) + "\n"
