# 0019: Unix native execution bridge (runner phases and build-account shim)

Status: **accepted inactive contract; implementation waits for this contract
PR to merge through independent exact-head review and all required hosted
gates**. Coordinator OpenAI / gpt-6.1-sol / medium, 2026-11-12, main after
S3 #50 (`4f01fca`). This disposition accepts the original Opus architecture
with the retained independent Sol high reviews, and the Sol validator-test
correction with its retained independent Opus high recheck. See the
[correction recheck](../reviews/0019-validator-correction-opus-recheck.md).

The staged architecture remains inactive and grants no production constructor
or M2 acceptance. D7a and D7b are consumed; A1/A2 acceptance is limited to
recorded hosted run `43627472312`. D7c is fully consumed; A3 is accepted only
for Standard `43635131545` and None `43636804545`. D7d is consumed by A5
run `43644661407`; A5 acceptance is limited to that reviewed run. D7e is consumed by accepted A4 baseline dispatch
`43666413693` on `a328060000b8cb7bb389d39f8785f96f6bcb0c25`.
Positive baseline feedback does not satisfy the native omitted-kill requirement
or grant A4/M2 acceptance. D7f is consumed by accepted dispatch `43672720190`;
its job skipped with zero steps, earning no native credit. No D7f retry is
authorized. D7g is consumed by `43674201597`; independent Muse assessment
`21c7615` accepts only its D9a A4 disposable present-residue and omitted-kill
scope. D7h (`43693642693`, FILE leg only), D7i (`43715436515`, pre-mutation
E_PATH) and D7j (`43720496074`, read-only E_PATH diagnosis) are consumed with no
A7/A11 credit. D7k is consumed by final run `43728169361` with no case
proof and confirmed cleanup; hosted A7/A11 is FINAL NO-GO. No source fix,
debug, bind, old host reuse, original policy/package/NNP probe or new grant
can reset that item. D7l below proposes exactly one different-item A12
producer attempt, inactive pending independent Opus source/content and
exact-head review, all five green checks, coordinator merge and exact source
and tree equality. It grants nothing during preparation. The historical E_PROTECTION diagnosis remains
unresolved and its environment excluded. Every other implementation,
native-acceptance and production-activation gate remains.

## Historical proposal and clarification checkpoints

Status: **proposed** (Anthropic / claude-opus-5-5 / high, architecture,
2026-11-11, base `07ae3c89`). A fresh independent Sol review is required
before implementation. ADR number 0018 is reserved for the concurrent owner-CLI
design; this ADR does not use it.

Clarified 2026-11-12 (same provider and model, so not an independent review)
on main `1d828ca7`, docs parent `d9b6fa30`. The
[Sol review](../reviews/0019-unix-bridge-sol-review.md) passed the inactive
design and raised advisories B-A1 (legacy error compatibility) and B-A2 (panic
diagnostics). [Contract clarifications](#contract-clarifications-2026-11-12)
C1–C3 settle them. D1–D7 stand, except for the labelled sentences in D1, D4
and D5. The status stays **proposed**. A fresh independent Sol review of the
clarified ADR and brief must pass before implementation. See the
[disposition](../reviews/0019-contract-clarification-disposition.md).

This ADR adds no native backend to production. `Execution::configured` stays
`MechanismUnavailable` on every OS. Native, installed and dedicated M2 paths
remain **unverified on real hardware** and incomplete. Nothing here was built,
run or tested. Statements about Rust std, the kernel or systemd that were not
checked against a primary source are labelled *source inference*.

The implementation brief is
[0019-unix-native-bridge-implementation.md](../briefs/0019-unix-native-bridge-implementation.md).

## Context

ADR 0017 (accepted for local implementation; its full-validator review is still
running) says the dispatcher maps a typed `Report`. A native adapter must prove
a creation outcome at every failure site. ADR 0009 says Linux and macOS use ADR
0007's `Runner` "unchanged in principle". It also says `run_command {tool}` is
re-resolved "inside the shim as the build account", and discovery runs
`arxd tool-probe` as the build account. Reading the source at `07ae3c89` shows
five mismatches that block a correct bridge.

1. **The runner's errors carry no phase.** `Runner::run` returns
   `ExecError::{Invalid, Unavailable(&str), OperatingSystem(i32)}`.
   `OperatingSystem` comes from sites before creation (`fstat`, cgroup setup,
   `socketpair`), from `Command::spawn` (`unix.rs:305`), and from sites after
   the child exists (`read_exact`, `drain`, `waitid`, `try_wait`;
   `unix.rs:319-369`). A bridge reading `ExecError` alone can only claim
   `Unknown` for all of them, or match on message strings. String matching is
   not a contract. Claiming `Unknown` everywhere turns ordinary per-run refusals
   (account changed, not root, no cgroup) into host quarantine.
2. **There is no Unix shim, and direct exec makes typos quarantine.** The
   runner opens the cwd and execs the target inside `pre_exec`
   (`unix.rs:289-295`, `305`). A missing cwd or executable fails in the child,
   so `spawn` returns `Err`. ADR 0017 classifies that as `Unknown` (row G,
   quarantine until restart). One mistyped `cwd` from any agent would disable
   execution on the host. `tool` cannot work at all: the service may not
   resolve `PATH`, and the runner requires an absolute executable.
3. **Cancellation and shutdown do not connect.** The executor passes
   `development::Cancellation`. The runner takes a different
   `exec::Cancellation` type and polls only its own internal shutdown token
   (`mod.rs:136-140`). The executor's shutdown token cannot reach a running
   child.
4. **The cancel recheck is not the last step before creation.** The runner
   checks cancellation (`unix.rs:148`) before cgroup creation and `socketpair`.
   ADR 0017 requires the final recheck immediately before the primitive. The
   check also merges cancel and shutdown into one string.
5. **macOS cannot meet the required boundary.** The runner reports
   `MacProcessGroupBestEffort` with `tree_cleanup_confirmed = false` always.
   ADR 0017 requires `mac_uid_sweep`. Every macOS run would be row H and
   quarantine. The ADR 0009 sweep, pre-run check and janitor do not exist.

## Decision

### D1. Typed runner phases (small ADR 0007 API addition)

`Runner` gains `run_phased`. It returns `Result<RunResult, RunError>`. Its
behaviour, fences, order and cleanup are those of `run`, except for D3 and D4.
`run` stays as a wrapper that maps `RunError` back to `ExecError`, so existing
callers and tests keep their meaning.

```rust
pub enum RunError {
    /// The creation primitive was not called.
    NotStarted(NotStarted),
    /// `Command::spawn` returned Err: a failed fork, a created child whose
    /// pre_exec failed, or a failed exec. Not distinguishable.
    SpawnFailed,
    /// `Command::spawn` returned a child; a later step failed. Drop cleanup is
    /// best effort and its result is not reported.
    AfterSpawn,
}
pub enum NotStarted {
    Invalid, Unsupported, Cancelled, Shutdown, ServiceIdentity,
    AccountChanged, AccountPolicy, PrivateRoots, Containment, Resource,
}
```

- Each site gets a fixed class at the point of failure. No variant carries a
  string, path, errno, UID or GID. `ExecError` text never crosses the bridge.
- `SpawnFailed` exists because std reports pre-fork pipe errors, fork errors,
  child-side `pre_exec` errors and exec errors through one `Err`. It is
  `Creation::Unknown`, as ADR 0017 already says. This ADR claims nothing about
  std reaping the child.
- `AfterSpawn` is `Creation::Created`. A richer evidence report (ADR 0017 row
  J) is allowed by 0017 but not adopted. It would need proof that the child
  joined the cgroup, and the gain (J instead of G, both quarantine) is small.

*Deviation, C1:* the sentence above that has `run` map `RunError` back to
`ExecError` cannot hold. `RunError` has no text or errno, so that reverse
mapping loses data. Both entry points instead wrap one private core whose
error carries both the phase and the exact legacy `ExecError`. The guarantee
that existing callers and tests keep their meaning is kept, with the three
labelled exceptions in C1.

### D2. A fixed build-account shim for every Unix run

Every dedicated Unix run executes the fixed `Layout::binary()` through the
runner. The agent's target is never the runner's executable.

- `run_command {executable}`: argv `["exec-child", "path", <exe>, <cwd>, args…]`.
- `run_command {tool}`: argv `["exec-child", "tool", <name>, <cwd>, args…]`.
- `discover_tools`: argv `["tool-probe"]`.
- `RunSpec.cwd` is always empty (the workspace root). The runner still opens
  the workspace root after the ID drop, with its existing no-symlink rules.
- The shim runs after exec as the build account. It inherits the observed
  identity, the cgroup (Linux), the process group, no-new-privileges (Linux),
  the cleared capabilities, the core limit, umask `077`, `/dev/null` stdin, the
  runner's stdout/stderr pipes and the owner environment.
- `exec-child` opens `<cwd>` beneath its current directory with the runner's
  rules (Linux `openat2` `RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS`; macOS
  `openat(O_NOFOLLOW)` per component), then `fchdir`. `path` mode spawns the
  absolute path. `tool` mode resolves the fixed-table name against the
  absolute, clean entries of the owner `PATH` (at most 64). It skips empty and
  relative entries, so there is no current-directory lookup. It spawns with
  stdin null and stdout/stderr inherited, waits, and exits with the target's
  code, or `128 + signal` when the target died from a signal.
- `exec-child` writes no bytes of its own. It reports its own failures only as
  fixed exit codes: `125` (bad shim argv, cwd refused or missing, euid 0), `126`
  (target found but could not be started), `127` (target or tool not found).
  These are real exit statuses of the observed child. They can collide with a
  target's own `125`–`127` or `≥ 128`. A row M (`failed`, `exited`, code) stays
  truthful about the process the runner observed.
- `tool-probe` changes directory to `/` before probing, so no workspace file
  (`rust-toolchain.toml`, `.cargo/config.toml`) can influence discovery. It
  prints exactly one JSON line per `TOOLS` entry, in table order.
- Both subcommands refuse euid 0, dispatch in `main` before any runtime is
  built, and read no configuration, layout or state.

Why: agent mistakes (cwd, executable, tool) move after the observation and
become row M, not quarantine. `SpawnFailed` and `AfterSpawn` are left for host
or install faults, where quarantine is correct. This also matches the Windows
contract (ADR 0009 step 8: the shim opens the cwd and runs the target with
`std::process::Command`). It does not weaken ADR 0007. The cwd is still opened
after the ID drop, by the same identity, with the same kernel rules. The
spawn-and-wait shape keeps signal deaths out of ADR 0017 row K
(`exited` without a code), which would otherwise quarantine on every crashing
build.

### D3. One cancellation type; the caller's shutdown reaches the runner

`exec::Cancellation` and `development::Cancellation` become one type (a
re-export; no behaviour change). `run_phased` takes the caller's request
cancellation and the caller's shutdown token. It also keeps polling the
runner's own shutdown token. Either shutdown wins over cancel, as in
`stop_reason`. The bridge never shares a token with `Runner::drop`, so dropping
a runner can never put `Execution` into `Stopping`.

### D4. The final recheck moves to immediately before `spawn`

The cancel/shutdown recheck moves after cgroup creation, `socketpair` and
`Command` construction, immediately before `command.spawn()`. It reports
`NotStarted::Shutdown` before `NotStarted::Cancelled`. Between `spawn` and the
end of the identity read, the runner already polls nothing (`unix.rs:305-326`),
as ADR 0017 requires.

*Deviation, C3:* the existing check at `unix.rs:148` is **kept**, not moved,
and the final recheck is **added** before `spawn`. The guarantees of D4 do not
change.

### D5. Argv allowance for the shim prefix

ADR 0007's argv bound stays the agent bound (128 items, 65,536 bytes, enforced
by `Request::validate`). `validate_spec` allows exactly the fixed prefix on top:
at most 4 more items and 8,206 more bytes (10 + 4 + 4,096 + 4,096). Each item
stays at most 8,192 bytes, and the executable path is at most 4,096.

