# ADR 0020 correction 1 independent Sol recheck

Reviewer: OpenAI / gpt-6.1-sol / high, 2026-11-12. Independent of the
Anthropic / Opus 5.5 high author. Single review agent; no subagents.

Disposition: **changes required; proposed contract remains unaccepted**.
This is docs-only design review, not implementation review or permission to
implement, activate, merge, retry an experiment or perform owner operations.

## Exact scope and comparison pins

- Prepared HEAD: `009c8b82cc738304eab4d298fd6a57fe2c124081`.
- Target: `66924d0e97d8168ce2eab0b4acf46b3b5a6ba9bf`, retained as
  `680b143c653ac333553d12c0af195bfc2d820008`.
- Both correction parents: `d44035dee79403b6386215d4aa8ca5254f37dc35`.
  Both correction trees: `e48f43aa57198dda62a8c323c19f3b57254b57d4`.
- Original proposal: `e3928f5f41530b4055958b82ee304a156bc3c9b8`, retained as
  `54032bf4e3da3b7c6f5b0008e5bef6ccd09438e7`.
- Original independent review: `8ff4196ab0d396c03c3d4de82c787c74af370fff`,
  retained as `8ff3a549766fe1217cdf7ae8d5a0a2cad8354878` in
  [0020-sol-independent-design-review.md](0020-sol-independent-design-review.md).
- Production-source comparison: `fdea1c6a5d9954e0ff9d20f10e7d8d9a7f930b28`
  (PR55 Stage D on Stage C), not main after PR52 `1604db1`.

`Get-Location` and `git rev-parse --show-toplevel` agreed on
`C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-e500b3ba-fc3fafc4eabb1264`,
distinct from canonical `D:/Code/Arxcovo`. Initial status was clean. No
checkout was created or switched. The retained correction has the identical
whole tree, not merely similar prose. Proposal/review retention comparisons
and `git diff fdea1c6 66924d0 -- crates packaging` were empty.

Reviewed all four correction files: the [ADR](../adr/0020-linux-execution-hierarchy-and-janitor.md),
[brief](../briefs/0020-linux-hierarchy-implementation.md),
[author disposition](0020-correction1-disposition.md) and
[evidence](0020-design-evidence.md). References and line numbers below are
at the target; actual source references are unchanged from `fdea1c6`.
The author disposition is a claim to check, not independent closure evidence.

## Standards

**S1 closed at design level.** D8 now anchors `/`, checks each protected
ancestor through descriptor-relative no-follow opens, validates the final
regular root-owned object through `O_PATH|O_NOFOLLOW|O_CLOEXEC`, and executes
that object with `execveat(AT_EMPTY_PATH)`. All candidates are classified
before execution; an untrusted candidate prevents every execution. This
addresses the original replaceable-ancestor and check-to-exec faults.
The exact root-owned `/bin -> usr/bin` or `/usr/bin` alias is dropped, never
followed; unusual layouts deliberately lose availability. Accept R-2.

The selected descriptor must remain owned and open through fork and the
child's exec attempt; CLOEXEC closes it only on successful exec, not fork.
The error-pipe ends need explicit ownership/closure on every branch so an
exec-error read cannot wait on the parent's own write end. These follow from
the specified fork/error-pipe contract and are implementation-review checks,
not evidence that code exists. Empty environment and system loader/library
trust are explicit. Renaming entries cannot redirect the held object;
in-place writes by privileged actors remain an explicit trust assumption.
There is no root-identity or sudoers-policy proof.

