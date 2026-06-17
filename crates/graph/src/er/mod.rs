//! Entity resolution → identity graph (T040; D-009 graph subsystem, D-021 petgraph backend).
//!
//! Pipeline: **normalise → block → match → cluster**. Records that share a *strong* identifier
//! (device, phone, email, address, card) are unioned into one identity; a *weak*, high-cardinality
//! identifier (a shared public IP) does **not** by itself merge distinct entities. Clustering is
//! connected-components via `petgraph`'s union-find.

use std::collections::HashMap;

use petgraph::unionfind::UnionFind;

/// The kind of an identifier — strong ones link entities, weak ones do not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentifierKind {
    /// Device fingerprint (strong).
    Device,
    /// Phone number (strong).
    Phone,
    /// Email address (strong).
    Email,
    /// Postal address (strong).
    Address,
    /// Card token (strong).
    Card,
    /// IP address (weak, high-cardinality).
    Ip,
}

impl IdentifierKind {
    /// Whether sharing this identifier alone justifies merging two records.
    #[must_use]
    pub fn is_strong(self) -> bool {
        !matches!(self, IdentifierKind::Ip)
    }

    fn key(self) -> &'static str {
        match self {
            IdentifierKind::Device => "device",
            IdentifierKind::Phone => "phone",
            IdentifierKind::Email => "email",
            IdentifierKind::Address => "address",
            IdentifierKind::Card => "card",
            IdentifierKind::Ip => "ip",
        }
    }
}

/// A record to resolve: a stable id plus its observed identifiers.
#[derive(Debug, Clone)]
pub struct Record {
    /// Stable record id.
    pub id: String,
    /// Observed `(kind, value)` identifiers.
    pub identifiers: Vec<(IdentifierKind, String)>,
}

/// Normalise an identifier value so trivially-different spellings match.
#[must_use]
pub fn normalise(kind: IdentifierKind, value: &str) -> String {
    match kind {
        IdentifierKind::Phone => value.chars().filter(char::is_ascii_digit).collect(),
        _ => value.trim().to_lowercase(),
    }
}

/// Resolve records into identity clusters (each a set of record ids).
#[must_use]
pub fn resolve(records: &[Record]) -> Vec<Vec<String>> {
    let mut uf = UnionFind::<usize>::new(records.len());
    // Block by normalised strong identifier; union records that collide in a block.
    let mut first_seen: HashMap<(&str, String), usize> = HashMap::new();
    for (i, record) in records.iter().enumerate() {
        for &(kind, ref value) in &record.identifiers {
            if !kind.is_strong() {
                continue;
            }
            let block = (kind.key(), normalise(kind, value));
            match first_seen.get(&block) {
                Some(&j) => {
                    uf.union(i, j);
                }
                None => {
                    first_seen.insert(block, i);
                }
            }
        }
    }

    let mut clusters: HashMap<usize, Vec<String>> = HashMap::new();
    for (i, record) in records.iter().enumerate() {
        clusters
            .entry(uf.find(i))
            .or_default()
            .push(record.id.clone());
    }
    clusters.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(id: &str, identifiers: &[(IdentifierKind, &str)]) -> Record {
        Record {
            id: id.to_string(),
            identifiers: identifiers
                .iter()
                .map(|&(k, v)| (k, v.to_string()))
                .collect(),
        }
    }

    fn cluster_of(clusters: &[Vec<String>], id: &str) -> Vec<String> {
        let mut c = clusters
            .iter()
            .find(|c| c.iter().any(|x| x == id))
            .unwrap()
            .clone();
        c.sort();
        c
    }

    #[test]
    fn records_sharing_a_strong_identifier_are_clustered() {
        let records = [
            rec(
                "a",
                &[
                    (IdentifierKind::Device, "D1"),
                    (IdentifierKind::Ip, "1.2.3.4"),
                ],
            ),
            rec("b", &[(IdentifierKind::Device, "D1")]), // shares device → same as a
            rec("c", &[(IdentifierKind::Email, "X@Y.com")]),
            rec("d", &[(IdentifierKind::Email, "x@y.com")]), // same email (normalised) → same as c
        ];
        let clusters = resolve(&records);
        assert_eq!(cluster_of(&clusters, "a"), vec!["a", "b"]);
        assert_eq!(cluster_of(&clusters, "c"), vec!["c", "d"]);
    }

    #[test]
    fn a_shared_weak_ip_does_not_merge_entities() {
        let records = [
            rec("a", &[(IdentifierKind::Ip, "1.2.3.4")]),
            rec("b", &[(IdentifierKind::Ip, "1.2.3.4")]), // only share IP → must NOT merge
        ];
        let clusters = resolve(&records);
        assert_eq!(clusters.len(), 2);
    }

    #[test]
    fn transitive_linking_merges_a_chain() {
        // a~b via device, b~c via email → all one identity.
        let records = [
            rec("a", &[(IdentifierKind::Device, "D9")]),
            rec(
                "b",
                &[
                    (IdentifierKind::Device, "D9"),
                    (IdentifierKind::Email, "p@q.com"),
                ],
            ),
            rec("c", &[(IdentifierKind::Email, "P@Q.com")]),
        ];
        let clusters = resolve(&records);
        assert_eq!(clusters.len(), 1);
        assert_eq!(cluster_of(&clusters, "a"), vec!["a", "b", "c"]);
    }
}
