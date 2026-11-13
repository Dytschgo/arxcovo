# ADR 0020 E5: current-contract local port

Builder: OpenAI / gpt-6.1-sol / high, 2026-11-13. Local inactive port only.
E5 has **never received independent source security review**. Fresh Opus
5.5 high source review is pending; byte identity is not security acceptance.
No subagents or checkouts were created. Completion returns to the coordinator.

## Pins and identity

Runtime cwd and `git rev-parse --show-toplevel` agreed on the supplied
isolated worktree, distinct from the canonical checkout. Initial status was
clean at `fc1a6648515ef2227b59623df75e599da3bc9abe` on branch
`t3/port-e5-exec-cgroup-evidence`. Local account paths are intentionally omitted.
The supplied current main/Stage D pin
`f91ae5a7d95645fb2057f70411675d6f1e07d1df` is an ancestor of that contract
stack (`git merge-base --is-ancestor`, exit 0). No remote state was queried.

Read AGENTS, KICKOFF, ADR 0020 D1/D6/A5, brief E5/R1-R4, original and
correction-1 Sol reviews, correction-2 disposition and Opus recheck, and the
local-delivery Opus review. These are design evidence, not E5 source review.
Contract PR56 remains pending its gates according to the supplied task.
Local inactive work is authorized; publication/integration waits for its
merge, exact re-pin and independent integration review. No gate is waived.

Original E5 source: `9f535693a4d901b422c86bb6efe872b930aaaa02` on prepared
base `9733841bd83c4fbbe877cb53a7af9950a123609b`. Original evidence:
`b181e72dfbfdd62c8f4f838dc01ee4662dc7978d:docs/evidence/linux-residue-e5.md`.
All three destination files before port were identical to that old base
(`git diff --quiet 9733841 fc1a664 --` the three source files, exit 0).
The port copies only their original E5 blobs. No adaptation was required.

## Exact source manifest and scope

Port source commit: `f866af45a31779abc30d7674a07aaa943dee940a`.
The evidence is committed separately. All three source blobs below are
byte-identical to original E5, including the test module.

| File | Git blob | SHA-256 (LF bytes) |
| --- | --- | --- |
| `crates/arxd/src/exec/mod.rs` | `fad2ef7cc2f613704cedd002fb5c94de305b4618` | `5df0b78177e7b7e87d0c6833f45ad72a12922776a838b379f45e6e337c2cf5a5` |
| `crates/arxd/src/exec/unix.rs` | `0d087e37f31f3e17db652a87f2a0364eb398479e` | `500a914a1f6c6b16de516a9c9df332a47ab89f17769b6a48cd5f06dcb70af0b7` |
| `crates/arxd/src/exec/cgroup.rs` | `d85544b0493727a780a902a0d934dd9ddea135e3` | `039a2f9c3a864816786cf8c11c63a3712601f1211a443874cfe7641d333b6c6c` |

`git diff --exit-code 9f535693a4d901b422c86bb6efe872b930aaaa02 --` these
three files returned 0. The only other changed file is this evidence report.
Current shim, bridge, scanner, owner SDK/client, CI policy and guest guard
remain at the contract-stack baseline. E1 from the other worktree is absent.
No PROGRESS, E2/E3/E4/E6/E7, construction, readiness or activation changes.

Runner owns a fresh per-instance `Arc<AtomicBool>`, reads with Acquire and
never clears the flag. A private Linux-only clone passes through `run_core`
and `unix::run` to `Group::create`. Both create cleanup sites call
`release_leaf`; after unlinkat it performs only a Release store on failure,
with no additional syscall or errno read. Failed-openat still reads errno
after cleanup; setup failure still returns its already captured error.
Drop retains its existing confirmation call and signals if it returns false.
Branch order, legacy/phased messages and results, cancellation and output
behavior are unchanged. No static residue, config flag or service caller.

R3 uses two distinct owned temporary domains with distinct subtrees and Arc
identities, checks fresh false flags, thread-signaled isolation and monotonicity
through shutdown. It does not permit replacement within a service subtree.
The portable inventory checks every production unlink site, both create
helper calls, Drop signaling, private Unix plumbing and legacy errno order.
Existing `tests.rs` is included in the trailing inline module to preserve
the unchanged Stage B panic scanner's test boundary and existing test names.
The expectation files and scanner are unedited.

## Current local checks

Windows `x86_64-pc-windows-msvc`; rustc 1.95.0 (`59807616e`),
cargo-nextest 0.9.151 (`6b3f99457`). `wsl --list --quiet` reported
"The Windows Subsystem for Linux is not installed." No installation or Unix
execution occurred. Linux-only code was not compiled or run on this host.

