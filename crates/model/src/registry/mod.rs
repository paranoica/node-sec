//! Model registry: champion-challenger with shadow scoring + PSI drift (T036; D-013 lifecycle).
//!
//! The **champion** serves decisions; a **challenger** shadow-scores the same traffic, its output
//! logged but never affecting the decision, so a new model can be evaluated safely on live traffic.
//! **PSI** (Population Stability Index) on a feature or score distribution detects drift; crossing
//! the threshold fires a drift alert and records a retraining trigger.

/// Anything that can score a feature vector to a fraud probability.
pub trait Scorer {
    /// Score a feature vector in `[0, 1]`.
    fn score(&self, features: &[f32]) -> f32;
}

struct Versioned {
    version: String,
    scorer: Box<dyn Scorer + Send + Sync>,
}

/// The result of a registry scoring: the decision-affecting champion score plus the challenger's
/// shadow score (logged, never used for the decision).
#[derive(Debug, Clone, PartialEq)]
pub struct ScoredDecision {
    /// The champion's score — the one the decision uses.
    pub champion_score: f32,
    /// The champion's version.
    pub champion_version: String,
    /// The challenger's shadow score, if a challenger is deployed (logged only).
    pub shadow_score: Option<f32>,
    /// The challenger's version, if deployed.
    pub challenger_version: Option<String>,
}

/// A champion model with an optional shadow challenger.
pub struct ModelRegistry {
    champion: Versioned,
    challenger: Option<Versioned>,
}

impl ModelRegistry {
    /// Create a registry with a champion (the model that serves decisions).
    #[must_use]
    pub fn new(version: impl Into<String>, champion: Box<dyn Scorer + Send + Sync>) -> Self {
        Self {
            champion: Versioned {
                version: version.into(),
                scorer: champion,
            },
            challenger: None,
        }
    }

    /// Deploy a challenger that shadow-scores live traffic without affecting decisions.
    #[must_use]
    pub fn with_challenger(
        mut self,
        version: impl Into<String>,
        challenger: Box<dyn Scorer + Send + Sync>,
    ) -> Self {
        self.challenger = Some(Versioned {
            version: version.into(),
            scorer: challenger,
        });
        self
    }

    /// Score a feature vector: the champion drives the decision, the challenger shadow-scores.
    #[must_use]
    pub fn score(&self, features: &[f32]) -> ScoredDecision {
        ScoredDecision {
            champion_score: self.champion.scorer.score(features),
            champion_version: self.champion.version.clone(),
            shadow_score: self.challenger.as_ref().map(|c| c.scorer.score(features)),
            challenger_version: self.challenger.as_ref().map(|c| c.version.clone()),
        }
    }
}

/// Population Stability Index between a `reference` and a `current` distribution over `bins`
/// equal-width bins. PSI < 0.1 stable, 0.1–0.25 moderate, > 0.25 significant.
#[must_use]
pub fn psi(reference: &[f64], current: &[f64], bins: usize) -> f64 {
    if reference.is_empty() || current.is_empty() || bins == 0 {
        return 0.0;
    }
    let lo = reference
        .iter()
        .chain(current)
        .copied()
        .fold(f64::INFINITY, f64::min);
    let hi = reference
        .iter()
        .chain(current)
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);
    if (hi - lo).abs() < f64::EPSILON {
        return 0.0; // single-valued → no drift signal
    }
    let width = (hi - lo) / bins as f64;
    let bin_of = |v: f64| (((v - lo) / width) as usize).min(bins - 1);

    let mut ref_counts = vec![0.0f64; bins];
    let mut cur_counts = vec![0.0f64; bins];
    for &v in reference {
        ref_counts[bin_of(v)] += 1.0;
    }
    for &v in current {
        cur_counts[bin_of(v)] += 1.0;
    }

    let eps = 1e-6;
    let rn = reference.len() as f64;
    let cn = current.len() as f64;
    (0..bins)
        .map(|i| {
            let r = (ref_counts[i] / rn).max(eps);
            let c = (cur_counts[i] / cn).max(eps);
            (c - r) * (c / r).ln()
        })
        .sum()
}

/// Whether a PSI value crosses the drift threshold (and a retraining trigger should be recorded).
#[must_use]
pub fn drift_alert(psi_value: f64, threshold: f64) -> bool {
    psi_value > threshold
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ConstScorer(f32);
    impl Scorer for ConstScorer {
        fn score(&self, _features: &[f32]) -> f32 {
            self.0
        }
    }

    #[test]
    fn registry_shadow_scores_without_affecting_the_decision() {
        let registry = ModelRegistry::new("champ-v1", Box::new(ConstScorer(0.2)))
            .with_challenger("chal-v2", Box::new(ConstScorer(0.9)));
        let decision = registry.score(&[1.0, 2.0, 3.0]);

        // The decision uses the champion's score, not the challenger's.
        assert_eq!(decision.champion_score, 0.2);
        assert_eq!(decision.champion_version, "champ-v1");
        // The challenger shadow-scored (logged) but did not change the decision.
        assert_eq!(decision.shadow_score, Some(0.9));
        assert_eq!(decision.challenger_version.as_deref(), Some("chal-v2"));
    }

    #[test]
    fn registry_without_challenger_has_no_shadow() {
        let registry = ModelRegistry::new("champ-v1", Box::new(ConstScorer(0.5)));
        let decision = registry.score(&[1.0]);
        assert_eq!(decision.shadow_score, None);
        assert_eq!(decision.challenger_version, None);
    }

    #[test]
    fn psi_near_zero_for_the_same_distribution() {
        let dist: Vec<f64> = (0..1000).map(|i| f64::from(i % 10)).collect();
        assert!(psi(&dist, &dist, 10) < 0.1);
    }

    #[test]
    fn psi_flags_a_shifted_distribution() {
        let reference: Vec<f64> = (0..1000).map(|i| f64::from(i % 10)).collect();
        let shifted: Vec<f64> = (0..1000).map(|i| f64::from(i % 10) + 8.0).collect();
        let value = psi(&reference, &shifted, 10);
        assert!(
            value > 0.25,
            "a large shift must exceed the significant threshold, got {value}"
        );
        assert!(drift_alert(value, 0.25));
    }
}
