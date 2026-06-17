"""Reject inference for the feedback loop (T054; D-011, ``term:reject-inference``).

The engine never observes outcomes for transactions it blocked, so a model retrained only on the
accepted population is biased by the engine's own declines (selection bias): the declined,
higher-risk rows are simply missing from training, dragging the learned base rate toward "legit".

Reject inference fits an outcome model on the accepted, labelled population and scores the rejected
population to impute soft outcome estimates that re-enter the retraining dataset.
"""

from __future__ import annotations

import numpy as np
from sklearn.linear_model import LogisticRegression


def impute_rejected_outcomes(
    accepted_x: np.ndarray,
    accepted_y: np.ndarray,
    rejected_x: np.ndarray,
) -> np.ndarray:
    """Estimate P(fraud) for engine-declined transactions.

    Args:
        accepted_x: Feature matrix of accepted (outcome-observed) transactions, shape (n, d).
        accepted_y: Observed binary outcomes for the accepted rows (1 = fraud), shape (n,).
        rejected_x: Feature matrix of declined transactions to score, shape (m, d).

    Returns:
        Imputed fraud probabilities for the rejected rows, shape (m,). Empty if there are none.
    """
    rejected_x = np.asarray(rejected_x, dtype=float)
    if rejected_x.size == 0:
        return np.empty(0, dtype=float)

    model = LogisticRegression(max_iter=1000)
    model.fit(np.asarray(accepted_x, dtype=float), np.asarray(accepted_y, dtype=int))
    return model.predict_proba(rejected_x)[:, 1]
