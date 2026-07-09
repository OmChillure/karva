# Quarantine

Some tests fail nondeterministically — a race, a timing-sensitive assertion, a
flaky network call. Retries catch flakes *within* a single run; quarantine
tracks flakes *across* runs and stops them from breaking CI.

When quarantine is enabled, karva records each test's pass/fail history and
automatically quarantines tests that:

1. **Fail then pass on retry** within a run (`--retry`), or
1. **Flip between passing and failing** across runs (at least two pass/fail
   transitions in recent history)

Quarantined tests still run and are shown clearly as `QUARANTINED`, but their
failures do **not** fail the run or count toward `--max-fail`.

## Enabling quarantine

```bash
karva test --quarantine
```

Combine with retries so within-run flakes are detected immediately:

```bash
karva test --retry 2 --quarantine
```

History and the quarantine set live under `.karva_cache/` (`test-history.json`
and `quarantine.json`), so they persist across local and CI runs that share the
same cache directory.

## Listing quarantined tests

```bash
karva quarantine list
```

```text
test::test_flaky (retry-flaky)
test_a::test_param(x=1) (history-flip)
```

Each line is the fully qualified test name (including parametrize ids) and the
detection reason.

## Parametrized and partitioned runs

Quarantine keys are full test identities:

- Parametrized variants are tracked separately (`test::test_param(x=1)` vs
  `test::test_param(x=2)`).
- Partitioned runs (`--partition=slice:M/N`) share the same cache-root
  quarantine set, so a flake found in one shard is suppressed in every shard.

## What a quarantined failure looks like

```text
     QUARANTINED [  0.01s] test::test_flaky
────────────
     Summary [  0.05s] 1 test run: 0 passed, 1 quarantined, 0 skipped
     QUARANTINED [  0.01s] test::test_flaky
```

The failure diagnostic is still printed so the flake remains visible; only the
exit status is softened.

## When not to quarantine

Quarantine hides regressions if overused. Prefer fixing the root cause. Use
quarantine for infrastructure flakes you cannot immediately fix, and revisit
`karva quarantine list` regularly. Clearing the cache (`karva cache clean`)
also clears the quarantine set and history.
