use insta_cmd::assert_cmd_snapshot;

use crate::common::TestContext;

#[test]
fn quarantine_list_empty() {
    let context = TestContext::with_files([(
        "test_a.py",
        "
            def test_pass(): pass
            ",
    )]);

    assert_cmd_snapshot!(context.quarantine("list"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    No quarantined tests.

    ----- stderr -----
    ");
}

#[test]
fn quarantine_auto_detects_retry_flaky_and_lists() {
    let context = TestContext::with_file(
        "test.py",
        r"
counter = 0

def test_flaky():
    global counter
    counter += 1
    assert counter >= 2
",
    );

    // First run with retries: fails then passes → auto-quarantined.
    assert_cmd_snapshot!(
        context
            .command_no_parallel()
            .arg("--retry=2")
            .arg("--quarantine"),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
      TRY 1 FAIL [TIME] test::test_flaky
      TRY 2 PASS [TIME] test::test_flaky
    ────────────
         Summary [TIME] 1 test run: 1 passed (1 flaky), 0 skipped
       FLAKY 2/3 [TIME] test::test_flaky
    Newly quarantined:
      test::test_flaky (retry-flaky)

    ----- stderr -----
    "
    );

    assert_cmd_snapshot!(context.quarantine("list"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    test::test_flaky (retry-flaky)

    ----- stderr -----
    ");
}

#[test]
fn quarantine_suppresses_failure_on_subsequent_run() {
    let context = TestContext::with_file(
        "test.py",
        r"
counter = 0

def test_flaky():
    global counter
    counter += 1
    assert counter >= 2
",
    );

    // Quarantine via retry-flaky detection.
    context
        .command_no_parallel()
        .arg("--retry=2")
        .arg("--quarantine")
        .output()
        .unwrap();

    // Rewrite so the test always fails — still should not break CI.
    context.write_file(
        "test.py",
        r"
def test_flaky():
    assert False
",
    );

    assert_cmd_snapshot!(
        context.command_no_parallel().arg("--quarantine"),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
     QUARANTINED [TIME] test::test_flaky

    diagnostics:

    error[test-failure]: Test `test_flaky` failed
     --> test.py:2:5
      |
    2 | def test_flaky():
      |     ^^^^^^^^^^
      |
    info: Test failed here
     --> test.py:3:5
      |
    3 |     assert False
      |     ^^^^^^^^^^^^
      |

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 quarantined, 0 skipped
     QUARANTINED [TIME] test::test_flaky

    ----- stderr -----
    "
    );
}

#[test]
fn quarantine_history_flip_detects_oscillation() {
    let context = TestContext::with_file(
        "test.py",
        r"
def test_flip():
    assert True
",
    );

    // pass
    context
        .command_no_parallel()
        .arg("--quarantine")
        .output()
        .unwrap();

    // fail
    context.write_file(
        "test.py",
        r"
def test_flip():
    assert False
",
    );
    let _ = context.command_no_parallel().arg("--quarantine").output();

    // pass again → history flip (P-F-P)
    context.write_file(
        "test.py",
        r"
def test_flip():
    assert True
",
    );
    assert_cmd_snapshot!(
        context.command_no_parallel().arg("--quarantine"),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_flip
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped
    Newly quarantined:
      test::test_flip (history-flip)

    ----- stderr -----
    "
    );

    assert_cmd_snapshot!(context.quarantine("list"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    test::test_flip (history-flip)

    ----- stderr -----
    ");
}

#[test]
fn quarantine_tracks_parametrized_variants_separately() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

counter = 0

@karva.tags.parametrize('x', [1, 2])
def test_param(x):
    global counter
    if x == 1:
        counter += 1
        assert counter >= 2
",
    );

    assert_cmd_snapshot!(
        context
            .command_no_parallel()
            .arg("--retry=2")
            .arg("--quarantine"),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
      TRY 1 FAIL [TIME] test::test_param(x=1)
      TRY 2 PASS [TIME] test::test_param(x=1)
            PASS [TIME] test::test_param(x=2)
    ────────────
         Summary [TIME] 2 tests run: 2 passed (1 flaky), 0 skipped
       FLAKY 2/3 [TIME] test::test_param(x=1)
    Newly quarantined:
      test::test_param(x=1) (retry-flaky)

    ----- stderr -----
    "
    );

    assert_cmd_snapshot!(context.quarantine("list"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    test::test_param(x=1) (retry-flaky)

    ----- stderr -----
    ");
}

#[test]
fn quarantine_consistent_across_partitions() {
    let context = TestContext::with_files([
        (
            "test_a.py",
            r"
counter = 0

def test_flaky_a():
    global counter
    counter += 1
    assert counter >= 2
",
        ),
        (
            "test_b.py",
            r"
def test_ok():
    pass
",
        ),
    ]);

    // Detect flaky on full suite.
    context
        .command_no_parallel()
        .arg("--retry=2")
        .arg("--quarantine")
        .output()
        .unwrap();

    // Always-fail rewrite of the flaky test.
    context.write_file(
        "test_a.py",
        r"
def test_flaky_a():
    assert False
",
    );

    // Partition that includes the flaky test should still quarantine it.
    // "Starting" reports the pre-partition count; only slice membership runs.
    assert_cmd_snapshot!(
        context
            .command_no_parallel()
            .arg("--quarantine")
            .arg("--partition=slice:1/2"),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
     QUARANTINED [TIME] test_a::test_flaky_a

    diagnostics:

    error[test-failure]: Test `test_flaky_a` failed
     --> test_a.py:2:5
      |
    2 | def test_flaky_a():
      |     ^^^^^^^^^^^^
      |
    info: Test failed here
     --> test_a.py:3:5
      |
    3 |     assert False
      |     ^^^^^^^^^^^^
      |

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 quarantined, 0 skipped
     QUARANTINED [TIME] test_a::test_flaky_a

    ----- stderr -----
    "
    );
}

#[test]
fn without_quarantine_flag_hard_failures_still_fail() {
    let context = TestContext::with_file(
        "test.py",
        r"
def test_fail():
    assert False
",
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test::test_fail

    diagnostics:

    error[test-failure]: Test `test_fail` failed
     --> test.py:2:5
      |
    2 | def test_fail():
      |     ^^^^^^^^^
      |
    info: Test failed here
     --> test.py:3:5
      |
    3 |     assert False
      |     ^^^^^^^^^^^^
      |

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}
