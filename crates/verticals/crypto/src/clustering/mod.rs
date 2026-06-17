//! Address clustering via the common-input-ownership heuristic (T062).
//!
//! The classic chain-analysis heuristic: when a transaction spends several inputs together, one
//! party controlled all of them, so their addresses belong to the same wallet cluster. The crucial
//! exception is **CoinJoin** — there the inputs belong to *many* independent participants who merely
//! co-signed, so merging them would wrongly fuse unrelated wallets. CoinJoin transactions are
//! therefore excluded from the merge (detected by an explicit flag or a structural heuristic:
//! enough inputs alongside enough equal-valued outputs).

use std::collections::HashMap;

use crate::ledger::Tx;

/// Tunables for clustering / CoinJoin detection.
#[derive(Debug, Clone)]
pub struct ClusterConfig {
    /// Minimum count of both inputs and equal-valued outputs to call a tx a CoinJoin.
    pub coinjoin_min_equal_outputs: usize,
}

impl Default for ClusterConfig {
    fn default() -> Self {
        Self {
            coinjoin_min_equal_outputs: 3,
        }
    }
}

/// Whether a transaction should be treated as a CoinJoin (and thus excluded from the merge).
#[must_use]
pub fn is_coinjoin(tx: &Tx, config: &ClusterConfig) -> bool {
    if let Some(flag) = tx.coinjoin {
        return flag;
    }
    let mut equal: HashMap<i64, usize> = HashMap::new();
    for output in &tx.outputs {
        *equal.entry(output.value_minor).or_insert(0) += 1;
    }
    let max_equal = equal.values().copied().max().unwrap_or(0);
    tx.inputs.len() >= config.coinjoin_min_equal_outputs
        && max_equal >= config.coinjoin_min_equal_outputs
}

/// A HashMap-backed union-find over address strings.
#[derive(Debug, Default)]
struct UnionFind {
    parent: HashMap<String, String>,
}

impl UnionFind {
    fn add(&mut self, addr: &str) {
        self.parent
            .entry(addr.to_string())
            .or_insert_with(|| addr.to_string());
    }

    fn find(&mut self, addr: &str) -> String {
        let mut node = addr.to_string();
        while self.parent[&node] != node {
            let parent = self.parent[&node].clone();
            let grandparent = self.parent[&parent].clone();
            self.parent.insert(node.clone(), grandparent.clone()); // path-halving
            node = grandparent;
        }
        node
    }

    fn union(&mut self, a: &str, b: &str) {
        let ra = self.find(a);
        let rb = self.find(b);
        if ra != rb {
            self.parent.insert(ra, rb);
        }
    }
}

/// The resolved clustering: each address mapped to its cluster representative.
#[derive(Debug, Default)]
pub struct Clustering {
    rep: HashMap<String, String>,
}

impl Clustering {
    /// The cluster representative for an address, if seen.
    #[must_use]
    pub fn cluster_of(&self, addr: &str) -> Option<&str> {
        self.rep.get(addr).map(String::as_str)
    }

    /// Whether two addresses are in the same cluster.
    #[must_use]
    pub fn same_cluster(&self, a: &str, b: &str) -> bool {
        match (self.rep.get(a), self.rep.get(b)) {
            (Some(ra), Some(rb)) => ra == rb,
            _ => false,
        }
    }

    /// All clusters as grouped address lists.
    #[must_use]
    pub fn clusters(&self) -> Vec<Vec<String>> {
        let mut groups: HashMap<&str, Vec<String>> = HashMap::new();
        for (addr, rep) in &self.rep {
            groups.entry(rep.as_str()).or_default().push(addr.clone());
        }
        groups.into_values().collect()
    }
}

/// Cluster the addresses across a ledger by common-input-ownership, excluding CoinJoins.
#[must_use]
pub fn cluster_addresses(txs: &[Tx], config: &ClusterConfig) -> Clustering {
    let mut uf = UnionFind::default();

    for tx in txs {
        for addr in &tx.inputs {
            uf.add(addr);
        }
        for output in &tx.outputs {
            uf.add(&output.address);
        }
        if is_coinjoin(tx, config) {
            continue; // independent participants — do not merge their inputs
        }
        if let Some((first, rest)) = tx.inputs.split_first() {
            for other in rest {
                uf.union(first, other);
            }
        }
    }

    let addresses: Vec<String> = uf.parent.keys().cloned().collect();
    let mut rep = HashMap::new();
    for addr in addresses {
        let root = uf.find(&addr);
        rep.insert(addr, root);
    }
    Clustering { rep }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::Output;

    #[test]
    fn clustering_common_input_ownership_merges() {
        let txs = vec![Tx::new(
            "t1",
            vec!["a".into(), "b".into(), "c".into()],
            vec![Output::new("d", 100)],
        )];
        let clustering = cluster_addresses(&txs, &ClusterConfig::default());
        assert!(clustering.same_cluster("a", "b"));
        assert!(clustering.same_cluster("b", "c"));
    }

    #[test]
    fn clustering_chains_across_transactions() {
        // Shared input "b" links the two spends transitively: {a,b,d}.
        let txs = vec![
            Tx::new(
                "t1",
                vec!["a".into(), "b".into()],
                vec![Output::new("x", 10)],
            ),
            Tx::new(
                "t2",
                vec!["b".into(), "d".into()],
                vec![Output::new("y", 10)],
            ),
        ];
        let clustering = cluster_addresses(&txs, &ClusterConfig::default());
        assert!(clustering.same_cluster("a", "d"));
    }

    #[test]
    fn clustering_excludes_coinjoin_from_merge() {
        // A CoinJoin: many inputs, many equal-valued outputs — inputs are independent participants.
        let txs = vec![Tx::new(
            "cj",
            vec!["x".into(), "y".into(), "z".into()],
            vec![
                Output::new("o1", 100),
                Output::new("o2", 100),
                Output::new("o3", 100),
            ],
        )];
        let clustering = cluster_addresses(&txs, &ClusterConfig::default());
        assert!(!clustering.same_cluster("x", "y"));
        assert!(!clustering.same_cluster("y", "z"));
    }

    #[test]
    fn clustering_distinct_entities_stay_separate() {
        let txs = vec![
            Tx::new(
                "t1",
                vec!["a".into(), "b".into()],
                vec![Output::new("x", 10)],
            ),
            Tx::new(
                "t2",
                vec!["m".into(), "n".into()],
                vec![Output::new("y", 10)],
            ),
        ];
        let clustering = cluster_addresses(&txs, &ClusterConfig::default());
        assert!(!clustering.same_cluster("a", "m"));
    }
}
