# N3 local integration boundary

Builder: OpenAI / gpt-6.1-sol / high, 2026-11-12; no subagents.
Base: `937af556af2acda5b2b1f73eda5002c4ae0e9cf0`, ledger on main
`0937cacfcdb5be036edcd1a126ba0f65521599df` (#36).
The commit containing this evidence is the retained N3 boundary; its exact SHA is
recorded by the child N2 evidence and local `integration/n3-reviewed-port` ref.

Manifest:
- `crates/arxd/src/service/executor.rs`
- `crates/arxd/src/service/executor/tests.rs`
- `docs/PROGRESS.md`
- this evidence
- [original Opus review](../reviews/executor-late-drain-independent-review.md)
- [priority PASS recheck](../reviews/executor-shutdown-priority-recheck.md)

Only the two executor files were extracted using `git diff 0937cac a26570d -- <files>`.
Both match a26570d exactly after LF normalization; no runtime outside these files
changed. In particular main local_execution_tests.rs is unchanged. The synchronous
late-refusal completion remains under the worker mutex; quarantine precedes stopping.
Original aea31e7 plus b3a457d source and tests are preserved. Historical mutation
results in the two reports apply by exact source equivalence; no mutation rerun is
claimed. The uncounted fake preflight advisory remains open.

Actual checks (all exit 0):
- `cargo fmt --all -- --check`
- `cargo clippy --locked --workspace --all-targets -- -D warnings`
- `cargo nextest run --locked -p arxd -E 'test(service::executor::tests::)' --no-fail-fast`
  — 27 passed, 199 filtered; default profile, run
  `844b1fe2-4e86-461e-ab95-faddefed99b0`, test phase 0.259s.
- `git diff --check`

No full suite at this boundary. Historical reviews are scoped to their source
heads; fresh coordinator Opus high integration review and hosted gates remain
required. No publication, activation or native/installed/M2 acceptance.
Native paths are **unverified on real hardware**. No owner/admin/native probe,
VM, Lab Manager or private state operation ran.
