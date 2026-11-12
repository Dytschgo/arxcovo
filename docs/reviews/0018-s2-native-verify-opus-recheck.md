# ADR 0018 S2 correction attempt 2: native Verify Opus recheck

Reviewer: Anthropic / Opus 5.5 / high. This is a single-agent focused recheck
with no subagents. It covers SP-3R and the documentation follow-ups from
[the attempt 1 recheck](0018-s2-correction1-opus-recheck.md), which are
retained unchanged. It does not repeat
[the original full review](0018-s2-independent-review.md). Date: 2026-11-12.
The review grants no implementation, publication, acceptance or activation
authority.

## Pins and scope

- Target: `3a4ef954fe4c645d0cbb4d3e6f14d65b7995106c`, OpenAI / gpt-6.1-sol /
  medium correction attempt 2. Base: `38becc7`, which retains the attempt 1
  recheck report from `a873c2a` (blob `5f147fdc` is the same at the base and
  the target). Previously reviewed target: `d3cf449`. Correction 1 source:
  `fd57bbd`.
- Worktree: `C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-28026d23-4ebb50ddd68341f1`.
  pwd and the Git top-level match, HEAD equals the target, and the tree was
  clean. It is not the canonical `D:/Code/Arxcovo`, which I did not touch.
  Builds and mutations ran only in an owned scratch `git archive 3a4ef95`
  extraction with a scratch `CARGO_TARGET_DIR`. The only file I added to the
  worktree is this report.
- `38becc7..3a4ef95` changes four files: `client_config/tests.rs` (+37, one
  new Windows test), `docs/PROGRESS.md`, `docs/evidence/owner-cli-s2-correction1.md`
  and the new `docs/evidence/owner-cli-s2-correction2.md`.
- `git diff fd57bbd 3a4ef95` is empty for `client_config.rs`,
  `client_config/windows.rs`, `client_config/unix.rs`, `lib.rs`,
  `crates/arxd/src/service`, `main.rs`, the Cargo manifests and lock, and
  `.github`. The blobs at the target are `client_config.rs` `0b96530b`,
  `windows.rs` `4e53527c`, `unix.rs` `fdc35127` and `lib.rs` `b43adc8d`, the
  same as at `fd57bbd`. The windows.rs checkout SHA-256 is `85aac020…f582d5`,
  which matches the evidence's restore hash. Production source is
  unchanged, so I credit the native security source to the attempt 1 recheck.
