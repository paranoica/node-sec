//! A minimal UTXO-style on-chain ledger simulation (T062).
//!
//! Just enough structure to drive address clustering and taint tracing: transactions spend a set of
//! input addresses and create value at a set of output addresses. CoinJoin transactions can be
//! flagged explicitly or left to the structural heuristic (many inputs, many equal-valued outputs).

/// A transaction output: value landing at an address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    /// The receiving address.
    pub address: String,
    /// Value (minor units, e.g. satoshis).
    pub value_minor: i64,
}

impl Output {
    /// Construct an output.
    pub fn new(address: impl Into<String>, value_minor: i64) -> Self {
        Self {
            address: address.into(),
            value_minor,
        }
    }
}

/// A simulated on-chain transaction.
#[derive(Debug, Clone)]
pub struct Tx {
    /// Transaction id.
    pub txid: String,
    /// Spent input addresses.
    pub inputs: Vec<String>,
    /// Created outputs.
    pub outputs: Vec<Output>,
    /// Explicit CoinJoin marker; `None` defers to the structural heuristic.
    pub coinjoin: Option<bool>,
}

impl Tx {
    /// A non-CoinJoin transaction (heuristic still applies via `coinjoin: None`).
    pub fn new(txid: impl Into<String>, inputs: Vec<String>, outputs: Vec<Output>) -> Self {
        Self {
            txid: txid.into(),
            inputs,
            outputs,
            coinjoin: None,
        }
    }

    /// The total output value (minor units).
    #[must_use]
    pub fn output_value(&self) -> i64 {
        self.outputs.iter().map(|o| o.value_minor).sum()
    }
}
