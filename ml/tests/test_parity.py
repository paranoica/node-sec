"""Online/offline parity + point-in-time correctness (T023; ``arch:feature-parity``).

The offline aggregation in :mod:`features.windows` must reproduce the golden fixture, which the
Rust online definition (``stream::EntityWindows``) also reproduces in
``crates/features/src/offline.rs`` — so online and offline agree.
"""

from __future__ import annotations

import json
from pathlib import Path

from features.windows import aggregate

FIXTURE = Path(__file__).parent / "fixtures" / "parity_case.json"


def _load() -> dict:
    return json.loads(FIXTURE.read_text())


def _events(case: dict) -> list[tuple[int, int]]:
    return [(e["ts_unix"], e["amount_minor"]) for e in case["events"]]


def test_offline_matches_online_golden() -> None:
    case = _load()
    got = aggregate(_events(case), case["now_unix"])
    for label, expected in case["expected"].items():
        assert got[label] == expected, f"window {label} online/offline parity mismatch"


def test_point_in_time_excludes_future_events() -> None:
    case = _load()
    future = [e for e in case["events"] if e["ts_unix"] > case["now_unix"]]
    assert future, "fixture must contain a future event to exercise point-in-time correctness"
    got = aggregate(_events(case), case["now_unix"])
    # The future event (amount 999) and the 40-day-old event are both excluded → 30d count is 3.
    assert got["30d"]["count"] == 3
    assert got["30d"]["sum_minor"] == 1300
