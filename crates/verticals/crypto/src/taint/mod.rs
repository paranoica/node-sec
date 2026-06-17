//! FIFO taint tracing + exposure scoring (T063).
//!
//! Tracks how "dirty" value from a known-bad cluster spreads through the value-flow graph. Taint is
//! accounted **FIFO** (Meiklejohn-style "first-in, first-out"): each address holds an ordered queue
//! of received lots, and a spend consumes the oldest lots first, forwarding their tainted share to
//! the destination. There is **no fixed hop cutoff** — taint propagates as far as the dirty value
//! flows. Exposure is split into **direct** (received straight from a bad address) and **indirect**
//! (received through intermediaries). A **peel chain** (bulk forwarded hop after hop while small
//! amounts are peeled off) is detected structurally and lifts the exposure score.

use std::collections::{HashMap, HashSet, VecDeque};

/// A single value transfer (the value-flow edge), assumed given in chronological order.
#[derive(Debug, Clone)]
pub struct Transfer {
    /// Sending address.
    pub from: String,
    /// Receiving address.
    pub to: String,
    /// Value moved (minor units).
    pub value_minor: i64,
}

impl Transfer {
    /// Construct a transfer.
    pub fn new(from: impl Into<String>, to: impl Into<String>, value_minor: i64) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
            value_minor,
        }
    }
}

/// Tunables for taint tracing / peel-chain detection.
#[derive(Debug, Clone)]
pub struct TaintConfig {
    /// Fraction of a hop's outgoing value that must go to one successor to count as a peel hop.
    pub peel_forward_ratio: f64,
    /// Consecutive peel hops required to call it a peel chain.
    pub peel_min_hops: usize,
}

impl Default for TaintConfig {
    fn default() -> Self {
        Self {
            peel_forward_ratio: 0.8,
            peel_min_hops: 3,
        }
    }
}

/// A received lot: its value and how much of it is tainted.
#[derive(Debug, Clone, Copy)]
struct Lot {
    value: i64,
    tainted: i64,
}

/// The exposure of a target address to a known-bad cluster.
#[derive(Debug, Clone, PartialEq)]
pub struct ExposureReport {
    /// Tainted value received directly from a bad address.
    pub direct_exposure_minor: i64,
    /// Tainted value received via intermediaries.
    pub indirect_exposure_minor: i64,
    /// Tainted share of the target's remaining balance, in `[0, 1]`.
    pub tainted_fraction: f64,
    /// Whether a peel-chain pattern was detected in the flow.
    pub peel_chain_detected: bool,
}

impl ExposureReport {
    /// Total tainted value reaching the target (direct + indirect).
    #[must_use]
    pub fn total_exposure_minor(&self) -> i64 {
        self.direct_exposure_minor + self.indirect_exposure_minor
    }

    /// A `[0, 1]` exposure score: tainted fraction, lifted when a peel chain is present.
    #[must_use]
    pub fn exposure_score(&self) -> f64 {
        let peel_bonus = if self.peel_chain_detected { 0.25 } else { 0.0 };
        (self.tainted_fraction + peel_bonus).min(1.0)
    }
}

/// Consume `amount` from an address's FIFO lots, returning the tainted value consumed.
///
/// A shortfall (the address received less than it spends) is treated as external clean funds.
fn consume_fifo(queue: &mut VecDeque<Lot>, amount: i64) -> i64 {
    let mut remaining = amount;
    let mut tainted_consumed = 0;
    while remaining > 0 {
        let Some(front) = queue.front_mut() else {
            break;
        };
        if front.value <= remaining {
            remaining -= front.value;
            tainted_consumed += front.tainted;
            queue.pop_front();
        } else {
            let take_tainted = front.tainted * remaining / front.value;
            front.value -= remaining;
            front.tainted -= take_tainted;
            tainted_consumed += take_tainted;
            remaining = 0;
        }
    }
    tainted_consumed
}

fn walk_peel(
    adj: &HashMap<String, Vec<(String, i64)>>,
    start: &str,
    config: &TaintConfig,
) -> usize {
    let mut current = start.to_string();
    let mut visited: HashSet<String> = HashSet::new();
    let mut hops = 0;
    while visited.insert(current.clone()) {
        let Some(outs) = adj.get(&current).filter(|o| !o.is_empty()) else {
            break;
        };
        let total: i64 = outs.iter().map(|(_, v)| *v).sum();
        let (dom_to, dom_v) = outs.iter().max_by_key(|(_, v)| *v).expect("non-empty");
        let dominant =
            outs.len() >= 2 && (*dom_v as f64) >= config.peel_forward_ratio * total as f64;
        if !dominant {
            break;
        }
        hops += 1;
        current = dom_to.clone();
    }
    hops
}

fn detect_peel_chain(
    adj: &HashMap<String, Vec<(String, i64)>>,
    bad: &HashSet<String>,
    config: &TaintConfig,
) -> bool {
    // Enter the chain at the successors of a bad source, then count consecutive peel hops.
    for source in bad {
        if let Some(outs) = adj.get(source) {
            for (entry, _) in outs {
                if walk_peel(adj, entry, config) >= config.peel_min_hops {
                    return true;
                }
            }
        }
    }
    false
}

