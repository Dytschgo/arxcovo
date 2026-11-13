# ADR 0020 independent Sol design review

Reviewer: OpenAI / gpt-6.1-sol / high, 2026-11-12. Independent review of
Anthropic / Opus 5.5 high authorship; one review agent, no delegates.

Disposition: **changes required; proposal remains proposed, not accepted**.
This is design review credit, not implementation review, hardware acceptance,
permission to implement, or permission to merge.

## Pinned scope and evidence identity

Reviewed target: `e3928f5f41530b4055958b82ee304a156bc3c9b8`.
Verified parent: `fdea1c6a5d9954e0ff9d20f10e7d8d9a7f930b28`.
Prepared checkout HEAD was the parent, on `t3/review-stage-e-architecture`,
with an initially clean tree. `Get-Location` and `git rev-parse --show-toplevel`
agreed on the runtime-prepared isolated worktree, distinct from the canonical
checkout. No checkout was created or switched. Proposal contents were read
with `git show e3928f5:<path>`; checkout HEAD was never substituted for target.

Exactly three added proposal files, 1,003 lines, were reviewed:

- `docs/adr/0020-linux-execution-hierarchy-and-janitor.md` (ADR below).
- `docs/briefs/0020-linux-hierarchy-implementation.md` (brief below).
- `docs/reviews/0020-design-evidence.md` (author evidence below).

Proposal line citations below refer to those blobs at the full target above.
Implementation and prior ADR citations refer to the parent above. Markdown
relative links to proposal files will resolve when the coordinator combines
the proposal and review; this review commit intentionally contains only this
new report.

## Standards

**S1 — blocking: fixed sudo paths do not establish the claimed trusted
executable.** [ADR D8, lines 329–338 and consequences 425–428](../adr/0020-linux-execution-hierarchy-and-janitor.md)
checks only final-file `lstat` ownership/mode, then executes by path. A
root-owned file inside a replaceable ancestor can be swapped after checking;
intermediate symlinks are followed. Fixed names remove PATH search but do not
prove the claimed impossibility of a planted false pass. This violates the
AGENTS trust/path fence and contrasts with the ancestor checks in
`exec/unix.rs:531–548` and descriptor walk at `:37`.

Bounded correction: define the executable trust boundary, validate protected
ancestors and bind execution to the validated object, or explicitly rely on
an immutable trusted system tree with checked ancestors and a stated
privileged-writer assumption. Account for normal `/bin` aliases without
silently allowing arbitrary symlinks. Add ordinary-user owned-tree seams for
replaceable ancestors, intermediate/final links, and check-to-exec replacement;
the executable marker must prove the untrusted target never ran. S9/S10 alone
exercise only final ownership and PATH and cannot catch this.

**S2 — advisory: unify the panic-scan contract.** ADR D10 names the hierarchy
root and readiness, whereas brief E5 includes `exec/hierarchy/*.rs`. Adopt
the broader inventory explicitly, including setup, parser and Linux adapter
production sections. ADR 0019 C2/P1–P7 (`0019:314–345`) explains why catching
a panic does not sanitize the default hook. Keep H15's subprocess hook
evidence and fixed payloads; do not infer privacy from `catch_unwind` alone.
No new smell-baseline concern warrants a separate finding in these docs.

## Spec