*Deviation, C1:* only `run_phased` applies the allowance, and only to the
exact four-item `exec-child` prefix. The agent tail keeps the ADR 0007 bound
on its own. Legacy `run` keeps the unchanged ADR 0007 `validate_spec`. As
written, a total of 132 items and 73,742 bytes would have let a short target
carry more than 65,536 agent bytes.

### D6. Platforms

- **Linux standard** is the only target of this bridge.
- **Linux admin** stays unavailable (`mechanism_unavailable`). `Runner::new`
  refuses `admin`, and ADR 0009's admin group and sudoers handling is not
  implemented.
- **macOS** stays unavailable (`mechanism_unavailable`). Lifting this needs
  the ADR 0009 `mac_uid_sweep` extension (a new `TreeBoundary`, the
  real-or-effective UID pre-run check, the post-run sweep and the janitor),
  separately reviewed. The bridge must never map `MacProcessGroupBestEffort` to
  `mac_uid_sweep`. macOS admin stays refused at config, start and every
  preflight (ADR 0009, ADR 0017 `Pin::new`).
- **Windows** is untouched. This ADR designs no Windows backend. S4U failed
  twice; no third probe is planned, and there is never a password fallback.

### D7. Staging; nothing is activated

The bridge is test-constructible only (`#[cfg(test)]` constructor and seams).
No production selector, environment variable, config key or CLI flag can enable
it. Activation is a separate, reviewed change. It requires: the full ADR 0017
review closed; independent implementation reviews of D1–D5 and the bridge;
green hosted gates; the separately reviewed Linux cgroup setup and janitor
slice; and an accepted, separately reviewed disposable native experiment. The
first experiment (43091339273) failed with `E_PROTECTION` before harness
acceptance. Its failure site and cleanup state remain unknown. That historical
run and its environment must not be retried or reused. D7a and D7b are
consumed, as are D7c, D7d, D7e, D7f and D7g; only D7h proposes one fixed
read-only package comparison, subject to its exact candidate and gates.

### D7a. One reviewed disposable A1/A2 run (2026-11-14)

**Historical; consumed.** The terms below record the original grant, not
current permission. PR #83 merged as
`f01da6f425cc8d9d3ee8c304ab8112017618da44`; accepted dispatch
`43622157531`, attempt 1, consumed it. The [unchanged run record](../evidence/linux-stagef-a1a2-run-43622157531.json)
reports `E_PROTECTION/run_parent/initial` before mutation (`none`), cleanup
`not_reached`. The reviewed-SHA variable was cleared and verified absent in
the closed disposition. D7b does not renew D7a or reuse that environment.

This amendment follows the owner's delegated instruction to use the normal
reviewed ADR path rather than require another owner confirmation for a bounded
hosted acceptance run. It becomes effective only after different-provider
review and all required checks pass and the coordinator merges it.

The coordinator may execute **one** new A1/A2-only run through the existing
`unix-execution-experiment.yml` workflow on a fresh GitHub-hosted Ubuntu 24.04
runner. This is an explicit, narrow disposition of the historical-diagnosis
prerequisite for that run; a new SHA alone grants no permission. It neither
reproduces nor diagnoses run `43091339273`, whose cause and cleanup remain
unknown and whose entire environment remains excluded.

The candidate is the first main squash commit containing this amendment. Its
execution-related source, fixture, driver, workflow, packaged unit and standard
profile must be byte-identical to reviewed main
`82162956839740753a0b97a790c04b76784e7c9a`; only this reviewed ADR and its
supporting review/progress documents may differ. The coordinator must verify
that condition and pin the actual full main SHA before dispatch. A source or
workflow difference, or main advancing to another candidate, requires a new
reviewed execution disposition; it cannot consume this grant by substitution.

All existing technical gates remain: manual trigger, exact repository and
owner actor/triggering actor, main ref, source/workflow/reviewed SHA equality,
read-only workflow credentials, a fresh disposable environment, packaged
standard hardening, fixed-unit collision refusal, bounded output/lifetime and
owned-only cleanup. The workflow and its permissions are unchanged. The
coordinator may set `UNIX_EXECUTION_REVIEWED_SHA` to the verified candidate,
dispatch once, and clear it in a finally-style step after that run completes
or fails (or dispatch is rejected). No further owner confirmation is required
for those three operations. No payment, account provisioning, owner-PC access,
owner-host administration or static-VM action is included.

Only `exec::disposable::disposable_linux_a1_a2` is selected. It checks the
packaged hierarchy and production setup before children; it performs no
account provisioning, cross-account command, shim or rights probe. No other
native case, A13, production constructor, Stage G activation or later milestone
is enabled. An A1/A2 pass accepts only those two cases, not full Stage F,
real-Runner PA6 or M2.

Stop on failure or uncertain cleanup, retain the existing bounded sanitized
record, exclude the environment, and do not dispatch again. There is no
automatic retry and no reusable run permission. After the one dispatch has
been accepted, the grant is consumed regardless of job outcome. An explicit
owner answer arriving before dispatch takes precedence over this delegated
instruction. Hardware and installed/dedicated paths receive only the evidence
actually observed; no owner-machine acceptance is implied.

### D7b. One corrected protected-parent A1/A2 run (2026-11-14)

**Historical; consumed.** PR #86 merged as
`1e9d8818de1e337b1402ca7e2b71ac02456b82f3`; accepted dispatch
`43627472312`, attempt 1, consumed this grant. Its successful A1/A2 evidence
and confirmed cleanup were independently accepted in the
[Muse evidence review](../reviews/m2-a1a2-hosted-evidence-muse-review.md);
the reviewed-SHA variable was verified absent. The terms below record that
grant, not current permission. D7c does not renew it or reuse its environment.

Run `43622157531` localized the fresh refusal to root-owned `/opt` with
group/other write bits `0o022`, before mutation. The [source correction](../evidence/linux-stagef-protected-parent-fix.md)
uses fixed `/var/lib/arx-ci-<run>-<attempt>` without weakening ancestor,
collision or cleanup guards. Its [independent Grok source/security review](../reviews/m2-stagef-protected-parent-grok-review.md)
passed; PR #85 passed normal gates and merged as
`7afea2fdcb17268396fbdb17c36ee8cf991e2ce4`. This is the first targeted
correction of that localized refusal, not a retry or diagnosis of historical
run `43091339273`. That historical run's failure site and cleanup remain
unknown; its entire environment remains excluded. The D7a environment is
also excluded. Source-review and publication success confer no native credit.

The sole candidate is the **first main squash containing D7b**. Compared with
reviewed main `7afea2fdcb17268396fbdb17c36ee8cf991e2ce4`, only this ADR and
supporting review/progress documents may differ. Execution-related source,
fixture, driver, workflow, packaged unit and standard profile must be
byte-identical; evidence and its hashes must remain unchanged. The coordinator
must verify this comparison and pin the actual full main squash SHA, not a
branch head or the source-baseline SHA. A source/workflow difference or main
advancing to another candidate requires a new reviewed disposition; no
substitution consumes this grant.

After those gates only, the coordinator may set `UNIX_EXECUTION_REVIEWED_SHA`
to that exact full candidate SHA, dispatch the existing
`unix-execution-experiment.yml` **once** on a fresh GitHub-hosted Ubuntu 24.04
runner, and clear the variable in a finally-style step after completion,
failure or rejected dispatch, verifying absence. No further owner confirmation
is required for these operations. An explicit owner answer arriving before
dispatch takes precedence. An accepted dispatch consumes the grant regardless
of job outcome; rejection grants no automatic retry.

All existing technical fences remain unchanged: manual trigger, exact
repository and owner actor/triggering actor, main ref,
source/workflow/reviewed-SHA equality, read-only credentials, packaged standard
hardening, protected ancestors, fixed-unit and staging collision refusal,
owned-only cleanup, and bounded reads, output and lifetime. There is no parent
override, fallback, guard relaxation or workflow/permission change.

Only `exec::disposable::disposable_linux_a1_a2` may run. It checks the packaged
hierarchy and setup before children; no account provisioning, cross-account
command, shim, rights probe, A13 or other native case is included. No payment,
owner-PC access, owner administration, account action, static VM or backup
deletion is authorized. Stop on failure or uncertain cleanup, retain the
existing bounded sanitized record, exclude the environment and do not dispatch
again. There is no automatic retry or reusable permission.

Any pass accepts only A1/A2. M2, full Stage F, Stage G, real-Runner PA6,
production constructors and activation receive no credit or permission and
remain open/inactive. `/var/lib` and installed/dedicated paths remain
**unverified on real hardware** except for evidence actually observed in a
future authorized run; optional diagnostics remain parked.

### D7c. Two fixed source-selected A3 cases (2026-11-14)

**Historical; fully consumed.** Standard `43635131545` and None `43636804545`
were independently accepted in the [Standard review](../reviews/m2-a3-standard-hosted-muse-review.md)
and [None review](../reviews/m2-a3-none-hosted-muse-review.md), with confirmed
cleanup and verified variable absence. The terms and preparation checks below
record the original grant, not current permission; D7d renews neither case.
The owner's delegated proportionate ADR route covered both
fixed cases in this one reviewed disposition, without another policy amendment
or owner confirmation for each hosted runner. Source preparation is accepted
by the [plan review](../reviews/m2-stagef-a3-driver-plan-muse-review.md) and
[source review](../reviews/m2-stagef-a3-driver-source-muse-review.md), not native
acceptance. PR #90 main `fba8b3c993b3d173125525c760ee2dc7234e3cfa`
(tree equal to `744f957`; five gates and Muse exact-head PASS `6883050524`
reported by the coordinator) is the immutable source baseline.

1. **Standard:** the sole candidate is the first main squash containing D7c
   and `TEST = 'exec::disposable::disposable_linux_a3_standard'`. Compared with
   that full PR #90 main SHA, only this one TEST line in
   `tools/unix-exec-validation/driver.py`, the reviewed selection-regression
   corrections in `test_driver.py` and `mutation_check.py`, this ADR and supporting
   review/progress documents may differ. Those test-only corrections pin all three
   closed argv selections and a wrong-selection mutant independently of the source
   default. No other runtime byte may change. The unchanged packaged standard
   profile must be present.
2. **None:** eligible only after Standard's successful result is independently
   verified and accepted, with confirmed owned cleanup and verified variable
   absence. Its sole candidate is the first subsequent independently reviewed,
   five-gates-green, coordinator-merged main squash changing only that TEST line
   to `exec::disposable::disposable_linux_a3_none` plus supporting review/progress
   documents, compared with the pinned Standard squash. None omits the entire
   standard profile; no weakening drop-in is allowed. D7c covers this second
   fixed case without a further policy amendment.

For each candidate the coordinator must verify the exact permitted diff and pin
its **actual full main squash SHA**, never the source baseline or branch head.
No arbitrary other change, intervening main candidate or source substitution is
permitted. Workflow, Cargo, package, production code and historical evidence/hash
bytes stay unchanged. A guard-test failure cannot be waived to fit this scope.

