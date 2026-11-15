# M2 A4 native omitted-S7 source - independent Muse review

Scope: base `ac5f069aab24e5a31e1ab33b5791f08f49aba707` (atop merged PR112/main
`f15e9acc46b4534eb9d760b546f358f83a47a12d`, all five gates green), builder
source `d29d085088e7181427d24a7d0bdc16d3d7655cdf` retained as
`3f1fad404d44e9f74462d19f20913dc9936a8a46` (empty `git diff d29d085 3f1fad4`),
builder evidence `7043a6c3a1640590561e85efb3c0d0b26ec8714c` retained as
`78bc3a27d0c918555ed08ee2b84c6d93e4939d90` (empty `git diff 7043a6c 78bc3a2`).
Reviewed head `78bc3a2` on `work/a4-native-negative-source`; working tree clean
at review time. Read-only review plus two narrow pure-test reruns and
`fmt`/`diff --check`; no helper/native/LinuxFs/ignored-fixture launch, no
process probes, no install/network/dispatch/variable/push/PR/status/comment,
no subagents. Read: AGENTS.md, KICKOFF, the accepted kill plan
(`docs/reviews/m2-a4-native-omitted-kill-plan.md`), its Muse plan review
(`m2-a4-native-omitted-kill-plan-muse-review.md`, B1 + D9a clarification),
ADR0020 D9a (with the test-only omitted-kill clarification and raw-fd marker
rule), the exact 7 changed source files, and the evidence note
(`docs/evidence/linux-a4-native-negative-source.md`, blob
`79fa01d635e0ad0a655c06300389137ddbefae8e`). Current blob hashes re-measured
at the reviewed head match the evidence table's "new" column on all 7 files.

## Standards - PASS, no findings

One-concern, small conventional commits; docs-only evidence commit separate
from the test-only source commit. No production-code behavior change:
`a5::finish` delegates to the extracted `finish_observed` with identical accept
semantics (early refusals still `(None, false)`; `Some(outcome)` only past the
previously inlined setup point), and `a4::finish` delegates to
`finish_observed` with the same accepted value. New hierarchy items are
test-only in effect (`OmitS7`, `Tagged`, `Observation`, `acceptance`,
`assertion_rejected`, `rejection`, `retained_structure` live in the test-only
A4 orchestration module beside existing `#[test]` fns; `linux.rs` additions are
`#[cfg(test)]`-gated). Driver TEST stays
`exec::disposable::a4::disposable_linux_a4`; the negative literal is only added
to the existing allowlist (`standard_profile`/`a4_selected`), with no new
CLI/env/path/selector parameter. No secrets, paths, PIDs, or panic payloads in
source, commits, or the fixed marker. `cargo fmt --check` and
`git diff --check` pass (reviewer-observed). Smell baseline: no new
duplication (positive/negative share one `fixture(Case)` body), no feature
envy (counters live on the wrapper that owns the omission decision), no
speculative generality (fixed two-case enum, no pluggable mutation/restart
framework), no message chains beyond one `inner()` dereference. The `forwarded`
counter is write-only in genuine code (see advisory A1); it is not a smell,
it is a tripwire.

## Spec - PASS, no findings

The implementation matches the accepted plan clause by clause:

- Single setup OmitS7 tag wrapper: `OmitS7::write` intercepts only
  `cgroup.kill`, requires `d.path == runs_path && b == b"1"` on the first
  attempt, counts reached/omitted, returns `Ok` without forwarding, and fails
  closed (`Err(Refused)` + `invalid`) on duplicates, other targets, or other
  bytes. Directory provenance travels in wrapper-owned `Tagged::path` vectors
  propagated by descriptor-relative traversal; `Held` handles borrow the
  already-seeded runs/leaf descriptors, `Owned` handles come from the wrapped
  `open_root`/`open_child`. No raw-FD export, no path override, no
  signal/remove capability (`rmdir` only counts; setup-time removal would trip
  `removals == 0`).
- Timeout/population/live-child/rejection attribution:
  `finish_observed` rechecks population, membership, limits, and `>6s`
  remainder both before release and immediately before the single production
  setup call, keeps the real monotonic clock and 5s setup bound, and computes
  `accepted && child.killed()` with short-circuit order so the negative never
  reaps on refusal. `rejection` requires the shared-assertion panic plus
  preconditions, exact `KillTimeout`, 1/1/0 attempt/omitted/forwarded
  counters, no invalid flag, zero removals, retained literal `arx-0-0`
  structure, successful `open_child(runs, arx-0-0)`, real populated events on
  both dirs, live child, and positive original-deadline remainder.
- Raw marker transport (plan-review B1): `rejection_marker` uses raw
  `libc::write` to `STDOUT_FILENO` with partial-write completion and at most
  64 EINTR-only retries, fixed bytes `ARX_A4_OMITTED_S7_REJECTED\n`, no
  payload. Driver requires exactly one occurrence in negative mode and strips
  that fixed sequence before its unchanged exact test-ok/one-pass/zero-failure
  oracle, with mock coverage for the interleaved-line case plus
  missing/duplicate marker and false summary/service success.
