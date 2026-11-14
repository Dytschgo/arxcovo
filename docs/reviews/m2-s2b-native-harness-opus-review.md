# M2 S2b native verification harness: independent Opus source/security review

Reviewer: Anthropic / claude-opus-5-5 / high, review sub-agent, 2026-11-14.
This is a single-agent review with no subagents. It reviews the
OpenAI / gpt-6.1-sol / medium harness, so it is a different-provider review.
Standards and Spec/security are reported separately.

**Verdict: no blockers on either axis.** The coordinator may publish this
harness to obtain the **first hosted evidence only**. This review grants no
merge, native credit, activation, experiment, A13 acceptance or M2 acceptance.
The native baseline and both native kills remain **UNRUN**.

## Isolation and pins

- Cwd and Git top level were the same isolated T3 worktree
  (`<isolated-worktree>`), not the canonical checkout. Branch
  `t3/review-s2b-native-harness`. Status was clean and HEAD was
  `b9c9425bf921e56b0e86a29a6c885428c3b6220d` before any work.
- Harness: builder `1e23ac02f75eb624225dc055a611145c69f4b393` is retained as
  `fcc4db43723a32a87b12d6258699cb13f050b00a`. `git diff 1e23ac02 fcc4db4` is
  empty. Both have the same stable patch-id
  `1906713f2e9e3d7df08a4ea72639030a18073abc`.
- Evidence: builder `c9911f3ce777f56c61b689f70b1d2f53a57c5f80` is retained as
  `b9c9425bf921e56b0e86a29a6c885428c3b6220d`. The diff is empty and the
  patch-id `629b8bf03152b53576ae7c20bd85639a4987478c` is the same.
- Prior source review: `d94b176` is retained as `e71fc59`. Both have the
  same patch-id `009006d92dd50237bd7fa07213890b108579f089`.
- Reviewed scope `284b930..b9c9425` covers five new paths only:
  `.github/workflows/s2b-native-evidence.yml`, `tools/ci/s2b-native/`
  `{driver.py,test_driver.py,mutations.jsonl}` and
  `docs/evidence/linux-unwind-local-s2b-native.md`.
- `git diff --stat efe8e55 HEAD` is empty for `crates`, `Cargo.toml`,
  `Cargo.lock`, `ci.yml`, the E3/E4 workflows, the policy workflows and
  `tools/ci/{e3,e4}-native`. The product S2b source therefore remains
  `efe8e55`. `git diff --check efe8e55 HEAD` is clean.
- Each working-tree SHA-256 equals its HEAD blob (`.gitattributes` sets
  `eol=lf`), the evidence manifest and the driver `PINS`:
  - workflow `40fb88d0ec96c28f4d4aa10d5f0cedc3fce778d913c0f24394ec255f28db346f`
  - `driver.py` `c64b52867e11e093a25206de8aec2855c188a4f73a42e57461b01296a33ab47c`
  - `test_driver.py` `71ca9f9f11af31e703ef8ba2e9dc82d3da4cf1783de379cab178d3f387a74271`
  - `mutations.jsonl` `28f05948ca4aace6944fbe8c77676e8f702575042cf77d9c9c176096ac5b0f94`
  - the six crate files: `64d4cb8c…1211d121`, `f8d425f7…dd3f9b4`,
    `45da7396…cf5ed`, `b2e0c0ef…780d`, `f26d1ed0…bb1`, `6eb93098…e089`.
    These are the same values as in review `e71fc59`.
- Spec: accepted proposal `docs/reviews/m2-remaining-gates-proposal.md`
  (U-a and the mutant list) and source review `e71fc59` (SP-1 root skip and
  SP-2 `group_forgotten` site, both conditions on native credit).
- Lessons applied from the E3/E4 harness reviews: the real nextest 0.9.151
  label, the padded counter, the exact footer, compile failures earn no kill
  credit, visible skips are rejected, and kills need an ordinary UID.
- Standards: AGENTS, KICKOFF (review, CI and honesty rules) and the review
  skill's smell baseline.

## Checks run by this reviewer (local Windows)

