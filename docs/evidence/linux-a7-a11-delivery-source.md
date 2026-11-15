# A7/A11 dormant delivery source evidence

Implementation: OpenAI / gpt-6.1-sol / high, 2026-11-15.
Source commit: `7cfb816` (`test(m2): prepare dormant fixed A7 A11 delivery`).
Prepared parent `6eea816`, main base `17a2cb47306f73730c2ce990f4661094c4d01f3b`.
Read AGENTS/KICKOFF, accepted plan and independent plan/source reviews,
delivery contract and independent Muse contract review (`2eb1810` / `050466c`),
and binding corrections `46f129c` plus final-newline follow-up.
A7/A11, M2, full F/G and real-Runner PA6 remain **unverified on real hardware**.

## Scope and admission

Only the four permitted source/test files and this evidence file change.
No PROGRESS, ADR, Cargo, helper, package, workflow or production rights algorithm
change. No host hash, future source SHA or policy attestation is invented.
The fixed reader is private and test-only through the existing Linux disposable
module. `prepared_delivery` and each per-call recheck retain literal `None`
profile expectations; refusal precedes even environment/filesystem observation.
Actual host qualification remains False, policy qualification False, and TEST
mapping closed. No new entry point, environment selector or CLI selection exists.

The private reader derives the run's stage from the controller executable's exact
layout, admits one strict bounded schema, binds source/workflow/reviewed/run/profile,
derives both OwnerConfigs and executable paths, validates root-only ancestry and
holds opened descriptors. Bounded NSS lookups revalidate nologin, pinned IDs,
sole primary group and empty supplementary membership. It compares the controller libtest bytes with reentry.
Immediately before EACH Runner call, it reopens and fstats pinned executable
identities and rechecks base/workspace ownership, mode and inode. RunSpec.cwd is
the derived workspace. Only root-writable ancestors make path-based exec after
the recheck safe; generated accounts have no write bit on those ancestors.

Dormant driver functions perform a single test build and single production build,
share 360 seconds and 4 MiB, reject duplicate or misplaced Cargo outputs, and copy
from held descriptors into an exclusive root-owned 0755 directory outside the
0700 stage. Copies use O_EXCL and destination rehash plus source rehash. The
single test artifact is recorded as the harness and copied as reentry. Partial
copies are captured for owned cleanup or quarantine. No sudo executable is copied.
Handoff remains policy-unattested before effects. These functions have NO edge
from run_body, service, main or the closed selector. Existing runtime setup,
MainPID/READY/ack, stop, reservation, account cleanup and bounds remain unchanged.
Future authorized wiring must call existing stop/cgroup confirmation and account
cleanup (including live-UID refusal) before delivery cleanup, retain reservations
on any uncertainty, and close captured descriptors in finally. No native wiring
or activation is claimed in this dormant slice.

## Call-graph audit and actual checks

Before commands, read driver entry/selector/run_body/harness/cleanup, controller,
Runner field definitions and existing source-only extraction. Python delivery
tests mock filesystem, account and process ports; build tests use BytesIO streams
and fake Popen, never Cargo. Extracted Rust contains only schema/layout/scalar
functions and tests, linked against existing locked offline serde dependencies.
It excludes the reader, libc adapters, LinuxFs, Runner and ignored fixtures.
Portable E4 uses its existing synthetic test harness; inventory reads source.

| Command/check actually run on Windows | Result |
| --- | --- |
| focused SudoNnp/SudoNnpDelivery/A4 closed mapping | PASS, then expanded through full discovery |
| `python -m unittest discover` in tools/unix-exec-validation | PASS 126 tests (previous 120 plus 6 delivery); internal=757, outer=2447 |
| `python -m unittest test_driver` in tools/ci/e4-native | PASS 22 portable tests |
| `mutation_check.sudo_nnp_delivery_checks()` | PASS 1 extracted Rust test, 6 mocks, 19 Rust and 5 Python assertion-killed mutants; source bytes unchanged |
| `mutation_check.sudo_nnp_source_checks()` | PASS 4 extracted Rust tests, 9 mocks, 45 assertion-killed mutants; source bytes unchanged |
| `cargo test --locked --offline -p arxd --lib exec::stage_b_tests::w8_e7_crate_wide_inactive_source_inventory -- --exact` | PASS 1 test |
| `cargo clippy --locked --offline -p arxd --all-targets -- -D warnings` | PASS, Windows target only, 29.42s |
| `cargo fmt --all -- --check`, `git diff --check` | PASS |

