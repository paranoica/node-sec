//! The online feature store (D-006): low-latency per-entity aggregates the hot path reads. The async
//! update path writes them. [`InMemoryFeatureStore`] backs tests; [`RedisFeatureStore`] is the
//! durable online store.

use std::collections::HashMap;
use std::fmt;
use std::sync::Mutex;

use redis::Commands;

use crate::window::WindowAggregates;

/// Error reading from or writing to the feature store.
#[derive(Debug)]
pub enum StoreError {
    /// Redis error.
    Redis(redis::RedisError),
    /// (De)serialisation error.
    Serde(serde_json::Error),
    /// A generic backend failure (the store is unavailable/faulted).
    Backend(String),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::Redis(e) => write!(f, "feature store redis error: {e}"),
            StoreError::Serde(e) => write!(f, "feature store serde error: {e}"),
            StoreError::Backend(msg) => write!(f, "feature store backend error: {msg}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<redis::RedisError> for StoreError {
    fn from(e: redis::RedisError) -> Self {
        StoreError::Redis(e)
    }
}

impl From<serde_json::Error> for StoreError {
    fn from(e: serde_json::Error) -> Self {
        StoreError::Serde(e)
    }
}

/// Where per-entity aggregates are materialised and read.
pub trait FeatureStore {
    /// Write the aggregates for an entity.
    ///
    /// # Errors
    /// Backend-specific (Redis / serialisation).
    fn put(&self, entity: &str, aggregates: &WindowAggregates) -> Result<(), StoreError>;

    /// Read the aggregates for an entity, if present.
    ///
    /// # Errors
    /// Backend-specific (Redis / deserialisation).
    fn get(&self, entity: &str) -> Result<Option<WindowAggregates>, StoreError>;
}

/// In-memory store for tests and small replays.
#[derive(Debug, Default)]
pub struct InMemoryFeatureStore {
    map: Mutex<HashMap<String, WindowAggregates>>,
}

impl FeatureStore for InMemoryFeatureStore {
    fn put(&self, entity: &str, aggregates: &WindowAggregates) -> Result<(), StoreError> {
        self.map
            .lock()
            .expect("feature store poisoned")
            .insert(entity.to_string(), aggregates.clone());
        Ok(())
    }

    fn get(&self, entity: &str) -> Result<Option<WindowAggregates>, StoreError> {
        Ok(self
            .map
            .lock()
            .expect("feature store poisoned")
            .get(entity)
            .cloned())
    }
}

/// Redis-backed online feature store. Aggregates are stored as JSON under `feat:<entity>`.
pub struct RedisFeatureStore {
    conn: Mutex<redis::Connection>,
    prefix: String,
}

impl RedisFeatureStore {
    /// Connect to Redis (e.g. `redis://127.0.0.1:6379`).
    ///
    /// # Errors
    /// [`StoreError::Redis`] if the connection fails.
    pub fn connect(url: &str) -> Result<Self, StoreError> {
        let client = redis::Client::open(url)?;
        let conn = client.get_connection()?;
        Ok(Self {
            conn: Mutex::new(conn),
            prefix: "feat:".to_string(),
        })
    }

    fn key(&self, entity: &str) -> String {
        format!("{}{entity}", self.prefix)
    }
}

impl FeatureStore for RedisFeatureStore {
    fn put(&self, entity: &str, aggregates: &WindowAggregates) -> Result<(), StoreError> {
        let json = serde_json::to_string(aggregates)?;
        let key = self.key(entity);
        let mut conn = self.conn.lock().expect("feature store poisoned");
        conn.set::<_, _, ()>(key, json)?;
        Ok(())
    }

    fn get(&self, entity: &str) -> Result<Option<WindowAggregates>, StoreError> {
        let key = self.key(entity);
        let mut conn = self.conn.lock().expect("feature store poisoned");
        let json: Option<String> = conn.get(key)?;
        match json {
            Some(s) => Ok(Some(serde_json::from_str(&s)?)),
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window::WindowStat;

    fn sample() -> WindowAggregates {
        WindowAggregates {
            windows: vec![WindowStat {
                label: "5m".to_string(),
                count: 3,
                sum_minor: 600,
                sum_sq: 120_000,
                ..Default::default()
            }],
        }
    }

    #[test]
    fn in_memory_store_round_trips() {
        let store = InMemoryFeatureStore::default();
        assert_eq!(store.get("card:abc").unwrap(), None);
        store.put("card:abc", &sample()).unwrap();
        assert_eq!(store.get("card:abc").unwrap(), Some(sample()));
    }

    #[test]
    #[ignore = "requires a running Redis (docker compose up redis); run with --ignored"]
    fn redis_store_round_trips() {
        let url =
            std::env::var("NODESEC_REDIS").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());
        let store = RedisFeatureStore::connect(&url).expect("connect");
        store.put("card:test", &sample()).expect("put");
        assert_eq!(store.get("card:test").expect("get"), Some(sample()));
    }
}
