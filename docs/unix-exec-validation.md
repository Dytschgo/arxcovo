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

## Manual CI wiring candidate (base 989f1c6)

The new `.github/workflows/unix-execution-experiment.yml` and
`tools/unix-exec-validation/` prepare a future fresh GitHub-hosted Ubuntu 24.04
experiment. They do not change the required three-OS CI, budget or policy gates.
No core-library review, hosted privileged run or containment pass is claimed.
The Windows experiment workflow is absent from this base; this wiring implements
the requested equivalent manual actor/ref/reviewed-SHA fence directly.

Only a manual dispatch by both actor and triggering actor `Dytschgo`, in
`Dytschgo/arxcovo` on `refs/heads/main`, can run. Both `github.sha` and
`github.workflow_sha` must equal the nonempty `UNIX_EXECUTION_REVIEWED_SHA`
repository variable. The driver independently validates those values and decimal
run identities. The workflow accepts no inputs, alternate commands, refs or SHAs.
These checks and OS flags are guards, **not cryptographic attestation** of review
or of a disposable host. No secrets or OIDC permission are requested.

Future coordinator sequence (not performed by this implementation sub-agent):

1. Obtain independent different-provider security review of both the Unix library
   and this setup/cleanup code, including the systemd reference and Cargo runner
   argument contract. Resolve blockers before any privileged execution.
2. Merge only through the normal green required checks and independent
   `agent-review`; preserve `ci-time-budget`. Select the resulting exact full
   40-character main commit containing all reviewed code and this workflow.
3. Have the authorized coordinator set repository variable
   `UNIX_EXECUTION_REVIEWED_SHA` to that exact merged main SHA. A variable is a
   selection mechanism, not a signed review record. Record the review evidence
   and selected SHA separately. This implementation does not change settings.
4. As `Dytschgo`, manually dispatch this workflow on main, while main still equals
   that SHA. Re-runs require the same trusted triggering actor. A later main
   commit requires new review and explicit variable reselection; never accept an
   arbitrary SHA input. Never dispatch on self-hosted infrastructure.
5. Inspect fixed preflight/test/cleanup evidence and the job conclusion. Failure
   is failure, not a skipped acceptance. Quarantine cleanup failures; discard the
   fresh host. No automatic retry, process-group fallback or security weakening.
6. Clear `UNIX_EXECUTION_REVIEWED_SHA` immediately after the experiment, including
   failure, and record run URL, exact code/workflow SHA and outcome here. This
   sub-agent neither selects/clears the variable nor dispatches the experiment.

Real proof requires successful read-only preflight, actual delegated controllers,
the one exact ignored test reporting one pass within 120 seconds, its internal
UID/environment/private-path/output/tree-kill assertions, and confirmed cleanup.
The complete invocation remains `cargo test -p arxd --locked --lib
exec::disposable::disposable_linux_cross_account -- --ignored --exact`, using the
copied pinned Cargo and the exact reviewed checkout. Compilation is separate from
the 120-second runtime bound. A zero-test success is rejected.

Root is used only inside this future disposable Linux CI job; this does not
authorize owner-PC administration, persistent accounts/services or VM operations.
See `tools/unix-exec-validation/README.md` for resource ownership, quarantine and
hard-cancellation limits. All privileged paths remain **unverified on real
hardware**.

Implementation evidence: Codex / gpt-6.1-sol, high effort. Ten safe Python tests
passed on Windows, including executable gate deletion mutations and workflow
guard removal mutations. YAML manual-only/read-only structure validated with
PyYAML; no runtime containment is established. Base `989f1c6` includes the reported
PR #14/root-MCP stable-ID merge; this task did not verify or change remote merge
state. Progress is recorded here within the assigned validation-doc ownership;
the coordinator must reconcile `docs/PROGRESS.md` and independent review status.
Git Bash `bash -n` also passed on the workflow shell block and generated runner
snippet. ShellCheck and actionlint were not available; neither was run. The
required Cargo/three-OS suites were not run for this CI/docs-only change.
