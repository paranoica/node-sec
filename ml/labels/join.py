"""Label resolution across the two streams (T034; D-012 delayed labels).

A training label is only known once its ``available_at`` has passed (delayed-label censoring). When
several labels are known, the authoritative **chargeback** supersedes the noisy **investigator**
label. Mirrors the streams produced by ``simulator::labels`` in the Rust simulator.
"""

from __future__ import annotations

from collections.abc import Iterable


def resolve_label(labels: Iterable[dict], as_of_unix: int) -> str | None:
    """Resolve the best-known label as of a time.

    Args:
        labels: Each a dict with ``value`` ("fraud"/"legit"), ``source``
            ("investigator"/"chargeback"), and ``available_at_unix`` (int).
        as_of_unix: The point-in-time at which the label is being resolved.

    Returns:
        The resolved label value, or ``None`` if nothing is known yet (censored).
    """
    known = [label for label in labels if label["available_at_unix"] <= as_of_unix]
    if not known:
        return None
    chargebacks = [label for label in known if label["source"] == "chargeback"]
    if chargebacks:
        return chargebacks[0]["value"]
    return known[0]["value"]
