//! `stream` — the async update path (D-001): per-entity windowed feature aggregation into the online
//! store.
//!
//! T020 maintains count + summed-amount aggregates over the 1m/5m/1h/24h/7d/30d windows per entity
//! (card/BIN/account/device/IP/merchant) and materialises them to the online feature store
//! ([`store::InMemoryFeatureStore`] for tests, [`store::RedisFeatureStore`] for real). The Redpanda
//! consumer that drives it is a thin adapter wired in `ingest`.
#![forbid(unsafe_code)]

pub mod aggregator;
pub mod store;
pub mod window;

pub use aggregator::{entity_keys, Aggregator, StreamProcessor};
pub use store::{FeatureStore, InMemoryFeatureStore, RedisFeatureStore, StoreError};
pub use window::{EntityWindows, WindowAggregates, WindowStat, WINDOWS};
