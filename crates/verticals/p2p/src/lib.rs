//! `verticals-p2p` — the P2P fraud pack (D-023, `arch:vertical-agnostic-core`).
//!
//! A self-contained pack of P2P-specific signals that the engine consumes through the shared
//! feature/rule/action vocabulary ([`rules::engine::RuleHit`], [`domain::Action`]). The dependency
//! points **into** the core (`domain`/`rules`), never out of it: no core crate depends on this
//! pack, so the engine stays vertical-agnostic and the pack plugs in.
//!
//! - [`app_fraud`] — Authorised-Push-Payment signals: new-payee + Confirmation of Payee (T060).
//! - [`coercion`] — coercion/behavioural signals, holds, and recipient-side mule freeze (T061).
#![forbid(unsafe_code)]

pub mod app_fraud;
pub mod coercion;

pub use app_fraud::{
    evaluate_app_fraud, P2pConfig, P2pPayment, P2pSignal, PayerHistory, PeerContext,
};
pub use coercion::{
    evaluate_coercion, evaluate_recipient, BehaviorSignals, CoercionConfig, CoercionSignal,
    RecipientOutcome,
};
