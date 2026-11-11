# Unix execution candidate validation

No privileged test was run on the owner's PC. No service/account was installed,
no VM was touched, and no execution surface was added to MCP/CLI. All Unix
execution paths are **unverified on real hardware**.

Local Windows evidence: `cargo clippy --workspace --all-targets --locked -- -D
warnings` and `cargo nextest run --workspace --locked`: 19 tests passed. Six new
portable exec tests cover argument/path/environment/output bounds, timeout
decision logic, cancellation signalling and Windows refusal. These tests do not
prove identity switching or tree termination.

Installed Linux x86_64 and macOS arm64 Rust standard-library targets. Independently
compiled the exact registry `libc-0.2.191/src/lib.rs` to a target rlib with rustc,
then compiled `crates/arxd/src/exec/mod.rs` against it. Both target libraries and
`--test --emit metadata` test builds pass `-D warnings` with rustc and
clippy-driver. This covers platform API types and test code, without linking or
running Unix binaries. It is **not** a full workspace Unix Cargo build or a hosted
CI result. No second Cargo workspace or lockfile was created.

Future ordinary hosted three-OS checks (no workflow changes in this branch):

```text
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo nextest run --workspace --locked
```

The ignored Linux test `exec::disposable::disposable_linux_cross_account` is an
actual execution harness, not a fake runner. It refuses unless real/effective
root, `CI=true`, `GITHUB_ACTIONS=true`, `RUNNER_ENVIRONMENT=github-hosted` and
`ARX_DISPOSABLE_EXEC_TEST=1`. It creates one generated `arxexec<PID>` user/private
group and an exact new `/var/lib/arxexec<PID>` fixture; existing names refuse.
Its guard removes only that generated fixture/account. Use **only** a fresh
disposable GitHub-hosted Ubuntu runner, never self-hosted CI or the owner PC.
Environment gates are accidental-use guards, not cryptographic host attestation.

Before this test, a separate reviewed disposable-CI setup must provide an empty
root-owned cgroup2 subtree, with pids+memory enabled, no build-user write access
and no direct tasks. Pass its real absolute path in `ARX_EXEC_CGROUP_PARENT`.
The harness does not reconfigure the host hierarchy, install packages, or claim
delegation automatically exists. Missing delegation fails. Do not run the
following command until that setup and the privileged test receive review:

```sh
# Only inside that disposable root CI job; enforce a 120-second job/step bound.
timeout 120s env CI=true GITHUB_ACTIONS=true RUNNER_ENVIRONMENT=github-hosted \
  ARX_DISPOSABLE_EXEC_TEST=1 ARX_EXEC_CGROUP_PARENT="$reviewed_delegation" \
  cargo test -p arxd --locked --lib \
  exec::disposable::disposable_linux_cross_account -- --ignored --exact
```

Assertions exercise observed UID plus a real `id -u`, cleared environment,
denied private-file access, symlink cwd refusal, head/tail flood quotas on both
streams, timeout/cancel/shutdown killing a `setsid` descendant, changed pinned UID
refusal and missing delegation refusal. The harness is type-checked but **unrun**.
macOS cross-account, set-ID policy, descriptor-inheritance probes, adversarial
rename races, resource-limit exhaustion, crash/restart cleanup and ActivityStore
audit-before-spawn integration still need platform evidence. Independent
different-provider review and required hosted checks remain coordinator work.
