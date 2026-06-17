"""T054: feedback loop — investigator labels feed the dataset; reject inference debiases blocks."""

from __future__ import annotations

import numpy as np

from feedback.dataset import investigator_rows, reject_inferred_rows
from feedback.reject_inference import impute_rejected_outcomes


def test_investigator_labels_feed_retraining_dataset() -> None:
    labels = [
        {"subject": "a", "value": "fraud", "source": "investigator", "available_at_unix": 1},
        {"subject": "b", "value": "legit", "source": "investigator", "available_at_unix": 2},
    ]
    rows = investigator_rows(labels)
    assert [r["value"] for r in rows] == ["fraud", "legit"]
    assert all(r["source"] == "investigator" and r["weight"] == 1.0 for r in rows)


def test_reject_inference_imputes_probabilities() -> None:
    rng = np.random.default_rng(0)
    # Accepted population: low feature -> legit, high feature -> fraud.
    x_acc = np.concatenate([rng.normal(0.0, 1.0, (100, 1)), rng.normal(4.0, 1.0, (100, 1))])
    y_acc = np.array([0] * 100 + [1] * 100)
    x_rej = np.array([[5.0], [-1.0]])

    proba = impute_rejected_outcomes(x_acc, y_acc, x_rej)

    assert proba.shape == (2,)
    assert ((proba >= 0.0) & (proba <= 1.0)).all()
    # The high-feature decline scores more fraud-like than the low-feature one.
    assert proba[0] > proba[1]


def test_reject_inference_reduces_decline_bias() -> None:
    rng = np.random.default_rng(1)
    # Accepted population: mostly legit, base rate ~0.2.
    x_acc = np.concatenate([rng.normal(0.0, 1.0, (200, 1)), rng.normal(3.0, 1.0, (50, 1))])
    y_acc = np.array([0] * 200 + [1] * 50)
    base_rate = float(y_acc.mean())

    # The declined population is the engine's blocks: systematically high-risk.
    x_rej = rng.normal(4.0, 1.0, (50, 1))
    proba = impute_rejected_outcomes(x_acc, y_acc, x_rej)

    # Imputed fraud rate on declines exceeds the accepted base rate — dropping them would
    # bias training toward "legit".
    assert proba.mean() > base_rate


def test_reject_inferred_rows_are_downweighted_soft_labels() -> None:
    rows = reject_inferred_rows(["a", "b"], [0.9, 0.1])
    assert rows[0]["value"] == "fraud"
    assert rows[1]["value"] == "legit"
    assert all(r["source"] == "reject_inference" for r in rows)
    # A soft label never outweighs an observed one.
    assert all(r["weight"] <= 1.0 for r in rows)
