//! Cross-run flaky detection and quarantine persistence.
//!
//! History and quarantine live at the cache root so they stay consistent
//! across partitioned CI shards and parallel workers.

use std::collections::{HashMap, HashSet};

use anyhow::Result;
use camino::Utf8Path;
use fs_err as fs;
use karva_diagnostic::{
    FlakyTest, QuarantineReason, QuarantinedTest, StoredOutcome, TestOutcomeRecord,
};
use serde::{Deserialize, Serialize};

use crate::artifact::{CacheFile, read_json, write_json};

/// Maximum number of recent outcomes retained per test.
const HISTORY_LIMIT: usize = 20;

/// Minimum number of recorded outcomes before history-flip detection runs.
const MIN_OUTCOMES_FOR_FLIP: usize = 3;

/// Minimum pass/fail transitions required to treat a history as flaky.
const MIN_TRANSITIONS_FOR_FLIP: usize = 2;

/// On-disk cross-run outcome history for every observed test.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TestHistory {
    #[serde(default)]
    tests: HashMap<String, TestHistoryEntry>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct TestHistoryEntry {
    /// Oldest first. Capped at [`HISTORY_LIMIT`].
    #[serde(default)]
    outcomes: Vec<StoredOutcome>,
}

impl TestHistory {
    pub fn record(&mut self, name: &str, outcome: StoredOutcome) {
        let entry = self.tests.entry(name.to_string()).or_default();
        entry.outcomes.push(outcome);
        if entry.outcomes.len() > HISTORY_LIMIT {
            let excess = entry.outcomes.len() - HISTORY_LIMIT;
            entry.outcomes.drain(..excess);
        }
    }

    /// Tests whose recent history oscillates between pass and fail.
    pub fn flaky_by_history(&self) -> Vec<String> {
        self.tests
            .iter()
            .filter(|(_, entry)| entry.is_history_flip())
            .map(|(name, _)| name.clone())
            .collect()
    }
}

impl TestHistoryEntry {
    fn is_history_flip(&self) -> bool {
        if self.outcomes.len() < MIN_OUTCOMES_FOR_FLIP {
            return false;
        }
        let has_pass = self.outcomes.iter().any(|o| o.is_pass());
        let has_fail = self.outcomes.iter().any(|o| o.is_fail());
        if !(has_pass && has_fail) {
            return false;
        }
        let transitions = self
            .outcomes
            .windows(2)
            .filter(|pair| pair[0].is_pass() != pair[1].is_pass())
            .count();
        transitions >= MIN_TRANSITIONS_FOR_FLIP
    }
}

/// The set of quarantined tests stored at the cache root.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QuarantineList {
    #[serde(default)]
    tests: Vec<QuarantinedTest>,
}

impl QuarantineList {
    pub fn tests(&self) -> &[QuarantinedTest] {
        &self.tests
    }

    pub fn names(&self) -> HashSet<String> {
        self.tests.iter().map(|t| t.name.clone()).collect()
    }

    pub fn contains(&self, name: &str) -> bool {
        self.tests.iter().any(|t| t.name == name)
    }

    /// Insert `name` if it is not already quarantined. Returns `true` when new.
    pub fn add(&mut self, name: impl Into<String>, reason: QuarantineReason) -> bool {
        let name = name.into();
        if self.contains(&name) {
            return false;
        }
        self.tests.push(QuarantinedTest::new(name, reason));
        true
    }

    pub fn is_empty(&self) -> bool {
        self.tests.is_empty()
    }

    pub fn len(&self) -> usize {
        self.tests.len()
    }

    /// Stable sort by name for deterministic listing/output.
    pub fn sort(&mut self) {
        self.tests.sort_by(|a, b| a.name.cmp(&b.name));
    }
}

/// Reads the quarantine list from the cache directory root.
///
/// Returns an empty list when the file is missing.
pub fn read_quarantine(cache_dir: &Utf8Path) -> Result<QuarantineList> {
    Ok(read_json::<QuarantineList>(cache_dir, CacheFile::Quarantine)?.unwrap_or_default())
}

