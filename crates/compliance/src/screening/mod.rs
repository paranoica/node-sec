//! Sanctions / PEP / adverse-media screening (T050; D-010, `term:sanctions-screening`).
//!
//! Names are matched with **fuzzy** (Jaro-Winkler) and **phonetic** (Soundex) comparison. To cut
//! false positives, a name hit only alerts when a **secondary identifier** (DOB, nationality, or
//! national id) corroborates it. A watchlist delta triggers a batch rescreen of the customer base.

use serde::{Deserialize, Serialize};

/// Which list an entry belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ListKind {
    /// OFAC Specially Designated Nationals.
    OfacSdn,
    /// Politically Exposed Person.
    Pep,
    /// Adverse media.
    AdverseMedia,
}

/// A watchlist entry.
#[derive(Debug, Clone)]
pub struct WatchlistEntry {
    /// Full name.
    pub name: String,
    /// Date of birth (secondary identifier).
    pub dob: Option<String>,
    /// Nationality (secondary identifier).
    pub nationality: Option<String>,
    /// National id (secondary identifier).
    pub national_id: Option<String>,
    /// The list this entry is on.
    pub list: ListKind,
}

/// A subject (customer) being screened.
#[derive(Debug, Clone)]
pub struct Subject {
    /// Full name.
    pub name: String,
    /// Date of birth.
    pub dob: Option<String>,
    /// Nationality.
    pub nationality: Option<String>,
    /// National id.
    pub national_id: Option<String>,
}

/// A screening alert (a corroborated name match).
#[derive(Debug, Clone, PartialEq)]
pub struct ScreeningAlert {
    /// The subject's name.
    pub subject_name: String,
    /// The matched watchlist name.
    pub matched_name: String,
    /// The list matched.
    pub list: ListKind,
    /// The fuzzy name score.
    pub name_score: f64,
    /// Which secondary identifiers corroborated (e.g. `dob`, `nationality`).
    pub corroborated_by: Vec<String>,
}

/// Screening thresholds.
#[derive(Debug, Clone)]
pub struct ScreeningConfig {
    /// Jaro-Winkler score at or above which a name is a fuzzy match.
    pub name_threshold: f64,
    /// Jaro-Winkler score at or above which an **OFAC SDN** name match alerts on its own, without
    /// secondary-identifier corroboration (sanctions must not be suppressed to nothing).
    pub sanctions_strong_threshold: f64,
}

impl Default for ScreeningConfig {
    fn default() -> Self {
        Self {
            name_threshold: 0.88,
            sanctions_strong_threshold: 0.95,
        }
    }
}

