"""Population Stability Index for drift monitoring (T036; D-013, ``term:psi``).

Mirrors ``model::registry::psi`` in the Rust crate. PSI on a feature or score distribution detects
drift; crossing the threshold flags a retraining trigger. < 0.1 stable, 0.1–0.25 moderate,
> 0.25 significant.
"""

from __future__ import annotations

import numpy as np

DEFAULT_THRESHOLD = 0.25


def psi(reference: np.ndarray, current: np.ndarray, bins: int = 10) -> float:
    """PSI between a reference and a current distribution over equal-width bins."""
    ref = np.asarray(reference, dtype=float)
    cur = np.asarray(current, dtype=float)
    if ref.size == 0 or cur.size == 0:
        return 0.0

    lo = float(min(ref.min(), cur.min()))
    hi = float(max(ref.max(), cur.max()))
    if hi - lo < np.finfo(float).eps:
        return 0.0

    edges = np.linspace(lo, hi, bins + 1)
    ref_pct = np.histogram(ref, bins=edges)[0] / ref.size
    cur_pct = np.histogram(cur, bins=edges)[0] / cur.size

    eps = 1e-6
    ref_pct = np.clip(ref_pct, eps, None)
    cur_pct = np.clip(cur_pct, eps, None)
    return float(np.sum((cur_pct - ref_pct) * np.log(cur_pct / ref_pct)))


def is_drift(psi_value: float, threshold: float = DEFAULT_THRESHOLD) -> bool:
    """Whether a PSI value crosses the drift threshold (record a retraining trigger)."""
    return psi_value > threshold
