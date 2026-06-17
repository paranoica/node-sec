"""T036: PSI drift detection (offline monitoring side)."""

from __future__ import annotations

import numpy as np

from registry.psi import is_drift, psi


def test_psi_near_zero_for_same_distribution() -> None:
    dist = np.array([i % 10 for i in range(1000)], dtype=float)
    value = psi(dist, dist, bins=10)
    assert value < 0.1
    assert not is_drift(value)


def test_psi_flags_a_shifted_distribution() -> None:
    reference = np.array([i % 10 for i in range(1000)], dtype=float)
    shifted = reference + 8.0
    value = psi(reference, shifted, bins=10)
    assert value > 0.25
    assert is_drift(value)
