//! `rules` — the deterministic, hot-reloadable rules engine (D-014 rules-as-data, D-015 reload).
//!
//! T011 delivers config loading + atomic hot reload + blocklists (hard declines with reason codes
//! and typology tags). Velocity, geo/impossible-travel, amount-anomaly and MCC rules (T012/T013)
//! plug into the same [`engine::RulesEngine::evaluate`].
#![forbid(unsafe_code)]

pub mod config;
pub mod engine;

pub use config::{Blocklists, CompiledConfig, ConfigError, RulesConfig};
pub use engine::{Disposition, Evaluation, RuleHit, RulesEngine};
