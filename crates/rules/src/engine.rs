//! The rules engine: evaluates a transaction against hot-reloadable config.
//!
//! Reads are lock-free ([`ArcSwap`]) so the hot path is never blocked by a concurrent reload, and a
//! reload swaps the whole compiled config atomically (D-014, D-015 — change thresholds/blocklists
//! without a restart). T011 covers blocklists (hard declines); velocity/geo/amount rules land in
//! T012/T013 through the same evaluation.

use std::path::PathBuf;
use std::sync::Arc;

use arc_swap::ArcSwap;
use domain::{ReasonCode, Transaction};

use crate::config::{self, CompiledConfig, ConfigError, RulesConfig};
use crate::velocity::VelocityTracker;

/// What a rule hit forces. T011 produces only hard declines (from blocklists); soft signals and
/// hard approvals arrive with the scoring rules in later tasks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    /// Force a decline regardless of any score.
    HardDecline,
}

/// A single rule firing.
#[derive(Debug, Clone)]
pub struct RuleHit {
    /// Stable rule identifier (e.g. `blocklist.bin`).
    pub rule_id: String,
    /// Human-readable reason code attached to the decision.
    pub reason_code: ReasonCode,
    /// The typology this rule maps to (e.g. `blocklist`).
    pub typology: String,
    /// What the hit forces.
    pub disposition: Disposition,
}

/// The result of evaluating a transaction against the current config.
#[derive(Debug, Clone, Default)]
pub struct Evaluation {
    /// Every rule that fired.
    pub hits: Vec<RuleHit>,
}

impl Evaluation {
    /// True if any hit forces a hard decline.
    #[must_use]
    pub fn is_hard_decline(&self) -> bool {
        self.hits
            .iter()
            .any(|h| h.disposition == Disposition::HardDecline)
    }

    /// The reason codes of all hits, in order.
    #[must_use]
    pub fn reason_codes(&self) -> Vec<&ReasonCode> {
        self.hits.iter().map(|h| &h.reason_code).collect()
    }
}

/// The rules engine. Holds the current compiled config behind an [`ArcSwap`] for lock-free reads and
/// atomic hot reloads.
pub struct RulesEngine {
    config: ArcSwap<CompiledConfig>,
    source: Option<PathBuf>,
    velocity: VelocityTracker,
}

impl RulesEngine {
    /// Build from an in-memory config (no file backing — [`RulesEngine::reload`] will error).
    #[must_use]
    pub fn from_config(config: RulesConfig) -> Self {
        Self {
            config: ArcSwap::from_pointee(config.into()),
            source: None,
            velocity: VelocityTracker::new(),
        }
    }

    /// Build from a config file, remembering the path so it can be reloaded.
    ///
    /// # Errors
    /// Propagates [`ConfigError`] from reading or parsing the file.
    pub fn from_path(path: impl Into<PathBuf>) -> Result<Self, ConfigError> {
        let source = path.into();
        let config = config::load(&source)?;
        Ok(Self {
            config: ArcSwap::from_pointee(config.into()),
            source: Some(source),
            velocity: VelocityTracker::new(),
        })
    }

    /// Re-read the source file and atomically swap in the new config — no restart, and in-flight
    /// reads keep using the old config until they finish.
    ///
    /// # Errors
    /// [`ConfigError::NoSource`] if the engine was built in-memory; otherwise read/parse errors.
    pub fn reload(&self) -> Result<(), ConfigError> {
        let source = self.source.as_ref().ok_or(ConfigError::NoSource)?;
        let config = config::load(source)?;
        self.config.store(Arc::new(config.into()));
        Ok(())
    }

    /// The current config version.
    #[must_use]
    pub fn version(&self) -> String {
        self.config.load().version.clone()
    }

    /// Evaluate a transaction against the current blocklists.
    #[must_use]
    pub fn evaluate(&self, txn: &Transaction) -> Evaluation {
        let config = self.config.load();
        let mut hits = Vec::new();

        if let Some(pan) = &txn.pan {
            if let Some(bin) = pan.bin() {
                if config.bins.contains(bin.as_str()) {
                    hits.push(blocklist_hit("blocklist.bin", "BLOCKLIST_BIN"));
                }
            }
            if config.pan_tokens.contains(&pan.redacted()) {
                hits.push(blocklist_hit("blocklist.pan", "BLOCKLIST_PAN"));
            }
        }
        if let Some(device) = &txn.device {
            if config.devices.contains(device.as_str()) {
                hits.push(blocklist_hit("blocklist.device", "BLOCKLIST_DEVICE"));
            }
        }
        if let Some(ip) = &txn.ip {
            if config.ips.contains(&ip.to_string()) {
                hits.push(blocklist_hit("blocklist.ip", "BLOCKLIST_IP"));
            }
        }
        if let Some(merchant) = &txn.merchant {
            if config.merchants.contains(merchant.as_str()) {
                hits.push(blocklist_hit("blocklist.merchant", "BLOCKLIST_MERCHANT"));
            }
        }
        if let Some(counterparty) = &txn.counterparty {
            if config.counterparties.contains(counterparty.as_str()) {
                hits.push(blocklist_hit(
                    "blocklist.counterparty",
                    "BLOCKLIST_COUNTERPARTY",
                ));
            }
        }

        // Velocity rules: record this attempt and append any burst/enumeration hits.
        hits.extend(self.velocity.observe(txn, &config.velocity));

        Evaluation { hits }
    }

