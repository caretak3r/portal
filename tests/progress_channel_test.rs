#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Tests for `ChannelProgress` — the `ProgressReporter` adapter that the
//! async TUI loader uses to forward phase / progress events back to the
//! main thread over an `mpsc` channel.

use portal::core::progress::{ChannelProgress, LoadEvent, ProgressReporter};
use std::sync::mpsc;

#[test]
fn phase_emits_a_phase_event() {
    let (tx, rx) = mpsc::channel();
    let reporter = ChannelProgress::new(tx);
    reporter.phase("Backing up");

    match rx.recv().expect("event") {
        LoadEvent::Phase(label) => assert_eq!(label, "Backing up"),
        other => panic!("expected Phase, got {other:?}"),
    }
}

#[test]
fn tick_carries_the_total_captured_by_set_total() {
    let (tx, rx) = mpsc::channel();
    let reporter = ChannelProgress::new(tx);
    reporter.set_total(42);
    reporter.tick(7, "skills/foo.md");

    match rx.recv().expect("event") {
        LoadEvent::Progress {
            current,
            total,
            item,
        } => {
            assert_eq!(current, 7);
            assert_eq!(total, 42);
            assert_eq!(item, "skills/foo.md");
        }
        other => panic!("expected Progress, got {other:?}"),
    }
}

#[test]
fn finish_does_not_emit_done() {
    // The reporter never emits a Done — that's the spawning thread's job
    // (the loader returns a Result the thread wraps and sends explicitly).
    // Calling finish() must therefore be a no-op on the channel.
    let (tx, rx) = mpsc::channel();
    let reporter = ChannelProgress::new(tx);
    reporter.finish("done");
    drop(reporter); // closes the channel
    assert!(rx.try_recv().is_err(), "no events should have been sent");
}

#[test]
fn dropped_receiver_does_not_panic() {
    // If the UI is torn down while the loader thread is still running,
    // sends will fail silently rather than panicking the worker.
    let (tx, rx) = mpsc::channel();
    let reporter = ChannelProgress::new(tx);
    drop(rx);

    reporter.phase("Building");
    reporter.set_total(10);
    reporter.tick(1, "file");
    reporter.finish("done");
}

#[test]
fn events_arrive_in_emit_order() {
    let (tx, rx) = mpsc::channel();
    let reporter = ChannelProgress::new(tx);

    reporter.phase("Backing up");
    reporter.phase("Building");
    reporter.set_total(2);
    reporter.tick(1, "a");
    reporter.tick(2, "b");
    reporter.phase("Atomic swap");
    drop(reporter);

    let labels: Vec<String> = rx
        .iter()
        .map(|ev| match ev {
            LoadEvent::Phase(l) => format!("phase:{l}"),
            LoadEvent::Progress { current, item, .. } => format!("tick:{current}:{item}"),
            LoadEvent::Done(_) => "done".to_string(),
        })
        .collect();

    assert_eq!(
        labels,
        vec![
            "phase:Backing up".to_string(),
            "phase:Building".to_string(),
            "tick:1:a".to_string(),
            "tick:2:b".to_string(),
            "phase:Atomic swap".to_string(),
        ]
    );
}