Initial new mock had an unpatched Windows platform; corrected the mock and reran.
Mutation checking initially required an assertion marker absent from a custom
Rust panic message; corrected the message, no acceptance relaxation. Full guard
discovery caught an extra protected-site inventory entry; switched the new build
check to its root-down descriptor opener, preserving the existing 14-site guard.
The old mutation anchor became ambiguous after adding a second stop-gated cleanup;
bounded it to the original fixture section. New shared-budget tests kill capture
reset, deadline reset, partial-write acceptance, destination hash bypass and source
identity bypass. Existing collision, partial-account, cleanup-failure, stop and
policy-closed tests are retained. No test failure is credited as a native attempt.

## Deferred gates

No native/helper/ignored fixture, LinuxFs, account, sudo, admin, process probe,
network, install, dispatch, variables, push, PR or subagent operation ran.
Linux integration was not compiled: this Windows session has no established Linux
cross-GCC build path; no installation or compiler probe was attempted. Windows
Clippy does not type-check the Linux-only reader or controller. Hosted Linux
compilation and fresh independent Muse source review remain required before
publication; no review was delegated under this sub-agent's no-subagent scope.
Separate read-only qualification grant, exact host/policy attestation, execution
grant and green required checks (including agent-review and ci-time-budget)
remain required. No A7/A11/M2/full-F/G/PA6 native credit is claimed.
Existing E4 production/source pin inputs are unchanged; no pin refresh is needed.

## Binding review corrections (verbatim)

1. JSON-to-gate binding. Schema list alone does not bind the JSON. Require:
   JSON `run` equals `gate`/`identity` (`GITHUB_RUN_ID-GITHUB_RUN_ATTEMPT`);
   `source_sha` equals `GITHUB_SHA == ARX_REVIEWED_SHA == ARX_WORKFLOW_SHA`;
   `host_profile_sha256` equals the independently reviewed attested profile
   digest for this exact host/image. Any mismatch refuses before effects.
2. Reentry-to-harness identity. "Libtest copy is both E4 caller and the same
   build as controller" is ambiguous across the two cargo invocations.
   Require: the staged libtest artifact comes from the single
   `cargo test --locked --lib --no-run --message-format=json` invocation that
   also produced the harness binary, and its bytes equal the harness artifact
   bytes (hash equality checked before copy and rechecked after copy);
   the staged arxd artifact comes from the single
   `cargo build --locked --bin arxd --message-format=json` invocation as the
   one non-test artifact. State the copy direction (held-fd read, `O_EXCL`
   create, destination byte reverify) for both.
3. Exact `OwnerConfig`/`RunSpec` field names. "Private roots=[stage]" and
   "cwd/rights config" do not name code fields. Require:
   `account=name`, `uid`/`gid`=captured `>=1000` distinct across cases,
   `workspace=/var/lib/<name>/workspace` derived (never from JSON),
   `protected_directories=[stage]`, `environment={PATH:/usr/bin:/bin}`,
   `admin=false`, `cgroup_parent=/sys/fs/cgroup/system.slice/arxd.service/runs`,
   `memory_bytes=64MiB`, `pids=32`, `RunSpec.cwd=workspace`,
   `timeout=20s`, `output_bytes=4096`; recheck owned base/workspace
   dev/ino + owner + `0700` immediately before each Runner call.
4. Executable admission TOCTOU scope. "Hold descriptors; recheck named identity"
   is insufficient unless the recheck is an `O_NOFOLLOW|O_CLOEXEC` open plus
   `fstat` dev/ino/mode/uid/nlink match against the pinned JSON immediately
   before each Runner call, and `protected()` covers every ancestor including
   `/usr`, `/usr/libexec`, and the exclusive
   `/usr/libexec/arxcovo-a7-a11-<run>-<attempt>` dir itself (root-owned `0755`,
   no pre-existing adoption). State the load-bearing assumption: only
   root-writable ancestry makes path-based exec after recheck safe; build
   accounts hold no write bit on any ancestor.

## Source Git blobs

| File | Git blob |
| --- | --- |
| `crates/arxd/src/exec/disposable/sudo_nnp.rs` | `04e619e652260f64c4b91c11ee30de33096430ba` |
| `tools/unix-exec-validation/driver.py` | `459fae45675c861ce4545fc1d369b95f942648cc` |
| `tools/unix-exec-validation/test_driver.py` | `1eb241067626a30261d7e2215ab241c7c64160ff` |
| `tools/unix-exec-validation/mutation_check.py` | `18b15aaa8eebb26eb7fbd886e70afcb2fe455f16` |