| Command | Observed result |
| --- | --- |
| `cargo fmt --all --check` | exit 0 |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | exit 0; Windows targets only |
| `cargo nextest run --locked -p arxd -E 'test(exec::)'` | exit 0; 17 passed, 310 skipped by filter; default profile; run `f691fb3f-13fa-48ac-8cca-0920a4856a3c` |
| `git diff --check`, `git diff --cached --check` (source commit) | exit 0 |
| Old-source byte comparison, Git blob and SHA-256 inventory | all three files identical |
| `git diff --quiet fc1a664 -- crates/arxd/src/exec/tests.rs crates/arxd/src/exec/stage_b_tests.rs` | exit 0; expectations/scanner unchanged |

The focused run passed
`independent_owned_domains_keep_distinct_monotone_runner_flags`,
`cleanup_source_pins_release_sites_and_legacy_errno_order`, all three
`p_scan_*` tests, and the other existing Windows-supported exec tests.
This is the only new nextest run; no full workspace or mutation rerun.
No ExecutionFailed occurred in this focused run; historical failures remain
unresolved. All checks covered the exact source bytes committed above.

Local supporting logs are outside the repository at
`%TEMP%/arxcovo-e5-fc1a664-validation`. Worktree/account prefixes were replaced
with placeholders before hashing; logs are UTF-8/LF and are not published.

| Log | SHA-256 |
| --- | --- |
| `fmt.log` (empty, materialized after successful command) | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `clippy.log` | `d20887ce82402e1d5f67ed796d2c4dc2cdba73a0b726fa71e2a68106529dc4de` |
| `focused.log` | `53573e3d5e7d1362fbba2c58a4a808d4179c09cc19ef941df22e2178a808b0de` |

## Retained old-source evidence only

The following are attributed solely to old source `9f53569` and evidence
`b181e72`; none were repeated or claimed as new-stack runs:

- Full default workspace: 325 passed, 3 skipped; nextest run
  `a8e8e37e-6e4d-4eb1-a4c3-6d4f7746f824`.
- M1 Drop ignores confirmation: run `016b6d32-45ff-4cd7-82b4-1c0695d161b0`.
- M2 create bare unlink: run `843f44e2-8526-4070-acae-04937696c249`.
- M3 process-global flag: run `cc9d136f-5eb6-47de-9d56-ffa65303a8b5`.

Each historical mutation compiled and failed its named Windows test with
exit 100, then was restored. M1/M2 exercised the portable source guard,
not Linux code. M3 exercised Windows-compiled Runner construction. Source
equality supplies no reason to repeat the full suite or those mutations.

## Limits and remaining gates

Linux owned-temp R1 (Drop) and R2 (helper/errno/monotonicity) remain **UNRUN**
locally. Portable inventory passing cannot prove Linux syscall behavior.
Representative real create-path propagation remains unexercised: the path
requires protected root-owned cgroup2 before mkdir, and no trust-bypass seam
was added. Linux compilation, R1/R2 execution and Linux runtime mutation
checks remain requirements, not passes. Fresh independent Opus high E5
security source review is still required, followed by merged-base re-pin and
integration review before publication.

No real cgroup, root, owner, service, account, VM, Lab Manager, experiment,
private-state or native ignored-fixture action. No GitHub, push, CI, PR or
diagnostic rerun. Production remains inactive/MechanismUnavailable. Native,
installed and dedicated paths are **unverified on real hardware**. M2 is
incomplete; ExecutionFailed / E_PROTECTION remain unresolved with unknown
cause, no fix, waiver or retry.

## First security-review test correction

Opus high first review4e1d6b8 (retained7ab6f3a) found S1 rustfmt coverage and
B1 unpinned forwarding. Coordinator Sol medium test correction c46304e restores
out-of-line mod tests and moves the two new tests to exec/residue_tests.rs.
Production mod.rs prefix, unix.rs and cgroup.rs remain identical to f866af4.
The portable pin now requires self.residue.clone at run_core, exactly one
false Arc allocation in Runner::new, exactly two production unix residue uses,
and no store(false). These are source-pin proofs, not Linux runtime proofs.

