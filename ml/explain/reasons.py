"""Authoritative TreeSHAP reason codes (T033; D-010 explainability — offline/audit attribution).

Uses LightGBM's exact per-feature SHAP contributions (``pred_contrib``). The Rust hot path uses a
lightweight threshold heuristic over the **same** versioned vocabulary
(``crates/model/src/explain.rs``); this offline path is the authoritative attribution for audit.
"""

from __future__ import annotations

import lightgbm as lgb
import numpy as np

#: Index-aligned with FEATURE_NAMES; order must match crates/model/src/explain.rs::RULES.
REASON_CODES: list[str] = [
    "MODEL_HIGH_VELOCITY_5M",
    "MODEL_HIGH_VELOCITY_1H",
    "MODEL_HIGH_VELOCITY_24H",
    "MODEL_AMOUNT_RATIO",
    "MODEL_AMOUNT_ZSCORE",
    "MODEL_DEVICE_SPREAD",
    "MODEL_DECLINE_RATE",
    "MODEL_HIGH_RISK_MCC",
]
REASON_CODE_VERSION = "rc-2026-06-17"


def top_reason_codes(model: lgb.LGBMClassifier, row: np.ndarray, top_k: int = 3) -> list[str]:
    """Top reason codes for one feature vector, ranked by exact TreeSHAP contribution toward fraud.

    Args:
        model: A fitted LightGBM classifier.
        row: A single feature vector.
        top_k: Maximum number of reason codes.

    Returns:
        Reason codes for the features that pushed the score most toward fraud (positive SHAP).
    """
    contributions = model.booster_.predict(row.reshape(1, -1), pred_contrib=True)[0]
    feature_contributions = contributions[:-1]  # last element is the base value
    order = np.argsort(-feature_contributions)  # most positive (toward fraud) first
    codes = [REASON_CODES[i] for i in order if feature_contributions[i] > 0]
    return codes[:top_k]
