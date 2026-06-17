"""T034: delayed-label resolution across the investigator + chargeback streams."""

from __future__ import annotations

from labels.join import resolve_label


def _streams() -> list[dict]:
    return [
        {"value": "fraud", "source": "investigator", "available_at_unix": 100},
        {"value": "fraud", "source": "chargeback", "available_at_unix": 1000},
    ]


def test_nothing_known_yet_is_censored() -> None:
    assert resolve_label(_streams(), as_of_unix=50) is None


def test_investigator_label_known_first() -> None:
    assert resolve_label(_streams(), as_of_unix=200) == "fraud"


def test_chargeback_supersedes_a_wrong_investigator_label() -> None:
    streams = [
        {"value": "legit", "source": "investigator", "available_at_unix": 100},  # noisy, wrong
        {"value": "fraud", "source": "chargeback", "available_at_unix": 1000},  # authoritative
    ]
    assert resolve_label(streams, as_of_unix=200) == "legit"  # only the investigator is known yet
    assert resolve_label(streams, as_of_unix=2000) == "fraud"  # chargeback supersedes
