//! The stream processor (D-001 async update path; `arch:partition-by-entity`): consumes transactions
//! and maintains per-entity windowed aggregates, materialising them to the online feature store.
//!
//! The Redpanda consumer that feeds [`StreamProcessor::process`] is a thin adapter wired in `ingest`;
//! the windowing here is broker-free so it tests hermetically.

use std::collections::HashMap;

use domain::Transaction;
use time::OffsetDateTime;

use crate::store::{FeatureStore, StoreError};
use crate::window::{EntityWindows, WindowAggregates};

/// Derive the entity keys a transaction contributes to (card token, BIN, account, device, IP,
/// merchant). Each key is namespaced so different entity kinds never collide.
#[must_use]
pub fn entity_keys(txn: &Transaction) -> Vec<String> {
    let mut keys = Vec::new();
    if let Some(pan) = &txn.pan {
        keys.push(format!("card:{}", pan.redacted()));
        if let Some(bin) = pan.bin() {
            keys.push(format!("bin:{}", bin.as_str()));
        }
    }
    if let Some(account) = &txn.account {
        keys.push(format!("account:{}", account.as_str()));
    }
    if let Some(device) = &txn.device {
        keys.push(format!("device:{}", device.as_str()));
    }
    if let Some(ip) = &txn.ip {
        keys.push(format!("ip:{ip}"));
    }
    if let Some(merchant) = &txn.merchant {
        keys.push(format!("merchant:{}", merchant.as_str()));
    }
    keys
}

/// Per-entity windowed aggregation. Each entity's state is independent — events for one key are
/// computed within that key's partition (`arch:partition-by-entity`).
///
/// # Bounding
/// `by_entity` is capped at [`MAX_ENTITIES`] with arbitrary eviction, so an open entity space cannot
/// exhaust memory (each entity's event deque already self-bounds to the largest window). A
/// production async path uses recency/TTL eviction or backs the working state in Redis; the
/// per-window read is also O(events) and would become incremental counters there.
/// Hard cap on tracked entities, bounding memory for an open entity space (the key set would
/// otherwise grow without limit under a high-cardinality flood → OOM). Eviction is arbitrary —
/// graceful degradation over a crash; recency/TTL eviction arrives with the Redis-backed store.
const MAX_ENTITIES: usize = 1_000_000;

#[derive(Debug)]
pub struct Aggregator {
    by_entity: HashMap<String, EntityWindows>,
    max_entities: usize,
}

impl Default for Aggregator {
    fn default() -> Self {
        Self::new()
    }
}

impl Aggregator {
    /// A fresh aggregator bounded to [`MAX_ENTITIES`].
    #[must_use]
    pub fn new() -> Self {
        Self {
            by_entity: HashMap::new(),
            max_entities: MAX_ENTITIES,
        }
    }

    /// A fresh aggregator with an explicit entity cap (for tests / tuning).
    #[must_use]
    pub fn with_max_entities(max_entities: usize) -> Self {
        Self {
            by_entity: HashMap::new(),
            max_entities: max_entities.max(1),
        }
    }

    /// Evict an arbitrary entity if adding `entity` would exceed the cap.
    fn make_room_for(&mut self, entity: &str) {
        if !self.by_entity.contains_key(entity) && self.by_entity.len() >= self.max_entities {
            if let Some(victim) = self.by_entity.keys().next().cloned() {
                self.by_entity.remove(&victim);
            }
        }
    }

    /// Record an event under one entity key.
    pub fn record(&mut self, entity: &str, at: OffsetDateTime, amount_minor: i64) {
        self.make_room_for(entity);
        self.by_entity
            .entry(entity.to_string())
            .or_default()
            .record(at, amount_minor);
    }

    /// Record an event with its device fingerprint and decline outcome, feeding the
    /// distinct-devices and decline-rate aggregates.
    pub fn record_full(
        &mut self,
        entity: &str,
        at: OffsetDateTime,
        amount_minor: i64,
        device: Option<String>,
        declined: bool,
    ) {
        self.make_room_for(entity);
        self.by_entity
            .entry(entity.to_string())
            .or_default()
            .record_full(at, amount_minor, device, declined);
    }

    /// The current aggregates for an entity as of `now` (empty if the entity is unknown).
    #[must_use]
    pub fn aggregates(&self, entity: &str, now: OffsetDateTime) -> WindowAggregates {
        self.by_entity
            .get(entity)
            .map(|e| e.aggregates(now))
            .unwrap_or_default()
    }
}

