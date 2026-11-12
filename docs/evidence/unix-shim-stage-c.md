# ADR 0019 Stage C local evidence

Builder: OpenAI / gpt-6.1-sol / high, 2026-11-12. Implementation sub-agent;
no subagents, publication, GitHub operations or independent-review credit.

## Pins and boundary

Source head: `656dde3c4c221b0048487a738ede5f0782f078c5`. Evidence head is
the following docs-only commit containing this report (exact SHA in handoff).
Prepared base: `c372865f3365ef8d8baf02e32044d607eef9d2b3`, differing from
supplied merged main `1604db1fbd686d3bf59e81f4d2702eaecc6b8bc0` only in
PROGRESS. The initial status was clean. Runtime cwd and git top-level both
matched `C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-6255da31-d8e35768d6f5bf64`.
No shell-created checkout/worktree or canonical-checkout write occurred.

Read AGENTS/KICKOFF, ADR 0019 and complete implementation brief (including
C2/P6, sanitation, S1–S7, mutations and staging), ADR 0007, Request validators,
TOOLS, Stage B runner/scanner source, independent review, tool-acceptance
recheck and F2 follow-up evidence. No architectural contract change is made.

Manifest (committed Git blobs, independent of checkout line endings):

| File | Blob |
| --- | --- |
| src/main.rs | a83727d168bee8fdd836a2341c1d322de0c08685 |
| src/lib.rs | d7e1917e8f331ae3d2277b10d617d492e72fbeac |
| src/unix_shim.rs | a071edcea2690800ed366130137ad54569be9ad0 |
| src/unix_shim_tests.rs | d8d879e795965650ec7a38fee896d0b45c2b8dfb |
| tests/unix_shim.rs | fc2f56a2a6c0b258defb6fe0cbdb3d62ed8adb80 |
| src/exec/stage_b_tests.rs | a2cb8da7f973bb8496885beb64c0401ca1742e12 |

All paths are under crates/arxd. The docs-only follow-up changes this report
and PROGRESS. Runner production, legacy bounds, cgroups, Report/Creation,
config, owner CLI, dependencies/Cargo/lockfile, CI, schemas and packaging are
unchanged. No bridge, setup, experiment or native activation is added.

## Implemented contract (source observations)

Unix-only reserved starting prefixes enter before service/owner routing,
Tokio, configuration, layout or state. Malformed reserved forms return silent
125. Exact owner routes, including service run's argument handling, remain
byte-inherited after the inserted Unix branch. Non-Unix commands are absent.
The only public shim operation calls the private common silent entry, whose
first statement installs the sole no-op hook. Its catch covers the real euid
getter, identity refusal, parsing and all execution/probe work. The hook is
never restored. No service or foreground invocation installs it.

Exec parsing reuses actual Request::validate, retaining lexical and argv
bounds. It opens cwd beneath the inherited directory using Linux openat2
BENEATH/NO_SYMLINKS or macOS descriptor-relative DIRECTORY/NOFOLLOW and
fchdir. Empty cwd stays put. Path mode passes only an absolute target to
Command; tool mode permits only TOOLS and manually tries clean absolute PATH
entries. Skipped entries count toward 64. ENOENT/ENOTDIR continue, EACCES is
remembered, other errors stop with 126. Target streams are inherited, stdin
is null, environment is inherited already owner-filtered by the future caller.
No environment filtering or identity switching is invented here. Status uses
the exit code or 128+signal. Failures expose fixed codes only.

Probe changes to /, uses the same resolver, and emits only fixed-table JSON
lines. Each version pipe is nonblocking, retains at most 4096 bytes, performs
one bounded read per tick and has a five-second deadline even after leader
exit if a pipe remains held. A guard kills/reaps the immediate child on all
returns/unwind. Found remains true for a started slow/invalid-version tool.
Versions use only a trimmed first line, 1–128 printable ASCII bytes excluding
slash/backslash/NUL. Nine worst-case escaped versions fit below 4096 bytes;
an explicit aggregate check precedes writing. No stderr writer or sudo probe.
Synchronous spawn, filesystem calls and kernel kill/reap retain the ordinary
OS-stall limitation; this is not a hard real-time or descendant-containment
claim. The later runner supplies containment separately.

## Tests and scanner audit

