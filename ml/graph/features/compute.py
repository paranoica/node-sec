"""Batch graph features (T042; D-009 — centrality, risk propagation, community, distance-to-bad).

Computed offline over the transaction graph and materialised to the online feature store (the Rust
hot path then reads them per entity). PageRank is on the directed graph; risk propagation
(personalised PageRank from known-bad seeds), community, and distance-to-bad use the undirected
projection so risk spreads regardless of flow direction.
"""

from __future__ import annotations

import networkx as nx

ALPHA = 0.85


def graph_features(
    edges: list[tuple[str, str, float]], bad_nodes: set[str]
) -> dict[str, dict[str, float]]:
    """Per-node graph features.

    Args:
        edges: directed weighted edges ``(from, to, weight)``.
        bad_nodes: known-bad node ids (sanctioned / confirmed-fraud / SAR'd).

    Returns:
        ``{node: {"pagerank", "ppr_bad", "community", "community_size", "dist_to_bad"}}``.
        ``dist_to_bad`` is hops to the nearest bad node, or ``-1`` if unreachable.
    """
    directed = nx.DiGraph()
    for src, dst, weight in edges:
        directed.add_edge(src, dst, weight=weight)
    undirected = directed.to_undirected()

    pagerank = nx.pagerank(directed, alpha=ALPHA, weight="weight")

    if bad_nodes & set(undirected.nodes):
        personalization = {n: (1.0 if n in bad_nodes else 0.0) for n in undirected.nodes}
        ppr = nx.pagerank(undirected, alpha=ALPHA, personalization=personalization, weight="weight")
    else:
        ppr = dict.fromkeys(undirected.nodes, 0.0)

    community_of: dict[str, int] = {}
    community_size: dict[str, int] = {}
    for cid, community in enumerate(nx.community.greedy_modularity_communities(undirected)):
        for node in community:
            community_of[node] = cid
            community_size[node] = len(community)

    dist_to_bad: dict[str, int] = {}
    for node in undirected.nodes:
        if node in bad_nodes:
            dist_to_bad[node] = 0
            continue
        best: int | None = None
        for bad in bad_nodes:
            if bad not in undirected:
                continue
            try:
                hops = nx.shortest_path_length(undirected, node, bad)
            except nx.NetworkXNoPath:
                continue
            best = hops if best is None else min(best, hops)
        dist_to_bad[node] = best if best is not None else -1

    return {
        node: {
            "pagerank": float(pagerank[node]),
            "ppr_bad": float(ppr[node]),
            "community": float(community_of.get(node, -1)),
            "community_size": float(community_size.get(node, 1)),
            "dist_to_bad": float(dist_to_bad[node]),
        }
        for node in directed.nodes
    }
