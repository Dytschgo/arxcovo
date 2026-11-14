# S1 failed Locate and live check: local evidence

Builder: OpenAI / gpt-6.1-sol / medium, 2026-11-14.
Base: `f56972832ef3e491994d5d29fadc30f0c7ad3e17`.
Source/tests commit: `be19717e78bcdc516c60ec7cd6cdd263a5350ffc`.
Scope: only accepted S1 in `docs/reviews/m2-remaining-gates-proposal.md`.
Status: local Windows preparation passed; ready for fresh Opus high source
review. Hosted Linux and the normal hosted gates remain required before delivery.
M2 remains incomplete. Native, installed and dedicated paths are
**unverified on real hardware**.

## Isolation and inputs

Initial cwd and Git top level both resolved to `<isolated-worktree>`; status
was clean and HEAD matched the requested base. No canonical checkout or private
state was accessed. All paths here are repository-relative; private paths are
represented only by placeholders.

Read AGENTS, KICKOFF, ADR 0020, brief 0020, the accepted S1 proposal, E6/E7
source and independent security reviews, linux-discovery-e7 and
linux-discovery-followup evidence, and the dytschgo-engineering skill plus its
code-improvement safeguards. The task's narrower instructions control: no
subagents, PROGRESS changes, hosted operations or owner actions.

## Implementation and assertions

Only cfg(test) constructors change. StartupDomain accepts an optional Location
and an owned live check. AdmissionDomain and UnixBridge accept optional parents.
None refuses before derive/factory and stores terminal Setup before any handle
can escape; its setup closure is absent. No placeholder OwnerConfig is built.
The pure wiring_location helper lives inside the existing cfg(test) tests module;
it takes fake ProcFile results, validates PROC_MAGIC, and uses shared locate.
No production Locate observation or NativeResolver was added.

L-a covers absent unified line, v1, wrong unit, 4097 bytes, wrong magic and port
error. Each checks zero factory/setup calls, absent setup closure, exact
MechanismUnavailable preflight, no daemon rows after start_discovery, a refused
agent preflight_failed row, and terminal refusal after set_ready. The fake port
panics if called. Factory absence plus that panic guards zero Runner calls;
setup absence prevents entry into its janitor.

L-b and L-b2 use AccountIdentityChanged, distinct from MechanismUnavailable,
and count repeated check invocations. Located readiness remains pending until
setup; after setup the refusing check still wins. Missing location has zero
factory/setup calls. This proves the live check runs FIRST only, not the relative
order of later identical refusal tokens. W1 retains its existing event-log
assertion and same-parent/same-domain assertions, with an owned closure invoking
execution_preflight using FakeResolver on each call (L-c).

One Readiness allocation and one AdmissionDomain construction in StartupDomain,
and one UnixBridge construction in AdmissionDomain, remain pinned. W8's exact
E7 test-call inventory changes from three to four for the single added S1 test
helper; no allowlist or scan boundary is widened. E6 ownership/slot pins and the
crate-wide source inventory pass unchanged. Existing configured and preflight
precedence are unchanged.

## Final source hash manifest

All four files contain LF bytes, with no CR. SHA-256 values:

| Path | Bytes | SHA-256 |
| --- | ---: | --- |
| `crates/arxd/src/service/unix_bridge.rs` | 15223 | `f41377fd15bc2e95e11778f882e6f9f6ffcccfa7f67facc02e86f6de1266364b` |
| `crates/arxd/src/service/unix_bridge/tests.rs` | 34871 | `63f494f0e4ba94d66df27d72e3e72fc9196917b5746c5af21a48bc6c0e5f9c9b` |
| `crates/arxd/src/service/unix_bridge/tests/e6.rs` | 20450 | `1f50ea88cf0e7aea5557a6e42be8f2cd180bdcb8cda0953bd32a98f752e3642b` |
| `crates/arxd/src/service/unix_bridge/tests/e7.rs` | 34423 | `56c68bd801f205a8b43a4dca66c1649230e68d0ec55a6ff57e498e8e3014072d` |

Removing cfg(test) items (including test constructors, structs, impls, imports
and modules, plus their separating newline) from unix_bridge.rs yields identical
bytes at base and final source. SHA-256 of both stripped streams:
`117543e91b84bb7ed219b6ad7ce0cfc3b39506cbe6beb28db663b4deb35089a0`.
The other changed files are entirely inside the existing test module.
This is a bounded source comparison, not a Linux execution claim.

## Final validation

Final validation tool session `68717`, after all mutants restored and before
source commit; exact source bytes are the manifest above. No source changes
followed these checks. Commands and observed summaries:

