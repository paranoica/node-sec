//! Sanctioned-address screening (date-versioned) + scam/poisoning/Travel-Rule signals (T064).
//!
//! Three crypto-specific controls, each emitted in the shared rule vocabulary:
//!
//! * **Date-versioned sanctions** — an address is screened *as of the transaction date*. A hit when
//!   the address was listed at that date is a **hard decline**, and it is immutable: a later
//!   delisting never retroactively unblocks a past decision (we never re-screen "as of now").
//! * **Address poisoning** — a look-alike address sharing a real counterparty's prefix *and* suffix,
//!   appearing shortly after a genuine transfer, raises a poisoning **warning** (the attacker hopes
//!   the victim copies the poisoned address out of their history).
//! * **Travel Rule** — a VASP-to-VASP transfer at or above the de-minimis with no originator/
//!   beneficiary data is **flagged**.
//!
//! Dates and times are comparable `i64` ordinals (e.g. unix days / unix seconds) to keep the pack
//! self-contained.

use domain::ReasonCode;
use rules::engine::{Disposition, RuleHit};

/// A date-versioned sanctions entry. `delisted_day == None` means still listed.
#[derive(Debug, Clone)]
pub struct SanctionEntry {
    /// The sanctioned address.
    pub address: String,
    /// Day the address was listed (inclusive).
    pub listed_day: i64,
    /// Day the address was delisted (exclusive), if ever.
    pub delisted_day: Option<i64>,
}

/// A date-versioned sanctions list.
#[derive(Debug, Clone, Default)]
pub struct SanctionsList {
    entries: Vec<SanctionEntry>,
}

impl SanctionsList {
    /// Build a list from entries.
    #[must_use]
    pub fn new(entries: Vec<SanctionEntry>) -> Self {
        Self { entries }
    }

    /// Whether `address` was sanctioned **as of** `as_of_day`.
    #[must_use]
    pub fn is_sanctioned_as_of(&self, address: &str, as_of_day: i64) -> bool {
        self.entries.iter().any(|e| {
            e.address == address
                && e.listed_day <= as_of_day
                && e.delisted_day.is_none_or(|d| as_of_day < d)
        })
    }

    /// Screen an address as of the transaction date; a hit is a hard decline.
    ///
    /// Screening is always as-of the transaction date, so a later delisting cannot unblock a past
    /// decision.
    #[must_use]
    pub fn screen(&self, address: &str, tx_day: i64) -> Option<RuleHit> {
        self.is_sanctioned_as_of(address, tx_day).then(|| RuleHit {
            rule_id: "crypto.sanctions.hit".to_string(),
            reason_code: ReasonCode::new("CRYPTO_SANCTIONS"),
            typology: "sanctions".to_string(),
            disposition: Disposition::HardDecline,
        })
    }
}

/// Tunables for poisoning / Travel-Rule detection.
#[derive(Debug, Clone)]
pub struct ScamConfig {
    /// Prefix/suffix length compared for look-alike addresses.
    pub poison_affix_len: usize,
    /// Window (seconds) after a real transfer within which a look-alike is suspicious.
    pub poison_window_secs: i64,
    /// Travel-Rule de-minimis threshold (minor units).
    pub travel_rule_de_minimis_minor: i64,
}

impl Default for ScamConfig {
    fn default() -> Self {
        Self {
            poison_affix_len: 4,
            poison_window_secs: 86_400,            // 24h
            travel_rule_de_minimis_minor: 100_000, // e.g. $1,000
        }
    }
}

fn affix_match(a: &str, b: &str, k: usize) -> bool {
    let (ab, bb) = (a.as_bytes(), b.as_bytes());
    ab.len() >= k && bb.len() >= k && ab[..k] == bb[..k] && ab[ab.len() - k..] == bb[bb.len() - k..]
}

/// Whether `candidate` is a look-alike of `real` (shared prefix and suffix, not identical).
#[must_use]
pub fn looks_like_poisoning(real: &str, candidate: &str, config: &ScamConfig) -> bool {
    real != candidate && affix_match(real, candidate, config.poison_affix_len)
}

/// Raise an address-poisoning warning when a look-alike appears shortly after a real transfer.
#[must_use]
pub fn poisoning_warning(
    real_address: &str,
    real_at_secs: i64,
    candidate_address: &str,
    candidate_at_secs: i64,
    config: &ScamConfig,
) -> Option<RuleHit> {
    let within_window = (candidate_at_secs - real_at_secs) >= 0
        && (candidate_at_secs - real_at_secs) <= config.poison_window_secs;
    (within_window && looks_like_poisoning(real_address, candidate_address, config)).then(|| {
        RuleHit {
            rule_id: "crypto.poisoning".to_string(),
            reason_code: ReasonCode::new("CRYPTO_ADDR_POISON"),
            typology: "address_poisoning".to_string(),
            disposition: Disposition::Soft,
        }
    })
}