- Failure-only identity cleanup before READY: attribution (`rejection`
  assert, unreaped assert, `retained_a4` held/fresh dev+ino equality) and the
  marker all precede `cleanup_failed_acceptance` (owned SIGKILL + checked reap
  through the existing `Child` path, `Drop` retained as fallback) and checked
  `cleanup_a4` (rechecks identity, bounded emptiness, removes only `arx-0-0`,
  confirms literal absence and empty runs). `READY` and identity-ack come only
  afterward, so existing driver MainPID checks still apply and in-fixture
  cleanup stays insufficient by itself (driver finally cleanup unchanged).
- Preservation: positive entry keeps `a4::acceptance(a4::finish(...))`
  untouched; A5 post-setup checks are extracted verbatim into `post_setup`
  with `finish` behavior unchanged; helper source/image pins, package, Cargo,
  workflows, ADRs, bounds (64 MiB/swap0/pids1, absolute 15s helper deadline
  with `>6s` gates, 4096-byte reads, 512-record lists, exact
  `--ignored/--exact`/one-thread argv, SHA/actor/ref fences) all unchanged.
- Coverage layering is proportionate: pure `Fake`/`Manual`/`MockChild`
  attribution tests (16-case mutation matrix + wrong-target/byte/duplicate
  test) plus driver mocks for both literals, unknown-name refusal before
  mutation, and cleanup failure; no pure test enters LinuxFs/helper/dispatch.

## Security - PASS, no findings

No new reachable surface. The wrapper forwards every port operation except
the single omitted kill write, rejects duplicates and wrong-target/wrong-byte
kills, propagates wrapper-owned tags over trusted descriptors, and cannot
signal or remove early. Failure-only cleanup compares held/fresh dev+ino over
trusted descriptors with bounded empty observations before `rmdir`; no
adoption. The caught assertion payload is dropped unread
(`assert!(accepted)` on a copied bool, no `#[should_panic]`, catch scope
limited to that call). The marker carries no panic text, PIDs, or paths.
Driver fences (exact TEST literals, source/workflow SHA/actor/ref equality,
unit/profile, 1 MiB log bound, owned-only cleanup, finally-delete with 404
verification) are unchanged, and in-fixture cleanup stays insufficient by
itself.

## Advisories (non-blocking)

- A1 (cosmetic tripwire): `OmitS7::forward_write` increments `forwarded` for
  `cgroup.kill`, but `OmitS7::write` never delegates kill writes to
  `forward_write`, so `forwarded` can only become nonzero via the
  `forward-omitted-S7` counterfactual mutant (which is exactly its purpose:
  `rejection` requires `forwarded == 0`). No change requested; record only.
- A2 (pin maintenance, E3 `linux.rs`): old blob
  `4ad78a672679226fc11c7899a6ef6b033f83a7d` to new blob
  `7c4196333f4938499188c7f99e5476c955b8e9d0`; the delta is the two
  `#[cfg(test)]` fns only (portable Windows build unaffected; E3 inventory
  test passes, reviewer-observed below). Future exact-refresh tooling must
  re-hash this file rather than assume the old pin; nothing was silently
  widened (E3 `e3_source_allowed` scan covers the new fns and still passes).
- A3: Linux cfg-only compilation remains unverified locally (missing
  `x86_64-linux-gnu-gcc`; builder evidence records the same block in
  `libsqlite3-sys`). Hosted Linux compile of the `cfg(test)` additions is
  still outstanding and is a publication precondition, not a review finding.

## Verification

Builder-reported (not re-claimed here): 20 Rust pure tests, 91 mock tests, 40
Python assertion mutants, 8 compiled Rust counterfactuals, fmt, source fences,
locked offline workspace Clippy green on Windows; Linux target check blocked
by the missing cross compiler; no native/helper/ignored execution.

Reviewer-observed in this session (narrow, pure-only, after callgraph audit
confirming no pure test enters LinuxFs/helper/spawn/dispatch):

- `cargo test -p arxd --lib --locked --offline
  exec::hierarchy::fake::a4::omitted_s7 -- --test-threads=1` — 2 passed, 0
  failed (`omitted_s7_observes_real_setup_timeout_without_reap_or_removal`,
  `omitted_s7_rejects_wrong_targets_bytes_and_duplicate_attempts`).
- `cargo test -p arxd --lib --locked --offline
  exec::stage_b_tests::l6_e3_tokenized_source_inventory_and_mutants -- --exact`
  — 1 passed (E3 inventory admits the new `cfg(test)` `linux.rs` fns; no
  banned token introduced).
- `cargo fmt --all -- --check` — pass. `git diff --check` — pass.

Deliberately not run (out of scope for a source review; honestly unrun):
ignored native fixtures, helper build/launch, Linux-only compile/tests,
Python mock suites, full workspace test suite, Clippy re-run, any hosted
dispatch. Native negative remains **UNRUN and NOT granted**; A4 acceptance,
M2, remaining F/G, and real-Runner PA6 stay open; installed/dedicated and all
hardware paths are **unverified on real hardware**.

## Verdict

No Standards, Spec, or Security blockers. Eligible for narrow publication of
exactly the reviewed 7-file source delta plus the retained evidence note
through the normal gates (independent-agent review satisfied by this note as
the sole reviewer slot; green required checks including hosted Linux compile,
exact-head review, coordinator merge). Publication grants no native execution:
a separate reviewed one-run disposition binding this fixed negative to a fresh
hosted host, bounded sanitized result, finally-clear/404, and no reuse/retry
is still required before any native run. No A4/M2/F/G/PA6 acceptance is
claimed or granted.
