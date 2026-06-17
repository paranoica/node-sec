//! Mule-account detection by signature fusion (T044; D-009, `term:mule-account`).
//!
//! A mule's signature is not one threshold but a fusion: **fan-in** from dispersed sources then
//! **fan-out** to few destinations, a **pass-through ratio** near 1 (money in ≈ money out), **short
//! dwell** between receiving and forwarding, and **dormant-then-active** (a long-idle account that
//! suddenly receives and forwards). Each fired signal adds weight; crossing the threshold alerts.

/// Observed activity for one account over the scoring window.
#[derive(Debug, Clone)]
pub struct AccountActivity {
    /// Distinct senders into the account (fan-in dispersion).
    pub distinct_sources: u64,
    /// Distinct destinations out of the account (fan-out concentration).
    pub distinct_destinations: u64,
    /// Total amount received (minor units).
    pub total_in_minor: i64,
    /// Total amount sent (minor units).
    pub total_out_minor: i64,
    /// Seconds between the first credit and the first debit.
    pub dwell_secs: i64,
    /// Days the account was inactive immediately before this activity.
    pub dormant_days_before: i64,
}

/// Thresholds for the mule signals.
#[derive(Debug, Clone)]
pub struct MuleConfig {
    /// Minimum distinct sources for the fan-in signal.
    pub min_sources: u64,
    /// Maximum distinct destinations for the fan-out signal.
    pub max_destinations: u64,
    /// How close `out/in` must be to 1 for the pass-through signal.
    pub pass_through_tolerance: f64,
    /// Maximum dwell (seconds) for the short-dwell signal.
    pub max_dwell_secs: i64,
    /// Minimum prior dormancy (days) for the dormant-then-active signal.
    pub min_dormant_days: i64,
    /// Score at or above which a mule alert is raised.
    pub alert_threshold: f64,
}

impl Default for MuleConfig {
    fn default() -> Self {
        Self {
            min_sources: 5,
            max_destinations: 2,
            pass_through_tolerance: 0.1,
            max_dwell_secs: 3_600,
            min_dormant_days: 30,
            alert_threshold: 0.5,
        }
    }
}

/// A mule assessment: a fused score and the signals that fired.
#[derive(Debug, Clone, PartialEq)]
pub struct MuleScore {
    /// Fused score in `[0, 1]`.
    pub score: f64,
    /// The signals that contributed.
    pub signals: Vec<String>,
}

impl MuleScore {
    /// Whether the score crosses the alert threshold.
    #[must_use]
    pub fn is_mule(&self, config: &MuleConfig) -> bool {
        self.score >= config.alert_threshold
    }
}

const W_FAN: f64 = 0.4;
const W_PASS_THROUGH: f64 = 0.3;
const W_SHORT_DWELL: f64 = 0.2;
const W_DORMANT: f64 = 0.3;

/// Score an account's mule likelihood by fusing the signature signals.
#[must_use]
pub fn score_mule(activity: &AccountActivity, config: &MuleConfig) -> MuleScore {
    let mut score = 0.0;
    let mut signals = Vec::new();

    if activity.distinct_sources >= config.min_sources
        && activity.distinct_destinations <= config.max_destinations
        && activity.distinct_destinations >= 1
    {
        score += W_FAN;
        signals.push("fan_in_fan_out".to_string());
    }

    if activity.total_in_minor > 0 {
        let ratio = activity.total_out_minor as f64 / activity.total_in_minor as f64;
        if (ratio - 1.0).abs() <= config.pass_through_tolerance {
            score += W_PASS_THROUGH;
            signals.push("pass_through".to_string());
        }
    }

    if activity.total_out_minor > 0 && activity.dwell_secs <= config.max_dwell_secs {
        score += W_SHORT_DWELL;
        signals.push("short_dwell".to_string());
    }

    if activity.dormant_days_before >= config.min_dormant_days && activity.total_in_minor > 0 {
        score += W_DORMANT;
        signals.push("dormant_then_active".to_string());
    }

    MuleScore {
        score: score.min(1.0),
        signals,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> MuleConfig {
        MuleConfig::default()
    }

    #[test]
    fn mule_classic_signature_is_flagged() {
        // Many dispersed sources → one destination, money in ≈ out, forwarded within minutes.
        let activity = AccountActivity {
            distinct_sources: 8,
            distinct_destinations: 1,
            total_in_minor: 100_000,
            total_out_minor: 99_000,
            dwell_secs: 600,
            dormant_days_before: 0,
        };
        let score = score_mule(&activity, &cfg());
        assert!(
            score.is_mule(&cfg()),
            "classic mule must alert, got {}",
            score.score
        );
        assert!(score.signals.contains(&"fan_in_fan_out".to_string()));
        assert!(score.signals.contains(&"pass_through".to_string()));
        assert!(score.signals.contains(&"short_dwell".to_string()));
    }

    #[test]
    fn mule_dormant_then_active_contributes() {
        // Long-idle account that suddenly receives and forwards.
        let activity = AccountActivity {
            distinct_sources: 6,
            distinct_destinations: 1,
            total_in_minor: 50_000,
            total_out_minor: 49_500,
            dwell_secs: 1_800,
            dormant_days_before: 200,
        };
        let score = score_mule(&activity, &cfg());
        assert!(score.signals.contains(&"dormant_then_active".to_string()));
        assert!(score.is_mule(&cfg()));
    }

    #[test]
    fn normal_account_is_not_flagged() {
        // Few sources, retains most funds, slow, active account.
        let activity = AccountActivity {
            distinct_sources: 1,
            distinct_destinations: 1,
            total_in_minor: 100_000,
            total_out_minor: 10_000,
            dwell_secs: 86_400 * 5,
            dormant_days_before: 0,
        };
        let score = score_mule(&activity, &cfg());
        assert!(
            !score.is_mule(&cfg()),
            "normal account must not alert, got {}",
            score.score
        );
    }
}
