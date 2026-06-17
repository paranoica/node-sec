"""T035: the evaluation harness reports imbalance metrics and never accuracy."""

from __future__ import annotations

from eval.metrics import evaluate
from training.synthetic import make_dataset
from training.train import train


def test_evaluate_reports_imbalance_metrics_and_no_accuracy() -> None:
    x, y = make_dataset(n=20_000, fraud_rate=0.01, seed=2)
    model = train(x, y, seed=2)
    scores = model.predict_proba(x)[:, 1]

    metrics = evaluate(y, scores, fpr_target=0.01, top_n=100)

    # Accuracy is banned (D-013).
    assert "accuracy" not in metrics

    assert set(metrics) == {
        "pr_auc",
        "recall_at_fpr",
        "precision_at_n",
        "alert_to_fraud_ratio",
    }
    assert 0.0 <= metrics["pr_auc"] <= 1.0
    assert metrics["pr_auc"] > 0.5  # the synthetic signal is strongly separable
    assert 0.0 <= metrics["recall_at_fpr"] <= 1.0
    assert 0.0 <= metrics["precision_at_n"] <= 1.0
    assert metrics["alert_to_fraud_ratio"] >= 1.0  # at least one alert per fraud caught


def test_perfect_scores_give_pr_auc_one() -> None:
    y = [0, 0, 1, 1]
    scores = [0.01, 0.02, 0.98, 0.99]
    metrics = evaluate(y, scores, fpr_target=0.5, top_n=2)
    assert metrics["pr_auc"] == 1.0
    assert metrics["precision_at_n"] == 1.0
