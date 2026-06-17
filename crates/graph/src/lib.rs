//! `graph` — identity + transaction graph and real-time graph signals (D-009).
//!
//! T040 delivers entity resolution → identity clusters (`er`). The transaction graph (T041),
//! graph features (T042), ring/motif detection (T043), and mule scoring (T044) follow, all over the
//! in-process `petgraph` backend (D-021).
#![forbid(unsafe_code)]

pub mod er;

pub use er::{normalise, resolve, IdentifierKind, Record};
