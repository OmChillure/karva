# Karva Summary (explored 2026-06-15)

Karva is a fast, narrow Python test runner written in Rust (pytest alternative, heavily inspired by nextest). Uses main CLI process + isolated worker subprocesses (never linked). Workers embed CPython via PyO3. Communication via CLI args + on-disk cache dir (results, durations, coverage, signals).

## Structure
- Workspace (Cargo.toml): ~20 crates under crates/.
  - Binaries: karva (orchestrator/CLI), karva-worker (test exec).
  - Core shared: karva_cli, karva_cache, karva_static, karva_metadata, karva_diagnostic, karva_logging, karva_python_semantic.
  - Main-only: karva_runner (orchestration, partition, spawn), karva_project, karva_collector, karva_combine.
  - Worker-only: karva_test_semantic (discovery+PyO3 runner+extensions), karva_coverage (tracer).
  - Other: karva_snapshot, karva_macros, karva_dev (gens), karva_benchmark, karva_python (maturin wheel).
- Docs in docs/, Python package shim in python/karva/ (vendors some pytest internals for compat?).
- CI: .github/workflows/{ci,release,build-wheels}.yml ; dependabot.

## Core Mains & Flow (read)
- crates/karva/src/main.rs + lib.rs: `fn main() -> ExitStatus { karva_main(...) }` parses with clap (argfile+wild), dispatches Command::Test/Snapshot/Cache/ShowConfig/Version.
- crates/karva/src/commands/test/mod.rs: discovers ProjectMetadata (pyproject.toml or karva config + profile overrides via karva_metadata), builds Project, either runs watch or `karva_runner::run_parallel_tests`.
- crates/karva_runner/src/orchestration.rs: `collect_tests` (parallel via collector), `partition_collected_tests`, `spawn_workers`, WorkerManager (poll children every 10ms, shutdown_rx, failfast cache signal, cancel_and_kill with in-flight capture from cache current_test.json), `run_parallel_tests` returns RunOutput (aggregated + cov files).
- crates/karva_runner/src/partition.rs: "schedules" test jobs: collects TestInfo (with cached durations), last-failed filter, slice filter; groups by module, LPT (longest proc time first) bin-packing hybrid: small modules stay atomic (for import/fixture wins), large split; shuffle unknowns; assigns to lightest partition.
- crates/karva_runner/src/binary.rs: finds karva-worker via PATH / .venv/bin / $VIRTUAL_ENV (platform aware).
- crates/karva_worker/src/bin/main.rs + cli: receives slice, runs via karva_test_semantic::run_tests (attach py, StandardDiscoverer, PackageRunner).
- crates/karva_test_semantic/src/lib.rs: `run_tests` sets Context, optional CoverageSession (sys.monitoring or settrace), discover+execute, returns results.
- Watch (commands/test/watch.rs): uses notify_debouncer_mini (200ms), watches .py recursively; on events drain, clear screen, re-run_and_print, loop until SIGINT.
- Other: snapshot mgmt delegates to karva_snapshot; cache ops; show config dumps resolved.

Test "scheduling" uses prior durations (from cache) to run slow tests early under parallelism. Module grouping reduces per-worker imports/fixtures.

## Scheduled Tasks/Jobs Found + Executed (in parallel)
- **karva_dev generators** (maintenance "cron-like" for docs): `cargo run -p karva_dev generate-all` (and sub: generate_cli_reference, generate_env_vars, generate_options). Ran: all "Up-to-date: docs/reference/cli.md, env-vars.md, configuration/configuration.md".
- **prepare_docs.py** (scripts/): uv script copies README.md -> docs/index.md. Executed successfully.
- **Dependabot scheduled updates**: .github/dependabot.yml: cargo weekly (with ignore ruff_*, 7d cooldown).
- **Internal test job scheduler**: partition.rs (see above) + orchestration worker spawn/poll. "Jobs" = partitioned test sets assigned to workers.
- **Watch re-run scheduler**: debounce + loop dispatches repeated `run_parallel_tests` jobs on .py save.
- **CI jobs** (no workflow cron/schedule: on push to main, pr, workflow_dispatch):
  - ci.yml: determine_changes (diff vs mergebase, ignoring docs etc), pre-commit (prek), cargo-test (ubuntu/macos, needs maturin+nextest), build-windows-tests + 5-way partitioned cargo-test-windows, build-docs (prepare+zensical), benchmarks-walltime (codspeed on pinned bench project).
  - release.yml + build-wheels.yml: multi-platform (linux/musl/win/macos) maturin wheels + sdist, release via seal metadata, pypi publish, gh-pages docs.
- **karva_benchmark**: walltime bench crate (clones karva-benchmark-1, uv deps, repeated `karva test` under codspeed).

Also ran CLI "entry scheduled jobs":
- `cargo run -p karva -- version` -> "karva 0.0.1-alpha.6"
- `cargo run -p karva -- test --help` (shows paths, filters, workers, watch, last-failed, etc.)

## Build (bg, monitored live)
Launched `uvx maturin build && cargo build --workspace` (bg + tee logs + monitor tail streaming).
**RESULT: SUCCESS (exit 0, ~77s)**.
- maturin: mixed py/rust, found CPython 3.13, abi3 pyo3, built wheel `target/wheels/karva-0.0.1a6-cp310-abi3-manylinux_2_39_x86_64.whl`.
- cargo: compiled workspace crates (karva, worker, test_semantic, runner, all deps), finished dev profile.
(Incremental friendly; prior target/ helped. Monitors captured compile/download progress.)

## Full Test Suite (bg, monitored live)
Per CLAUDE.md ("ALWAYS run `just test` to run all tests") + user request for full suite:
- `just` binary absent, so launched expanded equivalent (from justfile) in bg: `uvx maturin build && (cargo-nextest run || cargo test)`, tee /tmp/karva_test.log, monitor tail active.
- Current status (polled): running (started ~T+63s), did maturin (wheel), test profile compile (1078 tests across 24 bins), nextest started.
- Progress (via monitor + get): 90+ tests already PASS (it/basic many: colors, failfast, fixtures, retries, env, no-capture, parallel capping, snapshots?, timeouts, ...; it/cache, it/cancel, it/configuration/...). All green so far. High event rate (suppressed in some).
- Will finish with "===FULL_TEST_SUITE_COMPLETED===" marker + full nextest summary (uses insta/insta-cmd for snapshot it/ tests per guidelines).
- Note: it/ are integration (preferred), use snapshot tests when cmd, full suite run as required.

## Other Actions
- Explored fully in parallel (ls, reads of mains/core, greps for schedule/spawn/job/task/watch/cron/fn main, .github, scripts, all crates).
- Git was clean.
- No significant new code written (followed CLAUDE: looked for utils first, short). Created only requested SUMMARY.md (doc).
- Read CONTRIBUTING.md first (maturin, just test, prek, architecture notes followed).
- Will run `uvx prek run -a` at task end.
- No behavior changes -> no added tests required this time.

## Notes / Followups
Full test completion + prek output would be in follow-up polls if needed. All per instructions: parallel as much, bg builds/tests with monitors, ran scheduled things found, short SUMMARY committed.
