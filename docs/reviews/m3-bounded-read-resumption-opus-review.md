# M3 PR149 A0 bounded-read resumption: Opus delta review

2026-11-16. Anthropic / Claude Opus 5.5 / high. I worked as the independent review sub-agent with no children. Builder: OpenAI GPT-6.1 Sol / high, so the reviewer is a different provider.

- **Target:** head `8b5b0a0281b252a1bc1e340af28d7257dd7fff18`, base `9cf98a4b829491b9e91fd3aa0858cb7d8fef3708`.
- **Main:** live main is `a18e95c20b7340127d746fb3632f3b0eab32e586`. It is an ancestor of the base, and the base adds only docs on top of it.
- **Spec:** `docs/briefs/m3-bounded-read-resumption.md`, including the coordinator's Sol contract clarifications.
- **Evidence:** the "A0 resumption on sampler main" section of `docs/evidence/m3-bounded-fresh-read.md`.
- **Standards:** `AGENTS.md`, `docs/KICKOFF.md` and the review skill's smell baseline.

This review covers only the delta: merge `9137ef3`, C2 `d173641`, C3 `7420f1d` and evidence `8b5b0a0`. For the unchanged A0 hunks I rely on the retained reviews: `c0bf702`, `08f8832`, `df0d42e` and `62bfd25`. I made no change to source, PROGRESS or ADRs. I did no push, PR, status, merge, watch, rerun, dispatch, install, cache cleanup, native M2, VM or admin action.

**Verdict: PASS for the delta. No blocker.** Publication still needs the exact-head review and gates listed at the end.

## Verified by this reviewer

History and identity:

- **Ancestors of `8b5b0a0`:** `8a8c1fd` (reviewed local A0), `8eaed30` (old PR149 head, so a fast-forward needs no force), `01a42d5`, `9cf98a4` and `a18e95c`.
- **Merge parents:** `9137ef3` has first parent `9cf98a4` and second parent `8a8c1fd`.
- **Base vs. main:** `a18e95c..9cf98a4` changes only `docs/PROGRESS.md`, the resumption brief and the sampler evidence doc.

Diff-of-diffs:

- I compared the ordered `+`/`-` lines of `git diff --unified=0 01a42d5 8a8c1fd` with those of `git diff --unified=0 9cf98a4 9137ef3`, for each file.
- `consensus.rs` gives 144 lines and `three_node.rs` gives 309 on both sides. **The streams are byte-identical with no line excluded.**
- Main's `pub(super) struct Cluster` and the D1 diagnostic context sit outside both streams, so they are preserved. The merge diff shows the reviewed `APPLY_PAUSE_OWNER` static inserted above `pub(super) struct Cluster`, which is resolution C1 exactly.

Source scope:

- `git diff 9cf98a4 8b5b0a0 -- . ':!docs'` touches only `consensus.rs`, `three_node.rs` and `tools/ci/test_windows_diag_sample.py`.
- `7420f1d..8b5b0a0` touches only the evidence doc, so the source has not changed since `7420f1d`.

Blob pins: every blob the evidence lists matches at `8b5b0a0` and at `7420f1d`. These files are identical at head, base and main `a18e95c`:

| File | Blob |
| --- | --- |
| `adapter.rs` | `181e9dc` |
| `diagnostics.rs` | `6139626` |
| `windows_diag_sample.py` (sampler production) | `6ba6f25` |
| `test_windows_cluster_quarantine.py` (guard) | `bb376fc` |
| `ci.yml` (workflow) | `0c7af38` |

`PROGRESS.md` at head equals the prepared root `9cf98a4` byte for byte (`69594c8`), as C6 requires.

Quarantine:

- `three_node.rs` has exactly 8 `ignore` occurrences, all in the ADR 0024 `cfg_attr(windows, ignore = …)` form, on T1–T4, U6 and the three R6 tests.
- There is no `cfg(target_os` in the file.
- R1–R5 and the C2 unit carry no ignore, so they run as ordinary required tests on Windows. The quarantine is not extended.

A0 invariants (I re-read these; the behavior is unchanged from the reviewed source):

