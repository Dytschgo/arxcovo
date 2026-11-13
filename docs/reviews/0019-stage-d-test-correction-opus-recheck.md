# ADR 0019 Stage D test-correction recheck

Reviewer: Anthropic / claude-opus-5-5 / high, 2026-11-12. Review sub-agent,
no subagents. This recheck covers only the Sol coordinator's test correction
`21821f9` of the Sol Stage D implementation, so it counts as the
different-provider review of that correction. Opus wrote the ADR 0019
architecture and the prior Stage D review (`85da90f`, retained at `8afbcbc`).
This recheck claims no independence for that architecture.

**Standards: CHANGES REQUESTED, one small test-only finding (R1).** S2 and
the T8 part of S1 are closed. The T3 part of S1 is closed for the masking
defect, but its inputs still cannot tell request-sourced limits from the
probe constants. **Spec: CHANGES REQUESTED, same root cause.** T1 and T10 are
now met. T3 is met except for request timeout and output forwarding (R1).
Production behavior is unchanged and still matches the contract. Native,
installed and dedicated paths remain **unverified on real hardware**. M2
remains incomplete.

## Pins

- Worktree: the runtime-prepared T3 worktree, branch `t3/7292504d`. Its top
  level matched the working directory and is not the canonical checkout.
  Status was clean at exact HEAD `21821f909a287ae67a1224add816f718c420f51e`.
  No checkout or worktree was created, and nothing was written to the
  canonical checkout.
- Target `21821f9`, base `8afbcbc` (retains review `85da90f` of `3b95a1b` /
  source `d3615be` / base `7b2cfa1`). Base ancestry includes the **unmerged**
  Stage C source `2ce79ca`. Actual main is `1604db1`. This recheck does not
  claim that Stage C's merge gate has cleared.
- `git diff --stat 8afbcbc 21821f9` touches three files:
  `service/unix_bridge/tests.rs` (+53/−12), `docs/PROGRESS.md` (+2) and
  `docs/evidence/unix-bridge-stage-d-review-followup.md` (+43, new).
  `git diff --name-only d3615be 21821f9 -- crates` lists only the test file.
  So the bridge, executor and scanner are byte-equal to `d3615be`. The
  bridge's SHA-256 is `3acf3d23…`, the same as the prior review's pristine
  copy.

## Standards

Sources: AGENTS.md ("every behavior needs a meaningful test"), the prior
review's S1/S2 corrections and the smell baseline.

### Prior findings

- **S2 (T1 refusals): closed.** Lines 144–163 now use
  `ServiceConfig::parse_inventory(..).unwrap()` for all four cases, with no
  `if let`. Each case reaches `derive`. Each case also hits its own guard, as
  my single-check mutants show: name length only (M3), name charset only
  (M4) and `build_gid() == None` accepted (M5) each fail T1. The builder's
  `gid == 0` kill covers the fourth.
- **S1, T8 part: closed.** `marker_seen` is set inside the fake. It is
  checked after `dispatch` returns, outside the bridge's catch. A missing
  call or a missing marker leaves 0 and fails. M6 (request args dropped,
  T8 run alone) fails with `port marker observed`.
- **S1, T3 masking: closed.** The `Report::Completed`/`Discovered` assertion
  after `b.run` makes any fake panic visible. The expected `RunSpec` no
  longer calls `spec()`. Its argv comes from the loop's `mode`, the request
  fields and fixed literals. Its timeout and output come from the request or
  fixed probe constants. So a mutation of `spec()` cannot move both sides.
  M1, M2, M8 and M9 all fail this test.

### R1 (low-medium, required, test-only): T3 inputs equal the probe constants

- **Where:** `unix_bridge/tests.rs:766-817`
  (`t3_fake_port_receives_exact_request`).
- **Defect:** `request()` leaves `timeout_s` and `output_bytes` at their
  serde defaults, 60 and 4,096 (`executor.rs:29-34`). These equal the probe
  constants. `t3_exact_specs` also checks 60 and 4,096 for a run. So no test
  tells "the run forwards the request's limits" apart from "the run uses the
  fixed constants".
- **Proof:** mutation `(args, r.timeout_s, r.output_bytes)` →
  `(args, 60, 4096)` in `spec()` **survived** all 24 focused tests (run
  `214041dd-16d2-4b26-8cf6-623c9b70ce66`). Builder mutants that add one to
  the value pass through a different number, so they are caught. Mutants that
  hard-code the defaults are not.
- **Correction (checked in scratch only):** in the T3 loop, set
  `r.timeout_s = 7; r.output_bytes = 333;` before building `expected`. With
  that two-line change, the unmutated bridge passes 15/15 (run
  `2860dc99-21cf-4b27-baef-6ccaa56755d4`). The same hard-coded mutant then
  fails with `left: 60s, right: 7s` (run
  `f1c0914b-dc2f-4760-8be7-6a785a3b50b4`). The test file was restored to
  `e4b8f8bc…` afterwards. Optionally, give `t3_exact_specs` non-default
  values too.

### Lower-severity, non-blocking

- **N1 (advisory):** T8 still asserts privacy only, not each class's report.
  The T4 phase matrix covers the reason and quarantine flag for every
  `RunError` through real `dispatch`, so this is not a gap.
- **N2 (advisory):** the factory-once loop in T1 still discards `b.run`. Its
  fake contains no assertions, so nothing can be swallowed there.
- Prior S3–S6 stay deferred, unchanged. No baseline smell is worth raising
  in this delta. The expected-spec block duplicates `spec()`'s shape on
  purpose, which is what makes the oracle independent.

## Spec

