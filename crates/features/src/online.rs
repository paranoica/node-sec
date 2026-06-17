//! Hot-path read of the online feature store (D-006; `term:online-feature-store`,
//! `term:feature-freshness`).
//!
//! The hot path reads precomputed per-entity aggregates with a **per-call timeout**: if the store is
//! slow, the read returns [`ReadResult::TimedOut`] within budget rather than blocking, so the engine
//! can degrade to rules-only (fail-safe, T024) instead of busting the latency SLA. The blocking store
//! call is offloaded with `spawn_blocking` and bounded with `tokio::time::timeout`.

use std::sync::Arc;
use std::time::Duration;

use stream::{FeatureStore, WindowAggregates};

/// Outcome of a hot-path feature read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadResult {
    /// Aggregates were found for the entity.
    Hit(WindowAggregates),
    /// No aggregates for the entity (cold entity).
    Miss,
    /// The store did not answer within the per-call timeout.
    TimedOut,
    /// The store returned an error.
    Error,
}

impl ReadResult {
    /// The aggregates if this was a hit, else `None`.
    #[must_use]
    pub fn hit(self) -> Option<WindowAggregates> {
        match self {
            ReadResult::Hit(a) => Some(a),
            _ => None,
        }
    }

    /// True if the read did not yield usable features (the engine should degrade to rules-only).
    #[must_use]
    pub fn is_degraded(&self) -> bool {
        matches!(self, ReadResult::TimedOut | ReadResult::Error)
    }
}

/// Hot-path reader over an online feature store with a per-call timeout.
pub struct OnlineFeatures<S> {
    store: Arc<S>,
    timeout: Duration,
}

impl<S> OnlineFeatures<S>
where
    S: FeatureStore + Send + Sync + 'static,
{
    /// Build a reader over a shared store with a per-call timeout budget.
    #[must_use]
    pub fn new(store: Arc<S>, timeout: Duration) -> Self {
        Self { store, timeout }
    }

    /// The per-call timeout budget.
    #[must_use]
    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    /// Read an entity's aggregates within the timeout. A slow or failing store yields a degraded
    /// result instead of blocking the hot path.
    pub async fn read(&self, entity: &str) -> ReadResult {
        let store = Arc::clone(&self.store);
        let entity = entity.to_string();
        let blocking = tokio::task::spawn_blocking(move || store.get(&entity));
        match tokio::time::timeout(self.timeout, blocking).await {
            Ok(Ok(Ok(Some(aggregates)))) => ReadResult::Hit(aggregates),
            Ok(Ok(Ok(None))) => ReadResult::Miss,
            Ok(Ok(Err(_))) => ReadResult::Error,
            Ok(Err(_join)) => ReadResult::Error,
            Err(_elapsed) => ReadResult::TimedOut,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use stream::store::StoreError;
    use stream::{InMemoryFeatureStore, WindowStat};

    fn sample() -> WindowAggregates {
        WindowAggregates {
            windows: vec![WindowStat {
                label: "5m".to_string(),
                count: 3,
                sum_minor: 600,
            }],
        }
    }

    #[tokio::test]
    async fn online_read_returns_fresh_aggregates() {
        let store = Arc::new(InMemoryFeatureStore::default());
        // The async path writes; the hot path then reads the fresh value (freshness bound).
        store.put("card:abc", &sample()).unwrap();
        let reader = OnlineFeatures::new(Arc::clone(&store), Duration::from_millis(50));
        assert_eq!(reader.read("card:abc").await, ReadResult::Hit(sample()));
    }

    #[tokio::test]
    async fn online_read_misses_for_unknown_entity() {
        let store = Arc::new(InMemoryFeatureStore::default());
        let reader = OnlineFeatures::new(store, Duration::from_millis(50));
        assert_eq!(reader.read("card:unknown").await, ReadResult::Miss);
    }

    /// A store whose reads block longer than any sane budget.
    #[derive(Default)]
    struct SlowStore {
        inner: Mutex<()>,
    }

    impl FeatureStore for SlowStore {
        fn put(&self, _entity: &str, _aggregates: &WindowAggregates) -> Result<(), StoreError> {
            Ok(())
        }
        fn get(&self, _entity: &str) -> Result<Option<WindowAggregates>, StoreError> {
            let _guard = self.inner.lock().unwrap();
            std::thread::sleep(Duration::from_millis(500));
            Ok(Some(WindowAggregates::default()))
        }
    }

    #[tokio::test]
    async fn online_read_times_out_on_a_slow_store() {
        let reader = OnlineFeatures::new(Arc::new(SlowStore::default()), Duration::from_millis(20));
        let result = reader.read("card:abc").await;
        assert_eq!(result, ReadResult::TimedOut);
        assert!(
            result.is_degraded(),
            "a timeout must signal degradation for fail-safe"
        );
    }
}
