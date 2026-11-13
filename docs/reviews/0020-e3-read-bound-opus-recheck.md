# ADR 0020 E3 B1 read-bound narrow recheck

Status: **B1 RESOLVED by source analysis (test only). No new blocking
finding.** This is a narrow code recheck. It is not acceptance, it does not
approve a merge and it grants no publication or activation. Native execution of
the new test and its mutation kills is still owed by hosted Linux.

Reviewer: Anthropic / claude-opus-5-5 / high, review sub-agent, 2026-11-13.
No delegation and no subagents. One reviewer covered both axes, which are
reported separately below. The test and evidence under review are
Sol-authored (OpenAI / medium), so this is a different-provider check.

## Pinned target and scope

- Checkout: isolated worktree `t3/d2b7225a`, outside the canonical checkout
  `D:/Code/Arxcovo`. `git rev-parse --show-toplevel` matched the worktree.
  HEAD `dc89edde59b810a9821c51754d8db7382430dc93`. Status clean at start.
- Base: `f38d082` (retains the E3 review report from `de2cc44`).
- Reviewed `git diff f38d082..dc89edd` (3 files, +40): `c13fc98` adds 22
  lines to `crates/arxd/src/exec/hierarchy/linux.rs`; `dc89edd` adds the
  evidence section and one PROGRESS line.
- Production identity: `git rev-parse eb93620:…/linux.rs` = `f38d082:…` =
  `667a283`. The 364 lines before `#[cfg(test)]` (line 365) at HEAD are
  byte-identical to `eb93620`. `diff` of the whole file against `eb93620`
  shows only the 22 inserted lines at 398–419, inside `mod tests`.
  `git diff --stat eb93620 HEAD -- crates` lists only `linux.rs`.
- Corrected blob `945e34ccc1ab47c0836da5d2b4d5747df12594c6` and SHA-256
  `c45d2a416f8504cba34368c5d11558f00d149d4cc31b208ecb41a4b4bbeeb863` match
  the evidence.
- Read: AGENTS.md, the dytschgo-review skill, `0020-e3-independent-security-review.md`
  (B1 and advisories), brief line 301 (mutation list), the E3 evidence, the
  P-SCAN/L6 scanner in `stage_b_tests.rs`.

## Checks actually run (this recheck)

Host: Windows `x86_64-pc-windows-msvc`, rustc/clippy 1.95.0 (59807616e).

| Check | Result | What it proves |
| --- | --- | --- |
| `rustfmt +1.95.0 --check --edition 2024` on `linux.rs` at HEAD | pass | formatting only |
| `cargo +1.95.0 test -p arxd --locked --lib -- l6_e3 p_scan` at HEAD | 4 passed, 322 filtered | the scanner, which `include_str!`s `linux.rs`, still accepts the file with the new test; Windows only |
| **Scratch stub typecheck, NOT the real arxd crate:** `cargo +1.95.0 check --offline --target x86_64-unknown-linux-gnu --all-targets` | pass | verbatim `linux.rs` and `setup.rs` (SHA-256 matched to HEAD) typecheck with `libc =0.2.191` and `tempfile =3.23.0` Linux bindings; `parse.rs` reduced to lines 5–203; `development::Cancellation` stubbed. Nothing linked or executed. |
| Same stub: `cargo +1.95.0 clippy … --all-targets -- -D warnings` (forced rerun) | pass | same caveats; one stub-only lint on a unit-struct stand-in was removed by giving the stub a field |
| Stub positive control: replace the new test's final assert with `assert_eq!(unread, 24u32)` in the stub copy | E0277 at stub `linux.rs:417` | the stub really compiles the new `cfg(test)` function; the verbatim file was restored and re-hashed |

**Not run:** the real crate Linux `cargo check`/Clippy, any native execution
of the new pipe test or of L1–L5/L7–L10, and every native mutation kill. No
toolchain, cross C compiler or WSL was installed. No `/proc`, `/sys`,
cgroup, root, account, owner, service, VM, experiment, private-state or
ignored-fixture operation occurred. The only kernel object the test would use
is its own pipe, and it was not executed here. No full workspace rerun: the
change is a Linux-only test.

**Builder credits, accepted as stated, Windows only:** fmt and portable
L6/P-SCAN 4/4, run `2a369d29-5763-41ce-8aaf-79e6b4771a50`. These do not prove
any Linux path. Native paths stay **unverified on real hardware**.

## Standards

Sources: AGENTS.md (meaningful tests, honest evidence), CONTRIBUTING.md and
the Fowler baseline.

No hard standards breach:

