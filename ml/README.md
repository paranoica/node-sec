# ml — node-sec Python tree

Model training, evaluation, and batch graph analytics. Per D-002 (polyglot), the Rust hot path and
this Python tree meet only at artifact boundaries (the exported ONNX model, materialised features) —
never a Python call on the hot path.

Real code lands from sprint **S3** (training → calibration → ONNX export) and **S4** (batch graph
analytics). For now this is a minimal, CI-green skeleton.

```bash
uv sync --dev
uv run ruff check .
uv run pytest -q
```
