"""T031: the LightGBM → ONNX export reproduces LightGBM in-process (Python-side parity)."""

from __future__ import annotations

from pathlib import Path

from export.to_onnx import export


def test_onnx_export_matches_lightgbm_within_tolerance(tmp_path: Path) -> None:
    # Write to a temp dir so the committed artifacts (used by the Rust parity test) are untouched.
    fixture = export(
        seed=1, onnx_path=tmp_path / "model.onnx", fixture_path=tmp_path / "parity.json"
    )
    assert fixture["onnx_vs_lgbm_max_diff"] < fixture["tolerance"]
    assert len(fixture["vectors"]) == len(fixture["scores"])
    assert all(0.0 <= score <= 1.0 for score in fixture["scores"])