- Spec: ADR 0018 decision 7; brief `docs/briefs/owner-cli-wiring.md` §3 S2
  ("Both acquisition errors and checker rejections are required negative
  cases"), §7 test 9 and its mutation list ("bypass the Windows checker, or
  ignore its rejection"). Standards: AGENTS.md, CONTRIBUTING.md, KICKOFF
  rules, and the Fowler smell baseline (judgement only).

## Verdict

- **Standards: PASS with advisories.**
- **Spec: PASS.** SP-3R is resolved on Windows. SP-1 through SP-8 were already
  closed by the prior recheck, except for the Unix items marked pending
  hosted, which stay pending. Hosted Linux/macOS build, tests and relevant
  mutations are still mandatory before S2 publication or merge.

## Spec

**SP-3R: RESOLVED (Windows).** The new test
`windows_native_verify_rejection_and_acquisition_reach_partial`
(`tests.rs:754-789`) is the candidate from the prior review, reformatted.
It builds a synthetic credential with `create_with` and the test `Ops` in an
owned private temp directory. It then calls the real `NativeFileOps::verify`
(`windows.rs:365-366`) three ways:

1. On a normal read handle at the exact length it gets `Ok`. This control
   shows that the handle and current-user facts are valid, so later failures
   are not confounded.
2. With the expected length plus one, the real `check` rejects it, and the
   actual `partial` classifier maps the error to `Partial`.
3. With a handle opened with `FILE_READ_ATTRIBUTES` only, it expects
   `Partial`. On inspection, `GetFileInformationByHandle` succeeds on that
   handle, and `GetSecurityInfo` (owner and DACL) fails because the handle
   lacks `READ_CONTROL`. Mutant 2 below proves only that `facts` returns
   `Err` here. It does not show which call fails.

The test then checks that the bytes are unchanged and that `Fixture::private`
still holds. It does not call production `create` or `ClientConfig::load`.
Its assertions have fixed labels, and nothing prints a SID, token, path or
pointer (`Facts` has no `Debug`).

I compiled three mutants of `windows.rs:366` independently. Each one was
killed by an assertion in the new test and by no other test:

| Mutant | Run | Result |
| --- | --- | --- |
| `let _ = self.verify_with(\|\| facts(f), expected); Ok(())` | `3f3c6cad` | killed at "native checker rejection" (`tests.rs:768`) |
| `match facts(f) { Ok(x) => self.verify_with(\|\| Ok(x), expected), Err(_) => Ok(()) }` | `b2dffadd` | killed at "native acquisition failure" (`tests.rs:779`) |
| `facts(f).map(\|_\| ())` (call-site checker bypass, new) | `71b3163a` | killed at "native checker rejection" (`tests.rs:768`) |

These match the coordinator's reported kills `7a5296dc` and `7dce4b99`. The
third mutant adds coverage for the brief's "bypass the Windows checker" case
at the native call site.

**Retained advisories (unchanged, non-blocking):**

- **A-1:** the mutant that keeps only the final-parent pin is inconclusive.
  The NTFS immediate-parent handle already protects its ancestors from
  rename, so holding every intermediate pin is verified by source inspection
  only. The evidence now records this.
- **A-3:** native Mode has only `cv` conversion and source coverage.
  `openat(0600)` already yields mode 0600, so no Mode mutation kill is
  possible or claimed.

## Standards

- **A-2: RESOLVED (docs).** `owner-cli-s2-correction1.md:156-159` no longer
  lists fchmod `mode` among the expected hosted kills. It says native Mode
  has only conversion and source coverage. The remaining "mode/length checks"
  item names the Unix verify checker, which the hosted Unix verify test can
  kill.
- **A-4: RESOLVED.** `docs/PROGRESS.md` has exactly one `## Current state`
  heading (line 3). The older one is now "Historical checkpoint before S2
  correction attempt 1", and all history is retained. The file is valid
  UTF-8.
- **The new test is clean.** It follows the existing fixture style, adds no
  production hooks, and introduces no baseline smell.
- **ST-4 and ST-5 stay deferred** (the fixture-enum naming and the scoped
  `dead_code`). That is acceptable.

**New advisory (non-blocking):**

- **A-5 (Low, docs).** The current state in `docs/PROGRESS.md` contradicts
  itself. Line 7 says main CI run `43213333692` after #48 succeeded, but
  line 9, carried forward, still says "Main CI after #48 is not yet claimed
  green". Line 7 also still calls the Opus recheck of attempt 1 "next".
  Reconcile both on the next PROGRESS update. This report does not change
  PROGRESS.

## My commands and results

All runs used Windows 11 as the current ordinary user, on the scratch
extraction of `3a4ef95`, with
`cargo nextest run --locked --no-fail-fast -p arxd -E 'test(client_config::)'`.

- Baseline: `8c7d2786`, 16 passed, 272 skipped. The new test passed.
- Three mutants (table above): run IDs `3f3c6cad`, `b2dffadd` and `71b3163a`.
  Each replaced one span by exact bytes and restored the captured
  `windows.rs` in an EXIT trap. All three SHA-256 restore checks were true.
- Final run after restoration: `e863fcde`, 16/16. Afterwards, `git
  hash-object` of the scratch `client_config.rs`, `tests.rs`, `windows.rs`,
  `unix.rs` and `lib.rs` equaled the target blobs.
- `cargo fmt --all -- --check`: exit 0. `cargo clippy --locked -p arxd
  --all-targets -- -D warnings`: exit 0. This covers arxd only, not the
  workspace.
- I ran no full workspace run, stress, instrumentation, retry or timeout
  change. No natural ExecutionFailed occurred in my runs. It remains
  unexplained, and nothing here is a fix or a waiver.

**Evidence credits, not reproduced by me:** the coordinator's focused run
`73b28012` (1 passed, 287 skipped), the mutant kills `7a5296dc` and
`7dce4b99`, the restored windows.rs SHA-256 `85aac020…f582d5` (I confirmed it
matches the target checkout), the workspace fmt and locked all-target Clippy,
and exactly one normal full workspace run
`3dfa9110-e373-4a8b-9a7b-863ae7c707f6` (**287 passed, 2 skipped**, 11.488s).
The evidence says these ran in `D:/Code/Arxcovo`. I credit them at their
stated pins only and make no new claims about them.

## Trust, secrets and limits

All credentials were deterministic and synthetic, under owned temp roots. I
read no real credential and not `private-state.json`, and I did not run the
ignored fixtures directly. I did not invoke production `create`,
`ClientConfig::load`, valid owner argv, owner `NativeOps`, OwnerGuard,
sessions, native preflight or store, the resolver, fixed system paths,
pipes, SCM/LSA/account/privilege/service, VM, Lab Manager or experiments.
I made no GitHub writes, push, PR, watch, merge, dispatch or rerun, and I
changed no installation or toolchain.

Unix behavior, compilation and mutations are **pending hosted** Linux/macOS,
and no local Unix pass is claimed. A fresh run of all hosted required checks
at the published head is mandatory. Native, installed and dedicated paths
remain **unverified on real hardware**, and M2 is incomplete. No activation
is authorized. No installed state exists, so source rollback is a reviewed
revert. Correction attempt 2 of 3 passes this recheck.

Findings: Standards 0 blockers and 1 new advisory (worst: A-5, a
contradictory main-CI statement in PROGRESS current state). Spec 0 blockers
and 2 retained advisories (worst: A-1, intermediate pin retention is
inspection-only).