- `cargo fmt --all -- --check`: exit 0. Passed, no output.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: exit 0.     Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.85s
- `cargo test -p arxd --lib --locked service::unix_bridge::tests`: exit 0. test result: ok. 36 passed; 0 failed; 1 ignored; 0 measured; 316 filtered out; finished in 2.49s;     Finished `test` profile [unoptimized + debuginfo] target(s) in 5.37s
- `cargo test -p arxd --lib --locked exec::stage_b_tests::w8_e7`: exit 0. test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 351 filtered out; finished in 0.09s;     Finished `test` profile [unoptimized + debuginfo] target(s) in 0.28s
- `cargo nextest run --workspace --all-targets --locked`: exit 0.     Finished `test` profile [unoptimized + debuginfo] target(s) in 5.43s;  Nextest run ID 46093253-d705-4522-a1a4-5c654e25c1a9 with nextest profile: default;      Summary [  12.361s] 389 tests run: 389 passed, 3 skipped

`git diff --check` also passed before the source commit. The final full nextest
run ID above is the final acceptance run; 389 passed and 3 skipped on Windows.
This gives no Linux-gated compilation or execution credit. Existing normal
Windows tests ran as part of the requested suite; no ignored fixture was invoked
directly and no privileged/native activation experiment was dispatched.

## Mutation controls

Final mutation tool session `7582`. Each command selects exactly one named test.
Each mutant compiled (Finished test profile), executed, and exited 101 at the
named assertion. No compile error is counted as a kill. Every edit was a
single exact replacement (one occurrence), written as UTF-8 LF bytes. A finally
block restored the original bytes and asserted exact byte equality; restored
SHA-256 was checked after each run. The following data records the exact
replacement, mutant hash, restore hash, command and assertion output. These
runs use the final source manifest; no later source edit occurred.

### none_calls_factory

```json
{
  "name": "none_calls_factory",
  "path": "crates/arxd/src/service/unix_bridge.rs",
  "command": "cargo test -p arxd --lib --locked service::unix_bridge::tests::e7::s1::l_a_missing_location_matrix -- --exact",
  "compiled": true,
  "killed": true,
  "exit": 101,
  "old": "let runner = parent\n",
  "new": "let runner = parent.or_else(|| Some(\"/synthetic/runs\".into()))\n",
  "mutant_sha256": "5e0e61cdf93faac5d38c8332f4007fa2606225cb85232a8f267227f8dfc375e5",
  "assertion": "---- service::unix_bridge::tests::e7::s1::l_a_missing_location_matrix stdout ----\n\nthread 'service::unix_bridge::tests::e7::s1::l_a_missing_location_matrix' (43656) panicked at crates\\arxd\\src\\service\\unix_bridge\\tests\\e7.rs:869:13:\nassertion `left == right` failed: L-a no factory\n  left: 1\n right: 0\nnote: run with `RUST_BACKTRACE=1` environment variable to display a backtrace",
  "restore_sha256": "f41377fd15bc2e95e11778f882e6f9f6ffcccfa7f67facc02e86f6de1266364b"
}
```

### none_leaves_pending

```json
{
  "name": "none_leaves_pending",
  "path": "crates/arxd/src/service/unix_bridge.rs",
  "command": "cargo test -p arxd --lib --locked service::unix_bridge::tests::e7::s1::l_a_missing_location_matrix -- --exact",
  "compiled": true,
  "killed": true,
  "exit": 101,
  "old": "            readiness.fail(Cause::Setup);",
  "new": "            // pending",
  "mutant_sha256": "318c6fb1e9cd5b123776178ca686b37540fdd72a43cfc70b081ac2aa0c459bee",
  "assertion": "---- service::unix_bridge::tests::e7::s1::l_a_missing_location_matrix stdout ----\n\nthread 'service::unix_bridge::tests::e7::s1::l_a_missing_location_matrix' (48352) panicked at crates\\arxd\\src\\service\\unix_bridge\\tests\\e7.rs:904:13:\nL-a never Ready\nnote: run with `RUST_BACKTRACE=1` environment variable to display a backtrace",
  "restore_sha256": "f41377fd15bc2e95e11778f882e6f9f6ffcccfa7f67facc02e86f6de1266364b"
}
```

### none_runs_setup

