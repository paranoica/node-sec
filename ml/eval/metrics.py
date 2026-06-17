"""Imbalance-aware evaluation metrics (T035; D-013 — accuracy is banned).

Under ~1% fraud, accuracy is meaningless (approve-everything scores 99%). The harness reports
PR-AUC (primary), recall at a fixed low FPR, precision at N (review-capacity), and the
alert-to-fraud ratio (investigator workload). Accuracy is intentionally absent.
"""

from __future__ import annotations

import numpy as np
from sklearn.metrics import average_precision_score, roc_curve


def evaluate(
    y_true: np.ndarray,
    y_score: np.ndarray,
    fpr_target: float = 0.01,
    top_n: int = 100,
) -> dict[str, float]:
    """Compute the fraud-evaluation metrics.

    Args:
        y_true: Binary labels (1 = fraud).
        y_score: Model fraud probabilities.
        fpr_target: The false-positive rate the review capacity allows.
        top_n: Review capacity for precision@N.

    Returns:
        ``pr_auc``, ``recall_at_fpr``, ``precision_at_n``, ``alert_to_fraud_ratio`` —
        deliberately **no accuracy**.
    """
    y_true = np.asarray(y_true)
    y_score = np.asarray(y_score)

    pr_auc = float(average_precision_score(y_true, y_score))

    fpr, tpr, thresholds = roc_curve(y_true, y_score)
    allowed = fpr <= fpr_target
    if allowed.any():
        idx = int(np.argmax(tpr * allowed))  # the highest recall within the FPR budget
        recall_at_fpr = float(tpr[idx])
        threshold = float(thresholds[idx])
    else:
        recall_at_fpr = 0.0
        threshold = float("inf")

    order = np.argsort(-y_score)[:top_n]
    precision_at_n = float(np.mean(y_true[order]))

    alerts = int(np.sum(y_score >= threshold))
    caught = int(np.sum((y_score >= threshold) & (y_true == 1)))
    alert_to_fraud_ratio = float(alerts / max(caught, 1))

    return {
        "pr_auc": pr_auc,
        "recall_at_fpr": recall_at_fpr,
        "precision_at_n": precision_at_n,
        "alert_to_fraud_ratio": alert_to_fraud_ratio,
    }