After that case's gates only, set `UNIX_EXECUTION_REVIEWED_SHA` to its pinned
SHA and use the unchanged manual `unix-execution-experiment.yml` once on a
separate fresh GitHub-hosted Ubuntu 24.04 invocation. Each case has at most
**one accepted dispatch**, consumed regardless of outcome. Preserve exactly
`Dytschgo/arxcovo`, actor and triggering actor `Dytschgo`, `refs/heads/main`,
and `GITHUB_SHA == ARX_REVIEWED_SHA == ARX_WORKFLOW_SHA` equal to that variable;
read-only credentials and all existing workflow fences remain. No workflow/CLI
input, selector, extra workflow or general execution framework is introduced.
Clear the variable in a finally-style operation after each completed or failed
run or rejected dispatch, and verify absence. These bounded operations require
no repeated owner confirmation once this amendment is effective.

Retain exact package/runtime attribution, sole MainPID/supervisor checks,
protected staging, shared reservation, collisions, bounded output/lifetime and
owned-only cleanup. Accept each case only from its exact selected test-ok,
one-pass/zero-failure summary, real systemd success/zero exit and confirmed
cleanup, with existing bounded sanitized evidence. Stop both cases on any
failure, rejected dispatch, unknown result or uncertain cleanup; exclude the
environment and do not retry automatically. None is ineligible on any such
Standard outcome. The historical `43091339273` environment and all consumed
D7a/D7b environments remain excluded; neither is retried or diagnosed here.

No account provisioning, cross-account probe, owner-PC access, owner admin,
production activation, static VM or backup action is authorized. Only the
observed A3 case receives credit; M2, full Stage F, Stage G and real-Runner
PA6 remain open. Installed/dedicated paths are **unverified on real hardware**.

Local preparation evidence (Sol high, 2026-11-14): imported
`mutation_check.safe_suite()` via Python with `tools/unix-exec-validation` on
`sys.path` and bytecode writes disabled; no CLI/native/filesystem/provisioning
cases or ignored fixtures were run (only source/package/evidence reads).
72 tests ran: 71 passed, `A1A2.test_launch_contract` failed because its literal
still expects A1/A2. The `wrong test` mutant anchor also has zero source matches
(required one). The initial preparation was blocked: a minimal correction needed
`test_driver.py` to pin each closed selection independently and
`mutation_check.py` to retain an assertion-killed wrong-selection mutant against
the selected default; neither may be weakened to accepting any selection.
The coordinator subsequently corrected those two test files and explicitly
included them in Standard's reviewed candidate diff above. The original failed
run earns no validation credit; the corrected finite checks are recorded below. Direct imported F0/F0b `mutations()` calls passed: 23 and 36
assertion kills respectively, four equivalents uncredited, driver hash unchanged
during checks. `git diff --check` passed. Independent Muse review remains required.

Corrected coordinator validation (Sol medium):
`python -B tools/unix-exec-validation/mutation_check.py` passed 72 audited
mocked tests, 23 Python assertion kills, three pure Rust tests and four compiled
Rust assertion kills. Separate `test_f0_blockers.py --mutants` and
`test_f0b_transport.py --mutants` passed 23 and 36 assertion kills respectively;
four equivalents get no credit. All three commands left the selected driver
SHA-256 unchanged at
`88cc51ddf0b229e45f6c00f79ed03bcca978ee35d3b4e01306c1b210f5b1a028`.
The launch-contract test patches each of the three literal accepted selections
and checks the exact argv; the wrong-selection mutant changes that argv to the
excluded cross-account test. Neither relies on a hard-coded A1/A2 default.
No native operation or dispatch ran. These corrected checks supersede the
failed preparation run above and do not grant execution permission.

### D7d. One source-selected A5 empty-orphan run (2026-11-14)

**Historical; consumed.** Accepted dispatch `43644661407`, attempt 1, on
`cf3a2510fc45e0cb438ae14cc41115e492bc0f41` consumed D7d regardless of outcome.
Its A5-only acceptance and variable-clear evidence are recorded in PROGRESS.
The terms below retain the original proposal, not current permission.

**Proposed; no execution permission yet.** Independent Muse exact-head review,
all five required gates (three platform `verify` checks, `ci-time-budget`,
`agent-review`) and coordinator merge are required. The delegated proportionate
ADR route authorizes this planning; it bypasses none of those gates. Once
effective, the bounded operations below need no repeated owner confirmation.

PR #96 merged as `c7f44ecf74a19ebd6cda4ab5700a1eef7ae7426a` at
2026-11-14 21:37:02Z after all five required gates and Muse exact refresh
`6884475968`. The coordinator verified that the squash tree equals reviewed head
`71c590daabb0d7f9a1ef5d47df0aa233a0121c72`. That actual full main squash is the
immutable execution baseline; the conditional baseline check reviewed by Muse
is now satisfied before candidate publication.
The accepted [plan](../reviews/m2-stagef-a4a5-janitor-plan.md),
[seeder recheck](../reviews/m2-stagef-a5-seeder-plan-muse-recheck.md),
[source review](../reviews/m2-stagef-a5-source-muse-review.md) and
[mapping review](../reviews/m2-stagef-a5-driver-muse-review.md) establish source
preparation only, not A5 acceptance.

The sole candidate is the first main squash containing D7d and
`TEST = 'exec::disposable::disposable_linux_a5'`. Compared with the verified
PR #96 execution tree, only that one TEST line in
`tools/unix-exec-validation/driver.py`, the focused selection assertion and its
counterfactual in `test_driver.py` and `mutation_check.py`, this ADR and supporting
review/progress documents may differ. The assertion must pin A5 literally and the
mutant must revert to historical None, with assertion-failure credit only. Workflow,
Cargo, package, production, fixture, evidence, policy outside this amendment and
all other bytes remain unchanged; no stale
hash restoration or guard weakening is permitted. Verify the exact diff and
pin the **actual full candidate main squash SHA**, never the branch head or
baseline. An intervening candidate or other difference needs a new reviewed
disposition; a guard-test failure cannot be waived to fit this scope.

After all gates, set `UNIX_EXECUTION_REVIEWED_SHA` once to that exact full
candidate SHA and dispatch the unchanged manual `unix-execution-experiment.yml`
once on a fresh GitHub-hosted Ubuntu 24.04 host. At most **one accepted
dispatch** is allowed, consumed regardless of outcome. Preserve
`Dytschgo/arxcovo`, actor and triggering actor `Dytschgo`, `refs/heads/main`,
and `GITHUB_SHA == ARX_REVIEWED_SHA == ARX_WORKFLOW_SHA` equal to that variable;
read-only credentials and every existing guard remain unchanged. Clear the
variable in a finally-style operation after completion, failure or rejected
dispatch, and verify HTTP 404 absence. No selector input or framework is added.

Use the existing A5 fixture only: unchanged packaged Standard profile, one
empty canonical orphan `arx-0-0` seeded before exactly one production setup
call, exact one-leaf removal/runs-reuse/controller/S9 postconditions before
READY and identity hold. No child workload, account provisioning, real Runner
or shim is involved. Retain exact package/runtime attribution, sole MainPID/
supervisor checks, protected staging, reservation, collision refusals, bounded
output/lifetime and owned-only cleanup. Accept A5 only from its exact selected
test-ok, one-pass/zero-failure summary, real systemd success/zero exit and
confirmed cleanup, retaining existing bounded sanitized evidence.

Stop on any failure, rejected dispatch, unknown result or uncertain cleanup;
exclude the host and do not retry automatically. All historical D7a/D7b/D7c
hosts and `43091339273` remain excluded; no historical diagnosis or retry is
included. No owner-PC/admin, cross-account, production activation, static-VM
or backup action is included. Only A5 receives any observed acceptance; A4,
M2, full Stage F, Stage G and real-Runner PA6 remain open. Installed/dedicated
paths are **unverified on real hardware**.

Initial preparation blocker (Sol high, no validation credit): `test_source_default_remains_a3_none` still
pins A3 None, and the `select A5 by default` mutation anchors that old TEST
line. A5 selection requires a separately authorized focused correction to
`test_driver.py` and `mutation_check.py`, preserving literal selection and
assertion-killed wrong-selection coverage. The coordinator subsequently authorized these two focused test-only edits
inside the candidate predicate above; the failed preparation is retained, and
no gate is waived.
Imported `mutation_check.safe_suite()` with Python bytecode writes disabled:
76 mocked tests ran, 75 passed, that one literal assertion failed, zero errors.
The old default mutant anchor has zero matches (required one); no mutation
credit is claimed. Driver SHA-256 stayed
`aa8423e82006ee96a42c03e4267c1bdb777ae39b8e9a4b7f7c5ea84ff59b63de`.
`git diff --check` passed. That initial run earns no validation credit.
The correction and independent review remain required; no native run or dispatch
occurred.

Corrected coordinator validation (Sol medium): the selection test now pins A5
literally, and its mutant restores the historical None selection. The existing
four-case mapping/summary tests are unchanged. Ran
`python -B tools/unix-exec-validation/mutation_check.py`: 76 audited mocked tests,
29 Python assertion kills, three pure Rust baselines and four compiled assertion
kills passed. Separate F0 and F0b `--mutants` commands passed 23 and 36 kills,
with four equivalents uncredited. All three commands left driver SHA-256
`aa8423e82006ee96a42c03e4267c1bdb777ae39b8e9a4b7f7c5ea84ff59b63de`
unchanged. No native operation or execution permission follows from these checks.

### D7e. One hosted A4 baseline feedback run (2026-11-15)