/// Trace taint from a known-bad cluster and report a target address's exposure.
#[must_use]
pub fn trace_taint(
    transfers: &[Transfer],
    bad: &HashSet<String>,
    target: &str,
    config: &TaintConfig,
) -> ExposureReport {
    let mut lots: HashMap<String, VecDeque<Lot>> = HashMap::new();
    let mut adj: HashMap<String, Vec<(String, i64)>> = HashMap::new();
    let mut direct = 0;
    let mut indirect = 0;

    for transfer in transfers {
        adj.entry(transfer.from.clone())
            .or_default()
            .push((transfer.to.clone(), transfer.value_minor));

        let tainted_sent = if bad.contains(&transfer.from) {
            transfer.value_minor // a bad address is a source of fully-tainted value
        } else {
            consume_fifo(
                lots.entry(transfer.from.clone()).or_default(),
                transfer.value_minor,
            )
        };

        lots.entry(transfer.to.clone()).or_default().push_back(Lot {
            value: transfer.value_minor,
            tainted: tainted_sent,
        });

        if transfer.to == target && tainted_sent > 0 {
            if bad.contains(&transfer.from) {
                direct += tainted_sent;
            } else {
                indirect += tainted_sent;
            }
        }
    }

    let (tainted, total) = lots.get(target).map_or((0, 0), |queue| {
        queue
            .iter()
            .fold((0, 0), |(t, v), lot| (t + lot.tainted, v + lot.value))
    });
    let tainted_fraction = if total > 0 {
        tainted as f64 / total as f64
    } else {
        0.0
    };

    ExposureReport {
        direct_exposure_minor: direct,
        indirect_exposure_minor: indirect,
        tainted_fraction,
        peel_chain_detected: detect_peel_chain(&adj, bad, config),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bad(addrs: &[&str]) -> HashSet<String> {
        addrs.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn taint_direct_exposure_from_bad_source() {
        let transfers = vec![Transfer::new("hacker", "target", 100)];
        let report = trace_taint(
            &transfers,
            &bad(&["hacker"]),
            "target",
            &TaintConfig::default(),
        );
        assert_eq!(report.direct_exposure_minor, 100);
        assert_eq!(report.indirect_exposure_minor, 0);
    }

    #[test]
    fn taint_indirect_exposure_through_intermediary() {
        let transfers = vec![
            Transfer::new("hacker", "mixer", 100),
            Transfer::new("mixer", "target", 100),
        ];
        let report = trace_taint(
            &transfers,
            &bad(&["hacker"]),
            "target",
            &TaintConfig::default(),
        );
        assert_eq!(report.direct_exposure_minor, 0);
        assert_eq!(report.indirect_exposure_minor, 100);
        assert!((report.tainted_fraction - 1.0).abs() < 1e-9);
    }

    #[test]
    fn taint_has_no_fixed_hop_cutoff() {
        // Five hops deep: taint must still reach the target.
        let transfers = vec![
            Transfer::new("hacker", "a", 100),
            Transfer::new("a", "b", 100),
            Transfer::new("b", "c", 100),
            Transfer::new("c", "d", 100),
            Transfer::new("d", "target", 100),
        ];
        let report = trace_taint(
            &transfers,
            &bad(&["hacker"]),
            "target",
            &TaintConfig::default(),
        );
        assert_eq!(report.indirect_exposure_minor, 100);
    }

    #[test]
    fn taint_fifo_consumes_oldest_lot_first() {
        // mixer receives tainted first, then clean; the first 100 it sends is the tainted lot.
        let transfers = vec![
            Transfer::new("hacker", "mixer", 100),   // tainted (oldest)
            Transfer::new("exchange", "mixer", 100), // clean
            Transfer::new("mixer", "target", 100),   // FIFO -> tainted lot leaves first
            Transfer::new("mixer", "other", 100),    // clean lot leaves second
        ];
        let report = trace_taint(
            &transfers,
            &bad(&["hacker"]),
            "target",
            &TaintConfig::default(),
        );
        assert_eq!(report.indirect_exposure_minor, 100);
        let clean = trace_taint(
            &transfers,
            &bad(&["hacker"]),
            "other",
            &TaintConfig::default(),
        );
        assert_eq!(clean.indirect_exposure_minor, 0);
    }

    #[test]
    fn taint_peel_chain_detected_in_flow() {
        // hacker -> h1 (1000); each hop peels 50 and forwards the rest to a fresh address.
        let transfers = vec![
            Transfer::new("hacker", "h1", 1000),
            Transfer::new("h1", "peel1", 50),
            Transfer::new("h1", "h2", 950),
            Transfer::new("h2", "peel2", 50),
            Transfer::new("h2", "h3", 900),
            Transfer::new("h3", "peel3", 50),
            Transfer::new("h3", "h4", 850),
        ];
        let report = trace_taint(&transfers, &bad(&["hacker"]), "h4", &TaintConfig::default());
        assert!(report.peel_chain_detected);
    }

    #[test]
    fn taint_peel_chain_contributes_to_score() {
        // At a partial tainted fraction, a detected peel chain lifts the exposure score.
        let base = ExposureReport {
            direct_exposure_minor: 0,
            indirect_exposure_minor: 0,
            tainted_fraction: 0.5,
            peel_chain_detected: false,
        };
        let peeled = ExposureReport {
            peel_chain_detected: true,
            ..base.clone()
        };
        assert!(peeled.exposure_score() > base.exposure_score());
    }

    #[test]
    fn taint_plain_chain_is_not_a_peel_chain() {
        let transfers = vec![
            Transfer::new("hacker", "a", 100),
            Transfer::new("a", "b", 100),
            Transfer::new("b", "target", 100),
        ];
        let report = trace_taint(
            &transfers,
            &bad(&["hacker"]),
            "target",
            &TaintConfig::default(),
        );
        assert!(!report.peel_chain_detected);
    }
}
