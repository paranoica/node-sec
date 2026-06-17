//! `features` — the feature store reads and request-time derivation (D-006).
//!
//! T021 provides the hot-path online read with a per-call timeout ([`online::OnlineFeatures`]).
//! Request-time deviation features (T022) and the offline store + online/offline parity (T023)
//! follow. The aggregate types and the store backends live in `stream` (which writes them).
#![forbid(unsafe_code)]

pub mod online;

pub use online::{OnlineFeatures, ReadResult};
