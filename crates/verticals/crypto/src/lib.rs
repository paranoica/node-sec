//! `verticals-crypto` — the crypto fraud pack (D-023, `arch:vertical-agnostic-core`).
//!
//! On-chain analysis as a self-contained pack: ledger simulation, address clustering, taint
//! tracing, and sanctioned-address / scam-token screening. Like the P2P pack, the dependency points
//! into the core, never out of it — no core crate depends on this pack.
//!
//! - [`ledger`] — UTXO-style on-chain ledger simulation (T062).
//! - [`clustering`] — common-input-ownership clustering, CoinJoin-excluded (T062).
//! - [`taint`] — FIFO taint tracing + exposure scoring (T063).
//! - [`sanctions_scam`] — date-versioned sanctions + poisoning/Travel-Rule signals (T064).
#![forbid(unsafe_code)]

pub mod clustering;
pub mod ledger;
pub mod sanctions_scam;
pub mod taint;

pub use clustering::{cluster_addresses, is_coinjoin, ClusterConfig, Clustering};
pub use ledger::{Output, Tx};
pub use sanctions_scam::{
    looks_like_poisoning, poisoning_warning, travel_rule_flag, SanctionEntry, SanctionsList,
    ScamConfig, VaspTransfer,
};
pub use taint::{trace_taint, ExposureReport, TaintConfig, Transfer};
