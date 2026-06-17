//! `api` — read-only dashboard APIs over the compliance/case layer (D-022).
//!
//! T055 delivers the analyst review-queue API; T066 the simulation control API.
#![forbid(unsafe_code)]

pub mod analyst;
pub mod sim;
