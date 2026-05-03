#![cfg(feature = "tui-ratatui")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Integration coverage for the TUI quick-switch ranker. The pure scoring
//! logic also has unit tests inside `src/tui/quick_switch.rs`; this file
//! pins down the public crate path callers actually use.

use chrono::{TimeZone, Utc};
use portal::tui::quick_switch::{RankInput, rank_profiles};

#[test]
fn empty_query_returns_recency_ordered_indices_via_public_api() {
    let recent = Utc.with_ymd_and_hms(2026, 5, 1, 0, 0, 0).single();
    let older = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).single();

    let profiles = [
        RankInput {
            name: "alpha",
            last_loaded: older,
        },
        RankInput {
            name: "beta",
            last_loaded: recent,
        },
    ];

    let order = rank_profiles("", &profiles);
    assert_eq!(order, vec![1, 0]);
}

#[test]
fn fuzzy_query_filters_via_public_api() {
    let profiles = [
        RankInput {
            name: "work-redteam",
            last_loaded: None,
        },
        RankInput {
            name: "personal-webdev",
            last_loaded: None,
        },
        RankInput {
            name: "research",
            last_loaded: None,
        },
    ];

    let order = rank_profiles("web", &profiles);
    assert_eq!(order, vec![1]);
}
