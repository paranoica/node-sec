//! `domain` — core domain types: transactions, entities, decisions, and money.
//!
//! Invariants enforced here (see `docs/architecture.md`):
//! - `arch:money-integer` — [`money::Money`] is integer minor units + a currency; there is
//!   deliberately **no** floating-point money constructor or conversion.
//! - `arch:versioned-decision` — a [`decision::Decision`] records the rule and model versions that
//!   produced it, so a decision can be deterministically replayed.
#![forbid(unsafe_code)]

pub mod decision;
pub mod ids;
pub mod money;
pub mod transaction;

pub use decision::{Action, Decision, ReasonCode, RiskBand, RiskScore};
pub use ids::{AccountId, Bin, CounterpartyId, DeviceId, MerchantId, Pan, TransactionId};
pub use money::{Currency, Money, MoneyError};
pub use transaction::{Channel, Geo, Transaction, Vertical};
