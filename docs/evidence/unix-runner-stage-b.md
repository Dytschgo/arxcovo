# ADR 0019 Stage B runner evidence

Builder: OpenAI / gpt-6.1-sol / high, 2026-11-12. Implementation sub-agent,
no subagents. This is a local candidate, not independent review or acceptance.

## Pins and scope

- Runtime-prepared worktree top-level matched the supplied working directory,
  was clean at exact base `6d474b56ad88e90aff8441bf3bf64d27e65c8078`, and is
  distinct from the canonical checkout. No checkout/worktree was created.
- Actual-main reference supplied by coordinator: `064736022e559d0fb46a1de3943e57212eca87b0`
  (#51 inactive contract merged 14:02:39 UTC through all five gates and review).
  No GitHub operation or new hosted status verification occurred here.
- `git diff 1d828ca7 HEAD -- crates/arxd/src/exec` was empty before edits.
  Re-pinned current main owner route (#50), ConfigureRequest (#48), executor
  Windows path predicate (#48), Report/Creation/Evidence and native composition.
  These surrounding sources and their accepted tests remain unchanged.
- Read AGENTS/KICKOFF, ADR 0019 C1-C3 and D1-D7, complete implementation brief
  including classifications, tests, mutations and staging, retained Sol reviews
  and Opus B1 recheck, and relevant ADR 0017 types/creation/privacy requirements.
- Manifest: `exec/mod.rs`, `exec/unix.rs`, `exec/cgroup.rs`, new
  `exec/stage_b_tests.rs`, this evidence and scoped `docs/PROGRESS.md`.
  `exec/output.rs`, existing `exec/tests.rs` and `exec/disposable.rs` unchanged.
  Cancellation re-exports the existing development type; no development or
  lib.rs change is necessary. No dependency, Cargo, CI, schema, packaging,
  owner CLI, config or native production composition change.

## Failure-site inventory

Site numbers below are the contract's original unchanged runner source pin;
current source names identify the new explicit classifications. No From impl
exists for RunFailure. It derives nothing and is never formatted. Its new/os
constructors retain the original legacy ExecError, including errno fallback -1.
Only run_phased drops that legacy member and returns the fixed phase enum.

| Site / operation | Phase | Exact legacy value retained |
| --- | --- | --- |
| mod.rs:136 legacy spec check | Invalid (phased validator only for phased entry) | Three distinct Invalid literals: absolute executable and workspace-relative cwd required; argv bounds; timeout/output bounds |
| mod.rs:144 unsupported backend | Unsupported | Unavailable("execution backend unavailable on this OS") |
| unix.rs:143 real/effective non-root check | ServiceIdentity | Unavailable("root service identity required; no owner fallback") |
| account:65 CString | AccountPolicy | Invalid("account") |
| account:79 getpwnam_r failure/missing | AccountPolicy | Unavailable("pinned account missing or lookup failed") |
| account:85 changed UID/GID | AccountChanged | Unavailable("pinned identity changed") |
| account:98 supplementary membership/expansion | AccountPolicy | Unavailable("supplementary group membership forbidden") |
| account:114 primary group lookup/kind | AccountPolicy | Unavailable("dedicated account-named primary group required") |
| account:122 trusted_root walk/fstat/permission errors | PrivateRoots | OperatingSystem(errno or -1) |
| account:125 fstat | PrivateRoots | OperatingSystem(errno or -1) |
| account:129 directory policy | PrivateRoots | Unavailable("private directory is not root-only") |
| unix.rs:149 early check and final check | Shutdown, else Cancelled | Unavailable("run already cancelled or shutting down") |
| unix.rs:172 macOS getrlimit | Containment | OperatingSystem(errno or -1) |
| unix.rs:176 macOS descriptor cap | Containment | Unavailable("descriptor limit exceeds safe inheritance closure") |
| Group::create:35 missing delegation | Containment at caller | Unavailable("verified Linux cgroup delegation required") |
| Group::create:38 lexical path | Containment at caller | Invalid("cgroup path") |
| Group::create:41 trusted_root | Containment at caller | OperatingSystem(errno or -1) |
| Group::create:45-49 fstatfs/fstat | Containment at caller | OperatingSystem(errno or -1) |
| Group::create:54 filesystem/ownership/mode | Containment at caller | Unavailable("cgroup parent must be protected root-owned cgroup2") |
| Group::create:58 controllers text; :62 procs text | Containment at caller | OperatingSystem(errno or -1), including bounded-read refusal |
| Group::create:64 controllers/empty policy | Containment at caller | Unavailable("empty delegated subtree with pids+memory required") |
| Group::create:73 mkdirat; :84 openat | Containment at caller | OperatingSystem(errno or -1), existing unlinkat order unchanged |
| Group::create:90-97 setup pids.max, memory.max, memory.swap.max, kill/procs opens | Containment at caller | OperatingSystem(errno or -1), existing cleanup order unchanged |
| unix.rs:186 socketpair | Resource | OperatingSystem(errno or -1) |
| unix.rs:305 command.spawn | SpawnFailed | OperatingSystem(errno or -1) |
| unix.rs:319 identity read_exact | AfterSpawn | OperatingSystem(errno or -1) |
| unix.rs:323 identity mismatch | AfterSpawn | Unavailable("child identity attestation mismatch") |
| unix.rs:331 stdout take; :336 stderr take | AfterSpawn | Unavailable("stdout pipe"), Unavailable("stderr pipe") |
| unix.rs:337-338 nonblocking | AfterSpawn | OperatingSystem(errno or -1) |
| unix.rs:342-343 both loop drains; :352 waitid | AfterSpawn | OperatingSystem(errno or -1) |
| unix.rs:363 try_wait; :368-369 final drains | AfterSpawn | OperatingSystem(errno or -1) |

`account(c)?` passes its already-classified failure unchanged. Every pre_exec
return/?: setpgid, descriptor closure, cgroup join, prctl, setgroups, ID drop,
capability clear, identity/group/private-root checks, rlimit, cwd open/fchdir,
report write is an io::Error inside Command::spawn and therefore SpawnFailed.
No child errno is used to infer a narrower phase. trusted_root/walk helpers
inherit PrivateRoots from account and Containment from Group::create.
Nonblocking/drain/waitid helper errors inherit AfterSpawn. Cleanup's discarded
kill/reap/unlink failures remain best effort; no new cleanup proof is claimed.

Source placement: identity refusal precedes account lookup; account precedes
retained early check; cgroup/socket/Command/pre_exec construction precede final
check immediately before spawn. Neither token is polled between spawn and
identity read. Loop shutdown combines caller and runner tokens. Runner::drop
sets only its own token; legacy run passes no caller shutdown.

## Compatibility and panic boundary

Exactly the three accepted C1 deviations: (1) cancellation in the final race
window refuses before spawn using the existing literal; (2) Cancellation's
public type identity is shared additively; (3) unreachable panic payloads lose
error formatting. No lossy reverse mapping or legacy argv allowance is added.

Phased argv permits only exec-child/path-or-tool/target/cwd, target 1..=4096,
cwd <=4096, prefix NUL refusal, then legacy 128 item/65536 byte/8192 per-item
NUL-free tail. All other specs retain legacy bounds. Common executable/cwd,
timeout/output validation still runs. Legacy entry tests call actual run for
both 4+128 items and 4+8x8192 bytes, requiring exact Invalid("argv bounds").

Six expect sites: unix components/workspace/cwd and cgroup numeric name now
use fixed literal unreachable; the two four-byte identity conversions use
fixed array construction, a panic-free equivalent. Checked inventory is four
sites: cgroup unreachable "numeric name"; unix unreachable "validated cwd";
unix unreachable "validated path" twice. The scanner tokenizes all four
production files including Unix sources on Windows, strips nested comments,
preserves quoted tokens and structurally excludes only a final inline tests
module. Production after that module is rejected. External test declarations
do not truncate the scan. Negative controls include format leaks, expect and
unwrap variants, forbidden macros/hooks/From and blind test-module truncation.
Inventory equality and a vacuity mutant establish nonempty coverage.
No hook is installed or swapped. No dependency, implicit/aborting panic,
backtrace or real-log sanitation guarantee is claimed.

## Validation ledger

All commands ran with the explicit supplied workdir. No custom environment,
stress, instrumentation, native child experiment or ignored-fixture invocation.

- Initial cargo fmt failed because the newly declared test file did not yet
  exist; added it and formatting passed.
- Initial locked workspace all-target Clippy failed: unused non-Unix spec and
  collapsible_if in scanner. Corrected both. Second Clippy found collapsible_match
  in scanner; corrected it. Third Clippy passed (exec session 95543).
- Focused `cargo nextest run --locked -p arxd -E 'test(exec::)'` passed 14/14,
  293 filtered, run `ec3f34da-02ba-4a2c-ad3e-4724aa553777`. This preliminary run
  preceded the final restoration; final workspace verification is recorded below.
- Mutation details, final checks and source manifest follow below.

## Limits and handoff

Unix R1/L1 marker executable test is compiled/run only on a normal Unix host;
it returns before Runner invocation when euid is zero. L2b has the same root
skip before constructing/calling Runner. Wrong-validator mutation ran here on
Windows and can reach only Unsupported after pure validation; on non-root
Unix it can reach only ServiceIdentity, before account lookup/cgroup/spawn.
No actual build identity lookup or privileged runner site was exercised.

Root-only cgroup/spawn/post-spawn and placement mutants are source-reviewed
only, pending separately authorized experiments. Real caller-shutdown loop and
Unix marker execution refusal have no local runtime credit on Windows.
Native, installed and dedicated paths remain **unverified on real hardware**;
M2 is incomplete. Historical results at old pins are not new evidence.

The unexplained NaturalExecutionFailed and E_PROTECTION run 43091339273 remain
unknown-phase failures with unknown cleanup, no fix or waiver. No retry,
owner/admin/account/SCM/LSA/Guard/session, system hierarchy/pipe, VM/LabManager,
private-state, S4U/password fallback, babysitter or external operation occurred.
Stages C/D/E/F/G and production caller/constructor activation are not built.
Fresh Anthropic / Opus 5.5 / high independent review and all five hosted gates
are required after this candidate, before publication by the coordinator.

## Final candidate and executed mutation results

Source head: `4fe2bde4fddef381fc54dab5688f7300ad9e354e`. All source
mutations were restored byte-for-byte before this commit and final checks.
No source edits followed the final full run; the following commit is docs only.

Each compiled mutant ran `cargo nextest run --locked -p arxd -E
'test(exec::stage_b_tests::<named filter>)'` with unchanged normal environment.
Every compiled case below ran one test, failed its assertion, and exited 100.

| Mutation | Named test filter | Result | Nextest run ID |
| --- | --- | --- | --- |
| legacy-phased-validator | l2b_actual | compiled, assertion failed | 907bc524-7fcf-4329-8235-71404b4f5414 |
| lossy-legacy-validation | l1b_exact | compiled, assertion failed | 7f0e4485-1b67-48bc-bfc4-0e9e2b6607ff |
| caller-shutdown-ignored | r2_all | compiled, assertion failed | 6e7b63ce-fe99-4d43-a906-fe5645b4b497 |
| cancel-before-shutdown | r2_all | compiled, assertion failed | 552bfa05-e80e-4d9e-8a4a-5b9bd1abaf7d |
| blanket-argv-allowance | l2_exact | compiled, assertion failed | 7d4427aa-598b-4ee6-bff3-dd7f851a2955 |
| from-runfailure | p_scan_checked | did not compile (no kill credit) | none |
| from-runfailure-compiled | p_scan_checked | compiled, assertion failed | 405d7ef9-492e-4ee2-831a-bc0b526f65ce |
| expect-restored-unix | p_scan_checked | compiled, assertion failed | c6aa5c39-9b4e-4c5f-8afc-f4ce0675e45b |
| inventory-new-literal | p_scan_checked | compiled, assertion failed | 3f327cb6-9e89-47e7-8a00-cc5a5395a645 |
| inventory-vacuity | p_scan_checked | compiled, assertion failed | e6088c40-3f46-4eda-bd3d-b3fed749dc25 |

The first From mutant exited 101 (E0599): it called Self::os, which is
Unix/test-gated and unavailable in the non-test Windows library. The corrected
mutant calls Self::new(..., e.into()), compiles, and fails P-SCAN. This is a
mutation-harness correction, not a product fix or a claim for the first mutant.
The restored expect and cgroup literal mutations compile the Windows test
binary with include_str source data; the Unix production branch itself is not
compiled locally. Early-check removal/final-check placement mutants remain
root-only and unexecuted. Later C/D bridge/shim mutations are outside Stage B.

Both mutation scripts used finally restoration, checked every original file's
SHA256 and stopped on an unexpected outcome. Restored checkout byte hashes
(CRLF checkout where applicable; distinct from normalized Git blobs):

| Source | Checkout SHA256 | Git blob at source head |
| --- | --- | --- |
| mod.rs | `23975c0f8c781b5b9edbaf7ee3023aaca243f021b92fab7699b00b277f0608c5` | `e94549e81bd41f12c16e94ab2af3aeea1e841c59` |
| unix.rs | `4dbe55e1e3d67db611ca54624424b30e40308c52d465ca1e6e9c7be096c07258` | `d3ec3e7e26a0d4201e462cb7b966c8b0d48e3b22` |
| cgroup.rs | `e0ab2943f36b76b3b10864f6bab5d37e1b41fa4ea0999bdfed6ed0a15cb246ff` | `f6c65008a7bbb5bbbe0b053cb49a7f92a8405de3` |
| stage_b_tests.rs | `b1e386356989aa8a37dc4333a3c29b01cf1f0da2853515c23a6a2a2860f5565e` | `d4d029445366b4d584decd42797f7d6099ef0123` |

Final checks after restoration:

- `cargo fmt --all -- --check`: exit 0.
- `git diff --check`: exit 0. Git reports expected CRLF-to-LF normalization,
  with no whitespace errors.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: exit 0,
  final exec chunk c347d5, 2.43s build time.
- Exactly ONE final `cargo nextest run --locked --workspace`, normal default
  concurrency/profile, after final source/restores: exit 0, **306 passed,
  2 skipped**, 11.318s, run `fc80b434-d0e7-45ef-a419-a2b5d65a229c`.
  No retries, relaxed assertions or timeouts; no new ExecutionFailed observed.
- `cargo check --locked -p arxd --lib --target x86_64-unknown-linux-gnu`:
  exit 1 in libsqlite3-sys build, missing `x86_64-linux-gnu-gcc` (ToolNotFound).
  No compiler/toolchain installation or substitute typecheck was attempted.
  This supplies **no Unix compile credit**. Linux/macOS Clippy/tests, R1/L1
  marker refusal and Unix-native branches remain pending hosted verification.
- Final source scope diff against base is empty for main, service,
  development, lib.rs, Cargo/lockfile, CI and packaging. Existing exec tests,
  ignored disposable fixtures and output implementation remain unchanged.

Full independent review, hosted all-five-gate acceptance and publication are
coordinator work, not performed by this sub-agent. No PR or external write.
