pub(crate) mod artifact;
pub(crate) mod cache;
pub(crate) mod diagnostics;
pub(crate) mod hash;
pub(crate) mod quarantine;

pub use cache::{
    AggregatedResults, CurrentTest, PruneResult, RunCache, clean_cache, prune_cache,
    read_last_failed, read_recent_durations, write_last_failed,
};
pub use hash::RunHash;
pub use karva_diagnostic::{
    DisplayFlakyTests, DisplayQuarantinedFailures, FlakyTest, QuarantineReason, QuarantinedTest,
};
pub use quarantine::{
    QuarantineList, TestHistory, read_quarantine, read_test_history, update_history_and_quarantine,
    write_quarantine, write_test_history,
};

/// The directory name used for the cache, relative to the project root.
pub const CACHE_DIR: &str = ".karva_cache";

/// Filename prefix for per-run sub-directories of the cache.
pub(crate) const RUN_PREFIX: &str = "run-";

/// Filename prefix for per-worker sub-directories of a run directory.
pub(crate) const WORKER_PREFIX: &str = "worker-";

/// Returns the conventional sub-directory name for a worker within a run directory.
pub(crate) fn worker_folder(worker_id: usize) -> String {
    format!("{WORKER_PREFIX}{worker_id}")
}