**Consumed by accepted dispatch `43666413693`, attempt1, exact main
`a328060000b8cb7bb389d39f8785f96f6bcb0c25`.** Job success and confirmed
cleanup are recorded in [baseline feedback](../evidence/linux-a4-hosted-baseline-feedback.md);
the [independent baseline assessment](../reviews/m2-a4-hosted-baseline-muse-assessment.md)
passed for positive feedback only (PR #111). The reviewed-SHA variable
is absent. No retry or further execution is authorized. The preparation
conditions below are retained as the record of this consumed grant.

PR #106
merged as `40adbe55672293c894abd2d49be4de630e62577e`. PR #108's privacy
fixture is included in accepted main. PR #110's reviewed SDK-owned stdio
fixture merged as `8e4bf824b56a2c4df141f9ef8cd78616d9b53475` at
2026-11-15T01:53:32Z after all five gates and independent review. The coordinator
verified its tree `d12edb06bfd87374b6f94cc8b0800a1554e4ed5e` equals reviewed
head `bbdc71e446fa201947ed66b7c05a920775d7141f`.
The [execution baseline inventory](../evidence/a4-d7e-execution-baseline.json)
pins arxd source and integration tests, Unix-validation assets, both relevant
workflows, Cargo metadata, toolchain, unit and Standard profile at that exact
accepted baseline. This supersedes the earlier PR #108 baseline. The baseline
is never the dispatch SHA. The candidate's full main squash SHA remains pending
fresh exact-head review, required checks and coordinator merge.

The sole dispatch candidate is the **first normal gated main squash** containing
this amendment and `TEST = 'exec::disposable::a4::disposable_linux_a4'` after that
accepted baseline. Compare execution blobs and the exact diff to that baseline:
only the driver's one TEST line, the existing manual workflow's one unprivileged
`build.py linux-candidate` compile/compare step before the privileged driver,
the focused literal selection assertions and inverse A5 counterfactual anchor
in `test_driver.py`/`mutation_check.py`, explicit historical A5 mode pins in the
existing lifecycle mock fixtures in `test_driver.py`, `test_f0_blockers.py` and
`test_f0b_transport.py`, this ADR and supporting review/progress/evidence
documents may differ. The mode pins preserve all existing lifecycle assertions;
A4 admission remains covered by its dedicated mocks. The non-A4 delivery refusal test explicitly
selects historical A5; it retains refusal-before-open coverage. No other driver
fence, helper, Rust fixture, production, package, Cargo, selector, permission or
timeout changes are permitted. Never compile the helper as root. The fresh
checkout has no image; this step produces ignored candidate bytes and compares
them to the existing Linux pin without repinning or executing them.

Independent different-provider exact-head review, all five required checks
(three platform `verify`, `ci-time-budget`, `agent-review`) and coordinator
merge are mandatory. Then verify candidate-tree/blob equality and pin the
**actual full candidate main squash SHA**, never a branch head or baseline SHA.
An intervening candidate or execution difference requires a new reviewed
disposition. Record the full pins and read back main and workflow/source SHA
before dispatch. Failed tests cannot be omitted or waived to qualify it.

Only after those conditions may the coordinator set
`UNIX_EXECUTION_REVIEWED_SHA` once to that SHA, read it back exactly, and accept
**one** manual dispatch of `unix-execution-experiment.yml` on a fresh
GitHub-hosted Ubuntu 24.04 host. Preserve repository `Dytschgo/arxcovo`, actor
and triggering actor `Dytschgo`, `refs/heads/main`, and
`GITHUB_SHA == ARX_REVIEWED_SHA == ARX_WORKFLOW_SHA` equal to the reviewed
variable. Read-only credentials, fixed unit, packaged Standard profile,
protected staging, collision refusal, descriptor delivery, attribution,
MainPID/supervisor identity, bounded output/lifetime and owned cleanup remain.
Clear the variable in a finally-style operation after completion, failure or
rejected dispatch, and verify HTTP 404 absence. These bounded operations need
no repeated owner confirmation once effective; this source task performs none.

One accepted dispatch consumes D7e regardless of result. On failure, unknown
result or uncertain cleanup, stop, retain only existing bounded sanitized
feedback, exclude the host and do not retry. No old environment may be reused;
all D7a–D7d and historical `43091339273` hosts remain excluded. No owner-PC,
account provisioning, admin/payment, production activation, static-VM or backup
action is included. No helper launch or native execution is authorized by
local pure/mock validation, review preparation or a branch SHA.

Select only `exec::disposable::a4::disposable_linux_a4` with the existing fixed
helper, fixture and exact packaged Standard unit. Observe the existing exact
test-ok, one-pass/zero-failure summary, real systemd success/zero exit and
confirmed cleanup; retain the existing bounded sanitized feedback. This is
**baseline native feedback only**. ADR0020 D9a's native omitted-kill negative
case was **unrun and NOT granted under D7e** and remains unrun; only D7g
below proposes its separate conditional disposition. Pure/model assertion mutants cannot
satisfy it. **A4 acceptance stays open** until that requirement is independently
satisfied; no new mutation framework or waiver is introduced. A7–A12, full
Stage F, Stage G, real-Runner PA6 and M2 remain open. Installed/dedicated paths
are **unverified on real hardware**.

### D7f. One fixed native A4 omitted-S7-kill run (2026-11-15)

**Consumed by accepted dispatch `43672720190` on main `994760c98c2816f37b1acdcaa9d9827693765b17`. The job skipped with zero steps after monitor pin cleanup raced admission; no native credit. Variable absence independently verified HTTP 404. No retry or further D7f authority.**
The remaining D7f text records its historical grant only; it cannot authorize
another dispatch. See the [closed record](../evidence/linux-a4-negative-skipped-43672720190.json).
PR #113 merged the fixed source as `d0b4c5530b1c4c084a35fe40499afa96a2678b0b`,
tree `9cfd66dbdf7ce1afc43c1f5acecf4eb62fd09455`, equal to reviewed head
`15c4d73a3eaf02779672870e5054e34edea6f5c5`. Its all-five-gates/Muse PASS
is supplied coordinator evidence, not re-run here. D7e and all earlier grants
stay consumed. Positive run `43666413693` on `a328060000b8cb7bb389d39f8785f96f6bcb0c25`
remains feedback only; the native negative is **unrun**.

The sole candidate is the **first eligible normal main squash containing D7f**
after that baseline, after independent different-provider exact-head review,
all five green required checks (three platform `verify`, `ci-time-budget`,
`agent-review`) and coordinator merge. Before any grant is used, compare the
entire candidate against that baseline. The exact allowed non-document paths
are only:

- `tools/unix-exec-validation/driver.py`: the single fixed TEST assignment from
  positive to `exec::disposable::a4::disposable_linux_a4_omitted_kill`.
- `tools/unix-exec-validation/test_driver.py`: focused default-selection
  assertions/name and exact-summary mock marker correction, preserving every
  assertion and explicit historical positive/mode pins.
- `tools/unix-exec-validation/mutation_check.py`: the focused inverse selection
  mutant anchor, restoring positive A4 as the counterfactual.

Those three blobs must equal the independently reviewed preparation blobs
recorded in [preparation evidence](../evidence/linux-a4-negative-run-preparation.md).
Every other non-document execution blob must equal the baseline, including all
Rust/fixture/LinuxFs/production, helper source/recipe/build/image pins, workflows,
package/unit/Standard profile, Cargo/lockfile and toolchain. No new selector,
substitution or unrelated source/main change qualifies. Allowed document delta
is only this ADR, that evidence note, the retained D7f independent review
`docs/reviews/m2-d7f-a4-negative-muse-review.md`, and coordinator PROGRESS.
Verify exact reviewed-head/candidate tree equality after merge; record and pin
its actual full main squash SHA and tree (not a branch head, baseline or invented
future SHA), changed blobs and the complete diff. Read back main immediately
before dispatch. Main advancing, any blob mismatch, intervening unrelated main
content or a different candidate requires a new independently reviewed
disposition **before** grant; substitution cannot consume this grant.

Only after those conditions may the coordinator set `UNIX_EXECUTION_REVIEWED_SHA`
**once**, read back the exact candidate value, and dispatch the unchanged
`unix-execution-experiment.yml` **once**, on one fresh GitHub-hosted Ubuntu 24.04
host. Verify repository `Dytschgo/arxcovo`, actor and triggering actor `Dytschgo`,
`refs/heads/main`, source and workflow SHAs, and
`GITHUB_SHA == ARX_REVIEWED_SHA == ARX_WORKFLOW_SHA ==` the variable/candidate.
Retain manual-only/read-only credentials, protected staging, fixed-unit collision
refusal, packaged Standard hardening, descriptor-bound fixed helper delivery,
MainPID/supervisor attribution, exact ignored/exact/one-thread argv, all existing
bounds and owned-only finally cleanup. Clear the variable in a finally operation
on completion, failure or rejected dispatch, independently verifying HTTP 404
absence. The owner's delegated disposable-host authorization covers these
coordinator operations without further human approval; this preparation performs
none. No owner-PC, owner account/admin/payment, static-VM or backup action is
included.

An **accepted dispatch consumes D7f regardless of outcome**. No retries, reruns,
second dispatch, environment reuse or renewed older grant. A rejected dispatch
also grants no retry. Failure, unknown result or uncertain cleanup stops work;
retain bounded sanitized feedback and exclude the host. Exclude every previous
experiment host, including D7a through D7e and historical `43091339273`.

Only the fixed negative literal is selected. Native credit requires one real
production setup under the existing OmitS7, rejection by the shared inner positive
`assert!(accepted)` caught only at that assertion, exact
`Unavailable(KillTimeout)`, exactly 1 reached / 1 omitted / 0 forwarded S7 writes,
zero removals, real retained populated literal leaf and live unreaped sole helper
with original lifetime remaining **before failure cleanup**. Require the fixed
raw-fd `ARX_A4_OMITTED_S7_REJECTED` LF marker exactly once, then checked owned-child
SIGKILL/reap and owned-leaf emptiness/removal/absence before READY/identity-ack.
Require the outer exact test-ok, one-pass/zero-failure summary, real systemd
success/zero exit and independent driver finally stop/cgroup/helper/runtime
cleanup confirmation. A green job alone, false bool, other panic/refusal, early
death/expiry, fake clock/events, second setup or cleanup kill earns no S7 credit.
The unchanged native deadline, five-second wait, limits and bounds all apply.

Retain the existing bounded sanitized result/provenance and independent native
assessment, including rejection attribution and cleanup, before any A4 credit.
Pure/mock tests and assertion mutants provide no native evidence. Even a native
negative pass earns only the separately assessed D9a A4 scope; full F/G,
real-Runner PA6 and M2 stay open. Installed/dedicated paths remain
**unverified on real hardware**.

### D7g. One replacement native A4 omitted-S7-kill run (2026-11-15)

**Consumed by accepted dispatch `43674201597` on `1958f9975d1620151ca9a182c5047dd8aeb43e65`. The 92-second job succeeded with confirmed cleanup; the reviewed-SHA variable was finally deleted and independently verified absent HTTP404. Independent Muse assessment `21c7615` accepts only D9a A4 disposable present-residue plus native omitted-kill scope. No retry or further D7g authority.**
Baseline is actual main `994760c98c2816f37b1acdcaa9d9827693765b17` (PR #114),
tree `908abcf01c0969cb73d3486e92476f3c90c132f5`, identical to reviewed
`d60665c81f4e83c10382fea170244ee04db98bf1`. PR114's five green gates and Muse
PASS are supplied coordinator evidence. D7f's accepted run `43672720190`,
attempt 1/job `132686133651`, was `workflow_dispatch` on that exact main,
actor/triggering actor `Dytschgo`, completed **SKIPPED with zero steps**.
The dispatch command returned its run URL; an immediate ancillary run-list
lookup returned none, and monitor finally cleanup deleted the pin before job
admission. Root independently confirmed variable absence by HTTP 404. Nothing
native ran; D7f and all older grants remain consumed.

The sole candidate is the **first eligible normal main squash containing D7g**
after that baseline, following independent exact-head review, all five green
required checks (three platform `verify`, `ci-time-budget`, `agent-review`) and
coordinator merge. Compare the whole candidate tree against the baseline:
**zero executable/non-document delta is allowed**, including file modes.
Every source, fixture, workflow, driver/tool, helper/recipe/image pin,
package/unit/profile, Cargo/lockfile and toolchain blob must be identical.
The exact allowed document paths are only:

- `docs/adr/0019-unix-native-execution-bridge.md`
- `docs/evidence/linux-a4-negative-skipped-43672720190.json`
- `docs/evidence/linux-a4-negative-replacement-preparation.md`
- `docs/reviews/m2-d7g-a4-negative-replacement-muse-review.md`
- `docs/PROGRESS.md` (coordinator only)

Record the actual reviewed head/tree, main squash SHA/tree, full diff and blob
identity in the [replacement preparation](../evidence/linux-a4-negative-replacement-preparation.md).
Verify reviewed-head/main-squash tree equality after merge. Read back current
main immediately before dispatch; it must still be that exact candidate.
Advancing main, intervening unrelated content, any mismatch or substitution
requires a new independently reviewed disposition before grant.

Once effective, coordinator only: set `UNIX_EXECUTION_REVIEWED_SHA` **once**,
read back its exact candidate value, and dispatch unchanged
`unix-execution-experiment.yml` **once** on one fresh GitHub-hosted Ubuntu 24.04
host. **Capture the accepted run URL directly from dispatch output**, bind its
exact run ID, and hold the pin until **that exact run is terminal**, even if an
ancillary run-list lookup fails or returns none. Never redispatch on lookup
failure, infer rejection from it, or clear the pin while the accepted run is
nonterminal. Finally clear after that terminal result (or confirmed dispatch
rejection with no accepted run), then independently verify HTTP 404 absence.
Uncertain identity/result requires bounded reconciliation of the accepted URL,
not early pin deletion or a second dispatch. This coordinator monitor correction
is outside the repository; no new workflow, tool or runtime infrastructure is
authorized or needed.

Verify `Dytschgo/arxcovo`, actor/triggering actor `Dytschgo`, `refs/heads/main`,
and `GITHUB_SHA == ARX_REVIEWED_SHA == ARX_WORKFLOW_SHA ==` candidate/variable.
Retain all D7f native oracles and bounds above without exception: fixed negative
literal, one real setup, shared positive assertion rejection with exact
KillTimeout and 1/1/0 writes, zero removals, populated retained leaf/live unreaped
helper before cleanup, exactly one raw-fd LF marker, checked owned SIGKILL/reap
and leaf cleanup before READY/identity-ack, exact outer pass summary, real
systemd success/zero exit and independent driver finally cleanup. All D7f
staging, hardening, descriptor, attribution, argv, lifetime/read/output and
owned-only cleanup fences remain. No green-job-only or pure/mock native credit.

An accepted dispatch consumes D7g regardless of outcome; no retry, rerun,
second dispatch or host reuse. Rejection grants no retry. Failure, unknown result
or uncertain cleanup stops further execution; retain bounded sanitized feedback
and exclude the host. All previous experiment hosts, including D7f's skipped
run and historical `43091339273`, remain excluded. Existing owner authorization
covers this bounded disposable-host operation without extra approval; no
owner-PC/account/admin/payment, static-VM or backup action is included.
Independent native assessment remains mandatory before any D9a A4 credit.
A4/M2/full F/G/real-Runner PA6 remain open; installed/dedicated paths remain
**unverified on real hardware**. This preparation executes nothing.

### D7h. One fresh hosted read-only sudo package comparison (2026-11-15)

**Consumed by accepted run `43693642693` on `47b440a5faeaa24434e155b81d5d03a5c977e4e9`. Eight-second job succeeded with candidate_match=true and qualified=false; dedicated pin finally deleted and independently absent404. Independent Muse assessment a81c028 accepts the FILE leg only; no qualification/A7/A11 credit, retry or further D7h authority.** Baseline is PR #123 main squash
`4f1a6db3f652291ffb94550fea85aa395f78109a`, tree
`6fb6ffd467d81a5f1d2cd0a39da6ae92aab5bc3b`. Coordinator supplies equality to
reviewed `40e24e7`, all five gates and Muse PASS; this preparation does not
re-run those hosted checks. D7a–D7g remain consumed. D7h renews none of them.

The sole candidate is the **first eligible normal main squash containing D7h**
after that baseline. Fresh Muse architecture/security/source review is required
before publication; different-provider independent exact-head review, all five
green required checks (three platform verify, ci-time-budget, agent-review),
coordinator merge and reviewed-head/main-squash exact tree equality must precede
pinning. Compare the full candidate tree, including modes, with the baseline.
The exact allowed delta paths are ONLY:

- `.github/workflows/sudo-package-readonly.yml` (new regular 100644 file)
- `tools/ci/sudo-package-readonly.py` (new regular 100644 file)
- `tools/ci/test-sudo-package-readonly.py` (new regular 100644 file)
- `docs/adr/0019-unix-native-execution-bridge.md`
- `docs/evidence/linux-a7-package-readonly-preparation.md`
- `docs/reviews/m2-d7h-package-readonly-muse-review.md` (retained fresh review)
- `docs/PROGRESS.md` (coordinator only)

No other executable blob or mode change is allowed. Driver, fixture, comparator,
Cargo/lockfile/toolchain, package, helper and normal CI/policy remain identical.
Record actual reviewed head/tree and squash/tree in the evidence; do not invent
future identities. Read back current main immediately before pinning/dispatch;
it must remain exactly that candidate. Any unrelated delta, main advancement,
tree mismatch or substitution needs a new independently reviewed disposition.

Coordinator only, once effective: set **SUDO_PACKAGE_REVIEWED_SHA** once to that
candidate and read back equality, then dispatch **sudo-package-readonly.yml**
once on one fresh GitHub-hosted Ubuntu 24.04 amd64 host. This distinct variable
confers no authority on the old native workflow; do not set
UNIX_EXECUTION_REVIEWED_SHA. Both workflow and wrapper enforce repository
Dytschgo/arxcovo, actor and triggering actor Dytschgo, workflow_dispatch,
refs/heads/main and source SHA == workflow SHA == reviewed variable. The wrapper
also enforces attempt 1, bounded run ID, GitHub-hosted Linux/X64/ubuntu24,
root real/effective UID, no arguments, fixed workspace/script path and exact
workflow ref before importing the comparator or reading package bytes.

The manual-only job has contents:read, checkout credentials disabled, the
existing pinned checkout action and a three-minute deadline. It has no inputs,
artifact, secrets or permission expansion. Ten safe mocked wrapper tests run
before privilege. D7h explicitly grants **the existing coordinator-host
privilege wrapper** `sudo -n /usr/bin/env -i` invoking only isolated system
Python and the fixed repository wrapper, with the fixed forwarded gate fields
and PATH in the reviewed workflow. This necessarily executes the host sudo
transport; it grants no execution of the subject as a version/list/true oracle
and no sudo -V, sudo -l, policy/account/proc/sys/systemd query or helper/native
fixture. No account, installation, administrative mutation or policy change.

The wrapper loads only bounded fixed `__file__`-relative comparator source
(11180 bytes; SHA256
`12a122db7d30be04dd7523db046d49f4083882c318b375ccf0986ec0eb338e75`),
via nofollow held directories and regular single-link file, hashes before
compile, and invokes **only its existing `_read_linux()` once**. No argv/path/
expected-hash override, CLI activation or general import search exists. The
comparator CLI remains refusing. Scope is exactly three fixed payloads, two
fixed links and ancestor metadata described in the
[package source evidence](../evidence/linux-a7-sudo-package-comparison-source.md),
including existing fixed readlink/ELF/digest/budget/reopen fences. No extra
host collection is authorized. The baseline comparator blob is
`520d341633c67dc52ec483e9b3fcc92131a3da1f`.

Checkout at the admitted SHA and exact workflow ref bind the wrapper source;
the wrapper checks its own fixed path and hashes comparator bytes independently.
As in ADR0020 D8/A1, privileged concurrent writers/mounts and root-protected OS
dependencies are trusted; these fences are observations, not adversarial host
attestation. No whole-host audit is implied. Read-only refers to this comparison
leg: ordinary checkout and sudo transport may have platform-managed effects.

Before printing once, validate the entire closed comparison schema: fixed
revision/architecture, status and boolean flags, known digests only on a match,
null digests on refusal, qualified always false. Emit only that reconstructed
JSON plus validated source_sha/workflow_sha/run/attempt binding. Exceptions,
invalid schemas and gate failures emit one constant refusal with no partial
payload or private exception text. Exit 0 means candidate_match only; every
refusal exits 2. No raw file, unknown digest, path metadata or logs escape.

Capture the accepted run URL **directly from dispatch output**, bind the exact
run ID and attempt, and retain the pin until **that exact run is terminal**,
even if ancillary lookup returns none or fails. Finally delete the variable
and independently verify HTTP 404 absence after terminal outcome (or confirmed
dispatch rejection without an accepted run). Uncertain identity requires bounded
reconciliation against that direct URL, never early pin deletion or redispatch.
Any accepted dispatch consumes D7h regardless of outcome, including skipped jobs;
no retry, rerun, second dispatch or host reuse. Rejection grants no retry either.
Retain only bounded sanitized result/binding and exclude the host afterward.

This is ONLY the read-only FILE comparison leg of ADR0020 D9b. candidate_match
is not qualification, fixture admission, A7/A11 native acceptance or M2 credit.
C2, floor, four-mask in-unit view/literals/NSS and actual NNP allow/deny oracles
remain open. A7/A11/M2/full F/G/real-Runner PA6 remain **unverified on real
hardware**. Existing user authorization covers this fresh disposable-host leg;
no extra owner confirmation or product approval flow is introduced. This local
preparation performs none of the future coordinator operations above.

### D7i. Proposed single combined A7/A11 fresh-host run (2026-11-15)

**Consumed by accepted run43715436515 attempt1/job132815838689 on main0011d23a0681c0acb717064166c399e194937869. The18s job failed with first/final E_PATH, mutation lower bound none, cleanup not_reached and internal null. No native acceptance. Both pins independently absent404 after finally-clear. No retry or further D7i authority; remaining text records historical grant only.**

**Preparation only; inactive until the following review/merge gates. No dispatch
or runtime grant is exercised by this source preparation.** D7a-h remain consumed;
all their hosts, including D7h host43693642693, are excluded. No stale A1/A2 grant
is renewed. This proposal permits no retries, reruns, diagnostic redispatch or
host reuse. A7/A11/M2/full F/G/real-Runner PA6 remain **unverified on real hardware**.

Baseline is exactly main `7d8ab1c160a4af488f4a85abb8dc08a45e4d6103`, tree
`ea260f731c6d0149de68df7f0e2fb9428a9bfb7f`. Local Git verification confirms exact
reviewed `ebafeed` tree equality; PR127 all five green and Muse exact-head PASS
comment6893948634 are supplied coordinator evidence. Runtime source candidate
`7385785c50ec28118db6d84138f7dc9e21242ec7` is pinned in the
[preparation evidence](../evidence/linux-a7-a11-execution-preparation.md), including
every source blob/mode and the canonical profile facts. No future main SHA is
invented. That evidence identifies static reviewed package/default/policy facts,
not whole-host qualification before runtime checks.

The sole dispatch candidate is the **first eligible normal main squash containing
this D7i and its fixed runtime source**, after fresh independent Muse source/security
review before publication, different-provider exact-head review, all five green
required checks (three platform verify, ci-time-budget, agent-review), and coordinator
merge. The changed Linux controller must compile in hosted Linux CI. Required
source corrections need fresh review and updated exact blob inventory before this
proposal can become effective; they cannot be substituted after review.

Strict baseline-to-candidate diff predicate: compare recursive full trees including
modes, with no deletions, renames, type changes or executable-bit changes. ONLY these
regular100644 paths may differ:

- crates/arxd/src/exec/disposable/sudo_nnp.rs
- tools/unix-exec-validation/driver.py
- tools/unix-exec-validation/test_driver.py
- tools/unix-exec-validation/mutation_check.py
- docs/adr/0019-unix-native-execution-bridge.md
- docs/evidence/linux-a7-a11-execution-preparation.md (new)
- docs/reviews/m2-a7-a11-execution-candidate-muse-review.md (new retained fresh review)
- docs/PROGRESS.md (existing parent/coordinator checkpoint only; builder does not edit)

The four source blobs must equal the reviewed inventory; every other non-doc
blob/mode must equal both the inventory and baseline. Comparator, native workflow,
package/Standard drop-in, helper, Cargo/lockfile/toolchain, normal CI/policy and
production code must stay byte-identical. No unrelated delta is admitted. Record
reviewed head/tree, main squash/tree, exact delta and complete source inventory;
reviewed head and squash must have exact full-tree equality. Read back live main
immediately before pin/dispatch: it must still be that exact candidate. Any main
advance, mismatched tree/workflow/source/pin or unrelated delta stops this grant
and requires a new reviewed disposition, never normalization or adoption.

After those gates, coordinator only may set UNIX_EXECUTION_REVIEWED_SHA once to
that exact main squash, read back equality, then dispatch unix-execution-experiment.yml
once on one new GitHub-hosted Ubuntu24.04 amd64 host. Source SHA == workflow SHA ==
reviewed pin == checkout HEAD must all agree with the reviewed candidate. Existing
fixed repository/actor/triggering actor/event/main/host/root gates stay mandatory;
A7 attempt must be1. Use only the fixed combined TEST; no workflow input, alternate
selector, caller profile, package revision or fixture paths. Keep the distinct
SUDO_PACKAGE_REVIEWED_SHA absent; D7h is not a prerequisite to rerun on another host.

Native scope, on that SAME NEW host: existing fixed privilege transport, finite
runtime package _read_linux comparison of installed sudo/plugin/util against the
reviewed Noble1.9.15p5-3ubuntu5.24.04.4 amd64 payload/default facts under D8/A1;
protected C2 absent-or-trusted and /bin alias identity; root/kernel/systemd/cgroup2/
controller/delegation floor; owned fixture/artifact/account provisioning; exactly
four controlled read-only policy mappings and in-unit literal/entry/mount/NSS checks;
then actual production tool-probe and fixed E4 NNP reentry for BOTH accounts, with
all existing identity/status/descriptor/output/cleanup oracles and owned final cleanup.
Read equality from the consumed D7h host cannot substitute for these checks.
Unknown bytes, architecture, path, link/plugin/mapping/floor/account identity,
profile mismatch or failed oracle refuses. The canonical profile is a fixed label
for reviewed facts, never self-attestation by a delivery digest. No original-host
policy capture/reconstruction, sudo -V/list oracle, full OS audit, production
activation or owner administrative work. Existing A4 helper workflow compilation
remains compile-only; it gains no native execution authority.

Keep the merged budgets, two workers, both cancellation domains and join-both,
markers, reservations, stop/cgroup/live-UID confirmation, owned-only cleanup and
quarantine on uncertainty. No timeout relaxation or detached cleanup. A late syscall
or cleanup may exceed measured envelopes; it refuses, never earns bounded-completion
credit. No native account/policy effects are permitted outside the disposable fixture.

Capture the accepted run URL **directly from dispatch output**, bind run ID/attempt1,
and monitor THAT exact URL through terminal outcome. Retain the pin throughout,
including failed ancillary lookups. Uncertain identity requires bounded reconciliation
against that direct URL, no early pin deletion or second dispatch. Finally delete
the variable after the exact run is terminal (or confirmed dispatch rejection with
no accepted run), then independently verify HTTP404 absence. Verify both native and
read-only variables absent. Any accepted dispatch consumes D7i regardless of skip,
failure or success; rejection also grants no retry. Retain bounded sanitized source/
workflow/run/oracle/cleanup evidence, exclude the host, and obtain independent
source/run/oracle assessment before claiming either case or M2 credit. No extra
owner confirmation or product approval flow is introduced.

### D7j. Proposed single read-only admission-chain observation (2026-11-15)

**Consumed by accepted run43720496074 attempt1/job132831225756 on main7a26fcdd9729ee858c9fc5e1610ff4b75d408592. Six-second read-only leg intentionally refused E_PATH: usr_local_bin root/type accepted, group_other_write_absent=false; stopped before unsafe open. All three pins independently absent404 after finally-clear. No retry, qualification or native acceptance. Remaining text records historical grant only.**

**Preparation only; inactive.** D7a–i stay consumed, all their hosts excluded.
The binding coordinator correction in
[the next-execution disposition](../reviews/m2-a7-next-execution-disposition.md)
chooses this leg solely to identify a fresh host's blocking predicate/role before
another combined attempt. Mutable public recipes do not make D9b impossible;
original-host policy reconstruction is not a prerequisite. One observation
cannot establish historical or systematic cause or whole-host qualification.

Baseline main is `18b8fef58403e5ffadfd3f580d6a2a961c6d754c`, tree
`215d75d3eeb09dd513846c73143f757454e6ce7a`, equal to reviewed
`291a08a6f4ca302eb35bd5fabe0238d0c735ab92` (PR130 all five green supplied
by coordinator, not queried here). Parent is `fc4ceda`. The sole candidate is
the first eligible normal main squash containing this D7j and fixed source,
after fresh different-provider source/security review, independent exact-head
review, all five required checks (three platform verify, ci-time-budget,
agent-review), coordinator merge, exact source inventory and full reviewed-head/
main-squash tree equality. This source preparation grants no runtime authority.

Strict full-tree baseline-to-candidate predicate: no deletions, renames, type or
mode changes; only these regular 100644 files may differ:

- `.github/workflows/a7-admission-readonly.yml` (new)
- `tools/ci/a7-admission-readonly.py` (new)
- `tools/ci/test-a7-admission-readonly.py` (new)
- `docs/adr/0019-unix-native-execution-bridge.md`
- `docs/evidence/linux-a7-admission-readonly-preparation.md` (new)
- `docs/reviews/m2-a7-next-execution-disposition.md` (inherited correction)
- `docs/reviews/m2-a7-admission-readonly-muse-review.md` (new fresh review)
- `docs/PROGRESS.md` (coordinator only, inherited checkpoint)

Every other blob/mode must equal baseline. The evidence inventory pins the three
new runtime/test files and unchanged driver. No shared driver, selection, Rust,
helper, package, Cargo, normal CI or production behavior changes. Before once-only
pin/dispatch, read back main and require exact candidate SHA/tree and all source
inventory equality. Any advance, unrelated delta or substitution needs a new
reviewed disposition. Future identities must be recorded, never predicted.

Coordinator only, after those gates: independently confirm all three live pins
absent; set `A7_ADMISSION_REVIEWED_SHA` once to that exact main squash and read
back equality. Keep `UNIX_EXECUTION_REVIEWED_SHA` and `SUDO_PACKAGE_REVIEWED_SHA`
absent. Dispatch `a7-admission-readonly.yml` once on one fresh GitHub-hosted
Ubuntu24.04 X64 host. Both workflow and wrapper bind repo Dytschgo/arxcovo,
actor/trigger Dytschgo, manual event, main, source==workflow==reviewed pin,
attempt1; wrapper additionally checks CI/host/image, bounded run ID, real/effective
root, no arguments, exact workspace/script/workflow path before driver import
or root-down reads. No input/selector/alternate path/hash exists.

Manual-only contents:read job, pinned checkout, no persisted credentials and
3min bound. Safe mocked tests precede the existing fixed transport
`sudo -n /usr/bin/env -i` into `/usr/bin/python3 -I -B` and this wrapper only.
Transport is the sole sudo execution; no target sudo version/list/true probe.
Read-only scope: exact `/`, `/usr`, `/usr/local`, `/usr/local/bin`, `/usr/bin`
directory predicates, fixed `/bin` readlink/alias identity, optional
`/usr/local/bin/sudo` exact known payload. Bounded hashed reviewed driver bytes
are compiled; only `a7_alias_c2 -> sudo_nnp_open/a4_directory` is invoked.
The finite adapter observes owner-is-root, type-accepted and no group/other
write before opening each component, stops on first unsafe metadata, and holds
all ancestor FDs with nofollow/CLOEXEC/NONBLOCK, fstat/reopen/stability checks
excluding atime only. The accepted alias resolves through its held fixed target,
never a fresh symlink traversal. Unknown image/hash/metadata or drift refuses.
No installed-three-payload requalification, policy/NSS/account/proc/systemctl/
cgroup/mount/Runner/LinuxFs/helper/native execution or host mutation is allowed.
Root-protected OS dependencies and concurrent privileged writers/mounts remain
the same D8/A1 trust boundary; this is no adversarial host attestation.

Output is closed diagnosis code/role/three booleans plus validated run/source
binding. No raw IDs/modes/paths/inodes, unknown hashes, exception text or logs.
Untrusted source/gate failures emit constant refusal. `qualified` and
`native_credit` always false; exit0 means only PATH_CHAIN_OBSERVED, never admission.

Capture returned accepted URL directly, monitor that exact run/attempt until
terminal, retain pin even across ancillary lookup failures, and reconcile uncertain
identity against that URL without redispatch. Finally clear the pin after terminal
(or confirmed rejection with no accepted run), independently verify HTTP404 and
all three pins absent. Any accepted dispatch consumes D7j including skip/failure;
rejection grants no retry either. No retries/reruns; exclude host afterward.
Retain bounded sanitized evidence and obtain independent source/run assessment.
Any later D9b combined run needs its own reviewed disposition on a NEW host and
all actual NNP/oracle/runtime checks. A7/A11/M2/full F/G/real-Runner PA6 remain
**unverified on real hardware**. No additional owner approval flow is added.

### D7k. Final single Opus-owned hosted A7/A11 attempt (2026-11-15)

**Consumed by FINAL accepted run43728169361 attempt1/job132854558246 on main07d88692e2f1480c6e18ff5df846cfb871a18033. Failure121s: first/final E_ARTIFACT, staged lower bound, cleanup confirmed, internal null. All three pins independently absent404 after finally-clear. Pre-recorded outcome is NO-GO hosted A7/A11, unverified on real hardware pending dedicated host. No hosted retry, diagnostic leg, further grant reset or acceptance. Remaining text is historical only.**

**Docs-only disposition; inactive.** Anthropic claude-opus-5-5, high. This is the
ONE further hosted attempt allowed by the
[Opus achievability decision](../reviews/m2-a7-a11-opus-hosted-achievability.md)
under the A7/A11 attempt ceiling in the
[correction handoff](../reviews/m2-a7-a11-cross-provider-correction-handoff.md).
D7a–j stay consumed and all their hosts excluded; no earlier grant is renewed.
The D7 staging sentence naming only D7h is historical. Exact pins, inventory and
predictions are in the
[preparation evidence](../evidence/linux-a7-final-opus-execution-preparation.md).

**No source change.** Runtime source is the merged Opus D9c slice (ADR0020 D9c):
source `f41af38f570c3ceeaeff87ec7decfcc6f9b0d9c7`, retained
`c596f50cf79ec1ac51bae7d0ba615a0a2b0ef37e`, reviewed head
`caba0fe0635ad00784d9f2006cbf225b032f4ad5`, main squash
`52ce2a5d2443de018b06965dfca43fdc626b5f36` (PR #133, 2026-11-15T14:53:14Z), both
tree `923842bae41f48ec2be53b2f3a1372b0ede61e88`. Independent Sol source review
and exact-head PASS comment6896280064, status58964602017 and all five greens,
including hosted Linux compilation, are supplied coordinator evidence, not re-run
here. `TEST` is already fixed to the combined A7/A11 literal. D7k adds no fix,
bind, selection, workflow, package, Cargo or code change.

**Candidate.** The sole candidate is the **first eligible normal main squash
containing D7k**, after independent Sol content and exact-head review, all five
required checks green (three platform verify, ci-time-budget, agent-review),
coordinator merge, exact reviewed-head/main-squash full-tree equality and a new
check that the candidate's source inventory equals the recorded one. Strict
full-tree predicate against baseline `52ce2a5`: no deletions, renames, type or
mode changes; only these regular 100644 paths may differ:

- `docs/adr/0019-unix-native-execution-bridge.md`
- `docs/evidence/linux-a7-final-opus-execution-preparation.md` (new)
- `docs/reviews/m2-d7k-final-opus-disposition-sol-review.md` (new Sol review)
- `docs/PROGRESS.md` (existing coordinator checkpoint only)

Every other blob and mode, including every runtime, source, test, workflow,
helper, package, Cargo/toolchain and policy file, must equal the baseline tree;
the recorded non-doc inventory binds all of them. The canonical profile label
`99e7f5b4b3b63ff13955432bf9b7cde0436c9ab57afbc93fbe99bb45eb17017e` names reviewed
static facts only. It attests nothing about a host. The actual in-unit five-view
proof and native oracles are still required. Any main advance, tree or inventory
mismatch, unrelated delta or substitution stops D7k; it is never normalized or
adopted.

**Operation (coordinator only, after all gates).** Read back live main; it must be
the exact candidate. Independently confirm `UNIX_EXECUTION_REVIEWED_SHA`,
`SUDO_PACKAGE_REVIEWED_SHA` and `A7_ADMISSION_REVIEWED_SHA` all absent. Set
`UNIX_EXECUTION_REVIEWED_SHA` once to the candidate and read back equality; the
other two stay absent throughout. Dispatch `unix-execution-experiment.yml` once on
main, with no inputs, onto one fresh GitHub-hosted `ubuntu-24.04` host. The
existing workflow gate binds repo, actor, triggering actor, manual event and main,
and requires `github.sha == github.workflow_sha == pin`. The driver binds reviewed
SHA, workflow SHA and checkout HEAD equality, and requires attempt 1. The other
two pins are not observed in-run; their absence before and after is a
coordinator-verified precondition.

Capture the accepted run URL **directly from dispatch output**. Bind run ID and
attempt 1, and monitor THAT URL through terminal. Keep the pin until then, even if
ancillary lookups fail. Do not clear early or re-dispatch. Reconcile uncertain
identity only against that URL. A dispatch rejection counts only once it is
confirmed that no run was accepted. Finally delete the pin and independently
verify HTTP404, with all three pins absent.

**Budget.** Any accepted dispatch consumes D7k whatever the outcome: success,
failure, infrastructure failure, skipped job, cancellation or unknown. A confirmed
rejection consumes it too. No retry, rerun, image retry, diagnostic read, new
grant name, fence or timeout relaxation, or extra owner approval exists. The
coordinator dispatches operationally; Opus owns the attempt and its disposition.

**Native scope (unchanged merged source).** On that one new host, before effects:
finite installed package comparison of C1 sudo, plugin and util against the
reviewed Noble `1.9.15p5-3ubuntu5.24.04.4` amd64 facts, protected `/` and
`/usr/bin`, the `/bin -> usr/bin` alias identity, and the systemd/kernel/cgroup2/
controller/delegation floor. Host `/usr/local/bin` is not consulted. Then the D9c
controlled owned account/artifact fixture with exactly five read-only views,
in-unit five-set/mountinfo/statvfs/dev+ino/empty/C2-ENOENT proof before Runner and
before each call, and the actual production tool probe plus fixed E4 NNP reentry
for BOTH cases. Merged budgets, two workers, cancel-both, join-both, strict
markers, systemd stop/cgroup/live-UID confirmation and owned-only cleanup apply.

**Pre-registered predictions.** Both cases: `no_new_privs=true`, C1 chosen, C3
skipped by the alias, C2 exactly ENOENT through the view, and
`sudo=exit_nonzero`: case0 despite its NOPASSWD rule, case1 with no grant. All
identity, descriptor, record, residue and cleanup oracles pass. CASE0, CASE1 and
COMPLETE each appear once, and cleanup is `confirmed`.

**Outcomes.** `exit_zero` with NNP true is a product security finding: stop and
escalate, with no further hosted attempt. ANY other miss is NO-GO for hosted
A7/A11. That includes preflight, package or digest drift, view proof, envelope,
time, budget, cancel/join, cleanup, quarantine, unknown, unattributed, skipped or
infrastructure failure. A7/A11 then stay **unverified on real hardware** pending
a dedicated Linux host (owner's, not an agent admin step). Work advances to the
remaining M2 items that do not depend on A7/A11, then independent M3. M2 is not
marked complete. Source feasibility is not acceptance. Even a matching run needs
a mandatory independent post-run source/run/oracle assessment, and credit covers
only the exact case proof shown. Retain bounded sanitized evidence and exclude the
host.

### D7l. Proposed one bounded A12 producer attempt (2026-11-15)

**SOURCE/DISPOSITION CANDIDATE ONLY. No effective grant yet.** OpenAI
GPT-6.1 Sol / high, implementation sub-agent, no subagents. D7a-k are consumed;
all their environments and historical `43091339273` are excluded. Hosted
A7/A11 remains FINAL NO-GO; A12 is a different item and resets no counter.
The [preparation record](../evidence/linux-a12-one-run-preparation.md) fixes
full baseline/review identities, allowed paths/modes, complete non-doc inventory,
canonical LF hashes, tests and pre-registered limits. It is part of this contract.

**Candidate and eligibility.** The sole candidate is the FIRST eligible normal
main squash containing D7l and the fixed A12 source selection. No future SHA is
predicted. Before any grant takes effect: fresh independent Opus source/content
review, independent exact-head review, all five required checks green (Windows,
Linux and macOS verify, ci-time-budget, agent-review), coordinator merge, actual
first eligible main squash readback, reviewed-head/main-squash full-tree equality,
strict baseline path/mode predicate and complete source-inventory equality with
the preparation record. The record binds actual workflow/source bytes; require
candidate `github.sha == github.workflow_sha == reviewed pin` and checkout HEAD
identity. Any unrelated delta, main advance, mode/blob/tree mismatch or substitution
stops eligibility; do not adopt, normalize or guess a replacement candidate.

Only four source selections change: Rust DELIVERY_ENABLED=true, driver
A12_DELIVERY_ENABLED=true, A12_EXECUTION_ENABLED=true and literal
`TEST='exec::a12::native::disposable_linux_a12'`. Focused pure/mock tests and
mutation anchors adapt to that enabled candidate; only the consumed D7j wrapper
MODULE hash/size follow changed driver bytes. Production, workflow, Cargo,
package, helper, rights and A7 logic stay byte-identical. The native test remains
ignored: ordinary tests use pure/injected readers and source checks, never the
now-enabled `prepared_delivery` reader. An explicit fixed ignored selection is
required. Source flags alone grant no run.

**Operation, coordinator only after eligibility.** Independently verify all three
repository variables absent before starting: `UNIX_EXECUTION_REVIEWED_SHA`,
`SUDO_PACKAGE_REVIEWED_SHA`, `A7_ADMISSION_REVIEWED_SHA`. Set ONLY the native
`UNIX_EXECUTION_REVIEWED_SHA` once to the exact full main candidate SHA and read
back exact equality. D7h/D7j variables remain absent throughout. Dispatch the
unchanged `unix-execution-experiment.yml` ONCE, on main, no inputs, one fresh
GitHub-hosted ubuntu-24.04 host, attempt 1, fixed A12 only. Existing workflow
repo/actor/triggering-actor/event/ref/SHA gates and driver checks all apply.
Capture the returned URL DIRECTLY from dispatch output and monitor THAT URL
through terminal, binding run ID/attempt 1. Keep the pin until terminal even if
ancillary lookups fail; no early clear or redispatch. Finally delete the native
pin and independently verify all three variables absent with three HTTP404s.

Any accepted dispatch consumes D7l, including skip, failure, cancellation,
infrastructure failure or unknown. A confirmed rejection consumes it too. No
retry, rerun, new grant, diagnostic leg, fence relaxation or environment reuse
is authorized by this task. Exclude the host after this attempt. On ANY miss,
stop with a closed sanitized record and bounded source assessment; no automatic
retry. Escalate any security finding. No permission for the old A7 source or
runtime follows, even if that historical code remains present in the harness.

**Native scope and oracles, unchanged.** One exclusively owned fresh standard
nologin account/group/root/workspace and root-private stage; strict bounded
source/workflow-bound record, hash/size/inode/ownership/artifact pins and trusted
fixed `/usr/bin/true` then `/usr/bin/sleep 15`. No sudo, NNP, package/policy views,
A7 fixture or original qualification. Absolute OwnerConfig.workspace, empty
RunSpec.cwd under ADR0007; held/reopened account/workspace/artifact/target/runs
checks immediately before EACH actual Runner call. Root-only ancestry/no-follow,
collision refusal, pending-capture quarantine and unit-owned cleanup persist.

True must actually complete successfully through real Runner with the observed
standard UID/GID and daemon-root attribution, LinuxCgroupV2, zero exit and tree
cleanup confirmed. Cancellation requires a real sole populated owned leaf,
matching held/reopened inode and nonzero PID BEFORE cancelling the sleep call;
then actual Cancelled result, explicit owned join, real cleanup_residue=false,
empty runs/tasks and held-leaf literal ENOENT. No synthetic flag, pre-spawn
cancellation, error, missing join, fixture deletion or marker substitutes.
Require NORMAL, CANCEL, COMPLETE markers each exactly once, identity ACK with
exact runtime/MainPID/executable/cgroup/sole-PID attribution, exact single-test
summary, successful owned systemd exit and independent final cleanup confirmed.
Stop/confirm unit cgroup absent and live UID released BEFORE account, root,
workspace, runtime and stage deletion. Uncertainty quarantines owned artifacts.

Bounds remain preparation15s/ACK5s/Runner15s/proof5s/call25s/fixture90s,
packaged startup30s/monitor120s/runtime140s, output1024, memory64MiB/pids32,
harness128MiB with A12-only DEBUG=0, one bounded build. No increased limit.
Synchronous OS/NSS calls and scoped joins may stall; elapsed checks reject late
results, not guarantee hard interruption. No orphan workers are permitted.

**Pre-registered predictions and credit.** Source feasibility predicts true
Exited/exit0 and sleep Cancelled only after real post-spawn proof, correct identity,
confirmed cleanup, residue=false, empty runs, literal held-leaf ENOENT, three
markers once, exact summary, ACK and independent final cleanup. Feasibility is
not acceptance. Fresh independent post-run source/run/oracle assessment is
mandatory before any A12 credit, limited to that exact attempt. No A7/A11,
real-Runner PA6, full F/G or M2 completion credit is implied. All native paths
remain **unverified on real hardware**. Dedicated-host A7 work and owner admin
steps are outside this proposal. This implementation task runs no native code,
dispatch or variable action and publishes nothing.

## Contract clarifications (2026-11-12)

These settle Sol advisories B-A1 and B-A2 (credited to the
[Sol review](../reviews/0019-unix-bridge-sol-review.md)). They are
requirements for stages B–D and G. Nothing here was built or run. The
[brief](../briefs/0019-unix-native-bridge-implementation.md) §3a, §7a, §8 and
§9 give the exact tests and mutations.

### C1. Exact legacy errors through one private core (B-A1)

The exec module adds one private type: `RunFailure { phase: RunError, legacy:
ExecError }`. It is never `pub`, derives nothing, and has no `From` impl to or
from `std::io::Error`, `ExecError` or `RunError`. So every fallible site must
name its class explicitly. It has two constructors: `new(class, ExecError)`,
and `os(class, io::Error)`, which builds `legacy` through the existing
`ExecError::from` (`mod.rs:101-104`). The legacy errno value is therefore
byte-identical, including `-1` when there is no raw OS error.

- `Runner::run_core(spec, cancel, caller_shutdown: Option<&Cancellation>) ->
  Result<RunResult, RunFailure>` holds the one implementation. `unix::run` and
  `account` return `RunFailure`. The `Group::create` and `trusted_root` call
  sites map with `RunFailure::new(NotStarted::Containment, e)` and
  `RunFailure::os(NotStarted::PrivateRoots, e)`. `cgroup.rs` is unchanged.
- `run(spec, cancel)` runs the unchanged `validate_spec`, then `run_core(spec,
  cancel, None)`, mapping errors with `|f| f.legacy`. Its signature, error
  values (variant, `&'static str`, errno), site order, argv bounds, cleanup,
  `Runner::new`, `shutdown_handle` and `Drop` stay as they are at `1d828ca7`.
- `run_phased(spec, cancel, shutdown)` runs `validate_phased_spec` (C1/D5),
  then `run_core(spec, cancel, Some(shutdown))`, mapping errors with
  `|f| f.phase`. A validation failure is `NotStarted::Invalid`. The legacy
  value is dropped inside `run_phased`. No string, errno or ID crosses it.
- `exec::Cancellation` becomes `pub use crate::development::Cancellation`
  (D3). It has the same path, the same derives (`Clone`, `Default`) and the
  same methods.

Legacy `run` changes observably in exactly three ways. Each is labelled:

1. **Race-window refusal (D4).** Suppose cancel or shutdown becomes visible
   after the existing early check and before `spawn`. `run` now returns
   `Err(Unavailable("run already cancelled or shutting down"))`, an existing
   value. Before, it started a child and returned `Ok` with outcome
   `Cancelled` or `Shutdown`. No existing test asserts the old behaviour.
2. **Merged `Cancellation` type identity (D3).** This only adds; no legacy
   code stops compiling.
3. **Panic payload text** at the six unreachable `Result::expect` sites (C2).
   The panic itself is kept. Only its message loses the error's `Debug` text.

No other legacy deviation is allowed. A lossy reverse mapping, a shared
validator, or giving legacy `run` the shim allowance would each fail the brief's
L1–L3.

`validate_phased_spec` keeps the common executable, cwd, timeout and output
checks unchanged. Suppose `arguments[0] == "exec-child"`. Then it needs at
least four items, `arguments[1]` in `{path, tool}`, `arguments[2]` of 1–4,096
bytes, `arguments[3]` of at most 4,096 bytes, and no NUL. The unchanged
ADR 0007 argv rule then applies to `arguments[4..]`. Any other spec gets the
unchanged rule on all its arguments; `["tool-probe"]` is one such spec.

### C2. Panic diagnostics boundary (B-A2)

**Facts.** These come from the primary std docs as served at
doc.rust-lang.org, which rendered Rust 1.99.1. They were not checked against
the pinned 1.95.0 source, which is not installed locally.

- If a hook is set, it is invoked "before the panic is caught, before
  unwinding". `catch_unwind` "only catches unwinding panics, not those that
  abort the process".
- The default hook "prints a message to standard error and generates a
  backtrace if requested". The hook "is a global resource". `set_hook` and
  `take_hook` panic when called from a panicking thread.
- `Result::expect` panics "with a panic message including the passed message,
  and the content of the `Err`". `NulError` derives `Debug` over
  `(usize, Vec<u8>)`.

In this repository, no `set_hook` or `take_hook` call exists in
`crates/arxd/src`. The Linux unit sets no `StandardError=` (journal by systemd
default; *source inference*). The macOS plist sends stderr to
`/Library/Logs/Arxcovo/arxd.log`. So any service panic message becomes a
persistent log line. ADR 0017 retention rule 6 and ADR 0007 ("does not … log
argument/environment values") already forbid private data there. A caught
panic that maps to `Failed { Unknown }` is classification only. **It is not
sanitation.**

**Boundary (normative).** The *panic-reachable set* is the production code of
`exec/{mod,unix,cgroup,output}.rs`, the bridge module and the shim module.
Code after a trailing `#[cfg(test)] mod tests` is excluded.

- **P1. Fixed payloads.** `panic!`, `unreachable!`, `assert!`,
  `debug_assert!`, `todo!` and `unimplemented!` take one plain string literal
  with no `{` as their message. These are forbidden: the method calls
  `.expect(`, `.unwrap(`, `.unwrap_err(` and `.expect_err(`; `assert_eq!`,
  `assert_ne!` and their `debug_` forms; `dbg!`; `panic_any`;
  `resume_unwind`; `print!`, `println!`, `eprint!` and `eprintln!`. Their
  messages carry `Debug` output of operands or errors.
- **P2. No formatting** of `RunSpec`, `RunResult`, `ObservedIdentity`, either
  `CapturedOutput`, `OwnerConfig` or `ServiceConfig` fields, `Pin`, paths,
  account names, environment values, errno or a caught payload into any panic,
  log or stream.
- **P3. Payload dropped unread.** No downcast, no formatting and no logging.
- **P4. No range indexing on `str` or `String`** for agent, config or output
  text. The std char-boundary message includes the offending character (1.99.1
  docs; not verified for 1.95.0). Use byte slices or `str::get`.
- **P5. The `pre_exec` closure stays panic-free.** It is panic-free today at
  `unix.rs:200-302`.
- **P6. No hook in the service process.** No `set_hook` or `take_hook`, and
  never one swapped per run. The shim is a separate, short-lived process. It
  installs one no-op hook as its first statement, runs its body under
  `catch_unwind`, and exits `125` on a caught panic. This keeps D2's "writes no
  bytes of its own" true.
- **P7. Catch scope.** The bridge's `Executor::run` wraps its whole per-run
  body in one `catch_unwind(AssertUnwindSafe(..))`. That body is spec
  derivation, `run_phased`, result mapping and discovery parsing. `Err(_)`
  maps to `Failed { Unknown }`, so the row finishes as G and quarantines.
  Bridge preflight stays outside the catch; no child can exist there. This is
  a deviation from brief §3 "wraps `run_phased` only". A mapping panic would
  otherwise leave the row `started` (`executor.rs:296-316`).

Stage B must fix the six existing `Result::expect` sites (`unix.rs:22`, `160`,
`167`, `320`, `321`; `cgroup.rs:72`). Use `let … else { unreachable!("…") }`
with the same literal, or a panic-free equivalent. `unix.rs:22`, `160` and
`167` would print workspace, protected-directory, cgroup or request-cwd bytes.
They are unreachable only while `clean_path` refuses NUL (`mod.rs:164`,
`cgroup.rs:38`).

**What this proves, and what it does not.** The brief's P-SCAN and T10 can
show three things. Owned panic-reachable code carries only fixed payloads. The
bridge and dispatch path add no marker bytes to stdout or stderr for the faked
classes. The default hook does print payloads; a negative control shows it.
They do not show the absence of private data in std, tokio or dependency
payloads, in implicit panics, in aborting panics, under a non-default hook,
or in the root-only native paths, or in real journald or log contents. All of
those stay open, and native paths remain **unverified on real hardware**.

**Activation gate additions (stage G).**

- **G-P1.** Audit every explicit panic site on the Linux standard dispatch
  path outside the scanned set: executor dispatch, `complete`, `assess` and
  `output_value`, the store writes used by `complete`, `Execution::preflight`,
  and `ServiceConfig::execution_preflight` with `NativeResolver`. Record each
  site's payload class. Any data-bearing payload is fixed under P1, with the
  file added to P-SCAN, or activation stops.
- **G-P2.** The release profile must stay unwinding (no `panic = "abort"`). The
  test suite cannot show this (*source inference*: Cargo ignores the profile
  panic setting for tests).
- **G-P3.** Suppose a reachable data-bearing payload sits in code this
  repository cannot change. Activation then stops until a separate,
  independently reviewed service diagnostic ADR exists. It is limited to one
  hook installed once in `service::run` before the runtime, never swapped or
  restored, with fixed text only, unchanged exit and restart semantics, and
  its own subprocess test. This ADR neither designs nor authorizes it.

### C3. Recheck placement (D4)

One pure helper, `recheck(cancel, shutdown) -> Option<RunFailure>`, runs at
two sites. Here `shutdown` is the runner's token or the caller's token. The
first site is the existing check after `account` (`unix.rs:148`). The second
is a new final recheck immediately after `Command` construction and
immediately before `command.spawn()`. Shutdown wins: the phase is
`NotStarted::Shutdown`, otherwise `NotStarted::Cancelled`, and the legacy
value at both sites is the existing literal. Keeping the early check preserves
legacy order exactly. It also avoids a root `mkdirat` and removal of a cgroup
leaf for a request that is already dead. The poll loop uses the same
`shutdown_requested(runner, caller)` helper.

## Consequences

- One more ADR 0007 public type and method. `Runner::run` keeps its semantics
  and exact error values, except for the three labelled C1 deviations.
- The shim is new trusted code that runs as the build account. A bug in it can
  only act with build-account rights. Its integrity relies on the root-owned
  binary directory (ADR 0009 protection rules), as on Windows.
- Exit codes `125`–`127` and `128 + n` are ambiguous between the shim and the
  target. That is documented, not hidden.
- Pre-creation refusals map to rows D, E or F and do not quarantine. Spawn and
  post-spawn errors are row G and quarantine, as ADR 0009 and ADR 0017 require.
- A missing workspace root, an unreadable shim binary or any failed `pre_exec`
  step still quarantines. These are owner or install drift.

## Relation to other ADRs

- **ADR 0007:** additive API (D1, D3), reordered recheck (D4) and argv
  allowance (D5). Identity, capability, descriptor, cwd, cgroup and cleanup
  fences are unchanged.
- **ADR 0009:** makes the Unix "shim" and `tool-probe` concrete (D2). It
  deviates from "Runner unchanged in principle" only in that the runner execs
  the fixed shim, never the agent target. Tool discovery semantics, timeouts
  and output caps are unchanged. The ADR 0009 cgroup setup sentence is
  incomplete: the runner (`cgroup.rs:57-66`) also requires `+memory +pids` in
  `runs/cgroup.subtree_control`, not only in the unit cgroup. The setup slice
  must enable both.
- **ADR 0017:** no change. This ADR supplies the creation classification for
  each Unix site. If the running 0017 review changes the `Report` or `Creation`
  contract, this ADR must be rechecked first.

## Sources and inference

Repository sources read at `07ae3c89`: `crates/arxd/src/exec/{mod,unix,cgroup,output,tests,disposable}.rs`,
`crates/arxd/src/service/{executor,config,preflight,application,layout}.rs`,
`crates/arxd/src/development/{mod,types}.rs`, `crates/arxd/src/main.rs`,
`packaging/linux/arxd.service` and its standard-rights drop-in, and ADRs 0007,
0009, 0014, 0015 and 0017. Nothing was run, and no external source was fetched
for this ADR.

Clarification re-pin, `1d828ca7`. Between `07ae3c89` and `1d828ca7` the
following are byte-identical: `exec/`, `development/`, `main.rs`,
`service/{config,preflight,layout}.rs`, `packaging/`, `Cargo.toml`,
`rust-toolchain.toml` and ADRs 0007 and 0009. So every line cited above still
holds.

- `service/executor.rs` adds only `configured_rights`, and
  `Execution::native` gains that parameter. The `#[cfg(test)]` bridge
  constructor must pass it. `Report`, `Creation`, `Executor` and dispatch are
  unchanged.
- `service/application.rs` adds only store exit classification.
- ADR 0017 adds only status checkpoints. Its contract text is unchanged, and
  its validator merged as #42 (`4c2264d`).

C2 quotes the std docs at doc.rust-lang.org (1.99.1). The fetched URLs held no
repository or owner data.

Source inference, not verified:

- That std `Command::spawn` with a `pre_exec` closure forks (not
  `posix_spawn`), sends child-side errors over an internal CLOEXEC pipe, and
  returns them as `Err`. This only supports classifying `Err` as `Unknown`.
- That `ProtectHome=yes` and `PrivateTmp=yes` in the standard-rights drop-in
  apply to runner children through the inherited mount namespace. If true, the
  workspace and toolchain must live outside `/home`, `/root` and `/run/user`,
  and `/tmp` is one private directory that all runs share until restart.
- That cgroup v2 migration of the child into `runs/<leaf>` needs only the
  root-owned file permissions inside the service's unit cgroup under the
  drop-in's `CapabilityBoundingSet`.

## Clarification-review correction checkpoint — 2026-11-12

Independent Sol high review of b68a73eb passes Standards but requests B1 in
the test contract. The coordinator corrects T3's validator name and requires
L2b to exercise legacy Runner::run at its item/aggregate-byte boundary,
including the wrong-entry-point mutation. Review and correction are retained
in the accompanying reports. This proposal remains unaccepted pending a
fresh independent recheck; no implementation or activation is authorized.

The reviewer verified the panic facts against pinned Rust 1.95 documentation,
including that slicing diagnostics can include a truncated string excerpt.
Earlier author's 1.99 evidence remains historical. These source facts do not
prove absence of private data in dependency/implicit/aborting panics or logs.