- Source change is test-only and inside the single existing
  `#[cfg(test)] mod tests {}`. It adds no second test-module boundary, which
  `scan_with_hook` would reject.
- `try_clone()` (internally `F_DUPFD_CLOEXEC`) and `libc::pipe2` sit after the
  boundary. `e3_source_allowed` truncates its token stream at that boundary
  and `scan` excludes the test module, so the E3 denylist correctly does not
  apply. Production keeps no `dup`, `dup2`, `F_DUPFD*` or `try_clone`.
- fd ownership in the test is sound: each raw fd from `pipe2` is wrapped in
  exactly one `OwnedFd`; the `pipe2` assert fires before any `from_raw_fd(-1)`;
  the clone is a new fd; `text` consumes and closes `reader`.
- Evidence labels the test as not compiled or run on Linux and claims no
  mutation kill.

Advisories:

- **R-A1 Evidence wording.** Missing spaces in the new evidence and PROGRESS
  text: "retainedf38d082", "exactly24", "the32-byte", "passed4/4",
  "scanner4". Cosmetic; fix on the next docs touch.
- S-A1–S-A3 from the E3 review are preserved unchanged.

## Spec

Spec: brief line 301 "`take(4097)` raised (L1)"; E3 review B1 and its
suggested fix.

### B1: resolved by source analysis

`l1_read_limit_leaves_excess_pipe_bytes_unread` preloads 32 bytes into a pipe,
closes the only writer, calls `text(reader, 7)`, requires `TooBig`, then reads
the remainder through a second handle on the same pipe and requires exactly 24
bytes. 24 = 32 − (7 + 1), so the test observes the exact `max + 1`
consumption, which is the property B1 said was unobserved.

Mutation outcomes, by source reasoning only (none executed):

| Mutation in `text()` | Effect on test |
| --- | --- |
| `take(max + 1)` raised to `max + k`, k ≥ 2 | unread < 24 → fails |
| `take` replaced by a literal `TEXT_MAX + 1` (the brief's "take(4097)") | reads all 32 → unread 0 → fails |
| `take` removed | writer is closed, so `read_to_end` reaches EOF (no hang); unread 0 → fails |
| `take` lowered to `max` | returns `Ok` (7 bytes) → `TooBig` assert fails |
| buffered or read-ahead reader before the bound | over-reads → unread < 24 → fails |
| `>` to `>=` | survives this test; killed by the existing L1 4,096-byte `Ok` assertion |

The design differs from the review's FIONREAD suggestion but is equivalent and
better: closing the writer bounds every read by EOF, so a removed `take`
fails instead of hanging. 32 bytes fit any Linux pipe's minimum capacity, so
`write_all` cannot block. A concurrent test that forks between `pipe2` and
`drop(writer)` can hold the writer only until `exec` (`O_CLOEXEC`), so that
can delay the final read but not hang it.

Scope conformance: the test calls only the private `text()` on its own pipe.
It does not call `LinuxFs::system()`, `control`, `proc_file` or touch any
cgroup path. It uses the existing `ordinary()` root skip with its fixed marker;
a root run reports a skip, never a pass. No production behaviour was added.

### Advisories

- **A4 (preserved, extended).** `read()` clamps with `max.min(TEXT_MAX)`
  (`linux.rs:238`). Every L1 caller passes `max ≤ 4096`, so raising or
  removing that clamp still survives. This is a constant/bound pin like A4,
  not the `text()` mutation B1 named. Suggestion: one assertion that
  `fs.read(&dir, name, 8192)` on a 4,097-byte file returns `TooBig`.
- A1–A3 and A5–A8 from the E3 review are preserved unchanged.

## Platform limits and open gates

- The new test has a stub typecheck and Clippy only. It has no real-crate
  Linux compile, no native run and no mutation kill. Hosted Ubuntu CI, as an
  ordinary non-root user on the exact head, must run it and the read-limit
  mutants. A root run would print the skip marker and must be reported as
  skipped.
- Publication still needs green required checks on the exact re-pinned head
  and the coordinator's merge gate. Production stays
  `MechanismUnavailable`; the adapter has no callers and is not activated.
- Main `c56b6b5` run succeeded in 657 s (builder record, not re-observed here).
  M2 is incomplete. Native, installed and dedicated paths are **unverified on
  real hardware**. The dispatcher privacy flake, `E_PROTECTION` and
  `ExecutionFailed` causes remain unknown. No retry, diagnosis or activation
  is claimed or authorized.

**Summary.** Standards: 0 hard findings, 1 new cosmetic advisory (worst:
evidence spacing typos). Spec: B1 resolved by source analysis, 0 blockers,
1 extended advisory (worst: `read()` clamp still unpinned).
