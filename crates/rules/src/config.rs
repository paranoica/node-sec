//! Rule configuration: the on-disk form (D-014 — rules are data, not code) and a lookup-optimised
//! compiled form. Loaded as JSON so analysts can edit thresholds and blocklists without a redeploy.

use std::collections::HashSet;
use std::fmt;
use std::path::Path;

use serde::Deserialize;

/// Rule configuration as authored on disk.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct RulesConfig {
    /// Version stamped onto every decision this config produces (`arch:versioned-decision`).
    pub version: String,
    /// Hard-deny lists by identifier kind.
    #[serde(default)]
    pub blocklists: Blocklists,
    /// Velocity rule thresholds.
    #[serde(default)]
    pub velocity: VelocityConfig,
    /// Impossible-travel threshold.
    #[serde(default)]
    pub impossible_travel: ImpossibleTravel,
    /// Amount-anomaly threshold.
    #[serde(default)]
    pub amount_anomaly: AmountAnomaly,
    /// MCC risk list.
    #[serde(default)]
    pub mcc_risk: MccRisk,
}

/// Impossible-travel threshold: two transactions for one card whose implied travel speed exceeds
/// this is physically impossible.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ImpossibleTravel {
    /// Maximum plausible travel speed in km/h (above this → impossible travel).
    pub max_speed_kmh: f64,
}

impl Default for ImpossibleTravel {
    fn default() -> Self {
        Self {
            max_speed_kmh: 1000.0,
        }
    }
}

/// Amount-anomaly threshold relative to a card's running average ticket.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AmountAnomaly {
    /// Fire when the amount exceeds `factor` times the card's running mean.
    pub factor: f64,
    /// Require at least this many prior samples before the rule can fire.
    pub min_samples: u64,
}

impl Default for AmountAnomaly {
    fn default() -> Self {
        Self {
            factor: 10.0,
            min_samples: 5,
        }
    }
}

/// High-risk Merchant Category Codes.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct MccRisk {
    /// MCCs treated as elevated risk (e.g. gambling, money transfer, crypto).
    pub high_risk: Vec<u32>,
}

/// Velocity rule thresholds (card-testing, BIN-attack, decline-retry storm).
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct VelocityConfig {
    /// Bursts of low-value auths from one device (card testing).
    pub card_testing: CardTesting,
    /// Many distinct PANs sharing one BIN (BIN enumeration).
    pub bin_attack: BinAttack,
    /// Repeated declines for one card (retry storm).
    pub decline_retry: DeclineRetry,
}

/// Card-testing thresholds.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct CardTesting {
    /// Sliding window in seconds.
    pub window_secs: i64,
    /// Fire above this many low-value auths per device in the window.
    pub max_low_value_auths: u64,
    /// "Low value" ceiling in minor units (auths at or below this count toward the burst).
    pub low_value_threshold_minor: i64,
}

impl Default for CardTesting {
    fn default() -> Self {
        Self {
            window_secs: 300,
            max_low_value_auths: 5,
            low_value_threshold_minor: 200,
        }
    }
}

/// BIN-attack thresholds.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct BinAttack {
    /// Sliding window in seconds.
    pub window_secs: i64,
    /// Fire above this many distinct PANs sharing one BIN in the window.
    pub max_distinct_pans: u64,
}

impl Default for BinAttack {
    fn default() -> Self {
        Self {
            window_secs: 300,
            max_distinct_pans: 10,
        }
    }
}

/// Decline-retry-storm thresholds.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct DeclineRetry {
    /// Sliding window in seconds.
    pub window_secs: i64,
    /// Fire above this many declines for one card in the window.
    pub max_declines: u64,
}

impl Default for DeclineRetry {
    fn default() -> Self {
        Self {
            window_secs: 600,
            max_declines: 5,
        }
    }
}

/// Deny lists; a match forces a hard decline.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct Blocklists {
    /// Issuer BINs (matched against `Pan::bin`).
    #[serde(default)]
    pub bins: Vec<String>,
    /// Card tokens `<bin>****<last4>` (matched against `Pan::redacted` — full PANs are never stored).
    #[serde(default)]
    pub pan_tokens: Vec<String>,
    /// Device fingerprints.
    #[serde(default)]
    pub devices: Vec<String>,
    /// Source IPs (matched against the textual address).
    #[serde(default)]
    pub ips: Vec<String>,
    /// Merchant ids.
    #[serde(default)]
    pub merchants: Vec<String>,
    /// Counterparty ids.
    #[serde(default)]
    pub counterparties: Vec<String>,
}

/// Configuration compiled for O(1) lookup on the hot path (`Vec` → `HashSet`).
#[derive(Debug, Default)]
pub struct CompiledConfig {
    /// Config version.
    pub version: String,
    /// Blocked BINs.
    pub bins: HashSet<String>,
    /// Blocked card tokens.
    pub pan_tokens: HashSet<String>,
    /// Blocked devices.
    pub devices: HashSet<String>,
    /// Blocked IPs.
    pub ips: HashSet<String>,
    /// Blocked merchants.
    pub merchants: HashSet<String>,
    /// Blocked counterparties.
    pub counterparties: HashSet<String>,
    /// Velocity thresholds (numbers, no compilation needed).
    pub velocity: VelocityConfig,
    /// Impossible-travel threshold.
    pub impossible_travel: ImpossibleTravel,
    /// Amount-anomaly threshold.
    pub amount_anomaly: AmountAnomaly,
    /// High-risk MCCs, compiled to a set.
    pub mcc_risk: HashSet<u32>,
}

impl From<RulesConfig> for CompiledConfig {
    fn from(c: RulesConfig) -> Self {
        Self {
            version: c.version,
            bins: c.blocklists.bins.into_iter().collect(),
            pan_tokens: c.blocklists.pan_tokens.into_iter().collect(),
            devices: c.blocklists.devices.into_iter().collect(),
            ips: c.blocklists.ips.into_iter().collect(),
            merchants: c.blocklists.merchants.into_iter().collect(),
            counterparties: c.blocklists.counterparties.into_iter().collect(),
            velocity: c.velocity,
            impossible_travel: c.impossible_travel,
            amount_anomaly: c.amount_anomaly,
            mcc_risk: c.mcc_risk.high_risk.into_iter().collect(),
        }
    }
}

/// Error loading or reloading rule configuration.
#[derive(Debug)]
pub enum ConfigError {
    /// Reading the file failed.
    Io(std::io::Error),
    /// Parsing the JSON failed.
    Parse(serde_json::Error),
    /// `reload` was called on an engine built from an in-memory config (no source file).
    NoSource,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Io(e) => write!(f, "reading rule config: {e}"),
            ConfigError::Parse(e) => write!(f, "parsing rule config: {e}"),
            ConfigError::NoSource => f.write_str("reload requires a file-backed config"),
        }
    }
}

impl std::error::Error for ConfigError {}

impl From<std::io::Error> for ConfigError {
    fn from(e: std::io::Error) -> Self {
        ConfigError::Io(e)
    }
}

impl From<serde_json::Error> for ConfigError {
    fn from(e: serde_json::Error) -> Self {
        ConfigError::Parse(e)
    }
}

/// Load and parse a rule config file.
///
/// # Errors
/// [`ConfigError::Io`] if the file can't be read; [`ConfigError::Parse`] if it isn't valid config.
pub fn load(path: &Path) -> Result<RulesConfig, ConfigError> {
    let text = std::fs::read_to_string(path)?;
    let config = serde_json::from_str(&text)?;
    Ok(config)
}
