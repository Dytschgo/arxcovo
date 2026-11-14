# A4 source preparation: helper architecture blocker

2026-11-14; OpenAI / gpt-6.1-sol / high; implementation sub-agent.
Base: merged PR99 main `7f046caf25b820c68c548716d62058dd4d944876`.
Outcome: stopped at the task's explicit libtest/thread-count boundary; no source
implementation or acceptance claim. Fresh Muse review is required for any resolution.

Read AGENTS/KICKOFF, engineering safeguards, ADR 0020 D9a (all five bindings),
brief A4/A6, practical proposal and both Muse contract reviews, A5 source evidence
and source review, and the existing E4/A5 implementations. The accepted amendment
is treated as settled; this report does not propose weakening it.

## Concrete blocker

- E4's existing provenance pattern copies `current_exe()` and selects the ignored
  `unix_shim::rights::linux::tests::e4_helper` through libtest (`--exact`,
  `--ignored`, `--nocapture`), in `unix_shim_rights_linux_tests.rs:19–44`.
  That helper also reads executable storage and writes marker files (lines
  68–99); its behavior cannot be reused for A4's passive helper.
- The locally installed Rust **1.95.0** documentation includes libtest source
  at `share/doc/rust/html/src/test/lib.rs.html`. `run_test`, source lines
  627–649, selects `supports_threads` on Linux and calls
  `thread::Builder::spawn` independently of the requested concurrency.
  This is a source observation, not an executed Linux thread-count measurement.
  The documentation version was checked in `html/version_info.html`.
- Therefore ordinary ignored-test re-entry with a blocked handshake inside the
  test body has a live libtest harness thread plus a test worker at the handshake.
  `--test-threads=1` limits concurrent tests, not the helper's total live tasks.
  The existing driver already uses that flag (`driver.py:570`). It does not
  establish a one-task helper compatible with the accepted `pids.max=1` fence.
- Moving such a process only after reaching the handshake cannot demonstrate
  the required one-task bound merely by writing `pids.max=1` first. No migration
  exemption or over-limit task population is used as an acceptance substitute.
  Moving before libtest starts and relying on thread-spawn failure/fallback is
  also outside the reviewed blocked-helper pattern; it is not implemented here.
- The permitted files have no existing pre-libtest executable entry point.
  Bypassing the worker requires an independently reviewed entry strategy rather
  than copying E4's test function. No custom harness, constructor, extra binary,
  Cargo change, production entry branch, or fork workaround was introduced.

The next concrete decision belongs to architecture review: establish a fixed
test-only re-entry that reaches its blocked handshake with one live task while
preserving passivity, the absolute 15 s lifetime, parent-held identity/waitpid,
and all other D9a bindings. This report grants no authority to implement that
new architecture or execute it.

## Evidence and limits

Only repository/documentation reads and a documentation write were performed
before the documentation diff check and commit. No pure/fake regressions,
mutants, fmt, compile or Clippy checks were run: source work stopped before
implementation at the expressly requested blocker boundary. There is no red/green
or mutation-restoration claim. `git diff --check` passed for this document.

No owned source files changed. A6's service/unix_bridge files, driver TEST/A5,
production setup/parser/port/behavior, package/workflow/Cargo, ADR and PROGRESS
remain untouched. No ignored fixture, helper/native/product CLI, service/account,
owner/root probe, network, GitHub, CI, dispatch or subagent action occurred.
No execution authority; M2, A4, remaining F/G and real-Runner PA6 remain open.
Hardware paths are **unverified on real hardware**; no crash-provenance credit.
