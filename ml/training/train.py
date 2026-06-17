"""Train a cost-sensitive, calibrated LightGBM fraud model (T030; D-011/D-012).

The rare positive class is up-weighted by the imbalance ratio (cost-sensitive), and the raw scores
are mapped to true probabilities with isotonic calibration on a held-out split.
"""

from __future__ import annotations

import lightgbm as lgb
import numpy as np
from sklearn.calibration import CalibratedClassifierCV
from sklearn.frozen import FrozenEstimator
from sklearn.model_selection import train_test_split


def train(x: np.ndarray, y: np.ndarray, seed: int = 42) -> CalibratedClassifierCV:
    """Fit and calibrate the model.

    Args:
        x: Feature matrix.
        y: Binary labels (1 = fraud).
        seed: RNG seed.

    Returns:
        A calibrated classifier whose ``predict_proba`` yields calibrated fraud probabilities.
    """
    x_fit, x_cal, y_fit, y_cal = train_test_split(
        x, y, test_size=0.3, random_state=seed, stratify=y
    )

    positives = max(int(y_fit.sum()), 1)
    negatives = len(y_fit) - positives

    base = lgb.LGBMClassifier(
        n_estimators=200,
        num_leaves=31,
        learning_rate=0.05,
        scale_pos_weight=negatives / positives,  # cost-sensitive: up-weight the rare fraud class
        random_state=seed,
        verbose=-1,
    )
    base.fit(x_fit, y_fit)

    # Calibrate the frozen (already-fitted) model on the held-out split — isotonic → true
    # probabilities. (sklearn 1.6+ replaced cv="prefit" with FrozenEstimator.)
    calibrated = CalibratedClassifierCV(FrozenEstimator(base), method="isotonic")
    calibrated.fit(x_cal, y_cal)
    return calibrated