/// Consumes transactions, updates per-entity windows, and writes the fresh aggregates to the store.
pub struct StreamProcessor<S: FeatureStore> {
    aggregator: Aggregator,
    store: S,
}

impl<S: FeatureStore> StreamProcessor<S> {
    /// Build a processor over a feature store.
    pub fn new(store: S) -> Self {
        Self {
            aggregator: Aggregator::new(),
            store,
        }
    }

    /// Process one transaction: record it under each entity key, then materialise each key's fresh
    /// aggregates to the store. Returns the entity keys touched.
    ///
    /// # Errors
    /// Propagates [`StoreError`] from the store.
    pub fn process(&mut self, txn: &Transaction) -> Result<Vec<String>, StoreError> {
        let keys = entity_keys(txn);
        let device = txn.device.as_ref().map(|d| d.as_str().to_string());
        for key in &keys {
            self.aggregator.record_full(
                key,
                txn.occurred_at,
                txn.amount.minor_units(),
                device.clone(),
                false, // the decline outcome is fed back on the feedback path, not at ingest
            );
        }
        for key in &keys {
            let aggregates = self.aggregator.aggregates(key, txn.occurred_at);
            self.store.put(key, &aggregates)?;
        }
        Ok(keys)
    }

    /// Borrow the underlying store (for reads / tests).
    pub fn store(&self) -> &S {
        &self.store
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::InMemoryFeatureStore;
    use domain::{Channel, Currency, DeviceId, MerchantId, Money, Pan, TransactionId, Vertical};
    use time::macros::datetime;

    fn card_txn(pan: &str, device: &str, amount: i64, at: OffsetDateTime) -> Transaction {
        Transaction::new(
            TransactionId::new("t"),
            Money::from_minor_units(amount, Currency::Usd),
            at,
            Vertical::Card,
            Channel::CardNotPresent,
        )
        .with_pan(Pan::new(pan))
        .with_device(DeviceId::new(device))
        .with_merchant(MerchantId::new("mrc-1"))
    }

    #[test]
    fn aggregator_caps_the_entity_count() {
        let mut agg = Aggregator::with_max_entities(5);
        let at = datetime!(2026-06-17 12:00 UTC);
        // Record 50 distinct entities; the map must never exceed the cap.
        for i in 0..50 {
            agg.record(&format!("card:{i}"), at, 100);
        }
        assert!(
            agg.by_entity.len() <= 5,
            "entity map must be bounded, got {}",
            agg.by_entity.len()
        );
    }

    #[test]
    fn process_materialises_aggregates_for_each_entity_key() {
        let mut proc = StreamProcessor::new(InMemoryFeatureStore::default());
        let at = datetime!(2026-06-17 12:00 UTC);
        let keys = proc
            .process(&card_txn("4111110000001234", "dev-1", 500, at))
            .unwrap();

        // card, bin, device, merchant.
        assert!(keys.iter().any(|k| k.starts_with("card:")));
        assert!(keys.iter().any(|k| k.starts_with("bin:411111")));
        assert!(keys.iter().any(|k| k.starts_with("device:dev-1")));
        for key in &keys {
            let agg = proc.store().get(key).unwrap().unwrap();
            assert_eq!(agg.get("5m").unwrap().count, 1);
            assert_eq!(agg.get("5m").unwrap().sum_minor, 500);
        }
    }

    #[test]
    fn entities_are_partitioned_independently() {
        let mut proc = StreamProcessor::new(InMemoryFeatureStore::default());
        let at = datetime!(2026-06-17 12:00 UTC);
        // Two different cards, two events on the first.
        proc.process(&card_txn("4111110000001111", "dev-a", 100, at))
            .unwrap();
        proc.process(&card_txn("4111110000001111", "dev-a", 100, at))
            .unwrap();
        proc.process(&card_txn("4222220000002222", "dev-b", 100, at))
            .unwrap();

        let first = proc.store().get("card:411111****1111").unwrap().unwrap();
        let second = proc.store().get("card:422222****2222").unwrap().unwrap();
        assert_eq!(first.get("5m").unwrap().count, 2);
        assert_eq!(second.get("5m").unwrap().count, 1);
    }
}
