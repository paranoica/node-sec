//! `model` — in-process ONNX inference of the fraud model (D-004).
//!
//! T031 loads the exported LightGBM tree-ensemble ONNX graph and scores feature vectors in-process
//! (no network hop), verified against the Python ONNX golden. SHAP-based reason codes are T033;
//! score fusion + expected-value action selection are T032.
#![forbid(unsafe_code)]

pub mod inference;

pub use inference::FraudModel;
