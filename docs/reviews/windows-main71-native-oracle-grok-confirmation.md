# Windows main71 native oracle: Grok confirmation

Disposition: **SP-ORACLE closed for this delta.** Standards pass. Spec pass.
Security pass. This confirmation does not accept the proposal and grants no
root-contract disposition, implementation, compile, native-run, discovery, or
budget authority.

Reviewer: xAI / grok-4.7, one independent pass, 2026-11-14. The assignment
forbids subagents, so standards, spec, and security are this single reviewer.
Independence is from the Sol correction, not from a second reviewer. Label:
single-agent review.

The prior recheck's non-acceptance stands. No controller is named. Controller
and CI cost remain unmeasured. Native authorization remains OPEN. M2 is
incomplete. The cause of run `43545244266` attempt 1 is unknown. No retry is
authorized. Native, installed, and dedicated paths are **unverified on real
hardware**.

## Pins

| Item | Identity |
| --- | --- |
| Review cwd | Linked worktree. Git top level equals the review cwd. Porcelain was empty before this file. Per-worktree Git dir under the common Git dir. `<isolated-worktree>` |
| Branch | `t3/review-native-replay-scope` |
| Reviewed head | `76470aadcda8fa57ec98175c872e18373a540b6e` |
| Reviewed base | `0872975a2ce388fc078cde90244e4365dfeac481`, parent of head |
| Head tree | `001f809dfc07e5e3f847e362f17d76f6902cd7aa` |
| Delta | `git diff 0872975a2ce388fc078cde90244e4365dfeac481 76470aadcda8fa57ec98175c872e18373a540b6e` — `docs/PROGRESS.md` and `docs/reviews/windows-main71-native-replay-scope-proposal.md` only. 2 files, 35 insertions, 26 deletions |
| Proposal blob at base | `403a1bfc79951ea9c90d6431c4b5e08ad0456896` |
| Proposal blob at head | `4e2cb9c296b52ee051162d8bb1b2b42a89cf44c2` |
| Progress blob at base | `e24c4417a2777a08963d6d26a6be87db3fe3d49c` |
| Progress blob at head | `b3f1e802b844f7096d6b156bfccc791ddeb2199a` |
| Prior Grok recheck | `b2676a4e53ee3595cd141c30bf0f48b434849816` is not an ancestor of `0872975`. Both have parent `6c98d2cdf2548d6188315b1356f7e06fd61be068` and tree `35197bbff66e5f0efe15c1f84972d62b780ddf5e`. Recheck blob `0d2ce8b6839c40544981a820660a8c158c4d49c3` is identical at `b2676a4`, `0872975`, and head |
| Windows source | `crates/arxd/src/development/windows.rs` blob `4cb039f4742568a3eb3a124ae3626ac9511b8be2` at base and head. 75184 LF bytes, 2050 LF, CR count 0. SHA256 `e2c91088e83ac15984219ea4581f5effe77c0e2089a9222fa1bb4eb764e1433a` |

Docs read: `AGENTS.md`, `CONTRIBUTING.md`, `SECURITY.md`, the Review skill and
its active profile, the full prior recheck, the full Sol delta, and the
`windows.rs` blob above at `drain_at!`, `Invocation::enter`, `Invocation::drop`,
`totals`, `capture`, `drain`, the loop and final drains, `stage!(Wait)`, and
`stage!(ExitCode)`. No proposal or progress edit was made. No `rustc` or Cargo
was executed. The 28-row table was not taken from a captured record.

## Standards

**PASS. No finding.**

The delta is one docs concern: split the packed error-55 labels into 28 rows,
add pass and pre-read total columns, and record that correction in the current
progress bullet. The proposal stays proposed and unaccepted. It does not claim
an unrun test, a reproduction, a measured budget, or a hardware pass. The new
progress bullet says the recheck is still pending and grants no code or run
authority. Repo-relative pins and sanitized placeholders only. No credential
or owner path was added. Commit scope is the two docs files above.

Fowler baseline: no smell is filed. There is no code diff. Splitting combined
labels into one row each is the required oracle table, not duplicated logic.

