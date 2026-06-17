//! The transaction graph (T041; D-009, `term:transaction-graph`, D-021 petgraph backend).
//!
//! A directed, time-stamped, weighted graph of money flows between entities: nodes are entities,
//! edges aggregate the transfers between an ordered pair with count, summed amount, and recency
//! (last-seen). Built and updated as transactions are processed.

use std::collections::HashMap;

use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::Direction;
use time::OffsetDateTime;

/// The aggregated weight of all transfers from one entity to another.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferEdge {
    /// Number of transfers.
    pub count: u64,
    /// Summed amount (minor units).
    pub total_minor: i64,
    /// Most recent transfer time (recency).
    pub last_seen: OffsetDateTime,
}

/// A directed, weighted, time-stamped transaction graph.
#[derive(Debug, Default)]
pub struct TransactionGraph {
    graph: DiGraph<String, TransferEdge>,
    nodes: HashMap<String, NodeIndex>,
}

impl TransactionGraph {
    /// An empty graph.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn node(&mut self, entity: &str) -> NodeIndex {
        if let Some(&idx) = self.nodes.get(entity) {
            return idx;
        }
        let idx = self.graph.add_node(entity.to_string());
        self.nodes.insert(entity.to_string(), idx);
        idx
    }

    /// Record a transfer `from → to`, creating or updating the aggregated edge.
    pub fn record_transfer(&mut self, from: &str, to: &str, amount_minor: i64, at: OffsetDateTime) {
        let a = self.node(from);
        let b = self.node(to);
        if let Some(edge) = self.graph.find_edge(a, b) {
            let weight = &mut self.graph[edge];
            weight.count += 1;
            weight.total_minor = weight.total_minor.saturating_add(amount_minor);
            weight.last_seen = weight.last_seen.max(at);
        } else {
            self.graph.add_edge(
                a,
                b,
                TransferEdge {
                    count: 1,
                    total_minor: amount_minor,
                    last_seen: at,
                },
            );
        }
    }

    /// The aggregated edge `from → to`, if any.
    #[must_use]
    pub fn transfer(&self, from: &str, to: &str) -> Option<&TransferEdge> {
        let a = *self.nodes.get(from)?;
        let b = *self.nodes.get(to)?;
        let edge = self.graph.find_edge(a, b)?;
        Some(&self.graph[edge])
    }

    /// Number of distinct outgoing counterparties for an entity.
    #[must_use]
    pub fn out_degree(&self, entity: &str) -> usize {
        self.degree(entity, Direction::Outgoing)
    }

    /// Number of distinct incoming counterparties for an entity.
    #[must_use]
    pub fn in_degree(&self, entity: &str) -> usize {
        self.degree(entity, Direction::Incoming)
    }

    fn degree(&self, entity: &str, direction: Direction) -> usize {
        self.nodes.get(entity).map_or(0, |&idx| {
            self.graph.neighbors_directed(idx, direction).count()
        })
    }

    /// Number of entities (nodes).
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.graph.node_count()
    }

    /// Number of directed entity-pair edges.
    #[must_use]
    pub fn edge_count(&self) -> usize {
        self.graph.edge_count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    #[test]
    fn txn_graph_aggregates_repeated_transfers() {
        let mut g = TransactionGraph::new();
        let t1 = datetime!(2026-06-17 10:00 UTC);
        let t2 = datetime!(2026-06-17 11:00 UTC);
        g.record_transfer("a", "b", 100, t1);
        g.record_transfer("a", "b", 250, t2);

        let edge = g.transfer("a", "b").unwrap();
        assert_eq!(edge.count, 2);
        assert_eq!(edge.total_minor, 350);
        assert_eq!(edge.last_seen, t2); // recency = latest
        assert_eq!(g.node_count(), 2);
        assert_eq!(g.edge_count(), 1);
    }

    #[test]
    fn txn_graph_is_directed() {
        let mut g = TransactionGraph::new();
        let t = datetime!(2026-06-17 10:00 UTC);
        g.record_transfer("a", "b", 100, t);
        assert!(g.transfer("a", "b").is_some());
        assert!(g.transfer("b", "a").is_none()); // direction matters
    }

    #[test]
    fn txn_graph_degrees_count_distinct_counterparties() {
        let mut g = TransactionGraph::new();
        let t = datetime!(2026-06-17 10:00 UTC);
        // hub fans out to 3 sinks and receives from 1 source.
        g.record_transfer("hub", "x", 10, t);
        g.record_transfer("hub", "y", 10, t);
        g.record_transfer("hub", "z", 10, t);
        g.record_transfer("src", "hub", 10, t);

        assert_eq!(g.out_degree("hub"), 3);
        assert_eq!(g.in_degree("hub"), 1);
        assert_eq!(g.out_degree("unknown"), 0);
    }
}