The [execveat manual](https://man7.org/linux/man-pages/man2/execveat.2.html)
supports O_PATH execution and the CLOEXEC interpreter-script ENOENT case.
S8's script must leave no marker; ELF helper copies exercise successful
execution without root or setuid. This is script rejection through the exec
contract, not an implemented ELF-header validator or a claim about every
possible binfmt handler. No path fallback is acceptable. S9/S9c marker tests,
S12 constants and S13 source checks are feasible ordinary-user requirements.
They have not run. Installed sudo behavior remains A7/A11 evidence.

**S2 closed at design level.** D10 and E6 now share the broader panic-scan
inventory, including setup/parser/Linux adapter/readiness and changed runner
and shim files; the test fake is explicitly excluded. H15 checks the default
hook in a subprocess. Fixed payloads, unread caught payloads, the unwinding
release gate and dependency-panic limitations remain. Catching a panic alone
does not sanitize its hook. No new Fowler smell-baseline finding is warranted.
Repository routing, privacy, inertness and honest-evidence rules are retained.

## Spec

**P2 remains blocking: the new bound is proved for one admission domain,
but is still stated for the entire process.** ADR D6:337-379 defines a flag
per `Runner`, then concludes "at most L runner calls in one process
lifetime"; Bounds and Consequences repeat that scope. The proof uses one
`active` modification order and one shared readiness state. Neither the
assumptions nor E5/E6 prohibit a second runner/bridge/executor domain or a
replacement with a fresh flag and Ready state in the same process. E5 R3
explicitly requires a fresh runner's flag to be false. Two independent
limit-1 domains can each leave a leaf; sequential replacement can repeat it.
These are counterexamples to the stated scope, not an observed production
escape (production remains inactive).

Bounded correction: state the bound per immutable shared admission domain
and explicitly require one such domain for the service's exclusive `runs/`
subtree for its entire lifetime, with no replacement/reset before restart.
If several runners are allowed in that subtree, require common readiness,
slot accounting and residue observation, and prove that arrangement. Pin
the chosen ownership in wiring and add a multiple-runner/replacement test.
Do not restore a global four-orphan claim. No global static is required.

Within one such domain the proof is sound: the residue release store and
post-call latch precede the AcqRel slot decrement. Subsequent successful
increments acquire the preceding RMW chain and the in-slot gate sees the
monotone state/flag. Even busy increments and decrements are AcqRel RMWs.
At the first residue call's release, at most L calls can already hold slots.
Calls stalled after preflight but before acquiring a slot are refused at
the gate. This is reasoning from the source and
[Rust's ordering contract](https://doc.rust-lang.org/std/sync/atomic/enum.Ordering.html),
not an executed concurrency test. T18/T19 are useful requirements; neither
establishes the missing multiple-domain assumption.

**P6 new blocking: A5 cannot be wired through the permitted file boundary.**
ADR D1:124 says `exec/unix.rs` is unchanged; E5 lists only cgroup.rs and
the Runner owner. But `Runner::run_core` in `exec/mod.rs:202` calls
`unix::run` with config/spec/tokens only, and `unix.rs:219` alone calls
`Group::create(c)`. That function has no access to the new per-runner Arc.
Giving Group the required clone therefore needs plumbing through unix.rs,
or a different explicitly designed ownership arrangement. A static or an
OwnerConfig-carried agent/owner-facing flag is not the specified design.

Bounded correction: permit exactly the private Arc parameter plumbing through
`Runner::run_core -> unix::run -> Group::create` (Linux-gated as needed),
list unix.rs in E5, and replace "unchanged" with unchanged behavioral order,
errors, cancellation checks and fences. Preserve both legacy and phased
results, including the original errno captured before create-path cleanup.
This is a narrow contract repair, not permission for another runner redesign.

**P1 closed.** D4.3/E3 use a new `openat(dir, ".", ...O_CLOEXEC)` description
per complete list; no dup or cached stream. L9 checks repeated enumeration,
add/remove visibility, distinct descriptor, CLOEXEC and untouched handle
offset; H18 refuses a final leftover. This matches
[open's independent descriptions](https://man7.org/linux/man-pages/man2/open.2.html)
and [fdopendir's descriptor ownership](https://man7.org/linux/man-pages/man3/fdopendir.3.html).

**P3 closed.** S9, waits, leaf checks and verification use one bounded strict
populated parser, accepting valid extra keys. Entry kinds, DT_UNKNOWN errors,
links, control-name grammar and the dot-record budget are explicit. P3/P9,
H8/H9/H18-H20 and L10 cover the distinction. Accept R-3's availability cost
for future control names and R-4's VerifyFailed quarantine. These agree with
the [kernel cgroup interface](https://docs.kernel.org/admin-guide/cgroup-v2.html);
ordinary owned-directory tests cannot prove cgroupfs behavior.

**P4 closed.** L8 now expects Hardening before Identity, with zero writes and
explicit skip markers; H10 isolates Identity under fake hardening success,
and H10b tests precedence. A1's exact arxd.service component intentionally
changes accepted parser inputs, with P1 refusal fixtures rather than the old
incompatible promise. No privileged fixture is needed.

**P5 closed.** Rights values now mean in-fence exit observations only. The
[kernel NNP guarantee](https://docs.kernel.org/userspace-api/no_new_privs.html)
prevents set-ID/file-capability exec gains, but excludes non-exec channels.
No unfenced probe or NNP removal is proposed. Ordinary ELF/NNP seams are
feasible; whether installed sudo returns the predicted status is unverified.

**P7 advisory: make W3e's barrier precise.** E7's "after dispatch" barrier
does not itself hold a spawned worker before its runner call. A worker may
already pass the checks before shutdown. Hold it before its final
shutdown/admission check to assert zero calls; separately allow already
admitted work to cancel/drain with its honest row. This avoids a flaky
zero-call expectation and does not require serializing all execution.

## Independent amendment and open-decision dispositions

| Item | Recheck disposition |
| --- | --- |
| U1 | Accept discovery-time diagnostics and Ready before discovery on the per-run NNP fence basis; cadence deviation is explicit. |
| U2 | Accept exact mandatory user.delegate plus cumulative ownership/type/placement checks; no trusted.delegate fallback. |
| U3 | Accept conservative Containment/Resource latching; process-wide bound remains blocked by P2. |
| U4 | Accept exact packaged CapBnd c1 and NNP=1; no inference of effective/permitted sets or successful writes. |
| U5 | Accept Pending mapping, first-cause CAS and restart-only reset. |
| U6 | Accept whole-subtree kill including unknown-name processes, with removal withheld for unexpected structure. |
| U7 | Accept no startup retry; terminal failure needs restart. |
| U8 | Accept unknown/untrusted fail-closed diagnostic classification, distinct from absence. |
| U9 | Still unresolved installed systemd directory removal; Stage F A9, not design-review evidence. |
| A1 | Accept shared locate parser and deliberate exact-unit tightening, with P4's amended fixtures. |
| A2 | Accept strict extra rights line; invalid table remains K/no tools, valid failing rights fact may remain N/tools plus latch. Assessment and full identity match still control release. |
| A3 | Accept in-slot gate and readiness transitions locally, conditional on P2 ownership repair; no added report token. |
| A4 | Accept unified scan inventory and mutant requirements; no service hook change. |
| A5 / R-1 | Accept payload-free residue signaling in principle; withhold complete acceptance pending P2 and P6. |
| R-2 / R-3 / R-4 | Accept exact bin alias, fail-closed control-name grammar, and VerifyFailed quarantine respectively. |

## Cleanup, integration and remaining limits

Actual cgroup/unix source was checked, not inferred from the disposition.
Both post-mkdir create failures need release_leaf. Before Group creation,
invalid identity/account/spec or early cancellation creates no leaf. After
creation, socketpair failure, final cancellation/shutdown, spawn failure,
identity-read/output/wait errors and unwinding all release Group through
Drop. ChildGuard first attempts kill_tree; Group Drop can retry a failed
confirmation. Already-removed groups stay clear; an unconfirmed final Drop
sets the flag. The existing false cleanup result still produces H even if
Drop later succeeds. Live-process uncertainty keeps G/H quarantine.
Cancellation E, shutdown refusal, F resource failure, and output/identity
release rules are not changed by the side signal. R1/R2 cover representative
ordinary-directory failures, not every privileged path or test pass.

Concurrent startup and agent discoveries must use the same admission domain
and cache ticket allocator. Rights latches do not revoke reports of work
already admitted; each still undergoes ADR 0017 assessment. The cache uses
successful redacted response metadata only, with greater-ticket replacement
under its mutex. Ticket order prevents earlier-start overwrite; it does not
prove wall-clock monotonicity or later actual observation. No cache reset or
runner reconstruction may reset a terminal readiness latch. No new lock cycle
is apparent: admission uses atomics; dispatch releases the workers mutex
before awaiting its result; the sequence does not hold the cache mutex while
awaiting execution. Drain ordering is safe because shutdown serializes new
dispatch with JoinSet transfer; the tracked sequence can receive a synchronous
stopping completion after executor drain. P7 qualifies the test expectation.

Manual-clock waits are explicit. Five-second waits are logical polling
deadlines; individual opens/reads/writes, fork, error-pipe handling and reap
may overrun them. The nominal 50-second probe budget is not a hard wall-clock
bound. The existing runner timeout, 10-second drain failure and systemd
backstop do not prove interruption of every blocked syscall. No deadlock fix
or stronger timing guarantee is credited without implementation evidence.

## Checks actually performed

Read-only git identity/status/log, parent/tree resolution, retained-file and
source equality comparisons, diff statistics and targeted diff inspection;
rg file/text searches and source reads. Read AGENTS, KICKOFF, CONTRIBUTING,
the review skill/profile, original review, all correction documents, ADR
0019/C2 gates and ADR 0017 rows/release rules, relevant ADR 0009 lifecycle and
rights sections, runner/cgroup/Unix/shim, bridge/executor/application/service,
store/MCP integration points, scanner and both Linux packaging files. Public
documents linked above were fetched without repository or private data.
One wrong ADR filename search and one unquoted PowerShell tree-expression
command failed; corrected paths/quoted expressions succeeded.

Report checks performed before commit: strict UTF-8 without BOM, LF-only
bytes, local relative Markdown link existence, `git diff --cached --check`,
and staged-path restriction to this report. No build, behavior test, mutation
test, native run or CI check was run. Every future named test is a requirement.

No implementation/PROGRESS/proposal edit, private-state read, native/root/
owner/account/service/cgroup/VM/Lab Manager/experiment action, CI tuning,
retry, push or GitHub write occurred. Production remains MechanismUnavailable
on every OS. Stage C PR53 and Stage D PR55 remain unmerged per supplied task
context; GitHub state was not refreshed. Implementation waits for their merge
and design acceptance. CI investigations remain parked. Native, installed and
dedicated paths remain **unverified on real hardware**; M2 is incomplete.
ExecutionFailed / E_PROTECTION remain unknown, with no waiver or retry.

Standards: 0 open findings; S1/S2 closed at design level.
Spec: 2 blockers (P2 bound domain, P6 flag plumbing), 1 advisory (P7 shutdown
barrier); worst is the unsupported process-wide bound.
