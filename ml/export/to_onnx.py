"""Export the LightGBM fraud model to ONNX and emit a parity fixture (T031; D-004 in-process ONNX).

The raw tree model is converted to ONNX (clean, well-supported by `onnxmltools`); isotonic
calibration is a separable monotonic step applied downstream, so it is intentionally not part of
this graph. The conversion is verified in-process with `onnxruntime` against LightGBM, then a
fixture of ``(vectors, onnx_scores)`` is written for the Rust parity test (`crates/model`).
"""

from __future__ import annotations

import json
from pathlib import Path

import lightgbm as lgb
import numpy as np
import onnxruntime as ort
from onnxmltools import convert_lightgbm
from onnxmltools.convert.common.data_types import FloatTensorType

from training.synthetic import FEATURE_NAMES, make_dataset

ARTIFACTS = Path(__file__).resolve().parents[1] / "artifacts"
ONNX_PATH = ARTIFACTS / "fraud_lgbm.onnx"
FIXTURE_PATH = ARTIFACTS / "parity.json"
TOLERANCE = 1e-4


def _fraud_proba(outputs: list) -> np.ndarray:
    """Pull the fraud-class probability column from an ONNX session's outputs."""
    for out in outputs:
        arr = np.asarray(out)
        if arr.ndim == 2 and arr.shape[1] == 2:
            return arr[:, 1]
    raise ValueError("no [N, 2] probability tensor in ONNX outputs")


def export(
    seed: int = 1, onnx_path: Path = ONNX_PATH, fixture_path: Path = FIXTURE_PATH
) -> dict:
    """Train, convert to ONNX, verify parity in Python, and write the artifacts.

    Tests pass temporary paths so they never overwrite the committed artifacts (the ONNX bytes are
    not reproducible byte-for-byte). Returns the fixture dict that was written.
    """
    x, y = make_dataset(n=20_000, fraud_rate=0.01, seed=seed)
    positives = max(int(y.sum()), 1)
    negatives = len(y) - positives
    model = lgb.LGBMClassifier(
        n_estimators=200,
        num_leaves=31,
        learning_rate=0.05,
        scale_pos_weight=negatives / positives,
        random_state=seed,
        verbose=-1,
    )
    model.fit(x, y)

    n_features = x.shape[1]
    onnx_model = convert_lightgbm(
        model,
        initial_types=[("input", FloatTensorType([None, n_features]))],
        zipmap=False,
    )
    onnx_bytes = onnx_model.SerializeToString()

    # Verify the ONNX graph reproduces LightGBM in-process, then keep the ONNX scores as the golden.
    session = ort.InferenceSession(onnx_bytes, providers=["CPUExecutionProvider"])
    sample = x[:32].astype(np.float32)
    onnx_scores = _fraud_proba(session.run(None, {"input": sample}))
    lgb_scores = model.predict_proba(sample.astype(np.float64))[:, 1]
    max_diff = float(np.max(np.abs(onnx_scores - lgb_scores)))
    if max_diff > TOLERANCE:
        raise AssertionError(f"ONNX vs LightGBM parity {max_diff} > {TOLERANCE}")

    fixture = {
        "feature_names": FEATURE_NAMES,
        "vectors": sample.astype(float).tolist(),
        "scores": onnx_scores.astype(float).tolist(),
        "tolerance": TOLERANCE,
        "onnx_vs_lgbm_max_diff": max_diff,
    }
    onnx_path.parent.mkdir(parents=True, exist_ok=True)
    onnx_path.write_bytes(onnx_bytes)
    fixture_path.write_text(json.dumps(fixture, indent=2))
    return fixture


if __name__ == "__main__":
    result = export()
    print(f"exported {ONNX_PATH.name}; onnx_vs_lgbm_max_diff={result['onnx_vs_lgbm_max_diff']:.2e}")
