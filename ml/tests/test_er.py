"""T040: entity resolution clusters on strong identifiers, not weak ones."""

from __future__ import annotations

from graph.er.resolve import normalise, resolve


def _cluster_of(clusters: list[set[str]], rid: str) -> set[str]:
    return next(c for c in clusters if rid in c)


def test_records_sharing_a_strong_identifier_are_clustered() -> None:
    records = [
        {"id": "a", "identifiers": [("device", "D1"), ("ip", "1.2.3.4")]},
        {"id": "b", "identifiers": [("device", "D1")]},  # shares device → same as a
        {"id": "c", "identifiers": [("email", "X@Y.com")]},
        {"id": "d", "identifiers": [("email", "x@y.com")]},  # same email normalised → same as c
    ]
    clusters = resolve(records)
    assert _cluster_of(clusters, "a") == {"a", "b"}
    assert _cluster_of(clusters, "c") == {"c", "d"}


def test_a_shared_weak_ip_does_not_merge_entities() -> None:
    records = [
        {"id": "a", "identifiers": [("ip", "1.2.3.4")]},
        {"id": "b", "identifiers": [("ip", "1.2.3.4")]},  # only share IP → must NOT merge
    ]
    assert len(resolve(records)) == 2


def test_transitive_linking_merges_a_chain() -> None:
    records = [
        {"id": "a", "identifiers": [("device", "D9")]},
        {"id": "b", "identifiers": [("device", "D9"), ("email", "p@q.com")]},
        {"id": "c", "identifiers": [("email", "P@Q.com")]},
    ]
    clusters = resolve(records)
    assert len(clusters) == 1
    assert _cluster_of(clusters, "a") == {"a", "b", "c"}


def test_phone_normalisation_ignores_formatting() -> None:
    assert normalise("phone", "+1 (555) 010-2030") == "15550102030"
