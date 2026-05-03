#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Tests for the plugin-blueprint diff that drives `reinstall_with_diff`.
//!
//! These tests deliberately use only `Local` plugin sources with
//! non-existent paths so they never spawn `claude plugin install` — they
//! exercise the diff logic and the resulting skipped/failed result shape.

use portal::core::plugins::{PluginInstallResult, reinstall_with_diff};
use portal::core::profile::{PluginBlueprint, PluginEntry, PluginSource};
use std::collections::HashMap;

fn local(id: &str, path: &str) -> PluginEntry {
    PluginEntry {
        id: id.to_string(),
        enabled: true,
        source: PluginSource::Local {
            path: path.to_string(),
        },
    }
}

fn blueprint(plugins: Vec<PluginEntry>) -> PluginBlueprint {
    PluginBlueprint {
        version: 1,
        plugins,
        extra_known_marketplaces: HashMap::new(),
    }
}

fn count_skipped(results: &[PluginInstallResult]) -> usize {
    results.iter().filter(|r| r.skipped).count()
}

#[test]
fn no_active_blueprint_attempts_install_for_every_plugin() {
    let target = blueprint(vec![
        local("alpha@market", "/nonexistent/alpha"),
        local("beta@market", "/nonexistent/beta"),
    ]);

    let results = reinstall_with_diff(&target, None);
    assert_eq!(results.len(), 2);
    // Nothing skipped — without an active blueprint, every plugin is "new".
    assert_eq!(count_skipped(&results), 0);
    // Local sources point at nonexistent paths, so install fails — proves the
    // install path was reached for every entry.
    assert!(results.iter().all(|r| !r.success));
}

#[test]
fn identical_blueprints_skip_every_plugin() {
    let entries = vec![
        local("alpha@market", "/p/alpha"),
        local("beta@market", "/p/beta"),
        local("gamma@market", "/p/gamma"),
    ];
    let target = blueprint(entries.clone());
    let active = blueprint(entries);

    let results = reinstall_with_diff(&target, Some(&active));
    assert_eq!(results.len(), 3);
    assert_eq!(count_skipped(&results), 3);
    // Skipped plugins are reported as success — the desired end state holds.
    assert!(results.iter().all(|r| r.success));
    assert!(results.iter().all(|r| r.message == "already installed"));
}

#[test]
fn partial_overlap_only_installs_the_delta() {
    let active = blueprint(vec![
        local("alpha@market", "/p/alpha"),
        local("beta@market", "/p/beta"),
    ]);
    let target = blueprint(vec![
        local("alpha@market", "/p/alpha"), // unchanged → skip
        local("gamma@market", "/p/gamma"), // new → install (will fail, paths invalid)
    ]);

    let results = reinstall_with_diff(&target, Some(&active));
    assert_eq!(results.len(), 2);

    let alpha = results.iter().find(|r| r.id == "alpha@market").unwrap();
    assert!(alpha.skipped);
    assert!(alpha.success);

    let gamma = results.iter().find(|r| r.id == "gamma@market").unwrap();
    assert!(!gamma.skipped);
    // Install attempted; fails because /p/gamma doesn't exist on disk.
    assert!(!gamma.success);
}

#[test]
fn source_change_forces_reinstall_even_for_same_id() {
    let active = blueprint(vec![local("alpha@market", "/old/alpha")]);
    let target = blueprint(vec![local("alpha@market", "/new/alpha")]);

    let results = reinstall_with_diff(&target, Some(&active));
    assert_eq!(results.len(), 1);
    let alpha = &results[0];
    // Same id, different source path → fingerprint mismatch → reinstall.
    assert!(!alpha.skipped, "source change must trigger reinstall");
}

#[test]
fn results_are_sorted_by_id() {
    let target = blueprint(vec![
        local("zulu@market", "/p/zulu"),
        local("alpha@market", "/p/alpha"),
        local("mike@market", "/p/mike"),
    ]);

    let results = reinstall_with_diff(&target, None);
    let ids: Vec<&str> = results.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, vec!["alpha@market", "mike@market", "zulu@market"]);
}

#[test]
fn empty_target_returns_empty_results() {
    let target = blueprint(Vec::new());
    let active = blueprint(vec![local("alpha@market", "/p/alpha")]);
    assert!(reinstall_with_diff(&target, Some(&active)).is_empty());
    assert!(reinstall_with_diff(&target, None).is_empty());
}
