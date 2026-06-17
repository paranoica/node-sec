//! Reason codes for a model decision (T033; D-010 explainability, `term:reason-code`).
//!
//! The hot path attaches reason codes for the **top contributing features** using a lightweight,
//! in-process heuristic: features whose value most exceeds their alert threshold (scale-normalised).
//! The authoritative attribution is exact TreeSHAP, computed offline in Python (`ml/explain`) for
//! audit and dispute handling — the ONNX tree graph does not emit per-feature contributions, so the
//! in-process attribution is intentionally an approximation while the offline one is exact. Both map
//! onto the same versioned reason-code vocabulary.

use domain::ReasonCode;

/// Version of the reason-code vocabulary (stamped alongside the model).
pub const REASON_CODE_VERSION: &str = "rc-2026-06-17";

/// Feature → (reason code, alert threshold). Index-aligned with the model's feature vector
/// (`ml/training/synthetic.py::FEATURE_NAMES`).
const RULES: [(&str, f64); 8] = [
    ("MODEL_HIGH_VELOCITY_5M", 3.0),
    ("MODEL_HIGH_VELOCITY_1H", 10.0),
    ("MODEL_HIGH_VELOCITY_24H", 25.0),
    ("MODEL_AMOUNT_RATIO", 3.0),
    ("MODEL_AMOUNT_ZSCORE", 2.0),
    ("MODEL_DEVICE_SPREAD", 3.0),
    ("MODEL_DECLINE_RATE", 0.3),
    ("MODEL_HIGH_RISK_MCC", 0.5),
];

/// The top reason codes for a feature vector — the features most over their alert threshold,
/// strongest first, capped at `top_k`.
#[must_use]
pub fn reason_codes(features: &[f32], top_k: usize) -> Vec<ReasonCode> {
    let mut fired: Vec<(&str, f64)> = RULES
        .iter()
        .enumerate()
        .filter_map(|(i, &(code, threshold))| {
            features.get(i).and_then(|&value| {
                let strength = f64::from(value) / threshold;
                (strength >= 1.0).then_some((code, strength))
            })
        })
        .collect();
    fired.sort_by(|a, b| b.1.partial_cmp(&a.1).expect("finite strengths"));
    fired
        .into_iter()
        .take(top_k)
        .map(|(code, _)| ReasonCode::new(code))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reason_codes_surface_the_most_anomalous_features() {
        // velocity_5m=12 (4x threshold 3), amount_z_score=6 (3x threshold 2), rest below threshold.
        let features = [12.0, 2.0, 5.0, 1.0, 6.0, 1.0, 0.05, 0.0];
        let codes: Vec<String> = reason_codes(&features, 3)
            .iter()
            .map(|c| c.as_str().to_string())
            .collect();
        assert!(codes.contains(&"MODEL_HIGH_VELOCITY_5M".to_string()));
        assert!(codes.contains(&"MODEL_AMOUNT_ZSCORE".to_string()));
        // strongest first: velocity_5m (4.0x) ranks above amount_z_score (3.0x).
        assert_eq!(codes[0], "MODEL_HIGH_VELOCITY_5M");
    }

    #[test]
    fn reason_codes_empty_when_nothing_fires() {
        let features = [1.0, 3.0, 10.0, 1.0, 0.5, 1.0, 0.05, 0.0]; // all below threshold
        assert!(reason_codes(&features, 3).is_empty());
    }

    #[test]
    fn reason_codes_respects_top_k() {
        let features = [12.0, 40.0, 100.0, 9.0, 8.0, 9.0, 0.9, 1.0]; // everything fires
        assert_eq!(reason_codes(&features, 2).len(), 2);
    }
}
