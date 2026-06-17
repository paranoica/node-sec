"""Entity resolution → identity clusters (T040; mirrors graph::er in the Rust crate).

normalise → block → match → cluster. Records sharing a *strong* identifier (device, phone, email,
address, card) are unioned; a *weak* identifier (a shared public IP) does not by itself merge
distinct entities.
"""

from __future__ import annotations

from collections.abc import Iterable

STRONG_KINDS = frozenset({"device", "phone", "email", "address", "card"})


def normalise(kind: str, value: str) -> str:
    """Canonicalise an identifier so trivially-different spellings match."""
    if kind == "phone":
        return "".join(c for c in value if c.isdigit())
    return value.strip().lower()


class _UnionFind:
    def __init__(self, ids: Iterable[str]) -> None:
        self._parent = {i: i for i in ids}

    def find(self, x: str) -> str:
        while self._parent[x] != x:
            self._parent[x] = self._parent[self._parent[x]]
            x = self._parent[x]
        return x

    def union(self, a: str, b: str) -> None:
        ra, rb = self.find(a), self.find(b)
        if ra != rb:
            self._parent[ra] = rb


def resolve(records: list[dict]) -> list[set[str]]:
    """Cluster records into identities.

    Args:
        records: each ``{"id": str, "identifiers": [(kind, value), ...]}``.

    Returns:
        A list of clusters, each a set of record ids.
    """
    uf = _UnionFind(rec["id"] for rec in records)
    first_seen: dict[tuple[str, str], str] = {}
    for rec in records:
        for kind, value in rec["identifiers"]:
            if kind not in STRONG_KINDS:
                continue
            block = (kind, normalise(kind, value))
            if block in first_seen:
                uf.union(rec["id"], first_seen[block])
            else:
                first_seen[block] = rec["id"]

    clusters: dict[str, set[str]] = {}
    for rec in records:
        clusters.setdefault(uf.find(rec["id"]), set()).add(rec["id"])
    return list(clusters.values())
