"""T042: batch graph features — risk propagation, distance-to-bad, centrality, community."""

from __future__ import annotations

from graph.features.compute import graph_features


def test_distance_and_risk_propagate_from_known_bad() -> None:
    # x is known-bad; b → a → x is a chain; c — d are a separate component.
    edges = [("a", "x", 1.0), ("b", "a", 1.0), ("c", "d", 1.0)]
    feats = graph_features(edges, bad_nodes={"x"})

    assert feats["x"]["dist_to_bad"] == 0
    assert feats["a"]["dist_to_bad"] == 1  # adjacent to x
    assert feats["b"]["dist_to_bad"] == 2  # two hops
    assert feats["c"]["dist_to_bad"] == -1  # unreachable from x

    # Risk propagates: nodes near the bad seed carry more personalised-PageRank mass.
    assert feats["a"]["ppr_bad"] > feats["c"]["ppr_bad"]


def test_every_node_has_the_feature_set() -> None:
    feats = graph_features([("a", "b", 1.0), ("b", "c", 1.0)], bad_nodes=set())
    expected = {"pagerank", "ppr_bad", "community", "community_size", "dist_to_bad"}
    for node_features in feats.values():
        assert set(node_features) == expected
    # a, b, c form one connected community.
    assert feats["a"]["community"] == feats["c"]["community"]
    assert feats["a"]["community_size"] == 3
