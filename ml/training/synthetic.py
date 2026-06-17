"""Synthetic labelled training data (T030; D-022: the synthetic generator is the primary source).

Feature vectors mirror the engine's signals (velocity counts, amount-to-mean ratio, z-score,
device spread, decline rate, high-risk MCC). Fraud rows carry an injected, separable signal at a
realistic ~1% base rate so the imbalance and cost-sensitive handling are exercised end to end.
"""

from __future__ import annotations

import numpy as np

#: Feature column order — kept aligned with the engine's feature vector when ONNX is wired (T031).
FEATURE_NAMES: list[str] = [
    "velocity_5m",
    "velocity_1h",
    "velocity_24h",
    "amount_to_mean_ratio",
    "amount_z_score",
    "distinct_devices_24h",
    "decline_rate_1h",
    "high_risk_mcc",
]


def make_dataset(
    n: int = 20_000, fraud_rate: float = 0.01, seed: int = 42
) -> tuple[np.ndarray, np.ndarray]:
    """Generate a shuffled ``(X, y)`` dataset.

    Args:
        n: Total rows.
        fraud_rate: Fraction labelled fraud (the rest legitimate).
        seed: RNG seed for reproducibility.

    Returns:
        ``X`` of shape ``(n, len(FEATURE_NAMES))`` and integer labels ``y`` (1 = fraud).
    """
    rng = np.random.default_rng(seed)
    n_fraud = max(int(n * fraud_rate), 1)
    n_legit = n - n_fraud

    legit = np.column_stack(
        [
            rng.poisson(1.0, n_legit),  # velocity_5m
            rng.poisson(3.0, n_legit),  # velocity_1h
            rng.poisson(10.0, n_legit),  # velocity_24h
            rng.normal(1.0, 0.3, n_legit).clip(0.0),  # amount_to_mean_ratio
            rng.normal(0.0, 1.0, n_legit),  # amount_z_score
            rng.poisson(1.0, n_legit),  # distinct_devices_24h
            rng.beta(1.0, 20.0, n_legit),  # decline_rate_1h (low)
            rng.binomial(1, 0.03, n_legit),  # high_risk_mcc
        ]
    )
    fraud = np.column_stack(
        [
            rng.poisson(5.0, n_fraud),
            rng.poisson(15.0, n_fraud),
            rng.poisson(40.0, n_fraud),
            rng.normal(5.0, 2.0, n_fraud).clip(0.0),
            rng.normal(3.0, 1.5, n_fraud),
            rng.poisson(4.0, n_fraud),
            rng.beta(4.0, 6.0, n_fraud),  # decline_rate_1h (high)
            rng.binomial(1, 0.30, n_fraud),
        ]
    )

    x = np.vstack([legit, fraud]).astype(np.float64)
    y = np.concatenate([np.zeros(n_legit), np.ones(n_fraud)]).astype(int)
    order = rng.permutation(n)
    return x[order], y[order]
