//! In-process ONNX inference of the fraud model (D-004, `term:in-process-inference`).
//!
//! Loads the exported LightGBM tree-ensemble ONNX graph and scores feature vectors in-process via
//! ONNX Runtime — no network hop. The model outputs `[N, 2]` class probabilities; the fraud
//! probability is column 1.

use ort::session::Session;
use ort::value::Tensor;

/// A loaded fraud model ready for in-process scoring.
pub struct FraudModel {
    session: Session,
}

impl FraudModel {
    /// Load a model from ONNX bytes.
    ///
    /// # Errors
    /// Returns an [`ort::Error`] if the graph fails to load.
    pub fn from_onnx_bytes(bytes: &[u8]) -> ort::Result<Self> {
        let session = Session::builder()?.commit_from_memory(bytes)?;
        Ok(Self { session })
    }

    /// Score a batch of feature vectors, returning the fraud probability per row.
    ///
    /// # Errors
    /// Returns an [`ort::Error`] if inference fails.
    pub fn score(&mut self, rows: &[Vec<f32>]) -> ort::Result<Vec<f32>> {
        let n = rows.len();
        let cols = rows.first().map_or(0, Vec::len);
        let flat: Vec<f32> = rows.iter().flatten().copied().collect();

        let input = Tensor::from_array(([n, cols], flat))?;
        let outputs = self.session.run(ort::inputs!["input" => input])?;

        let (shape, data) = outputs["probabilities"].try_extract_tensor::<f32>()?;
        let stride = *shape.last().unwrap_or(&1) as usize; // 2 classes
        Ok((0..n).map(|i| data[i * stride + 1]).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONNX: &[u8] = include_bytes!("../../../ml/artifacts/fraud_lgbm.onnx");
    const FIXTURE: &str = include_str!("../../../ml/artifacts/parity.json");

    #[test]
    fn inference_parity_matches_python_onnx() {
        let case: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
        let vectors: Vec<Vec<f32>> = case["vectors"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| {
                row.as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_f64().unwrap() as f32)
                    .collect()
            })
            .collect();
        let expected: Vec<f32> = case["scores"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap() as f32)
            .collect();

        let mut model = FraudModel::from_onnx_bytes(ONNX).unwrap();
        let got = model.score(&vectors).unwrap();

        assert_eq!(got.len(), expected.len());
        for (g, e) in got.iter().zip(&expected) {
            assert!(
                (g - e).abs() < 1e-4,
                "rust onnx score {g} vs python onnx golden {e}"
            );
        }
    }
}