| Check | Result |
| --- | --- |
| `python -I -B -m unittest discover -s tools/ci/s2b-native -p test_driver.py -v` | 11 run, OK (reproduces the evidence claim) |
| Same for `tools/ci/e3-native` and `tools/ci/e4-native` | OK with 1 skip / OK (no regression; both unchanged) |
| **Real-format probe.** I made a throwaway, dependency-free crate named `arxd` in the session scratchpad, outside the project. It has the exact module path `service::unix_bridge::tests::s2b::s2b_real_group_unwinds_before_bridge_returns` plus 10 other lib tests. I ran it with cargo-nextest 0.9.151 and rustc 1.95.0 (already installed), using the driver's own `run_bounded` and exact `command()` argv plus `--offline`. The fixture panics internally with `ARX-S2B-FIXED-PANIC`, as U-a does. | PASS mode: exit 0, `survived`. FAIL mode (named assertion): exit 100, `statuses == ["FAIL"]`, error lines exactly `["error: test run failed"]`, `killed`. Root-skip mode: the `SKIP …` stderr line is visible under `--success-output immediate`, so the result is `invalid`. All 11 tests: padded `( 1/11)` counters, and extra status lines give `invalid`. Summary `10 skipped` is lowercase and does not trip the guard. Capture sha256: pass `ea72a6b5…d937c`, fail `d62971bc…2705f`, skip `5e7c746d…70c2`. |
| **Negative controls**: 16 one-at-a-time guard removals on scratch copies of `driver.py` (from `git archive HEAD`), each followed by the HEAD test suite | Caught (suite fails): SKIP guard, other-marks guard, exact footer, exit 100, eUID check, final baseline, inner restore, hardlink guard, hosted guard, exact-count. **Not caught (suite OK):** outer restore loop, `killpg`, workflow pin, clean-status check, source pin, baseline fingerprint compare. See ST-1. |

Not run: project Cargo of any kind, the native driver, the native fixture,
hosted CI, installs, GitHub, push or PR. I made no source, PROGRESS or
proposal edits. Probe artefacts stay in the session scratchpad and are not
committed.

## Standards

**0 blockers; 3 advisories.**

- **ST-1 (advisory; AGENTS "every behavior needs a meaningful test").** Six
  fail-closed guards have no test that pins them. The suite still passes
  with each one removed: source `PINS`, `WORKFLOW_SHA`, the
  `git status --porcelain` clean check, the in-baseline `fingerprint(root)`
  comparison, the outer `finally` restore loop (the inner restore covers it
  today), and `os.killpg` descendant cleanup (POSIX-only; untestable on
  Windows). Reading the code shows each guard is present and only fails
  closed, so this does not block the first hosted run. Add synthetic tests,
  or explicitly accept the gap, before merge.
- **ST-2 (advisory, wrong path).** The workflow path filter lists
  `docs/evidence/linux-rights-probe-s2b-native.md`, which does not exist. It
  was copied from E4 naming. The evidence file is
  `docs/evidence/linux-unwind-local-s2b-native.md`. The impact is only that
  an evidence-only edit will not trigger the run.
- **ST-3 (advisory, smell: Speculative Generality / leftover).** The
  `status()` helper in `test_driver.py:18-20` is unused. It carries E4's
  `unix_shim` branch. Delete it.

These are accepted, not findings: the deliberate copy of E4's
`regular`/`bounded_read`/`run_bounded` (disclosed; it keeps the executable E4
harness unimported and unchanged), and the job name `native-evidence`, which
is the E3/E4 convention and is not a required context. The commits are
conventional and single-concern.

## Spec / security

**0 blockers; 5 advisories.**

Verified:

- **Workflow.** It runs on plain `pull_request` only, with no
  `pull_request_target`, `permissions: contents: read`,
  `persist-credentials: false`, and no `secrets.*` or token use. Its three
  action SHAs are identical to E4's, which already ran hosted. It uses
  ubuntu-latest, Rust 1.95.0, nextest 0.9.151, a 10-minute timeout,
  incremental builds and no dev debug info. It is not referenced as a
  required check. A shell step prints the numeric UID and fails on 0.
- **Identity.** `main()` emits numeric uid/euid, refuses either being zero
  before any read or command, then requires `linux`, `GITHUB_ACTIONS=true`,
  `RUNNER_OS=Linux` and `cwd == repo root`. Tests cover root, eUID-only root,
  non-hosted and Windows (`execute` is never reached).
- **Checkout.** Required: exactly eight tracked `100644` paths, top level
  equal to root, empty porcelain status, a 40-hex HEAD (emitted), and pinned
  workflow, manifest and six sources.
- **Bounds.** No traversal, symlink, reparse point, hardlink or nonregular
  target on any path component. Reads are capped at 256/32 KiB and output at
  2 MiB, 8 KiB and 128 B. Every process runs with no shell, stdin set to
  `/dev/null` and its own session/process group, and is SIGKILLed via
  `killpg` afterwards. `close_fds` is the default. The global budget is
  480 s: 210 + 2×85 (95 reserved) + 90 ≤ 480, plus the 10 s Git steps. Only
  fixed-label JSON is emitted, never raw output. E4's hosted cold library
  baseline took 40.6 s, so the 210 s cap is plausible. S2b hosted durations
  are unmeasured.
- **Mutations.** The schema is exact, there are exactly two names and each
  has a fixed target. `apply` requires a non-empty `old != new` that occurs
  exactly once. All rows are pre-applied in memory before any write. A write
  happens only after a byte check against the original. Restoration runs in
  the context `finally`, is checked against the bytes, and runs again in the
  outer `finally` with the six-source fingerprint. The final baseline runs
  on every path; the timeout, survival and compile cases are tested
  synthetically.
