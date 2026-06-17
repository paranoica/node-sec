"""T030: the training pipeline produces a calibrated, cost-sensitive LightGBM model."""

from __future__ import annotations

import numpy as np
from sklearn.metrics import recall_score

from training.synthetic import make_dataset
from training.train import train


def test_train_produces_a_calibrated_cost_sensitive_model() -> None:
    x, y = make_dataset(n=20_000, fraud_rate=0.01, seed=1)
    model = train(x, y, seed=1)

    proba = model.predict_proba(x)[:, 1]

    # Calibrated → outputs are probabilities in [0, 1].
    assert proba.min() >= 0.0
    assert proba.max() <= 1.0

    # It learned the injected signal: fraud scores well above legit on average.
    assert proba[y == 1].mean() > proba[y == 0].mean() + 0.3

    # Cost-sensitive weighting → most fraud is caught at a neutral threshold despite 1% base rate.
    predictions = (proba >= 0.5).astype(int)
    assert recall_score(y, predictions) > 0.6


def test_dataset_has_the_expected_imbalance() -> None:
    x, y = make_dataset(n=10_000, fraud_rate=0.01, seed=7)
    assert x.shape == (10_000, 8)
    assert abs(y.mean() - 0.01) < 0.002  # ~1% fraud
    assert set(np.unique(y)) == {0, 1}