fn normalise(name: &str) -> String {
    name.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Soundex phonetic code (4 chars), so e.g. `Smith` and `Smyth` share a code.
fn soundex(name: &str) -> String {
    let digit = |c: char| match c.to_ascii_uppercase() {
        'B' | 'F' | 'P' | 'V' => Some('1'),
        'C' | 'G' | 'J' | 'K' | 'Q' | 'S' | 'X' | 'Z' => Some('2'),
        'D' | 'T' => Some('3'),
        'L' => Some('4'),
        'M' | 'N' => Some('5'),
        'R' => Some('6'),
        _ => None,
    };
    let letters: Vec<char> = name.chars().filter(char::is_ascii_alphabetic).collect();
    let Some(&first) = letters.first() else {
        return "0000".to_string();
    };

    let mut code = String::new();
    code.push(first.to_ascii_uppercase());
    let mut last = digit(first);
    for &c in &letters[1..] {
        let d = digit(c);
        if let Some(dc) = d {
            if Some(dc) != last {
                code.push(dc);
            }
        }
        // H and W are transparent (don't reset); vowels reset so a repeat is re-counted.
        if !matches!(c.to_ascii_uppercase(), 'H' | 'W') {
            last = d;
        }
        if code.len() >= 4 {
            break;
        }
    }
    while code.len() < 4 {
        code.push('0');
    }
    code.truncate(4);
    code
}

/// Fuzzy + phonetic name match; returns the Jaro-Winkler score if it matches.
#[must_use]
pub fn name_match(a: &str, b: &str, threshold: f64) -> Option<f64> {
    let (na, nb) = (normalise(a), normalise(b));
    let score = strsim::jaro_winkler(&na, &nb);
    if score >= threshold || soundex(&na) == soundex(&nb) {
        Some(score)
    } else {
        None
    }
}

fn corroborate(subject: &Subject, entry: &WatchlistEntry) -> Vec<String> {
    let mut hits = Vec::new();
    let eq =
        |a: &Option<String>, b: &Option<String>| matches!((a, b), (Some(x), Some(y)) if x == y);
    if eq(&subject.dob, &entry.dob) {
        hits.push("dob".to_string());
    }
    if eq(&subject.nationality, &entry.nationality) {
        hits.push("nationality".to_string());
    }
    if eq(&subject.national_id, &entry.national_id) {
        hits.push("national_id".to_string());
    }
    hits
}

/// Screen a subject against a watchlist. A name hit only alerts when a secondary identifier
/// corroborates it (false-positive reduction).
#[must_use]
pub fn screen(
    subject: &Subject,
    watchlist: &[WatchlistEntry],
    config: &ScreeningConfig,
) -> Vec<ScreeningAlert> {
    watchlist
        .iter()
        .filter_map(|entry| {
            let name_score = name_match(&subject.name, &entry.name, config.name_threshold)?;
            let corroborated_by = corroborate(subject, entry);
            // Sanctions (OFAC SDN) must NOT be gated by secondary-identifier corroboration: SDN
            // records frequently carry no DOB/nationality, so requiring corroboration would silently
            // drop a true sanctioned-party hit. A strong name match to an SDN entry alerts on its
            // own; corroboration still gates fuzzy/phonetic matches and the PEP/adverse-media lists
            // (where name-only suppression is the right false-positive control).
            let strong_sanctions_hit =
                entry.list == ListKind::OfacSdn && name_score >= config.sanctions_strong_threshold;
            if corroborated_by.is_empty() && !strong_sanctions_hit {
                return None; // fuzzy / PEP / adverse-media name-only match → suppressed
            }
            Some(ScreeningAlert {
                subject_name: subject.name.clone(),
                matched_name: entry.name.clone(),
                list: entry.list,
                name_score,
                corroborated_by,
            })
        })
        .collect()
}

/// Rescreen the customer base against newly-added watchlist entries (a list delta).
#[must_use]
pub fn rescreen_on_delta(
    subjects: &[Subject],
    new_entries: &[WatchlistEntry],
    config: &ScreeningConfig,
) -> Vec<ScreeningAlert> {
    subjects
        .iter()
        .flat_map(|s| screen(s, new_entries, config))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, dob: Option<&str>, list: ListKind) -> WatchlistEntry {
        WatchlistEntry {
            name: name.to_string(),
            dob: dob.map(str::to_string),
            nationality: None,
            national_id: None,
            list,
        }
    }

    fn subject(name: &str, dob: Option<&str>) -> Subject {
        Subject {
            name: name.to_string(),
            dob: dob.map(str::to_string),
            nationality: None,
            national_id: None,
        }
    }

    #[test]
    fn screening_alerts_on_a_corroborated_fuzzy_match() {
        let wl = [entry(
            "Vladimir Petrov",
            Some("1970-05-01"),
            ListKind::OfacSdn,
        )];
        // Slight spelling variation + matching DOB.
        let alerts = screen(
            &subject("Vladimer Petrov", Some("1970-05-01")),
            &wl,
            &ScreeningConfig::default(),
        );
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].list, ListKind::OfacSdn);
        assert!(alerts[0].corroborated_by.contains(&"dob".to_string()));
    }

    #[test]
    fn screening_suppresses_a_name_only_pep_match() {
        // PEP / adverse-media: a name-only hit with no shared identifier is a likely false positive
        // and stays suppressed (the corroboration FP-control still applies to non-sanctions lists).
        let wl = [entry("John Smith", Some("1970-05-01"), ListKind::Pep)];
        let alerts = screen(
            &subject("John Smith", Some("1991-12-31")),
            &wl,
            &ScreeningConfig::default(),
        );
        assert!(alerts.is_empty());
    }

    #[test]
    fn screening_alerts_on_strong_ofac_match_without_corroboration() {
        // F1 regression: an OFAC SDN entry with NO secondary identifiers must still alert on a
        // strong name match — sanctions cannot be suppressed to nothing for lack of a DOB.
        let wl = [entry("Vladimir Petrov", None, ListKind::OfacSdn)];
        let alerts = screen(
            &subject("Vladimir Petrov", None),
            &wl,
            &ScreeningConfig::default(),
        );
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].list, ListKind::OfacSdn);
        assert!(alerts[0].corroborated_by.is_empty());
    }

    #[test]
    fn screening_suppresses_sub_strong_uncorroborated_ofac_match() {
        // With the strong-gate set above any achievable score, an uncorroborated SDN match is
        // suppressed — confirming the fix opens the gate only for STRONG matches, not a blanket
        // alert-on-any-name.
        let cfg = ScreeningConfig {
            name_threshold: 0.88,
            sanctions_strong_threshold: 1.01,
        };
        let wl = [entry("Vladimir Petrov", None, ListKind::OfacSdn)];
        let alerts = screen(&subject("Vladimir Petrov", None), &wl, &cfg);
        assert!(alerts.is_empty());
    }

    #[test]
    fn screening_matches_phonetically() {
        // "Smyth" vs "Smith" — same Soundex; corroborating DOB.
        let wl = [entry("John Smith", Some("1980-01-01"), ListKind::Pep)];
        let alerts = screen(
            &subject("John Smyth", Some("1980-01-01")),
            &wl,
            &ScreeningConfig::default(),
        );
        assert_eq!(alerts.len(), 1);
        assert_eq!(soundex("Smith"), soundex("Smyth"));
    }

    #[test]
    fn screening_list_delta_rescreens_the_customer_base() {
        let customers = [
            subject("Maria Garcia", Some("1985-03-03")),
            subject("Vladimir Petrov", Some("1970-05-01")),
        ];
        // A new SDN entry is ingested.
        let delta = [entry(
            "Vladimir Petrov",
            Some("1970-05-01"),
            ListKind::OfacSdn,
        )];
        let alerts = rescreen_on_delta(&customers, &delta, &ScreeningConfig::default());
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].subject_name, "Vladimir Petrov");
    }
}
