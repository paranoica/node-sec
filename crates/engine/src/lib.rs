//! `engine` — the synchronous decision engine (the hot path).
//!
//! T010 delivers the gRPC `Decide` endpoint (D-001 sync path) behind an idempotency cache (D-016,
//! `arch:idempotent-decision`). The rules engine (T011), feature lookup (T021), in-process model
//! inference (T031), and expected-value action selection (T032) plug into the [`decide::Decider`]
//! seam in later tasks.
#![forbid(unsafe_code)]

/// gRPC types generated from `proto/decision.proto`. Generated code is exempt from our lints.
pub mod pb {
    #![allow(clippy::all, clippy::pedantic, clippy::nursery)]
    tonic::include_proto!("nodesec.decision.v1");
}

pub mod api;
pub mod decide;
pub mod decision;
pub mod degrade;

pub use api::DecisionEngine;
pub use decide::{ApproveAllDecider, Decider};
pub use decision::RulesDecider;
pub use degrade::FeatureAwareDecider;
