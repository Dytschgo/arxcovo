# Unix execution candidate validation

No privileged test was run on the owner's PC. No service/account was installed,
no VM was touched, and no execution surface was added to MCP/CLI. All Unix
execution paths are **unverified on real hardware**.

Original library-only Windows evidence (historical): `cargo clippy --workspace --all-targets --locked -- -D
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

Ordinary hosted three-OS checks (the current branch also adds the manual
workflow and the required pure-Python guard step described below):

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
PR #13 persistent host-identity merge; this task did not verify or change remote merge
state. Progress is recorded here within the assigned validation-doc ownership;
`docs/PROGRESS.md` now also records the task and independent review status.
Git Bash `bash -n` also passed on the workflow shell block and generated runner
snippet. ShellCheck and actionlint were not available; neither was run. The
required Cargo/three-OS suites were not run for this CI/docs-only change.


## Historical first security-review corrections (base c508de0)

Builder: Codex / gpt-6.1-sol, high effort. The first Opus review failed B1–B3;
no privileged run is authorized by these changes. Global cgroup checks retain
no-links/no-group-or-other-write; unrelated user delegation is allowed. Unit
ancestry/subtree stays root-owned and a synchronous post-account-creation scan
rejects generated UID/GID ownership anywhere before the first Runner call.
The 20,000-entry bound is retained; actual hosted hierarchy size remains unmeasured.

The ignored test now checks an accessible outside symlink target, an ordinary
subdirectory positive control, and an inside-workspace symlink (which would be
allowed by BENEATH if NO_SYMLINKS were removed). It also asserts actual child
cgroup membership, reads the run's pids/memory/swap controls from the root test
observer, and forces fork exhaustion above pids.max. **All these runtime assertions
are unrun and unverified on real hardware**, including the requested Rust
symlink-protection deletion experiment. They require fresh independent review
before a disposable root runtime experiment; no runtime mutation pass is claimed.

Copying uses pinned no-follow descriptors and inode/type comparison. Output is
bounded by a drain collector, rather than an inherited artifact file-size limit.
Git uses exact-checkout command-scoped safe.directory and retains SHA equality.
Normal Linux cleanup confirmation now includes successful leaf removal; removal
failure reports false and requires caller quarantine. Drop/error-unwind cleanup
remains best effort and cannot provide a success record by itself.

Local evidence on Windows after changes:

- `python -I -m unittest discover -s tools/unix-exec-validation -p test_driver.py`:
  28 passed; mocks and temporary files only. Ordinary required CI now runs these.
- `python -I tools/unix-exec-validation/mutation_check.py`: nine separate
  executable guard mutants killed: fixture inode, live UID, unit ownership,
  fixture ownership, global link/write scan, copy type refusal, exact argv,
  zero-test acceptance and run gate. Each mutant reruns its actual regression.
  These provide red/green evidence without privileged calls, not runtime containment.
- `cargo test --workspace --locked` and `cargo nextest run --workspace --locked`:
  22 Windows tests passed (post-#13 baseline).
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: passed.
- `cargo fmt --all --check`: passed.
- Direct rustc and clippy-driver library and test metadata checks with `-D warnings` for Linux x64
  and macOS arm64: passed against exact registry libc 0.2.191. No Unix linking/run.
- Full workspace Linux-target Cargo check attempted but unavailable: bundled SQLite
  requires missing `x86_64-linux-gnu-gcc`. This is not a full Unix Cargo-check pass.

The safe Python output-cap test also proves oversize output is truncated/refused
without imposing any process-wide artifact limit. Descriptor race, cleanup identity,
missing capture, orphan group and runner collision checks have pure regressions.
The cgroup removal-failure regression is Linux-only. Hosted run 43000972816
at 4383b56 passed it, the positive-control mode test and the nofollow test on
Ubuntu; macOS passed nofollow. The ignored root runtime did not run.
No new hosted CI run, workflow dispatch, repository setting change, push or PR
was made by this correction task. Review, integration and any privileged run
remain coordinator work. ShellCheck/actionlint remain unrun.

## Current integrated candidate evidence

At b3b808c, independent Opus review passed formatting, Clippy, all 44 Windows
nextest tests, 29 pure Python tests and ten guard mutations (assertion failures,
zero errors). Those counts supersede the historical 19/22/28/nine counts above.
The newer head incorporates path-main b29f531 and needs fresh hosted checks and
exact-head review. The executor, driver and manual workflow remain unchanged.
No privileged cross-account test, runtime symlink mutation or controller-cap
experiment has run. These paths remain **unverified on real hardware**.

## Service-stack integration checkpoint

The candidate now includes merged service/owner/SCM foundations through main
`cd464b2`. Unix executor source is unchanged from published `9055ab0`. On the
combined Windows tree, formatting, locked all-target Clippy and 172 normal
nextest tests passed, along with 29 Python driver guards, 26 CI-helper tests
and three artifact checks; two native artifact checks skipped. Subprocess-only
fixtures were exercised through parents. This does not rerun the historical
mutation checks or establish native Unix execution. Fresh exact-head independent
review and hosted gates remain required; the privileged experiment remains unrun.

## First reviewed hosted dispatch

The exact reviewed #14 merge `5b4f6b2` was dispatched once in run 43091339273.
Guard tests and dependency fetch passed; setup refused with `E_PROTECTION`.
There was no harness-pass or cleanup-confirmed line. The refusal site and mutation
phase are not established; no runtime containment or cleanup acceptance follows.
The reviewed-SHA variable was cleared and verified absent. No retry occurred.
See `evidence/unix-execution-first-dispatch.json`. All earlier unrun descriptions
above are historical preparation; the privileged execution harness has no pass.