    /// Record a declined decision so the decline-retry-storm rule sees it on later attempts.
    pub fn record_decline(&self, txn: &Transaction) {
        self.velocity
            .record_decline(txn, &self.config.load().velocity);
    }
}

fn blocklist_hit(rule_id: &str, reason_code: &str) -> RuleHit {
    RuleHit {
        rule_id: rule_id.to_string(),
        reason_code: ReasonCode::new(reason_code),
        typology: "blocklist".to_string(),
        disposition: Disposition::HardDecline,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Blocklists, RulesConfig};
    use domain::{Channel, Currency, DeviceId, Money, Pan, TransactionId, Vertical};
    use time::macros::datetime;

    fn card_txn() -> Transaction {
        Transaction::new(
            TransactionId::new("txn-1"),
            Money::from_minor_units(4_999, Currency::Usd),
            datetime!(2026-06-17 12:00 UTC),
            Vertical::Card,
            Channel::CardNotPresent,
        )
        .with_pan(Pan::new("4111110000001234"))
        .with_device(DeviceId::new("dev-1"))
    }

    fn config_with(blocklists: Blocklists) -> RulesConfig {
        RulesConfig {
            version: "test".to_string(),
            blocklists,
            ..Default::default()
        }
    }

    #[test]
    fn clean_transaction_has_no_hits() {
        let engine = RulesEngine::from_config(config_with(Blocklists::default()));
        assert!(!engine.evaluate(&card_txn()).is_hard_decline());
    }

    #[test]
    fn blocked_bin_hard_declines_with_reason_and_typology() {
        let engine = RulesEngine::from_config(config_with(Blocklists {
            bins: vec!["411111".to_string()],
            ..Blocklists::default()
        }));
        let eval = engine.evaluate(&card_txn());
        assert!(eval.is_hard_decline());
        assert_eq!(eval.hits[0].reason_code.as_str(), "BLOCKLIST_BIN");
        assert_eq!(eval.hits[0].typology, "blocklist");
        assert_eq!(eval.hits[0].rule_id, "blocklist.bin");
    }

    #[test]
    fn blocked_device_hard_declines() {
        let engine = RulesEngine::from_config(config_with(Blocklists {
            devices: vec!["dev-1".to_string()],
            ..Blocklists::default()
        }));
        let eval = engine.evaluate(&card_txn());
        assert!(eval.is_hard_decline());
        assert_eq!(eval.hits[0].reason_code.as_str(), "BLOCKLIST_DEVICE");
    }

    #[test]
    fn reload_picks_up_new_rules_without_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rules.json");

        std::fs::write(&path, r#"{"version":"v1","blocklists":{}}"#).unwrap();
        let engine = RulesEngine::from_path(&path).unwrap();
        assert_eq!(engine.version(), "v1");
        assert!(!engine.evaluate(&card_txn()).is_hard_decline());

        // Analyst edits the file to block the BIN — no restart, same engine instance.
        std::fs::write(
            &path,
            r#"{"version":"v2","blocklists":{"bins":["411111"]}}"#,
        )
        .unwrap();
        engine.reload().unwrap();
        assert_eq!(engine.version(), "v2");
        assert!(engine.evaluate(&card_txn()).is_hard_decline());
    }

    #[test]
    fn reload_without_source_errors() {
        let engine = RulesEngine::from_config(config_with(Blocklists::default()));
        assert!(matches!(engine.reload(), Err(ConfigError::NoSource)));
    }

    #[test]
    fn failed_reload_keeps_the_running_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rules.json");

        std::fs::write(
            &path,
            r#"{"version":"v1","blocklists":{"bins":["411111"]}}"#,
        )
        .unwrap();
        let engine = RulesEngine::from_path(&path).unwrap();
        assert!(engine.evaluate(&card_txn()).is_hard_decline());

        // A corrupt edit must fail the reload and leave the previous config in force — a bad
        // analyst edit can never silently disable the rules.
        std::fs::write(&path, "{ not valid json").unwrap();
        assert!(engine.reload().is_err());
        assert_eq!(engine.version(), "v1");
        assert!(engine.evaluate(&card_txn()).is_hard_decline());
    }
}
