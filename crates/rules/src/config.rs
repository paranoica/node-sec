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