/// Writes the quarantine list to the cache directory root.
pub fn write_quarantine(cache_dir: &Utf8Path, list: &QuarantineList) -> Result<()> {
    fs::create_dir_all(cache_dir)?;
    let mut list = list.clone();
    list.sort();
    write_json(cache_dir, CacheFile::Quarantine, &list)
}

/// Reads cross-run test history from the cache directory root.
pub fn read_test_history(cache_dir: &Utf8Path) -> Result<TestHistory> {
    Ok(read_json::<TestHistory>(cache_dir, CacheFile::TestHistory)?.unwrap_or_default())
}

/// Writes cross-run test history to the cache directory root.
pub fn write_test_history(cache_dir: &Utf8Path, history: &TestHistory) -> Result<()> {
    fs::create_dir_all(cache_dir)?;
    write_json(cache_dir, CacheFile::TestHistory, history)
}

/// Apply this run's outcomes to history and auto-quarantine newly detected flakes.
///
/// Detection rules:
/// 1. Tests that passed only after a retry (`flaky_tests`) → `retry-flaky`
/// 2. Tests whose history oscillates between pass and fail → `history-flip`
///
/// Returns the names that were newly added to the quarantine set.
pub fn update_history_and_quarantine(
    history: &mut TestHistory,
    quarantine: &mut QuarantineList,
    outcomes: &[TestOutcomeRecord],
    flaky_tests: &[FlakyTest],
) -> Vec<QuarantinedTest> {
    for record in outcomes {
        history.record(&record.name, record.outcome);
    }

    let mut newly_quarantined = Vec::new();

    for flaky in flaky_tests {
        let name = flaky.full_name();
        if quarantine.add(name.clone(), QuarantineReason::RetryFlaky) {
            newly_quarantined.push(QuarantinedTest::new(name, QuarantineReason::RetryFlaky));
        }
    }

    for name in history.flaky_by_history() {
        if quarantine.add(name.clone(), QuarantineReason::HistoryFlip) {
            newly_quarantined.push(QuarantinedTest::new(name, QuarantineReason::HistoryFlip));
        }
    }

    newly_quarantined.sort_by(|a, b| a.name.cmp(&b.name));
    newly_quarantined
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;

    #[test]
    fn history_flip_requires_multiple_transitions() {
        let mut history = TestHistory::default();
        // Single fail after passes is a regression, not a flake.
        history.record("t", StoredOutcome::Passed);
        history.record("t", StoredOutcome::Passed);
        history.record("t", StoredOutcome::Failed);
        assert!(history.flaky_by_history().is_empty());

        // P-F-P oscillates.
        history.record("t", StoredOutcome::Passed);
        assert_eq!(history.flaky_by_history(), vec!["t".to_string()]);
    }

    #[test]
    fn update_quarantines_retry_flaky() {
        let mut history = TestHistory::default();
        let mut quarantine = QuarantineList::default();
        let flaky = FlakyTest {
            module_name: "mod".into(),
            function_name: "test_x".into(),
            params: Some("(x=1)".into()),
            passed_on: 2,
            total_attempts: 3,
            duration: std::time::Duration::from_millis(10),
        };
        let outcomes = [TestOutcomeRecord::new(
            "mod::test_x(x=1)",
            StoredOutcome::Flaky,
        )];
        let newly =
            update_history_and_quarantine(&mut history, &mut quarantine, &outcomes, &[flaky]);
        assert_eq!(newly.len(), 1);
        assert_eq!(newly[0].name, "mod::test_x(x=1)");
        assert_eq!(newly[0].reason, QuarantineReason::RetryFlaky);
        assert!(quarantine.contains("mod::test_x(x=1)"));
    }

    #[test]
    fn quarantine_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let cache_dir = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let mut list = QuarantineList::default();
        list.add("a::test_1", QuarantineReason::RetryFlaky);
        write_quarantine(&cache_dir, &list).unwrap();
        let read = read_quarantine(&cache_dir).unwrap();
        assert_eq!(read.tests().len(), 1);
        assert_eq!(read.tests()[0].name, "a::test_1");
    }
}