- **Classification.** It matches the real 0.9.151 format (probe above).
  Credit needs the `arxd` label, an optional padded counter and the exact
  test name. A kill needs exit 100, exactly one named FAIL, exactly the
  footer `error: test run failed`, no other status line and no `SKIP`
  anywhere. Compiler failure (101), no tests (4), retries,
  SIGABRT/LEAK/TIMEOUT statuses, an extra test and a renamed test are all
  `invalid`. Source-review SP-1 is closed: a root skip prints a visible
  `SKIP` under explicit success output, which is rejected, and the driver
  refuses root before Cargo anyway.
- **`drop_skips_store` (source analysis).** It removes only the
  `Group::drop` Release store. Cleanup (`kill_and_confirm`) is kept. The
  `old` string matches exactly once (in-memory test). The only effect is a
  rustc `dead_code` warning: `self.residue` has no other reader, and nothing
  denies warnings. Review `e71fc59` rv-L2 recorded a Linux `cargo check
  --tests` pass. The bridge returns `Failed{Unknown}` whatever the residue,
  and the latch depends on `cleanup_residue()`
  (`unix_bridge.rs:154-167`). So the first failing assertion is
  `S2b real Drop published before return`.
- **`group_forgotten` (source analysis).** It is SP-2's recommended site: a
  `std::mem::forget(_group)` of the local `TestGroup` in the owned fake
  `Holding::run` frame, before the fixed panic. `_group` is a real binding,
  so the code compiles (`!` panic follows). The forgotten Group never drops,
  so the residue stays false and the same assertion fails. The leaked
  descriptors are ordinary tempdir directory/file fds in the nextest child,
  released when that process exits. `TempDir` still removes the files on
  unwind. It is **decomposition-only, not real-Runner proof**. It changes no
  root, trust-root, NNP, `run_core`, `Runner` or production path.

Advisories:

- **SP-1 (advisory, kill attribution).** `classify` does not pin the
  intended assertion. Any panic in the named test earns `killed`. Because no
  raw output is emitted, the hosted evidence cannot show which assertion
  fired. Two things support attribution by source analysis: the before and
  after baselines bracket environmental failure, and both mutants reach the
  residue assertion first. Before native credit is relied on, consider a
  fixed boolean such as "exactly one `S2b real Drop published before return`
  in captured output", emitted without raw text.
- **SP-2 (advisory, evidence wording).** The evidence calls the classifier
  tests "real nextest 0.9.151 output-format samples". They are hand-written,
  format-shaped lines. The format is consistent with the E3/E4 real-output
  rechecks and with this review's scratch probe. Reword it.
- **SP-3 (advisory, pin scope).** U-a's outcome depends on the S2a catch arm
  in `service/unix_bridge.rs` (`323fa965…026d1`, pinned in review
  `e71fc59`). That file is neither in `PINS` nor in the path filter. The
  emitted HEAD still binds the hosted evidence to an exact commit, so this
  only matters if credit is carried across heads.
- **SP-4 (advisory, residual process risk; same as E4).** First, `killpg`
  runs after the leader is reaped. If no group members remain, a reused
  PGID could theoretically be signalled. Second, a descendant that escapes
  through `setsid` and keeps the pipe open would stop
  `proc.stdout.close()` from returning while the reader thread holds the
  buffered lock. The driver would then hang until the 10-minute job timeout
  rather than the 480 s budget. Cargo, rustc and the fixture do not call
  `setsid`.
- **SP-5 (advisory).** GitHub cancellation sends SIGINT first.
  `KeyboardInterrupt` is outside the fixed-category handler, so it prints a
  driver-frame traceback (no process output and no secrets). The `finally`
  restoration still runs. A `GuardError` raised in `finally` (for example,
  the final-baseline deadline) replaces the original failure category.

## Verdict and gates

**No blockers on either axis.** The coordinator may publish this harness for
its **first hosted evidence only**, not to merge. Outstanding gates:

1. A hosted run on the exact head showing ordinary uid/euid, the clean
   checkout and HEAD, `baseline_before` passed, `drop_skips_store` and
   `group_forgotten` both `killed` with exit 100, `restored_sources` matching
   the pinned fingerprint, and `baseline_after` passed. Until that run
   exists, the native baseline and both kills are **UNRUN**.
2. Disposition of ST-1 to ST-3 and SP-1 to SP-3 before merge (fix or
   explicitly accept), then PR publication, green required checks (CI,
   `ci-time-budget`, `agent-review`) on the exact head, and a coordinator
   merge decision. None of these is granted here.

M2 is incomplete. Real-Runner PA6 remains **OPEN**, and A13 is not accepted.
The native, root and cgroupfs paths are **unverified on real hardware**, and
production remains inactive. No project Cargo, native harness or fixture
execution, install, hosted CI, GitHub, push, PR, owner/admin or private-state
action occurred. Old Lab Manager and `agent-windows-test` were not touched.

Standards: 0 blockers / 3 advisories (worst: ST-1 six guards untested).
Spec/security: 0 blockers / 5 advisories (worst: SP-1 kill credit not tied to
the intended assertion).