Sources: brief `docs/briefs/0019-unix-native-bridge-implementation.md` §2
and §8 (T1, T3, T8, T10), plus the prior review's partial-requirement rows.

- **T1 ("every row … including refusals"): met.** The four account and gid
  refusal rows now run, and each fails if its guard is removed.
- **T3 (three request shapes, exact spec at the port): partly met.** Shapes,
  argv fidelity (`-x`, `--`, empty, Unicode), fixed binary, empty cwd and the
  60 s / 4,096 probe constants are verified at the port. §2 also says a run
  carries the request timeout and output. That part rests on R1's
  non-default inputs.
- **T8 sanitation: met** for the scope the brief sets. The port marker check
  is now unmaskable.
- **T10 diagnostics: met.** The parent asserts `3141592653` is absent from
  the combined stdout and stderr in both backtrace modes. M7 (a production
  `eprintln!` of the observed UID) fails T10 with `identity marker withheld`.
  The fixed-literal counts and the negative control are unchanged. The parent
  still re-executes only the new purpose-built ignored fixture.
- **Production:** unchanged from `d3615be`, so the prior review's contract
  conformance stands. No unrequested behavior was added. Nothing here touches
  stages E, F or G, an owner operation, CI or policy.
- **Evidence accuracy:** the follow-up report correctly records the first T3
  attempt's surviving timeout mutant (`6053c69d`) with no kill credit. It
  explains why that mutant survived. It credits the builder's 323/3 run only
  to `d3615be`. Its four final kills are consistent with my reproduction M9,
  the same mutant class as `7d6ee0de`. It does not claim the R1 mutant class,
  and it claims no Unix compile.

## Commands and credits

Reviewer runs on a Windows host. They used a scratch `git archive` export of
`21821f9` and their own target directory. No worktree or canonical writes.
After each mutant, the bridge was restored and its SHA-256 checked against
`3acf3d23…`. Every check matched.

| Run | Result |
| --- | --- |
| `cargo fmt --all --check` (export) | pass |
| focused `test(service::unix_bridge) \| test(exec::stage_b_tests)` | 24/24 pass, `a3bb91e6-2ee2-4cfe-91bd-7580b0d78252` |
| M1 probe timeout 61, `service::unix_bridge` | killed (T3 port, T3 exact), `2a9c9f2a-9417-40c9-bcd2-a6e260b93c61` |
| M2 probe output 4095, same | killed (T3 port, T3 exact), `56bd0170-528a-4e31-a033-790adb7b5cad` |
| M3 name length check only removed, T1 | killed, `28bdfdaf-2d52-44a5-a2ff-a7692c10ba29` |
| M4 name charset check only removed, T1 | killed, `8e170af4-8fd0-4f93-8c3b-2aa95cda9b6a` |
| M5 missing gid accepted, T1 | killed, `c4f67034-864f-45f9-abd4-006b8b359709` |
| M6 request args dropped, T8 alone | killed (`port marker observed`), `12c7e2ea-22f8-4e8a-a57d-620cb2a7e137` |
| M7 observed UID printed, T10 alone | killed (`identity marker withheld`), `5b71721d-25d4-4471-abeb-16e4419a4a32` |
| M8 tool/path mode swapped, T3 port alone | killed, `17371682-a81a-409b-a0be-dd215840f576` |
| M9 run timeout plus one, T3 port alone | killed, `d7e74b13-5c68-4697-ae1d-2da5f65cb0f8` |
| M10 run limits hard-coded to 60/4096, focused 24 | **survived** 24/24, `214041dd-16d2-4b26-8cf6-623c9b70ce66` |
| R1 fix in scratch, `service::unix_bridge` | 15/15 pass, `2860dc99-21cf-4b27-baef-6ccaa56755d4` |
| M10 with R1 fix, T3 port alone | killed, `f1c0914b-dc2f-4760-8be7-6a785a3b50b4` |

Limits: no Clippy, no full workspace run, no Unix compilation, and no stress,
retry or instrumentation. No real `Runner`, resolver, native preflight,
owner or service operation, fixed system path, cgroup, VM, Lab Manager or
experiment was used. No old ignored fixture ran; T10's parent re-executes
only its own new fixture. No GitHub, CI or settings action was taken.

Credited by exact pin, not rerun: the coordinator's final fmt, locked
workspace all-target Clippy and focused run `4dffa45b` (24/24) at `21821f9`.
Also its four kills `7d6ee0de`, `677d1b8f`, `93941ae5` and `ccfcfca9`. The
builder's full run `8195e507` (323 passed / 3 skipped) stays credited to
`d3615be` only. Stage D still has **no Unix compile or runtime credit**.
`ExecutionFailed` / `E_PROTECTION` remain unexplained, with no fix, waiver
or retry inferred.

## Blockers versus deferrals

- **Blocking Stage D acceptance (test-only, two lines):** R1. The
  coordinator can apply it directly. Done means M10 is killed with production
  bytes still `3acf3d23…`. A narrow different-provider check of that delta is
  enough; this report's other conclusions carry over.
- **External gates, unchanged:** Stage C stays unmerged. Hosted Linux/macOS
  compile and run, green required checks and this review on the publishing
  PR are still needed. No publication, activation, stage E setup, stage F
  experiment or M2 acceptance is granted here.
- **Non-blocking, can defer:** N1, N2, prior S3–S6, G-P1/P2/P3 and the
  stage E/F/G gates.
- Product work can continue in parallel. R1 does not depend on the CI queue.

Standards: 1 required finding (R1, low-medium), 2 advisory; worst: R1, the
T3 inputs equal the probe constants. Spec: 1 partial requirement (T3 limit
forwarding, same root cause); worst: T3.