```json
{
  "name": "none_runs_setup",
  "path": "crates/arxd/src/service/unix_bridge.rs",
  "command": "cargo test -p arxd --lib --locked service::unix_bridge::tests::e7::s1::l_a_missing_location_matrix -- --exact",
  "compiled": true,
  "killed": true,
  "exit": 101,
  "old": "        let valid = constructed.get();",
  "new": "        let valid = constructed.get();\n        if location.is_none() {\n            let _ = setup(&locate(b\"0::/arxd.service/supervisor\\n\").unwrap(), &Cancellation::default());\n            return Self { domain, readiness: state, setup: std::sync::Mutex::new(None) };\n        }",
  "mutant_sha256": "f026e6c516d925a3efc66df0848c4f37c25b3a1971ceadf6ade828c53b8f8461",
  "assertion": "---- service::unix_bridge::tests::e7::s1::l_a_missing_location_matrix stdout ----\n\nthread 'service::unix_bridge::tests::e7::s1::l_a_missing_location_matrix' (37828) panicked at crates\\arxd\\src\\service\\unix_bridge\\tests\\e7.rs:902:13:\nassertion `left == right` failed: L-a no setup or janitor\n  left: 1\n right: 0\nnote: run with `RUST_BACKTRACE=1` environment variable to display a backtrace",
  "restore_sha256": "f41377fd15bc2e95e11778f882e6f9f6ffcccfa7f67facc02e86f6de1266364b"
}
```

### check_ignored

```json
{
  "name": "check_ignored",
  "path": "crates/arxd/src/service/unix_bridge.rs",
  "command": "cargo test -p arxd --lib --locked service::unix_bridge::tests::e7::s1::l_b_live_check_first_with_and_without_location -- --exact",
  "compiled": true,
  "killed": true,
  "exit": 101,
  "old": "        (self.check)()?;",
  "new": "        // ignored",
  "mutant_sha256": "c94b17fbc90f0903040b02e19d22155757592222410117a487178e9e4dacec38",
  "assertion": "---- service::unix_bridge::tests::e7::s1::l_b_live_check_first_with_and_without_location stdout ----\n\nthread 'service::unix_bridge::tests::e7::s1::l_b_live_check_first_with_and_without_location' (40988) panicked at crates\\arxd\\src\\service\\unix_bridge\\tests\\e7.rs:928:17:\nassertion `left == right` failed: L-b live check token FIRST only\n  left: Err(MechanismUnavailable)\n right: Err(AccountIdentityChanged)\nnote: run with `RUST_BACKTRACE=1` environment variable to display a backtrace",
  "restore_sha256": "f41377fd15bc2e95e11778f882e6f9f6ffcccfa7f67facc02e86f6de1266364b"
}
```

### magic_unchecked

```json
{
  "name": "magic_unchecked",
  "path": "crates/arxd/src/service/unix_bridge/tests.rs",
  "command": "cargo test -p arxd --lib --locked service::unix_bridge::tests::e7::s1::l_a_missing_location_matrix -- --exact",
  "compiled": true,
  "killed": true,
  "exit": 101,
  "old": "if file.magic != exec::hierarchy::setup::PROC_MAGIC {",
  "new": "if false {",
  "mutant_sha256": "3b625ce118e9c1d0f3e52e1991201718dffca8bc4c163ca6fe3a082321041632",
  "assertion": "---- service::unix_bridge::tests::e7::s1::l_a_missing_location_matrix stdout ----\n\nthread 'service::unix_bridge::tests::e7::s1::l_a_missing_location_matrix' (32804) panicked at crates\\arxd\\src\\service\\unix_bridge\\tests\\e7.rs:859:13:\nL-a Locate refuses\nnote: run with `RUST_BACKTRACE=1` environment variable to display a backtrace",
  "restore_sha256": "63f494f0e4ba94d66df27d72e3e72fc9196917b5746c5af21a48bc6c0e5f9c9b"
}
```

## Failure history and limits

Initial compile attempt (session 70456) failed because shared locate returns
Option and the helper incorrectly called .ok(). Next compile attempt failed
because Execution::preflight is private; corrected to the existing
preflight_for_test seam. Neither was a mutation kill.

The first wider inventory check failed on the top-level helper's generic return
type: the unchanged lexical cfg scanner stops at that delimiter. Full nextest
run `08a2bb98-c0a2-4e0a-8e2f-1ad395de03d9` consequently stopped with 104 passed,
1 failed, 3 skipped and 284 unrun (session 54818). Moving the helper into a
test-only module fixed inventory, but Clippy rejected items after that test
module (session 35547). The final correction moved the pure helper into the
existing tests module, without weakening either guard. Intermediate successful
tests/mutants and an intermediate full suite are not substituted for the final
source-pinned runs above. An incidental encoding conversion in the Unicode
request fixture was caught during diff review and restored before final tests.
No unresolved failed fix remains; no same failure received three failed fixes.

No S2a, S2b, F0, activation, production setup, native resolver, root, service,
account, VM, proc/sys/cgroup observation, install, private-state access, CI,
GitHub, push or PR operation was performed. No policy, Cargo, AGENTS, PROGRESS,
existing review or proposal file changed. The historical native failures were
not retried or diagnosed. Fresh independent Opus high review and normal hosted
gates, including Linux, are outstanding. This evidence does not accept M2.
