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
}

/// A no-op reporter for contexts where progress isn't displayed (tests, TUI).
pub struct NoProgress;

impl ProgressReporter for NoProgress {
    fn set_total(&self, _total: u64) {}
    fn tick(&self, _current: u64, _item: &str) {}
    fn finish(&self, _message: &str) {}
}
