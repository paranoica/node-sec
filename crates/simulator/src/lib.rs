//! `simulator` — synthetic transaction generator with a fixed entity population and a pluggable
//! event sink.
//!
//! v0 (T004) emits **legitimate** card traffic at a configurable, reproducible rate, drawing from a
//! reusable [`Population`] so velocity and linkage features are meaningful. Fraud-pattern injection,
//! ground-truth, and delayed (chargeback) labels are layered on in T034. The real Redpanda-backed
//! [`EventSink`] is wired by the `ingest` crate, so unit tests need no broker.
#![forbid(unsafe_code)]

pub mod generator;
pub mod labels;
pub mod population;
pub mod rng;
pub mod sink;

pub use generator::{Generator, GeneratorConfig};
pub use labels::{Label, LabelConfig, LabelSource, LabelValue, LabeledOutcome, Labeler};
pub use population::Population;
pub use rng::Rng;
pub use sink::{CountingSink, EventSink, InMemorySink};