- **Read admission:** `READ_BUDGET` is 1500 ms (absolute) and `READ_PERMITS` is 8. Admission uses `try_acquire_owned`; a failure returns `Busy`.
- **Owner tracking:** the owner is tracked in `Owners`. The permit is released when the owner completes, or after the store guard and Arc drops inside `blocking` (#147).
- **Error ownership:**
  - `NotLeader` and `NoQuorum` come only from an engine reply received before the deadline.
  - Every late, closed or unknown outcome becomes `Unavailable`.
  - There is no retry, no cancellation of the engine wait, and no invented quorum claim.
- **Shutdown counters:** shutdown caps them at `64 + READ_PERMITS` = 72. It captures `read_held` and `propose_held` before `abort_all` in the deadline branch, and also in the normal branch.
- **`APPLY_PAUSE_OWNER`:** a `tokio::sync::Mutex` held across the whole async fixture by R1 (`three_node.rs:893`) and R6c (`:1219`) only.
- **T2 pattern:** T2 alone adds `Unavailable` (`three_node.rs:536`), matching the `8237db1` approval. No other pre-existing assertion widened.

C2 (`d173641`):

- `read_fail(&ReadError) -> Fail` is an exhaustive four-arm match with no wildcard. It keeps the three earlier mappings and adds `Busy => Fail::Busy`. `Fail::Busy` already existed and derives `PartialEq` and `Debug`.
- `attributed_read` now calls the helper, which also removes the inline duplicate.
- The unit `read_failure_attribution_covers_all_errors` asserts all four variants.

C3 (`7420f1d`) is test-only:

- It adds five non-ignored R1–R5 cases with `{"status":"mismatch","reason":"ignored"}`, and `check_inventory` accepts them. Flipping each case on its own to `matches` raises `Guard("inventory")`.
- `check_inventory` (unchanged, `windows_diag_sample.py:404`) collects only `matches` entries and requires them to be exactly the eight ignored IDs. Inventory failures therefore still gate the job.

Checks I ran once (no repeats). Run root `D:/Temp/arxcovo-a0-opus-review-8b5b0a0-1794820821`, with `CARGO_TARGET_DIR`, `TEMP` and `TMP` inside it, `CARGO_NET_OFFLINE=true`, `--locked --offline`, fresh build. Toolchain: Rust 1.95.0, nextest 0.9.151, Python 3.12.10.

| Check | Result |
| --- | --- |
| `cargo nextest run -p arxd -E 'test(/cluster::/)'` | **PASS 119/119**, 468 skipped, summary 8.157 s, 82 s including the build |
| `python -I -B -m unittest … test_windows_cluster_quarantine.py` | **PASS 1** (exactly eight) |
| `python -I -B -m unittest … test_windows_diag_sample.py` | **PASS 58**, including the new C3 fixture |
| `cargo nextest list -p arxd --lib --message-format json --run-ignored only -E 'package(arxd) & kind(lib) & test(/^cluster::consensus::three_node::/)'`, then the unchanged `check_inventory` on the output bytes with this run root | **PASS**. Exactly the 8 guard IDs are `matches` with `ignored: true`. R1–R5 and the C2 unit are `mismatch/ignored` with `ignored: false` |

Details from the nextest run:

- R1 passed in 2.061 s, R2 in 2.027 s, R3 in 1.951 s, R4 in 1.915 s, R5 in 7.177 s and the C2 unit in 0.022 s.
- My summary time equals the builder's 8.157 s. I checked whether that meant a reused log. It does not: my log was built fresh in my own target directory, its bytes differ from the builder's log, and the per-test times differ (for example R1 took 2.061 s here and 1.949 s for the builder). R5 is the long pole, so the summary tracks its end time.

Spot check of the builder's retained logs (`D:/Temp/arxcovo-a0-resume-7420f1d-d466393d/logs/`): their summaries match the evidence table:

- nextest 119 passed;
- in-process 119 passed, 13 ignored;
- the eight passed in 7.840 s;
- the three controls passed in 5.496 s.

The builder's nextest log contains the string "diagnostic observer panicked" exactly once, from the intentional `completed_observer_panic_is_joined_and_reported` unit.

Prerequisite: `9cf98a4` records the first green sampler main.

- **Run:** CI43836099670 on `a18e95c`, success.
- **Windows job:** 133192981010, 433 s.
- **Sampler:** wrapper 29031 ms, all eight pass, orphans 0.

The records are in `docs/PROGRESS.md:26` and `docs/evidence/m3-windows-hosted-diagnostic-sample.md:115`. That result was verified by the root coordinator and I reuse it here. I did not query hosted CI.

## Reused, not re-run by this reviewer

These are the builder's single local round, as reported, consistent with the retained logs I spot-checked:

- in-process `cargo test -p arxd --lib cluster::`;
- the three D1 controls (`--run-ignored only`);
- the quarantined eight once, as a local sanity check;
- `node --test scripts/ci-workflow.test.mjs`, 4/4;
- workspace Clippy `-D warnings`;
- `cargo fmt --all --check`;
- `git diff --check`.

The C7 claim that R5, the controls and the eight produce no observer panic also comes from the builder's success-output capture. My nextest run did not capture success output; it shows only R5 PASS.

Production correctness of the unchanged hunks rests on the retained reviews `c0bf702`, `08f8832`, `df0d42e` and `62bfd25`. Lock correctness for the unchanged `APPLY_PAUSE_OWNER` rests on the `62bfd25` R1/R6 pair evidence: the pair passed 8/8 where it had failed 8/8, and removing the lock compiled and then failed 4/4.

## Standards

No findings that block.

- **Commits:** the commits are small, conventional and one concern each: merge, `test(consensus)`, `test(ci)`, `docs(evidence)`. Each behavior added (Busy attribution, inventory tolerance) has a meaningful test.
- **Secrets and private data:** none in the diff. The evidence names only local temp roots and public SHAs.
- **Smell baseline:** C2 removes a Duplicated Code / Repeated Switches risk by putting the `ReadError → Fail` map in one helper. I found no Speculative Generality, no new abstraction and no new parameter.
- **Nit S1 (non-blocking, judgement):** `read_fail` and its `#[test]` sit between the `APPLY_PAUSE_OWNER` static and `pub(super) struct Cluster`, so the static's comment no longer directly precedes the struct. This is cosmetic only and needs no change.

## Spec

No findings that block.

- **C1:** satisfied. `pub(super)` is kept and the static plus its comment are inserted above it. Nothing else changes.
- **C2:** satisfied. The brief's "exhaustive `read_fail(&ReadError) -> Fail` helper with a unit test over all four variants" is exactly what landed.
- **C3:** satisfied. The brief asks for "five non-ignored module cases with mismatch reason `ignored` → inventory OK; one non-ignored `matches` → Guard". The fixture does this and goes further, flipping each of the five. The reason string is confirmed on local nextest 0.9.151.
- **C4 and C5:** recorded honestly in the evidence. R1/R3/R4/R5 and the controls get no natural-latency credit, and the merge SHA is treated as a sampler epoch boundary.
- **C6:** satisfied. PROGRESS equals the prepared root.
- **C7:** reused from the builder's evidence, as described above.
- **Step 1:** satisfied. "Prove that `git diff M HEAD -- crates/` equals `git diff 01a42d5 8a8c1fd -- crates/` apart from the C1 context line" holds. The A0 ±line streams are byte-identical, and main's D1 context is preserved.
- **Not authorized:** honored. The diff adds no new or moved quarantine, no timeout, deadline, retry, scheduling or test-group change, no helper or `Params` change, no adapter change and no fourth cluster fix.
- **Note P1 (evidence wording, non-blocking):** the evidence says "the old private Cluster remove/add pair is excluded because main's public visibility is retained". In fact neither ±stream contains a `struct Cluster` line, and the streams match with no exclusion at all. The real proof is therefore stronger than the stated one. No change is required.
- **Note P2 (non-blocking):** the real module listing has six non-ignored tests: R1–R5 plus the C2 unit. The C3 fixture names five. `check_inventory` treats every `mismatch` the same way, and the real listing passes, so there is no gap. A future reader should not assume five is the total.
- **Gap G1 (acknowledged, not a blocker):** this round has no fresh same-process R1/R6c contention test. R6c is Windows-ignored, so the in-process run ignores it, and it ran in its own nextest process during the sanity run of the eight. The unchanged lock relies on the retained `62bfd25` pair and mutation evidence, which the brief permits.

## Security

- The production delta after the merge is empty. `consensus.rs` matches reviewed A0 line for line.
- There are no new filesystem paths, no input parsing and no external requests.
- There is no new MCP or argument surface.
- The C3 change is a test fixture only. Inventory integrity still raises `Guard`, so a wrapper compromise or selection drift still fails the required job.
- Bounded-read ownership is unchanged: absolute deadline, non-cancelled engine wait, permit freed only after guard and Arc drop, and shutdown counters captured before abort. No secrets or private owner data appear in the source, logs or evidence.

## Blockers and claims

- **Blockers for this delta:** none.
- **Claims not made:**
  - no Windows three-node stability credit;
  - no re-enable credit for T1–T4, U6 or R6;
  - no natural-seam fix;
  - no hosted sampler, admission, budget or Job Object result for this head;
  - no M2 or M3 acceptance.
- **Status:** M2 stays open (A7 FINAL NO-GO, A12 with no retry). M3 stays unchecked. Windows three-node is **unverified on hosted Windows** for this head and **unverified on real hardware**, as are real transport, hosts and every hardware path. Advisory 1 (Weak expiry does not prove file closure) stays open.
- **Next, before coordinator-only publication:**
  1. Fast-forward PR149 from `8eaed30`, with no force.
  2. Get a separate exact-head `agent-review`.
  3. Pass all five required gates on the latest green main. R1–R5 must have executed and passed on all three OS. The guard must pass with exactly 8, and the sampler must report the exact-8 inventory. The Windows job must stay within the unchanged budget.
- **Red handling:** any red follows the brief's pre-registered rules. That means no rerun, no new ignore, and no change to deadlines, budgets or permits.

**Counts:** Standards has 0 blocking findings and 1 cosmetic nit (S1, helper placement). Spec has 0 blocking findings, 2 evidence notes (P1, P2) and 1 acknowledged gap (G1, no fresh same-process R1/R6c). Security has 0 findings.
