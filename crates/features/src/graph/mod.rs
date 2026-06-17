//! Graph features materialised to the online feature store (T042; D-009).
//!
//! Computed in batch over the transaction graph (`ml/graph/features`) and written here per entity so
//! the hot path can read them alongside the window aggregates. The Redis-backed store uses the same
//! JSON pattern as the window store (`stream::RedisFeatureStore`); [`InMemoryGraphStore`] backs tests.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// Per-entity graph features (mirrors `ml/graph/features/compute.py`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GraphFeatures {
    /// PageRank centrality on the directed graph.
    pub pagerank: f64,
    /// Personalised-PageRank mass from known-bad seeds (risk exposure).
    pub ppr_bad: f64,
    /// Community id.
    pub community: i64,
    /// Size of the entity's community.
    pub community_size: u64,
    /// Hops to the nearest known-bad node, or `-1` if unreachable.
    pub dist_to_bad: i64,
}

/// Where per-entity graph features are materialised and read.
pub trait GraphFeatureStore {
    /// Write the graph features for an entity.
    fn put(&self, entity: &str, features: &GraphFeatures);
    /// Read the graph features for an entity, if present.
    fn get(&self, entity: &str) -> Option<GraphFeatures>;
}

/// In-memory graph-feature store for tests.
#[derive(Debug, Default)]
pub struct InMemoryGraphStore {
    map: Mutex<HashMap<String, GraphFeatures>>,
}

impl GraphFeatureStore for InMemoryGraphStore {
    fn put(&self, entity: &str, features: &GraphFeatures) {
        self.map
            .lock()
            .expect("graph store poisoned")
            .insert(entity.to_string(), features.clone());
    }

    fn get(&self, entity: &str) -> Option<GraphFeatures> {
        self.map
            .lock()
            .expect("graph store poisoned")
            .get(entity)
            .cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> GraphFeatures {
        GraphFeatures {
            pagerank: 0.25,
            ppr_bad: 0.4,
            community: 1,
            community_size: 3,
            dist_to_bad: 2,
        }
    }

    #[test]
    fn graph_features_materialise_and_read_back() {
        let store = InMemoryGraphStore::default();
        assert_eq!(store.get("card:abc"), None);
        store.put("card:abc", &sample());
        assert_eq!(store.get("card:abc"), Some(sample()));
    }

    #[test]
    fn graph_features_serialise_for_the_online_store() {
        let json = serde_json::to_string(&sample()).unwrap();
        assert_eq!(
            serde_json::from_str::<GraphFeatures>(&json).unwrap(),
            sample()
        );
    }
}