Fresh fmt and locked workspace/all-target Clippy passed; baseline and final
focused exec runs each passed17. Final run c40ef903-2234-47fb-ad57-b90eb66c6ae8.
Both surviving mutations now compile on Windows and fail source-pin assertions:
- run_core fresh flag: 2d5a2342-116f-4a1c-919d-69a471644855
- unix::run fresh flag: 9c1f6532-6c2b-49b9-b439-93d96ce90720
All original bytes restored and checked. Deliberate tests.rs misformatting is
now detected by fmt --check; restored afterward. An initial duplicate test
helper visibility caused a fmt parse error and was restored to the unchanged
original helper before successful verification. No claim of Linux compilation
or R1/R2 execution, no full-suite repeat or native action. Fresh narrow Opus
recheck is pending. Publication requires that review plus exact main-base
integration and all hosted gates. M2 incomplete; hardware remains unverified.

## B2 returned-Group flag correction

Coordinator OpenAI / gpt-6.1-sol / medium added only the proposed portable
assertion pinning the returned Group field to the incoming residue parameter.
Source/test commit `e36f4fdc0fa877200ca7aec5c1c79331debe5dea`. Production bytes remain unchanged.
Fmt passed; baseline 2/2 passed (428e581e), both fresh-field mutants failed
at the new assertion with exit 100, and restored 2/2 passed
(0b0804b8-53e8-4a34-a285-367645dc280a). Mutated production bytes were restored
exactly. These Windows source-pin runs give no Linux syscall/runtime credit.
No full-suite repeat, native action or hosted run. Narrow Opus review pending.

Current local manifest (before merged-main re-pin):

| Path | Git blob | SHA-256 of committed bytes |
| --- | --- | --- |
| `crates/arxd/src/exec/mod.rs` | `7d63eee9ae4c78d5f963b92976bd1f7ba22c338c` | `b6cc7f6b8be9d08e06d3541542d20fa1a435b3de6af749b0c093973b9b9aa4a2` |
| `crates/arxd/src/exec/residue_tests.rs` | `91c52516fba2b4b1740309c7b986759e6aa60fc4` | `8f3af42826c53896aa43282df75aecba6c835fb404bed35c90896495fecb8af8` |
| `crates/arxd/src/exec/unix.rs` | `0d087e37f31f3e17db652a87f2a0364eb398479e` | `500a914a1f6c6b16de516a9c9df332a47ab89f17769b6a48cd5f06dcb70af0b7` |
| `crates/arxd/src/exec/cgroup.rs` | `d85544b0493727a780a902a0d934dd9ddea135e3` | `039a2f9c3a864816786cf8c11c63a3712601f1211a443874cfe7641d333b6c6c` |

## Integration onto merged E1

Sol medium integrated main `2fbc647` in source merge `589fc2a`. E1 hierarchy
module is preserved; unix.rs/cgroup.rs exactly match reviewed f866af4.
Fmt and locked workspace all-target Clippy passed. First full run
`65762554-f812-4f0f-9a08-0b1af8d6ddfa` failed an unchanged dispatcher durable
privacy assertion (case unix_windows): 143 passed, 1 failed, 3 skipped,
193 not run due to fail-fast. No secret or raw database contents were logged.
The focused test then passed once and 10 bounded repeats, with no source
change. Cause remains UNKNOWN; this is not a fix or waiver. A single complete
no-fail-fast diagnostic workspace run `c5443913-3035-4def-9ae5-305cf9a16659` then passed 337/337, 3 skipped
in 13.021s. Both full-run outcomes remain credited separately.

Current integration manifest (committed source bytes):

| Path | Git blob | SHA-256 |
| --- | --- | --- |
| `crates/arxd/src/exec/mod.rs` | `ddf668d71c78aefe10103ee0e8d8b4ab4eea18f4` | `16201cfb37a927bfcf7afc45481ef988b6dca418a27bb2a74b3738e316da1514` |
| `crates/arxd/src/exec/residue_tests.rs` | `91c52516fba2b4b1740309c7b986759e6aa60fc4` | `8f3af42826c53896aa43282df75aecba6c835fb404bed35c90896495fecb8af8` |
| `crates/arxd/src/exec/unix.rs` | `0d087e37f31f3e17db652a87f2a0364eb398479e` | `500a914a1f6c6b16de516a9c9df332a47ab89f17769b6a48cd5f06dcb70af0b7` |
| `crates/arxd/src/exec/cgroup.rs` | `d85544b0493727a780a902a0d934dd9ddea135e3` | `039a2f9c3a864816786cf8c11c63a3712601f1211a443874cfe7641d333b6c6c` |

Independent exact-head integration review and hosted gates remain required.
Linux R1/R2 have not run locally. Production remains inactive; M2 incomplete.
Native paths remain **unverified on real hardware**.