/// A VASP-to-VASP transfer carrying (or missing) Travel-Rule data.
#[derive(Debug, Clone)]
pub struct VaspTransfer {
    /// Originator is a VASP.
    pub from_vasp: bool,
    /// Beneficiary is a VASP.
    pub to_vasp: bool,
    /// Transfer amount (minor units).
    pub amount_minor: i64,
    /// Whether originator/beneficiary Travel-Rule data is attached.
    pub travel_rule_data: bool,
}

/// Flag a VASP-to-VASP transfer at/above the de-minimis that lacks Travel-Rule data.
#[must_use]
pub fn travel_rule_flag(transfer: &VaspTransfer, config: &ScamConfig) -> Option<RuleHit> {
    let applies = transfer.from_vasp
        && transfer.to_vasp
        && transfer.amount_minor >= config.travel_rule_de_minimis_minor;
    (applies && !transfer.travel_rule_data).then(|| RuleHit {
        rule_id: "crypto.travel_rule.missing".to_string(),
        reason_code: ReasonCode::new("CRYPTO_TRAVEL_RULE"),
        typology: "travel_rule".to_string(),
        disposition: Disposition::Soft,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list() -> SanctionsList {
        // Listed on day 10, delisted on day 20.
        SanctionsList::new(vec![SanctionEntry {
            address: "0xBAD".to_string(),
            listed_day: 10,
            delisted_day: Some(20),
        }])
    }

    #[test]
    fn sanctions_scam_blocks_sanctioned_as_of_date() {
        // A transfer on day 15, while listed, is hard-declined.
        let hit = list().screen("0xBAD", 15).expect("sanctioned");
        assert_eq!(hit.disposition, Disposition::HardDecline);
    }

    #[test]
    fn sanctions_scam_delisting_does_not_unblock_past_decision() {
        // The transfer happened on day 15 (listed). Screening it as-of day 15 still blocks,
        // regardless of the later day-20 delisting — past decisions are immutable.
        assert!(list().is_sanctioned_as_of("0xBAD", 15));
    }

    #[test]
    fn sanctions_scam_after_delisting_is_not_blocked() {
        // A transfer on day 25, after delisting, is not sanctioned.
        assert!(list().screen("0xBAD", 25).is_none());
        // Nor before listing.
        assert!(list().screen("0xBAD", 5).is_none());
    }

    #[test]
    fn sanctions_scam_poisoning_lookalike_warns() {
        let real = "0xABCD0000000000000000WXYZ";
        let poison = "0xABCD1111111111111111WXYZ"; // same 4-prefix + 4-suffix
        let hit = poisoning_warning(real, 1_000, poison, 1_500, &ScamConfig::default())
            .expect("poisoning");
        assert_eq!(hit.typology, "address_poisoning");
    }

    #[test]
    fn sanctions_scam_poisoning_identical_address_no_warn() {
        let real = "0xABCD0000000000000000WXYZ";
        assert!(poisoning_warning(real, 1_000, real, 1_500, &ScamConfig::default()).is_none());
    }

    #[test]
    fn sanctions_scam_poisoning_outside_window_no_warn() {
        let real = "0xABCD0000000000000000WXYZ";
        let poison = "0xABCD1111111111111111WXYZ";
        // Far outside the 24h window.
        assert!(
            poisoning_warning(real, 1_000, poison, 1_000 + 200_000, &ScamConfig::default())
                .is_none()
        );
    }

    #[test]
    fn sanctions_scam_travel_rule_missing_is_flagged() {
        let transfer = VaspTransfer {
            from_vasp: true,
            to_vasp: true,
            amount_minor: 500_000,
            travel_rule_data: false,
        };
        assert!(travel_rule_flag(&transfer, &ScamConfig::default()).is_some());
    }

    #[test]
    fn sanctions_scam_travel_rule_present_or_small_not_flagged() {
        let cfg = ScamConfig::default();
        // Data present.
        let with_data = VaspTransfer {
            from_vasp: true,
            to_vasp: true,
            amount_minor: 500_000,
            travel_rule_data: true,
        };
        assert!(travel_rule_flag(&with_data, &cfg).is_none());
        // Below de-minimis.
        let small = VaspTransfer {
            from_vasp: true,
            to_vasp: true,
            amount_minor: 1_000,
            travel_rule_data: false,
        };
        assert!(travel_rule_flag(&small, &cfg).is_none());
    }
}