**P1 — blocking: re-listing must start at the beginning.** Brief E3
(`:171–174`) prescribes `fdopendir` on a `dup`, but D4 (`:169–182`) requires
both enumeration and independent final re-enumeration. Duplicates share the
open-file-description offset; a second listing can start at EOF. This defeats
the evidence for zero remaining directories. That API fact is documented by
[dup(2)](https://man7.org/linux/man-pages/man2/dup.2.html); the janitor failure
is an inference from the prescribed combination, not an observed run.

Bounded correction: specify fresh directory descriptions, or explicit safe
rewinding with serialized ownership, for every list invocation. Preserve
CLOEXEC (plain `dup` also clears that descriptor flag). Add a real owned-temp
adapter test that lists the same handle twice, then adds an entry between
lists and requires it to be observed. Fake H3/H6 cannot establish this OS
property. Inject a leftover at final verification and require refusal.

**P2 — blocking: the claimed maximum of four orphan leaves is not proved.**
ADR D6 (`:232–266`) both excludes cancellation from the latch and claims at
most `max_concurrent_runs <= 4` orphans. Repeated final-recheck cancellation
with failed Drop cleanup can accumulate empty leaves without any latch.
Additionally `executor.rs:337–352` checks preflight *before* incrementing
`active`; a worker can pass preflight, pause, and enter after a different run
latches and frees a slot. A concurrency limit bounds simultaneous work,
not all workers that already passed an earlier readiness check. This also
qualifies the promise that later requests make zero runner calls.

Bounded correction: withdraw the global four-orphan claim unless admission
and failure latching are serialized and every residue source is covered.
Keep cancelled rows unchanged; a typed cleanup/residue signal can be designed
as an explicit limited runner-contract amendment if needed. Alternatively
document the unbounded cancel residual honestly and decide whether it meets
the containment/resource requirement before acceptance. Add barrier tests
with limit 1: hold a worker after preflight, latch in another, then release;
also repeat cancellation plus failed cleanup more than four times. Define
whether already admitted work may continue, without treating an empty orphan
as a live-process escape. The current T11–T13 serial tests miss both cases.

**P3 — blocking: make cleanup postconditions internally consistent.**
ADR S9 (`:146`) says `cgroup.events` is *exactly* `populated 0`, although D4
accepts other valid keys and brief P3 explicitly accepts `frozen` and
`future_key`. A normal file may include `frozen 0`; use the same strict parser
and require its populated result to be false, rather than exact whole-file
equality. The kernel documents both keys in
[cgroup.events](https://docs.kernel.org/admin-guide/cgroup-v2.html#core-interface-files).
Also D4 counts only directory entries, while H8 demands that a symlink child
fail: specify explicit refusal for links/unexpected entry kinds, distinguish
kernel control files from child directories, and exclude `.`/`..` from child
classification while defining whether they consume the listing budget.
H1/H2 must include realistic multi-key events and ordinary adapter tests
must cover links and repeated lists. This is a contract ambiguity, not a claim
that normal cgroupfs allows users to create symlinks.

**P4 — blocking: L8 cannot reliably reach its expected failure.** Brief L8
(`:193–196`) expects ordinary-user `setup(system())` to return `Identity`,
but ADR D3 S0 runs R4 before T1. An ordinary test process without the exact
NNP/CapBnd pair returns `Hardening` first. Change the expectation to the
actual precedence, and use H10's fake hardening success to isolate Identity;
retain zero writes and a non-root execution guard. Do not force a privileged
test environment to satisfy this fixture. P1's "every Stage D T2 fixture
keeps its result" also needs an explicit exception for the intentional A1
unit-name tightening. Test the changed classification instead of preserving
an incompatible old expectation.

**P5 — advisory: distinguish policy rights from in-fence observations.**
D8 calls zero exit `effective`, but it observes a command status, not a root
identity or sudoers policy. The separate A7 already predicts identical refusal
with and without NOPASSWD under NNP. Narrow the claims and cite NNP's upstream
guarantee; do not advertise slice 9 as detecting all account-level escalation
rights. This also resolves the stale discovery observation question below.

## Decisions on U1–U9

These are review recommendations for the next bounded proposal revision;
they do not accept this target or authorize implementation.

| Item | Decision and boundary |
| --- | --- |
| U1 | Accept discovery-time diagnostics, with P5's narrowed meaning and explicit ADR 0009 cadence deviation. NNP is set and capabilities cleared on every run (`unix.rs:242–310`); discovery is not the security gate for set-ID exec. Ready before initial discovery is acceptable only on that basis. If successful rights observation is instead a prerequisite, use an internal discovery-only admission state and test a racing agent request; the present design does not provide it. A previous discovery cannot prove current sudo policy. Do not remove NNP or add an unfenced privileged probe to obtain that proof. |
| U2 | Accept mandatory exact `user.delegate = 1`. The marker, root ownership, cgroup2 type and exact `arxd.service/supervisor` placement are cumulative checks; no fallback to `trusted.delegate`. Missing marker disables execution while inventory remains available. |
| U3 | Accept conservative Containment/Resource latching, even for ENOMEM, as an availability tradeoff with unchanged row D/F. Reject the claimed bound pending P2. Cancellation does not itself prove successful cleanup. |
| U4 | Accept exact `CapBnd = 0xc1` and NNP=1 as the packaged standard profile. A subset is not an equivalent profile: missing SETUID/SETGID can prevent the identity drop. This does not establish effective/permitted capability sets or successful cgroup writes; Stage F retains those checks. |
| U5 | Accept Pending mapping to existing `mechanism_unavailable`. No new response token is necessary. Keep first-cause CAS semantics, terminal failures and restart-only reset. |
| U6 | Accept killing the entire trusted `runs/` subtree before enumeration, then refusing unknown/nested names without deleting them. Explicitly say unknown-name processes are killed too; only removal is withheld. The boundary is the verified exclusive execution subtree, not the name grammar. P3 must settle entry kinds. |
| U7 | Accept no automatic startup-discovery retry. Inventory continues, no false cache refresh, agent discovery can refresh later only while readiness permits it. A terminal latch still needs restart; no agent reset. |
| U8 | Accept `unknown` => `rights_mismatch` conservatively for this diagnostic contract. It costs availability and proves no sudo policy fact. Include spawn/signal/deadline cases and distinguish absent from an untrusted candidate. |
| U9 | Keep systemd directory-removal behavior unresolved for the installed versions; absent/present runs and leftovers are both covered by the design. Upstream KillMode=mixed documents process termination, not a portable guarantee of delegated-directory removal. Stage F must verify the actual package. |

## Decisions on amendments A1–A4 and integration

- **A1:** accept one pure bounded locate parser with exact unit component;
  correct P1's fixture promise as in P4. T1–T8 precede controller writes.
  Keep descriptor-relative no-follow walks, per-component ownership/mode and
  filesystem checks. The supervisor PID check is consistency evidence on the
  trusted mounted hierarchy, not cryptographic proof against a privileged
  namespace/mount adversary. No unprivileged build can mount a substitute;
  privileged mounts/writers remain a trust assumption. Test wrong magics,
  ownership, symlink components and substituted fake handles before writes.
- **A2:** accept an additional strictly parsed rights line only with the
  narrowed semantics above. Missing/duplicate/extra lines remain row K,
  quarantine, zero released tools. Valid-table rights failure may still be
  row N with tools and a terminal rights latch; make that deliberate behavior
  explicit. Tools are released only after ADR 0017 assessment and full pinned
  identity match, never directly from bridge output.
- **A3:** accept atomic Pending/Ready/Unavailable structure and terminal
  latches subject to P2's admission correction. Pending setup failure is
  nonfatal to inventory. Janitor timeout/removal failure/panic maps to
  quarantine; missing mechanism maps to mechanism_unavailable. A kill-write
  error does not prove no live processes, though refusal until restart is
  still safe. Preserve that distinction in cleanup claims.
- **A4:** accept broader scanner inventory with S2 reconciliation and mutant
  self-tests; no service hook changes. Explicitly include newly split files.

Enabling controllers on both the service and `runs/` is required: parent
enablement exposes child interface files, then runs enablement exposes leaf
limits. S5/S8 readbacks and S9 swap-interface check are appropriate. systemd
delegation makes controllers available without enabling them; supervisor
placement keeps internal nodes empty. Do not touch `.control/`, supervisor,
other units, or slices. The janitor confirms populated=0 and successful
removal separately; neither alone proves complete cleanup.

D7's janitor-before-discovery ordering is sound. Keep row insertion after
setup, daemon identity, ordinary dispatch/ADR 0017 classification, and cache
updates only from successful redacted response metadata. No Evidence-derived
identifiers, rights text or raw stdout in cache, rows, responses or logs.
Concurrent discoveries may finish out of order: document completion-order
cache replacement or compare observation timestamps (advisory); timestamps
must not imply stronger freshness than the observation.

Shutdown still has an independent execution cancellation gate, so a late
setup Ready must not dispatch a child. Add a barrier at S9/S10 and between
Ready/row insertion/dispatch; assert shutdown yields no runner call, terminal
row handling is honest, and drain owns all spawned work. W3 covers only the
kill wait. Tokio time pausing alone does not advance a blocking std clock;
connect the injected clock explicitly. Individual cgroupfs calls, sudo spawn
and reap can stall despite logical polling deadlines. Retain the documented
10-second service drain failure semantics and systemd backstop; do not claim
every syscall or task is interrupted by a timeout. Panic-abort is not caught;
keep ADR 0019's unwinding activation gate and dependency-panic limitations.

## Primary-source verification and limits

Public upstream documents fetched during this review (current heads, not an
installed-distribution certification):

- [Kernel cgroup v2](https://docs.kernel.org/admin-guide/cgroup-v2.html):
  multi-controller writes, recursive populated state, recursive cgroup.kill,
  fork/migration handling and frozen-task fatal signals support the proposed
  ordering. Ownership/capability sufficiency on the installed system remains
  inference and Stage F evidence, not proved by this review.
- [systemd delegation](https://systemd.io/CGROUP_DELEGATION/): marker,
  DelegateSubgroup, single-writer convention and enablement responsibility.
  Single-writer is a convention, not a kernel exclusion lock.
- [Kernel no-new-privileges](https://docs.kernel.org/userspace-api/no_new_privs.html):
  inherited, irreversible NNP blocks set-ID/file-capability gains through
  exec. Thus ordinary setuid sudo under this runner cannot test whether an
  unfenced login has NOPASSWD rights. NNP does not block all non-exec privilege
  channels; do not generalize it to complete account-policy proof.
- [systemd.kill source](https://raw.githubusercontent.com/systemd/systemd/main/man/systemd.kill.xml):
  mixed sends termination to main and final kill to remaining unit processes.
- [systemd.exec source](https://raw.githubusercontent.com/systemd/systemd/main/man/systemd.exec.xml):
  checked filesystem protection semantics against existing packaging. The
  freedesktop rendered kill/exec pages failed to fetch (400 timeout); raw
  upstream sources were the fallback. No installed behavior was exercised.
- [dup(2)](https://man7.org/linux/man-pages/man2/dup.2.html): shared offsets
  and cleared close-on-exec flag, used for P1.

## Checks actually performed and retained gates

Read-only inspection commands: `Get-Location`; `git rev-parse --show-toplevel`;
`git status --short` / `--porcelain=v1`; `git rev-parse HEAD`;
`git branch --show-current`; `git cat-file -p <target>`;
`git log --oneline <parent>..<target>`; `git diff --stat <parent> <target>`;
`git diff --name-status <parent> <target>`; `git show <target>:<scope-file>`;
`rg --files` and targeted `rg -n` / `Get-Content` reads. One wildcard `rg`
invocation for prior ADRs failed under PowerShell path handling; subsequent
reads used their explicit paths. Read AGENTS, KICKOFF, CONTRIBUTING, the
dytschgo-review skill/profile, ADRs 0009/0017/0019, brief 0019, current
cgroup/Unix runner/shim/bridge, executor/application/service lifecycle,
packaging, and relevant progress status. Public web reads are listed above.
Report-only formatting/diff verification and commit are recorded below.

No build, test, mutation run or CI check was run or claimed. Future H/P/L/S/T/W
tests are requirements, not passes. Ordinary-user fake and owned-temp tests
are appropriate after the corrections; they cannot prove delegated kernel
behavior. Privileged acceptance remains separate Stage F and is unauthorized
here. No native/root/owner/account/service/cgroup/VM/Lab Manager/experiment
command, private state read, implementation edit, PROGRESS edit, CI tuning,
retry, label, push or GitHub write was performed.

Production must stay `MechanismUnavailable` on every OS; bridge construction
and wiring remain test-only until all activation gates. No Linux admin or
macOS allowance, report weakening or new product approval flow is recommended.
Stage C PR53 and Stage D PR55 remain unmerged on main `1604db1` in the supplied
task context; this review did not refresh GitHub state. The parked CI budget
investigation neither blocks design review nor waives any merge gate. Only
the coordinator merges after independent review and green required checks.
Native, installed and dedicated paths remain **unverified on real hardware**;
M2 is incomplete. ExecutionFailed / E_PROTECTION remain unknown: no fix,
waiver or retry.

Report artifact checks: `git diff --cached --check` and a byte-level UTF-8/no-BOM/LF
check were run before commit; only this report was staged.

Standards: 1 blocking, 1 advisory; worst is unbound sudo executable trust.
Spec: 4 blocking, 1 advisory; worst is unsupported orphan/admission bound.