Unix S1 checks target-only bytes, exit, stdin EOF, inherited synthetic
environment, cwd and argument fidelity. S2 checks missing/non-executable
targets, absent/link/dot/absolute cwd, reserved malformed prefixes, relative
targets and oversized argv. S3 requires 137. S4 checks absolute resolution,
empty/relative/unsafe skipping, the 64-entry edge, non-TOOLS, no path fallback,
EACCES memory and continuation to a later executable. S6 checks slash/NUL,
five-second termination of a ten-second exec-replaced sleeper, found=true,
reaping via scalar ESRCH, root cwd, fixed order and output bounds. Fixtures
are ordinary-current-user owned temp directories; no detached descendants.
Their watchdog kills only its still-unreaped owned fixture process group on
expiry. Every process test skips before spawn on root.

S5 tests the same pure identity gate used by production with injected zero:
125 without evaluating its operation. No root child or identity change.
S7 reexecutes only the ignored test fixture with an environment cleared to
test flags and backtrace unset/1/full. A test-only panicking closure in the
real common entry must return 125 without stderr or stdout marker. The
parallel parent never changes its hook. Additional version unit tests cover
invalid bytes/lengths and trimming. These Unix tests are authored, not run here.

Before extending P-SCAN, the six F2 forms were manually audited: no UFCS or
function-value unwrap, raw unwrap, renamed logging import, RunFailure alias
From, or stderr writeln exists in shim production. Harmless unwrap_or and
unwrap_or_default carry no panic payload. The scanner now rejects unwrap and
logging identifiers in any position, RunFailure type aliases and writeln,
with all six negative controls. Hook count must be exactly one in the shim
and zero in the other four production files. Fixed payload inventory remains
the four accepted Stage B unreachable literals, with no shim panic sites.
Only a structurally trailing test module is excluded. Legitimate probe stdout
uses serde_json/write_all, never logging macros. This remains a lexical check,
not a Rust semantic privacy proof; aliases through arbitrary wrappers, implicit,
dependency and aborting panics and real journal/log privacy remain outside it.

## Own validation, failures and unrun work

- cargo fmt --all; rustfmt --edition 2024 crates/arxd/src/unix_shim_tests.rs;
  final cargo fmt --all --check and git diff --check: passed.
- cargo clippy --locked --workspace --all-targets -- -D warnings: initial
  and final Windows passes. Final log target/stage-c-clippy-final.log.
- Focused filter `test(exec::stage_b_tests::p_scan) | test(unix_shim::tests::s5)`:
  first run `61e05b60-4a58-4a9b-bf56-27da7a7e80e9`, 3 pass/1 fail, because
  an overbroad stderr identifier rule rejected existing fields. Replaced that
  rule with a writeln identifier prohibition. Corrected run
  `5a120636-b555-4711-8764-c12f29661d7a`: 4 passed, 305 filtered.
- cargo check --locked -p arxd --target x86_64-unknown-linux-gnu: exit 101,
  stopped at libsqlite3-sys/cc-rs before arxd: missing x86_64-linux-gnu-gcc.
  Log target/stage-c-linux-check.log. No install, stub or bypass.
- Exactly ONE final normal default-profile/default-concurrency workspace run
  after final source: cargo nextest run --locked --workspace at 656dde3,
  `be7440d0-0f61-4a24-8201-90a42bf1f5c5`: **308 passed, 2 skipped**, 10.608s.
  Log target/stage-c-workspace-final.log. Existing SDK and exact service-run
  assertions passed. No instrumentation, retry, stress or second full run.
- Exploratory file searches included missing-path and PowerShell glob/brace
  syntax errors; these supplied no validation credit.

No compiled Unix mutation was possible. Path-mode PATH fallback (S4), cwd
symlink following (S2), signal-to-1 (S3), hook/catch removal (S7) and probe cwd
mutation (S6) remain **unrun**, with zero mutation-kill credit. Windows
include_str scanning is source-data validation only, never Unix compile or
mutation credit. Linux/macOS build, Clippy, tests and mutations await hosted
or another authorized Unix validation environment. A Windows cfg-excluded
Unix integration binary contributes no behavioral credit.

Inherited only: Stage A/B acceptance, Stage B hosted Linux 243/3 and macOS
244/1 and R1/L1 evidence; #52 five-gate/independent-review merge. Not rerun
or reverified through GitHub here. Main after #52 is not claimed green.

Fresh Opus 5.5 high source/security review is required BEFORE publication;
all five hosted checks at the actual candidate head are mandatory before
coordinator merge. No push/PR/watch/dispatch/settings/babysitter operation.
No owner argv, NativeOps/create, root runner, cgroups, account/service/SCM/LSA,
installed binary, VM/Lab Manager, ignored native fixture, private state or
experiment was exercised. Existing benign workspace fixtures alone ran.
ExecutionFailed and E_PROTECTION remain unknown-phase/unresolved; a pass
is no fix or waiver. Native, installed and dedicated paths remain
**unverified on real hardware**; M2 is incomplete.
