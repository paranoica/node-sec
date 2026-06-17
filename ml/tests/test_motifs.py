"""T043: ring + temporal-motif detection."""

from __future__ import annotations

from graph.motifs.detect import detect_motifs


def _typology(alerts: list[dict], typ: str) -> list[dict]:
    return [a for a in alerts if a["typology"] == typ]


def test_time_ordered_ring_is_detected() -> None:
    # a → b → c → a with increasing times → a causally-traversable ring.
    edges = [("a", "b", 1.0), ("b", "c", 2.0), ("c", "a", 3.0)]
    rings = _typology(detect_motifs(edges, fan_threshold=10), "ring")
    assert any(set(r["entities"]) == {"a", "b", "c"} for r in rings)


def test_non_temporal_cycle_is_not_a_ring() -> None:
    # Edge times decrease around the cycle → no rotation is non-decreasing → not a ring.
    edges = [("a", "b", 3.0), ("b", "c", 2.0), ("c", "a", 1.0)]
    assert not _typology(detect_motifs(edges, fan_threshold=10), "ring")


def test_fan_in_fan_out_hub_is_detected() -> None:
    edges = [("s1", "m", 1.0), ("s2", "m", 1.0), ("s3", "m", 1.0), ("m", "sink", 2.0)]
    alerts = _typology(detect_motifs(edges, fan_threshold=3), "fan_in_fan_out")
    assert any("m" in a["entities"] for a in alerts)


def test_scatter_gather_is_detected() -> None:
    edges = [
        ("src", "m1", 1.0),
        ("src", "m2", 1.0),
        ("src", "m3", 1.0),
        ("m1", "sink", 2.0),
        ("m2", "sink", 2.0),
        ("m3", "sink", 2.0),
    ]
    alerts = _typology(detect_motifs(edges, fan_threshold=3), "scatter_gather")
    assert any(set(a["entities"]) == {"src", "sink"} for a in alerts)
