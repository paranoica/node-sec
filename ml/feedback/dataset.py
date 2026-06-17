"""Feed investigator labels and reject-inferred estimates into the retraining dataset (T054).

Investigator dispositions (written by ``compliance::feedback`` to the offline store) enter the
dataset as hard, full-weight labels. Reject-inferred estimates enter as soft, confidence-weighted
labels — they recover signal the engine's declines hid, but must never count as much as an observed
outcome.
"""

from __future__ import annotations

from collections.abc import Iterable, Sequence


def investigator_rows(labels: Iterable[dict]) -> list[dict]:
    """Map investigator dispositions (the offline-store record shape) to training rows.

    Args:
        labels: Dicts with at least ``subject`` and ``value`` ("fraud"/"legit"), as written by
            ``compliance::feedback::InvestigatorLabel``.

    Returns:
        Full-weight training rows tagged ``source="investigator"``.
    """
    return [
        {
            "subject": label["subject"],
            "value": label["value"],
            "weight": 1.0,
            "source": "investigator",
        }
        for label in labels
    ]


def reject_inferred_rows(
    subjects: Sequence[str],
    proba: Sequence[float],
    threshold: float = 0.5,
) -> list[dict]:
    """Turn reject-inferred fraud probabilities into soft-labelled, down-weighted training rows.

    The weight is the imputation's confidence — ``|p - 0.5| * 2`` — so a coin-flip estimate
    contributes ~nothing and a confident one approaches, but never reaches, an observed label's
    weight of 1.0.
    """
    rows: list[dict] = []
    for subject, p in zip(subjects, proba, strict=True):
        p = float(p)
        rows.append(
            {
                "subject": subject,
                "value": "fraud" if p >= threshold else "legit",
                "weight": abs(p - 0.5) * 2.0,
                "source": "reject_inference",
            }
        )
    return rows