## Spec

**PASS. No finding.** The blocking SP-ORACLE text from recheck `b2676a4` is
corrected, and the new cells match the test-configuration source. These values
were not executed.

Checked, not blocking:

- The literal table has 28 data rows and seven columns. Rows 1–18,
  `daemon_token_open` through `resume`, are pass missing, stdout missing,
  stderr missing. `wait` and `exit_code` are pass missing and totals 0/0.
  The eight stream rows match the unchanged grouped oracle: loop out missing
  and 0/0, loop err missing and 1/0, final out pass 0 and 0/0, final err pass
  0 and 1/0. Missing is stated to mean `None`, never numeric zero.
- Test `drain_at!` calls `diag::totals` before the drain expression. `totals`
  stores `Some(out)`, `Some(err)`, and the given pass. `capture` copies those
  three fields. `drain` captures a peek or read failure before `c.total` grows.
  A failure therefore records the pre-read snapshot, not the byte just read.
- `Invocation::enter` sets `out`, `err`, and `pass` to `None`. No `totals`
  call occurs before the loop. The eighteen rows through `resume` capture that
  initial state. Missing is `None`, not `Some(0)`.
- Loop drains pass `None`. With empty pipes, both `Captured` totals stay 0, so
  the last loop snapshot is pass `None` and totals `Some(0)` / `Some(0)`.
  `stage!(Wait)` runs after those drains. `stage!(ExitCode)` runs after the
  loop. `wait` and `exit_code` have no prefill, so they copy that snapshot:
  pass missing and totals 0/0.
- L prefill is one byte `X` on each pipe, out before err, and below the 8192
  limit. Loop out rows copy pass `None` and 0/0. After the out drain counts
  that byte, loop err rows copy pass `None` and 1/0. F prefill is withheld
  until final pass 0. Final drains pass `Some(_pass)` and `_pass` starts at 0,
  so final out copies pass 0 and 0/0, and final err copies pass 0 and 1/0.
- The grouped eight-row oracle is unchanged and agrees with those individual
  rows. The paragraph no longer calls `wait` and `exit_code` missing totals.
  The table remains an independent literal. It is not generated from a
  captured record, and a later build must reject a mismatch instead of
  regenerating the oracle from output.
- Create-call counts, routes, the 14/4/10 case split, fixture fences, Gate 2,
  and the unmeasured budget text are unchanged. No source, scope, or authority
  sentence was added.

## Security

**PASS. No finding.** The delta does not change a fence. The prior security
corrections remain in the proposal and still authorize nothing.

Checked, not blocking:

- The Windows blob is unchanged at base and head, so the source fences the
  prior recheck checked are the same bytes.
- The proposal still requires one owned fixture directory, `PATH` exactly that
  directory, only the hashed helper `cargo.exe`, empty arguments, a byte hash
  match before every row, the helper outside workspace members, and real Cargo
  unselectable. `run_inner(..., false)` stays the policy path. `policy_fixture`
  is not a bypass. `pin` still rejects a reparse on the resolved path.
- Duplicates stay non-inheritable, absent from the sealed inherit list, and
  taken before the writer drops at the recorded baseline. The list stays
  NUL/stdout/stderr. Retained writers close before temp removal.
- `run_inner` still does not read quarantine on entry. The matrix still uses a
  fresh `Execution` per row and adds no `work` call to the child budget.
- Gate 2 still excludes `#[test]` registration, ignored-test registration, and
  new discovery. No controller is named. Nested Job containment, helper hashes,
  and the non-replay CI share stay OPEN. Accepting the duplicate design still
  does not authorize compile or execution.

## Close

Standards: 0 findings. Spec: 0 findings. Security: 0 findings.
SP-ORACLE is closed. No contract acceptance and no run authority.

Performed: read-only Git inspection, one SHA256 of blob `4cb039f4`, a row
count of the literal table, and this file's write and commit. Not performed:
source, proposal, or PROGRESS edits, build, test, native API, child launch,
owner or account or service or VM inspection, private-state inspection,
network, GitHub, install, CI, subagents.
