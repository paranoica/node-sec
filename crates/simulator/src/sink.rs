//! The event-backbone seam. The generator publishes through [`EventSink`] so it can be driven
//! against an in-memory collector in tests and against a real Redpanda producer in the running
//! system (the production sink is wired by the `ingest` crate, keeping a broker out of unit tests).

use domain::Transaction;

/// Where generated transactions are published.
pub trait EventSink {
    /// Publish one transaction.
    fn publish(&mut self, txn: &Transaction);
}

/// Collects every published transaction in memory — for tests and small replays.
#[derive(Debug, Default)]
pub struct InMemorySink {
    /// Everything published, in order.
    pub events: Vec<Transaction>,
}

impl EventSink for InMemorySink {
    fn publish(&mut self, txn: &Transaction) {
        self.events.push(txn.clone());
    }
}

/// Counts published transactions without retaining them — for throughput checks.
#[derive(Debug, Default)]
pub struct CountingSink {
    /// Number of transactions published.
    pub count: usize,
}

impl EventSink for CountingSink {
    fn publish(&mut self, _txn: &Transaction) {
        self.count += 1;
    }
}
