"""Offline window-feature aggregation (T023).

Mirrors the online path (`stream::EntityWindows` in the Rust `stream` crate) exactly, so features
computed offline for training equal the ones served online (``arch:feature-parity``). Aggregation is
**point-in-time correct**: only events at or before the query time count, so a training label never
sees future data.
"""

from __future__ import annotations

from collections.abc import Iterable

# Must match stream::window::WINDOWS (label, seconds).
WINDOWS: list[tuple[str, int]] = [
    ("1m", 60),
    ("5m", 300),
    ("1h", 3_600),
    ("24h", 86_400),
    ("7d", 604_800),
    ("30d", 2_592_000),
]


def aggregate(
    events: Iterable[tuple[int, int]], now_unix: int
) -> dict[str, dict[str, int]]:
    """Aggregate ``(ts_unix, amount_minor)`` events into per-window count/sum/sum_sq.

    Args:
        events: Iterable of ``(ts_unix, amount_minor)`` pairs.
        now_unix: Query time (epoch seconds). Events after this are excluded (point-in-time).

    Returns:
        ``{window_label: {"count", "sum_minor", "sum_sq"}}`` for every window.
    """
    out: dict[str, dict[str, int]] = {
        label: {"count": 0, "sum_minor": 0, "sum_sq": 0} for label, _ in WINDOWS
    }
    for ts, amount in events:
        age = now_unix - ts
        if age < 0:
            continue  # future event — point-in-time correctness
        for label, secs in WINDOWS:
            if age <= secs:
                stat = out[label]
                stat["count"] += 1
                stat["sum_minor"] += amount
                stat["sum_sq"] += amount * amount
    return out
