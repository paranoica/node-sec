"""T033: TreeSHAP reason codes come from the shared, versioned vocabulary."""

from __future__ import annotations

import lightgbm as lgb
import numpy as np

from explain.reasons import REASON_CODES, top_reason_codes
from training.synthetic import make_dataset


def _fit_base_model(seed: int = 3) -> lgb.LGBMClassifier:
    x, y = make_dataset(n=10_000, fraud_rate=0.01, seed=seed)
    positives = max(int(y.sum()), 1)
    negatives = len(y) - positives
    model = lgb.LGBMClassifier(
        n_estimators=100, scale_pos_weight=negatives / positives, random_state=seed, verbose=-1
    )
    model.fit(x, y)
    return model


def test_top_reason_codes_are_from_the_vocabulary() -> None:
    model = _fit_base_model()
    # A high-risk-looking row: elevated velocity, amount, device spread, decline rate.
    row = np.array([10.0, 30.0, 80.0, 8.0, 6.0, 8.0, 0.8, 1.0])
    codes = top_reason_codes(model, row, top_k=3)
    assert 1 <= len(codes) <= 3
    assert all(code in REASON_CODES for code in codes)


def test_reason_codes_capped_by_top_k() -> None:
    model = _fit_base_model()
    row = np.array([12.0, 40.0, 100.0, 9.0, 8.0, 9.0, 0.9, 1.0])
    assert len(top_reason_codes(model, row, top_k=2)) <= 2
