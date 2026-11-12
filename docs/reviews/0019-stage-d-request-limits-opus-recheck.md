# 0019 Stage D R1 request-limits recheck (Opus)

Narrow cross-provider check of the Sol test correction for R1 only. This is
not a new broad review or an Opus architecture independence claim. It checks
one Sol test-literal change and the evidence that goes with it.

## Verdict

**Standards: APPROVED for the R1 delta.** **Spec: T3 now met.** **R1:
closed.** The two added lines are exactly the correction proposed in
`7569728` (retained as `612c3a6`). Run requests now use 7 s / 333 bytes in
both path and tool modes. Probe expectations stay at 60 / 4,096. No other
finding is raised. The other conclusions of review `0019-stage-d-test-correction-opus-recheck.md`
carry over unchanged: T1, T8 and T10 met, N1/N2 advisory, S3–S6 deferred.

## Pins and source identity

- Worktree: the runtime-prepared T3 worktree, branch `t3/c7e59c6c`. Its top
  level matched the working directory. It is not the canonical
  `D:/Code/Arxcovo`. Status was clean at HEAD
  `6b953ba4ffc758925277d2a0ed0228080a2f0bec`. No checkout or worktree was
  created and nothing was written to the canonical checkout. No subagents.
- Target `6b953ba`, parent `612c3a6`. `git diff 612c3a6 6b953ba` touches only
  `unix_bridge/tests.rs` (+2), `docs/PROGRESS.md` (+2) and
  `docs/evidence/unix-bridge-stage-d-review-followup.md` (+13).
- Prior report blob is identical at `612c3a6` and HEAD
  (`1aed07fa06b22754fd401ef4f99099457fe1b6b3`).
- Production `crates/arxd/src/service/unix_bridge.rs`: `git diff d3615be HEAD`
  is empty; blob `00f8f2c0aae63e83f58f81158eb42fc88c41dff7` at both;
  SHA-256 of the exported file `3acf3d23bf6f8800…bca7b1`, matching the prior
  report's pin.
- `unix_bridge/tests.rs` at HEAD: SHA-256 `c820f904…958d6a62`.

## Standards

- The diff adds `r.timeout_s = 7; r.output_bytes = 333;` immediately after
  `let mut r = request();` in `t3_fake_port_receives_exact_request`, so the
  values feed both `expected` and the `b.run` input.
- The expected oracle still selects `r.timeout_s` / `r.output_bytes` for path
  and tool, and literal `60` / `4096` for probe. Probe passes `None`, so the
  new literals cannot leak into it. Both new values differ from the probe
  constants, which is what R1 required. A run spec hard-coded to 60 / 4096
  (`unix_bridge.rs:215`) now gives `60s != 7s` at the port.
- The values are inside the request schema bounds (`timeout_s` 1–3600,
  `output_bytes` 2–8192, `executor.rs:100`), so the test does not depend on an
  input that real validation would reject.
- The port assertions still sit inside the fake, and the outer
  `Completed`/`Discovered` check still proves they ran past the panic
  boundary. Nothing else in the test changed.
- **Advisory, not blocking, outside R1:** report output capping
  (`unix_bridge.rs:445`, `request.map_or(4096, …)`) is not exercised with
  non-default limits by T3. R1 named only `spec()`. The optional
  `t3_exact_specs` non-default values remain optional.

## Spec

- **T3 (brief §2/§8): met.** The three request shapes, argv fidelity, fixed
  binary, empty cwd and probe constants were already verified. Request timeout
  and output forwarding to the port now rest on non-default inputs.

## Evidence accuracy

- The follow-up `R1` section and the new `PROGRESS.md` line match the diff.
  They state that production is unchanged, claim no new full suite or Clippy
  run, and still require this narrow check.
- Credited by exact pin, not rerun: the coordinator's compiled hard-coded
  60/4096 mutant kill `ac960a28-8ca9-4bf4-a5ed-6a1c5d825ad0` with production
  restored to exact bytes, and final fmt plus focused 15/15
  `a052ca8f-c825-4281-9db8-f5bc96f3a877`. The kill matches this reviewer's own
  scratch kill `f1c0914b` from the prior report, which used the same mutant
  and the same two lines.

## Own checks

Windows host. Scratch `git archive` export of `6b953ba` with its own target
directory. No mutation in this pass.

| Run | Result |
| --- | --- |
| `cargo nextest run --locked -p arxd -E 'test(t3_fake_port_receives_exact_request)'` (pure fake) | 1/1 pass, `b241748b-627a-4edd-817e-9428afa741f4` |

Limits: no Clippy, no fmt rerun, no full suite, no other mutation, no Unix
compile. No real `Runner`, native preflight, account, owner, service, VM,
Lab Manager or CI operation. No ignored fixture ran. No GitHub write, push,
PR, watch, merge, rerun or settings action. No source or `PROGRESS.md` edit.

## Gates unchanged

- Stage C (`2ce79ca`) is still an explicit unmerged dependency.
- Hosted Unix compile and run are pending. Stage D has no Unix compile or
  runtime credit.
- Native, installed and dedicated paths remain **unverified on real
  hardware**. M2 is incomplete. Production remains inactive.
- `ExecutionFailed` / `E_PROTECTION` remain unexplained, with no waiver or
  retry inferred.
- CI investigations stay parked under the user's product-first direction.
- This check grants no activation and no stage E, F or G authorization. A
  publishing PR still needs green required checks.

Standards: 0 required findings, 1 advisory; R1 closed. Spec: T3 met; no
partial requirements remain in this delta.
