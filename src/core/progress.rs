use crate::core::loader::LoadResult;
use std::sync::mpsc::Sender;

/// Progress reporting for long-running operations.
///
/// Implementors receive updates as files are processed during
/// save, load, and clone operations. The trait is `Send + Sync` so reporters
/// can be shared across the rayon worker threads that drive parallel loads.
pub trait ProgressReporter: Send + Sync {
    /// Called once when the total number of items is known.
    fn set_total(&self, total: u64);
    /// Called after each item is processed.
    fn tick(&self, current: u64, item: &str);
    /// Called when the operation completes.
    fn finish(&self, message: &str);
    /// Called when the operation transitions to a new high-level phase
    /// (preflight, backup, build, swap, plugin reinstall, …). The default
    /// implementation discards the label, so existing reporters keep working
    /// without modification.
    #[allow(unused_variables)]
    fn phase(&self, label: &str) {}
}

/// A no-op reporter for contexts where progress isn't displayed (tests, TUI fast path).
pub struct NoProgress;

impl ProgressReporter for NoProgress {
    fn set_total(&self, _total: u64) {}
    fn tick(&self, _current: u64, _item: &str) {}
    fn finish(&self, _message: &str) {}
}

/// Stream of events emitted by the async TUI loader. The TUI main loop
/// drains these between draws to keep the spinner / phase indicator current.
#[derive(Debug)]
pub enum LoadEvent {
    /// A new high-level phase started (e.g. "Building target").
    Phase(String),
    /// File-level progress within the current phase.
    Progress {
        current: u64,
        total: u64,
        item: String,
    },
    /// The loader thread has returned. Either the successful `LoadResult`
    /// or a stringified error suitable for display in the status bar.
    Done(Result<LoadResult, String>),
}

/// `ProgressReporter` that forwards every callback as a [`LoadEvent`] over an
/// `mpsc` channel. Used by the TUI to drive an async, animated load view
/// while the actual swap runs on a worker thread.
pub struct ChannelProgress {
    tx: Sender<LoadEvent>,
    /// Total file count for the current phase, captured by `set_total` so
    /// `tick` can include it in `Progress { current, total, .. }` events.
    total: std::sync::Mutex<u64>,
}

impl ChannelProgress {
    #[must_use]
    pub const fn new(tx: Sender<LoadEvent>) -> Self {
        Self {
            tx,
            total: std::sync::Mutex::new(0),
        }
    }
}

impl ProgressReporter for ChannelProgress {
    fn set_total(&self, total: u64) {
        if let Ok(mut t) = self.total.lock() {
            *t = total;
        }
    }

    fn tick(&self, current: u64, item: &str) {
        let total = self.total.lock().map_or(0, |g| *g);
        // Send is best-effort: a closed receiver means the user already cancelled
        // / dropped the in-flight UI, so we just discard the update.
        let _ = self.tx.send(LoadEvent::Progress {
            current,
            total,
            item: item.to_string(),
        });
    }

    fn finish(&self, _message: &str) {
        // The terminal `Done` event is emitted by the spawning thread once
        // `load_with_progress` returns; nothing to do here.
    }

    fn phase(&self, label: &str) {
        let _ = self.tx.send(LoadEvent::Phase(label.to_string()));
    }
}
