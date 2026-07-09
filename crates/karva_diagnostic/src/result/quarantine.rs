use std::fmt;
use std::time::Duration;

use colored::Colorize;
use karva_logging::time::format_duration_bracketed;
use karva_python_semantic::QualifiedTestName;
use serde::{Deserialize, Serialize};

/// Why a test was added to the quarantine set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum QuarantineReason {
    /// Failed on at least one attempt, then passed on a later retry.
    RetryFlaky,
    /// Outcome history flipped between pass and fail across runs.
    HistoryFlip,
}

impl QuarantineReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RetryFlaky => "retry-flaky",
            Self::HistoryFlip => "history-flip",
        }
    }
}

impl fmt::Display for QuarantineReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A test that is currently quarantined.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuarantinedTest {
    /// Fully qualified name (`module::function` or `module::function[params]`).
    pub name: String,
    pub reason: QuarantineReason,
}

impl QuarantinedTest {
    pub fn new(name: impl Into<String>, reason: QuarantineReason) -> Self {
        Self {
            name: name.into(),
            reason,
        }
    }
}

/// A quarantined failure observed during a run (for the end-of-run summary).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuarantinedFailure {
    pub module_name: String,
    pub function_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<String>,
    pub duration: Duration,
}

impl QuarantinedFailure {
    pub fn from_qualified_name(test_name: &QualifiedTestName, duration: Duration) -> Self {
        Self {
            module_name: test_name
                .function_name()
                .module_path()
                .module_name()
                .to_string(),
            function_name: test_name.function_name().function_name().to_string(),
            params: test_name.params().map(str::to_string),
            duration,
        }
    }

    pub fn full_name(&self) -> String {
        match &self.params {
            Some(params) => format!("{}::{}{params}", self.module_name, self.function_name),
            None => format!("{}::{}", self.module_name, self.function_name),
        }
    }

    pub fn display(&self) -> DisplayQuarantinedFailure<'_> {
        DisplayQuarantinedFailure(self)
    }
}

pub struct DisplayQuarantinedFailure<'a>(&'a QuarantinedFailure);

impl fmt::Display for DisplayQuarantinedFailure<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let record = self.0;
        let label = "QUARANTINED";
        let padding = " ".repeat(12usize.saturating_sub(label.len()));
        let colored_label = label.yellow().bold();
        let duration_str = format_duration_bracketed(record.duration);
        let module = record.module_name.cyan();
        let fn_name = record.function_name.blue().bold();
        let params = record
            .params
            .as_deref()
            .map(|p| p.blue().bold().to_string())
            .unwrap_or_default();

        writeln!(
            f,
            "{padding}{colored_label} {duration_str} {module}::{fn_name}{params}"
        )
    }
}

/// Empty slices render as the empty string (no trailing newline).
pub struct DisplayQuarantinedFailures<'a>(&'a [QuarantinedFailure]);

impl<'a> DisplayQuarantinedFailures<'a> {
    pub fn new(records: &'a [QuarantinedFailure]) -> Self {
        Self(records)
    }
}

impl fmt::Display for DisplayQuarantinedFailures<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for record in self.0 {
            write!(f, "{}", record.display())?;
        }
        Ok(())
    }
}

/// Single recorded outcome used for cross-run flaky detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StoredOutcome {
    Passed,
    Failed,
    /// Passed after at least one failed attempt within the same run.
    Flaky,
}

impl StoredOutcome {
    pub fn is_pass(self) -> bool {
        matches!(self, Self::Passed | Self::Flaky)
    }

    pub fn is_fail(self) -> bool {
        matches!(self, Self::Failed)
    }
}

/// One test's final outcome for a single run, keyed by full qualified name.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestOutcomeRecord {
    /// Fully qualified name including params when present.
    pub name: String,
    pub outcome: StoredOutcome,
}

impl TestOutcomeRecord {
    pub fn new(name: impl Into<String>, outcome: StoredOutcome) -> Self {
        Self {
            name: name.into(),
            outcome,
        }
    }

    pub fn from_qualified_name(test_name: &QualifiedTestName, outcome: StoredOutcome) -> Self {
        Self::new(test_name.to_string(), outcome)
    }
}
