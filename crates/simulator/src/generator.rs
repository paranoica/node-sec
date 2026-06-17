//! The synthetic transaction generator (v0 — legitimate baseline traffic).
//!
//! It draws entities from a fixed [`Population`] so velocity/linkage features are meaningful, and
//! advances a **virtual clock** by `1/rate` per event so a configured rate is reproducible and
//! testable without sleeping. Fraud-pattern injection and labels are layered on later (T034); this
//! v0 emits only legitimate card traffic.

use domain::{Channel, Currency, Money, Transaction, TransactionId, Vertical};
use time::{Duration, OffsetDateTime};

use crate::population::Population;
use crate::rng::Rng;
use crate::sink::EventSink;

/// Configuration for a generator run.
#[derive(Debug, Clone)]
pub struct GeneratorConfig {
    /// Target events per second (must be > 0). Modeled as virtual-time spacing of `1/rate`.
    pub rate_per_sec: u32,
    /// Number of cards in the population (other pools scale off this).
    pub population_size: usize,
    /// Seed for reproducibility.
    pub seed: u64,
    /// Virtual start time of the first event.
    pub start: OffsetDateTime,
    /// Currency of generated amounts.
    pub currency: Currency,
}

/// A stateful generator producing legitimate card transactions.
#[derive(Debug, Clone)]
pub struct Generator {
    rng: Rng,
    population: Population,
    currency: Currency,
    interval: Duration,
    clock: OffsetDateTime,
    seq: u64,
}

impl Generator {
    /// Build a generator from config. Panics if `rate_per_sec == 0`.
    #[must_use]
    pub fn new(config: GeneratorConfig) -> Self {
        assert!(config.rate_per_sec > 0, "rate_per_sec must be > 0");
        let mut rng = Rng::new(config.seed);
        let population = Population::generate(config.population_size, &mut rng);
        let interval = Duration::nanoseconds(1_000_000_000 / i64::from(config.rate_per_sec));
        Self {
            rng,
            population,
            currency: config.currency,
            interval,
            clock: config.start,
            seq: 0,
        }
    }

    /// The population this generator draws from.
    #[must_use]
    pub fn population(&self) -> &Population {
        &self.population
    }

    /// Produce the next legitimate card transaction and advance the virtual clock.
    pub fn next_transaction(&mut self) -> Transaction {
        let occurred_at = self.clock;
        let id = TransactionId::new(format!("txn-{}", self.seq));
        // Legitimate ticket sizes: $1.00 .. $500.00 (integer minor units).
        let amount = Money::from_minor_units(self.rng.range_i64(100, 50_000), self.currency);

        let pan = self.rng.pick(&self.population.pans).clone();
        let merchant = self.rng.pick(&self.population.merchants).clone();
        let device = self.rng.pick(&self.population.devices).clone();
        let ip = *self.rng.pick(&self.population.ips);

        self.clock += self.interval;
        self.seq += 1;

        Transaction::new(
            id,
            amount,
            occurred_at,
            Vertical::Card,
            Channel::CardNotPresent,
        )
        .with_pan(pan)
        .with_merchant(merchant)
        .with_device(device)
        .with_ip(ip)
    }

    /// Generate `count` transactions and publish each to `sink`.
    pub fn run(&mut self, count: usize, sink: &mut impl EventSink) {
        for _ in 0..count {
            let txn = self.next_transaction();
            sink.publish(&txn);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sink::{CountingSink, InMemorySink};
    use domain::Pan;
    use std::collections::HashSet;
    use time::macros::datetime;

    fn config(rate: u32) -> GeneratorConfig {
        GeneratorConfig {
            rate_per_sec: rate,
            population_size: 100,
            seed: 1,
            start: datetime!(2026-06-17 00:00 UTC),
            currency: Currency::Usd,
        }
    }

    #[test]
    fn publishes_requested_count() {
        let mut gen = Generator::new(config(50));
        let mut sink = CountingSink::default();
        gen.run(1_000, &mut sink);
        assert_eq!(sink.count, 1_000);
    }

    #[test]
    fn events_are_schema_valid_card_transactions() {
        let mut gen = Generator::new(config(50));
        let mut sink = InMemorySink::default();
        gen.run(200, &mut sink);
        for txn in &sink.events {
            assert_eq!(txn.vertical, Vertical::Card);
            assert!(txn.pan.is_some(), "card txn must carry a PAN");
            assert!(txn.merchant.is_some(), "card txn must carry a merchant");
            assert!(txn.device.is_some());
            assert!(txn.amount.minor_units() >= 100);
            assert_eq!(txn.amount.currency(), Currency::Usd);
        }
    }

    #[test]
    fn transactions_reuse_the_population() {
        let mut gen = Generator::new(config(50));
        // Exact membership: Pan is Eq + Hash on the full number (no lossy redaction here).
        let pop_pans: HashSet<Pan> = gen.population().pans.iter().cloned().collect();
        let mut sink = InMemorySink::default();
        gen.run(500, &mut sink);
        for txn in &sink.events {
            assert!(
                pop_pans.contains(txn.pan.as_ref().unwrap()),
                "generated PAN must come from the population"
            );
        }
    }

    #[test]
    fn virtual_clock_advances_at_configured_rate() {
        let mut gen = Generator::new(config(50));
        let mut sink = InMemorySink::default();
        gen.run(100, &mut sink);
        let first = sink.events.first().unwrap().occurred_at;
        let last = sink.events.last().unwrap().occurred_at;
        // 100 events at 50/s span (100-1) intervals of 20ms = 1.98s.
        let span = last - first;
        assert_eq!(span, Duration::milliseconds(20) * 99);
    }

    #[test]
    fn same_seed_is_deterministic() {
        let mut a = Generator::new(config(50));
        let mut b = Generator::new(config(50));
        let (mut sa, mut sb) = (InMemorySink::default(), InMemorySink::default());
        a.run(50, &mut sa);
        b.run(50, &mut sb);
        let ids_a: Vec<_> = sa.events.iter().map(|t| t.amount.minor_units()).collect();
        let ids_b: Vec<_> = sb.events.iter().map(|t| t.amount.minor_units()).collect();
        assert_eq!(ids_a, ids_b);
    }
}
