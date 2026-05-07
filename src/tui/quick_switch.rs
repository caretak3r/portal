//! Pure scoring + ranking logic for the TUI's `/`-driven quick switcher.
//!
//! Kept free of TUI types so it can be unit-tested with synthetic profile
//! lists. The TUI shell (`app.rs`, `event.rs`) holds the live query and
//! cursor state; this module only answers "given a query, which profiles
//! match and in what order?".

use chrono::{DateTime, Utc};
use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;

/// Minimal projection of a profile that the ranker needs. Borrowed so callers
/// can build it cheaply from `ProfileInfo` without cloning manifests.
#[derive(Debug, Clone, Copy)]
pub struct RankInput<'a> {
    pub name: &'a str,
    pub last_loaded: Option<DateTime<Utc>>,
}

/// Rank a slice of profiles for the quick switcher.
///
/// - **Empty query**: returns every index, ordered by `last_loaded` descending
///   (most-recent first), with ties broken by case-insensitive name. This is
///   the "I just opened the picker" state — recency beats alphabetics.
/// - **Non-empty query**: keeps only profiles whose name fuzzy-matches the
///   query (skim algorithm), ordered by score descending, ties broken by
///   case-insensitive name.
///
/// Returned indices reference positions in the *input slice* — the caller maps
/// them back to whatever underlying profile collection they came from.
#[must_use]
pub fn rank_profiles(query: &str, profiles: &[RankInput<'_>]) -> Vec<usize> {
    if query.is_empty() {
        return rank_by_recency(profiles);
    }
    rank_by_fuzzy(query, profiles)
}

fn rank_by_recency(profiles: &[RankInput<'_>]) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..profiles.len()).collect();
    idx.sort_by(|&a, &b| {
        // Newest first; profiles with no last_loaded sink below loaded ones.
        let la = profiles[a].last_loaded;
        let lb = profiles[b].last_loaded;
        match (la, lb) {
            (Some(x), Some(y)) => y.cmp(&x),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        }
        .then_with(|| {
            profiles[a]
                .name
                .to_ascii_lowercase()
                .cmp(&profiles[b].name.to_ascii_lowercase())
        })
    });
    idx
}

fn rank_by_fuzzy(query: &str, profiles: &[RankInput<'_>]) -> Vec<usize> {
    let matcher = SkimMatcherV2::default().smart_case();
    let mut scored: Vec<(usize, i64)> = profiles
        .iter()
        .enumerate()
        .filter_map(|(i, p)| matcher.fuzzy_match(p.name, query).map(|s| (i, s)))
        .collect();
    scored.sort_by(|a, b| {
        b.1.cmp(&a.1).then_with(|| {
            profiles[a.0]
                .name
                .to_ascii_lowercase()
                .cmp(&profiles[b.0].name.to_ascii_lowercase())
        })
    });
    scored.into_iter().map(|(i, _)| i).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(year: i32, month: u32, day: u32) -> Option<DateTime<Utc>> {
        Utc.with_ymd_and_hms(year, month, day, 0, 0, 0).single()
    }

    fn input(name: &str, last: Option<DateTime<Utc>>) -> RankInput<'_> {
        RankInput {
            name,
            last_loaded: last,
        }
    }

    #[test]
    fn empty_query_orders_by_recency_desc() {
        let profiles = [
            input("alpha", at(2024, 1, 1)),
            input("beta", at(2026, 5, 1)),
            input("gamma", at(2025, 6, 1)),
        ];
        let order = rank_profiles("", &profiles);
        assert_eq!(order, vec![1, 2, 0]); // beta, gamma, alpha
    }

    #[test]
    fn empty_query_pushes_never_loaded_to_the_back() {
        let profiles = [
            input("never", None),
            input("recent", at(2026, 5, 1)),
            input("ancient", at(2020, 1, 1)),
        ];
        let order = rank_profiles("", &profiles);
        assert_eq!(order, vec![1, 2, 0]); // recent, ancient, never
    }

    #[test]
    fn empty_query_breaks_recency_ties_by_name() {
        let same = at(2026, 1, 1);
        let profiles = [
            input("zulu", same),
            input("alpha", same),
            input("mike", same),
        ];
        let order = rank_profiles("", &profiles);
        assert_eq!(order, vec![1, 2, 0]); // alpha, mike, zulu
    }

    #[test]
    fn non_empty_query_filters_to_fuzzy_matches() {
        let profiles = [
            input("work-redteam", None),
            input("personal-webdev", None),
            input("research", None),
        ];
        let order = rank_profiles("web", &profiles);
        // "personal-webdev" contains "web"; the others do not.
        assert_eq!(order.len(), 1);
        assert_eq!(order[0], 1);
    }

    #[test]
    fn fuzzy_query_scores_consecutive_letters_higher() {
        let profiles = [
            input("research", None),
            input("redteam", None),
            input("rover-engine", None),
        ];
        // "re" should match all three; "redteam" and "research" should
        // outrank "rover-engine" because their match is contiguous.
        let order = rank_profiles("re", &profiles);
        assert_eq!(order.len(), 3);
        assert_ne!(order[2], 0); // research never last
        assert_ne!(order[2], 1); // redteam never last
    }

    #[test]
    fn fuzzy_query_with_no_matches_returns_empty() {
        let profiles = [input("alpha", None), input("beta", None)];
        assert!(rank_profiles("zzz", &profiles).is_empty());
    }

    #[test]
    fn smart_case_is_case_insensitive_for_lowercase_query() {
        let profiles = [input("WorkRedTeam", None), input("Personal", None)];
        let order = rank_profiles("work", &profiles);
        assert_eq!(order, vec![0]);
    }
}
