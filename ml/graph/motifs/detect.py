"""Ring + temporal-motif detection (T043; D-009 — fraud rings, fan-in/out, scatter-gather).

Raises a fraud-ring alert for each matched motif over the transaction graph, listing the
participating entities and the hypothesised typology. Cycles must be **time-ordered** (causally
traversable) to count — a structural cycle whose edges cannot be ordered in time is not a ring.
"""

from __future__ import annotations

from collections import Counter

import networkx as nx


def _is_temporal_cycle(cycle: list[str], graph: nx.DiGraph) -> bool:
    """True if some rotation of the cycle's edge times is non-decreasing (causally traversable)."""
    times = [
        graph[cycle[i]][cycle[(i + 1) % len(cycle)]]["time"] for i in range(len(cycle))
    ]
    n = len(times)
    for start in range(n):
        rotated = times[start:] + times[:start]
        if all(rotated[i] <= rotated[i + 1] for i in range(n - 1)):
            return True
    return False


def detect_motifs(
    edges: list[tuple[str, str, float]], fan_threshold: int = 3
) -> list[dict]:
    """Detect fraud-ring motifs.

    Args:
        edges: directed timed edges ``(from, to, time)``.
        fan_threshold: minimum branching for fan-in/out and scatter-gather.

    Returns:
        Alerts ``{"typology": str, "entities": [...]}`` for ``ring``, ``fan_in_fan_out``,
        ``scatter_gather``.
    """
    graph = nx.DiGraph()
    for src, dst, time in edges:
        if graph.has_edge(src, dst):
            graph[src][dst]["time"] = max(graph[src][dst]["time"], time)
        else:
            graph.add_edge(src, dst, time=time)

    alerts: list[dict] = []

    # Rings: time-ordered directed simple cycles.
    for cycle in nx.simple_cycles(graph):
        if len(cycle) >= 2 and _is_temporal_cycle(cycle, graph):
            alerts.append({"typology": "ring", "entities": sorted(cycle)})

    # Fan-in → fan-out hubs (mule signature): many distinct sources, at least one forward.
    for node in graph.nodes:
        sources = list(graph.predecessors(node))
        sinks = list(graph.successors(node))
        if len(sources) >= fan_threshold and len(sinks) >= 1:
            alerts.append(
                {"typology": "fan_in_fan_out", "entities": [node, *sorted(sources)]}
            )

    # Scatter-gather: a source splits across intermediaries that re-merge into a common sink.
    for source in graph.nodes:
        intermediaries = list(graph.successors(source))
        if len(intermediaries) < fan_threshold:
            continue
        sink_hits: Counter[str] = Counter()
        for mid in intermediaries:
            for sink in graph.successors(mid):
                if sink != source:
                    sink_hits[sink] += 1
        for sink, hits in sink_hits.items():
            if hits >= fan_threshold:
                alerts.append({"typology": "scatter_gather", "entities": [source, sink]})

    return alerts
