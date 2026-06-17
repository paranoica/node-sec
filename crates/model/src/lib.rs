//! `model` — in-process ONNX inference of the fraud model (D-004).
//!
//! T031 loads the exported LightGBM tree-ensemble ONNX graph and scores feature vectors in-process
//! (no network hop), verified against the Python ONNX golden. T033 adds reason codes (top
//! contributing features → a versioned vocabulary); score fusion + expected-value selection are T032.
#![forbid(unsafe_code)]

pub mod explain;
pub mod inference;
pub mod registry;

pub use explain::{reason_codes, REASON_CODE_VERSION};
pub use inference::FraudModel;
pub use registry::{drift_alert, psi, ModelRegistry, ScoredDecision, Scorer};
