# CORE M2 Stage F: smallest A1/A2 alignment

2026-11-14; OpenAI / gpt-6.1-sol / high; design sub-agent. **Proposed,
accepted for source preparation after Grok review `43656c2`, with the
capture correction incorporated by Sol medium.** Native execution and publication
remain separately closed. Delivery of the original task was this document only.
No runtime authority, historical diagnosis, native pass or activation follows.

## Pins and boundary

- Isolated assigned worktree: cwd equals Git top-level; branch
  `t3/m2-stagef-alignment-proposal`; initial status clean, HEAD
  `4a883906d0c399a5dcfd5c0f81b9034278ad886f`.
- Public-source baseline / cached `origin/main`:
  `625eeb062bd89c1b2692b2cc99759a809e69e849`. Full-tree diff against initial
  HEAD lists **only `docs/PROGRESS.md`**. Thus no source/workflow/package
  changes follow that baseline in this checkout. Local `main` is stale at
  `176af7a180387c0423fe70a013567d99def9f14c`; no network read was made and
  live public-main freshness is not independently asserted.
- Baseline blobs: driver `bb5a3004a6420379a2c7fb4cd327d840181d864e`;
  disposable fixture `586b30811b3af485f6b96f7a4a989b2ba64e3e70`;
  setup `b33bed7cd5d7022f30c039e761c544567304d28c`;
  packaged unit `0b337773c60e49fe84de76db018d2235ae076254`;
  standard drop-in `93a3abcce9782cc7fddf4c2c1dcc5fc74ec18a74`;
  manual workflow `e5f1a96dbfa522bda602db45bf24db000ebddd5c`.
- Read AGENTS, KICKOFF, installed engineering skill/profile, ADRs 0019/0020,
  their implementation briefs, hierarchy location/setup/Linux/fake seams,
  disposable fixture, driver staging/build/service/mutation/cleanup paths,
  pure tests, mutation runner, packaged profile, validation guide/README and
  manual workflow. Historical Phase 1 report read from
  `20f39c02953c82f11ef8871450f856611a49e1e3`:
  `docs/reviews/unix-protection-phase1-evidence.md`.

Scope is the settled brief's A1 (initial layout/marker/controllers) and A2
(production setup's controller/runs/postcondition result), on one newly owned
disposable hosted Linux environment. M2/F/G and real-Runner PA6 remain OPEN;
all native paths remain **unverified on real hardware**. No A13, Stage G,
Windows harness or launchd-profile work. No new owner configuration.

## Concrete mismatch and chosen slice

`driver.service()` currently expects a generated `arx-ci-*.service` root,
creates `control`, moves Python there, enables controllers, creates `runs`,
then launches timeout/Cargo/runner/libtest descendants. `locate` instead
requires the exact final components `arxd.service/supervisor`.
`setup::setup` requires root identity, standard hardening, trusted cgroup2
ancestry, exact `user.delegate` byte `1`, an empty domain unit and exactly
`[getpid()]` in supervisor before its first cgroup write. Renaming alone fails.

Add one ignored test `exec::disposable::disposable_linux_a1_a2`. Select only
this test in the manual driver's fixed `TEST`; keep the old cross-account
fixture present but unselected and its assertions intact. A1/A2 needs no OS
account, Runner invocation, shim, subprocess or build workload. Do not pull
cross-account provisioning into this patch: the packaged `ProtectSystem=full`
also prevents that fixture's `useradd` writes. Later acceptance must provision
its fixture outside the hardened service without widening that profile.

## Small patch order after design acceptance

1. **Pure driver plan and tests first.** Keep the exclusive run/attempt stage;
   change only the unit identity to fixed `arxd.service`. Add a small fixed
   A1/A2 launch plan and mocked lifecycle calls, not a general service framework.
   Keep gate ordering, protected copying, exact source/workflow SHA equality,
   offline locked build environment and protected artifact checks. Retain the
   360-second prebuild; require one arxd libtest executable beneath the protected
   staged deps path. Build finishes before installing/starting any unit.
2. **A1/A2 fixture and exec handoff.** Reuse `LinuxFs::system`, shared `locate`,
   `CgroupFs` bounded reads and `setup::setup`, with a test-local monotonic
   `Clock` and cancellation token. No production constructor or visibility
   expansion is needed. Add ordinary fake-port sequencing/failure tests beside
   the existing setup tests. Do not edit the production parser/setup algorithm.
3. **Driver integration.** Replace generated transient-unit/control/runs setup
   with the packaged runtime unit described below. Move output/result monitoring
   to outer `run_body`; make `service`/`harness` a single nonforking exec path.
   Update the existing Python regressions/mutation cases to exercise the new
   path. Keep the legacy fixture cleanup guards available, but A1/A2 creates
   no account and must not call account deletion. Adapt existing bounded F0/F0b
   lifecycle bookkeeping to fixed unit identity; no new evidence schema.

Expected implementation files: `tools/unix-exec-validation/{driver.py,
test_driver.py,mutation_check.py}`, `crates/arxd/src/exec/disposable.rs`, and
focused fake-port tests if needed. Update existing F0 test expectations only
where their exercised lifecycle changed. Packaging, workflow, owner-install
APIs and production activation stay byte-identical. Documentation/evidence
updates belong to the subsequent implementation delivery, not this commit.

## Exact launch and package contract

Use the baseline `packaging/linux/arxd.service` and
`50-arxcovo-standard-rights.conf` bytes as runtime files under the fresh
environment's systemd runtime unit directory. This is test-environment setup,
never owner installation. Reject any existing loaded unit, alias, fragment,
runtime file/drop-in or cgroup at the fixed name, even inactive/failed or
apparently identical. Use exclusive creation and retain identities of created
files. No replacement, reset-failed, adoption or restart of someone else's unit.

One fixture-only runtime drop-in has a closed property allowlist:
`ExecStart=` reset plus protected staged launcher command; `Restart=no`;
`RuntimeMaxSec=140`. It must not set `StandardOutput=` or `StandardError=`
to the capture path. Only the launcher creates that capture using the existing
exclusive no-follow open and redirects the inherited descriptors.
It changes no delegation, hardening, user/group, umask, notification, kill or
start/stop timeout property. Preserve `Type=notify`, `NotifyAccess=main`,
`KillMode=mixed`, both 30-second timeouts, `UMask=0077`,
`Delegate=memory pids` and `DelegateSubgroup=supervisor`.

These launch/lifetime/output exceptions are unavoidable fixture substitutions,
not an exact installed-daemon acceptance claim. `Restart=no` prevents a failed
test becoming an automatic retry. A prebuilt libtest binary cannot implement
`arxd service run`, so executable dispatch necessarily differs; do not silently
change `Type` to simple or replace the standard rights profile.

The protected installed Python launcher verifies the fixed artifact and exact
argv, records its PID exclusively, opens capture with private/no-follow rules,
redirects descriptors and **execve**s the prebuilt executable. Final argv is
`[artifact, TEST, --ignored, --exact, --test-threads=1, --color=never]` (pin this
exact vector in tests). No Cargo invocation or `/usr/bin/timeout` wrapper in
the service. No Python monitor, fork-and-wait launcher or extra service process
remains. Exec preserves MainPID and placement; libtest threads share its process
identity. Outer Python remains outside this unit and never migrates into it.
Preserve systemd's notification socket explicitly in the otherwise fixed,
minimal test environment; never copy the general runner environment.

The new test performs existing disposable-context/root gates, bounds and parses
its own cgroup through the production port, and calls `setup` before any child
creation. Production setup is the sole creator of `runs` and enabler of unit/
runs controllers. Never synthesize `user.delegate`, move into supervisor in
fixture code, or accept a second PID. Missing systemd support is a refusal,
not permission to emulate delegation. Before mutation require systemd >=254,
kernel >=5.14 and cgroup v2 with memory/pids; actual runtime checks remain
authoritative. Standard R4 must read NNP=1 and CapBnd=`00000000000000c1`;
do not add CAP_SYS_ADMIN/KILL/DAC_OVERRIDE or accept subsets. A1/A2 makes no
claim that this proves effective/permitted capabilities or cross-account rights.

After `SetupOutcome::Ready`, assert via bounded trusted port operations: unit
and runs subtree controls contain both controllers, runs UID=0 and mode=0700,
empty procs/events and no children, and S9 resource files open successfully.
Assert inherited umask=077 (test process only); do not chmod around a failure.
Use existing `service::unix::Notify::send_to` to send `READY=1` from this PID.
Retain the process for a bounded outer identity check using one fixed private
acknowledgment file, then exit normally. Outer requires systemd MainPID equals
the recorded PID, the live executable equals the staged artifact, and placement
matches that same supervisor. Check while it is held alive to avoid PID reuse/
fast-exit ambiguity; release only after all comparisons pass. No `MAINPID=`
notification or fork handoff. No test can count as passed on READY alone.

Use the existing output cap (1 MiB). The outer monitor stops only the unit
proven created by this run if the capture exceeds that cap while the process
is still held. Systemd never opens the capture. Require exact test-ok plus
one-pass/zero-failure check,
and actual service exit/result status together. Missing output, zero tests,
wrong test, stale ack, timeout or nonzero result refuses. Root-private output
is not uploaded; retain closed fixed diagnostics and the existing F0 bounds.

## Concurrency, failure, cleanup and rollback

Only one acceptance process per environment. Exclusive stage/file reservation
plus a nonblocking fixed-unit lock must serialize competing invocations; check
unit absence again while holding it. Any collision exits before unit mutation;
its finally must never stop/remove the colliding unit. Distinct run IDs do not
make two fixed units safe. The trusted disposable environment excludes other
privileged unit writers; no claim of protection from a hostile root manager.

Keep total manual job under ten minutes: 360-second build, <=120-second live
test/hold interval within RuntimeMaxSec=140, bounded outer commands, and allow
the packaged 30-second stop timeout (outer stop timeout must exceed it, e.g.
40 seconds). Do not retain the old 20-second outer stop bound or 10-second
unit stop override. No polling without a monotonic deadline or automatic retry.

On every post-start exit, stop only the unit proven created by this invocation;
snapshot existing feedback only under its stop/cgroup-absence rules. Confirm
that its cgroup tree is gone, then remove only the exact captured runtime
unit/drop-in files and stage using no-follow/inode checks and symlink-safe
cleanup; reload systemd after removing owned runtime files. No recursive
cgroup deletion, PID/UID killing, wildcard account cleanup or global controller
writes. Preserve global hierarchy/UID/GID scan fences for the older fixture;
the A1/A2 path supplies no generated identity to delete. Raw PID/path metadata
stays protected and local, never in public records.

If stop, removal, capture, ownership or cgroup disappearance is uncertain,
fail/quarantine and retain protected evidence; exclude that entire environment
from reuse. Hard cancellation may prevent finally; discard the fresh environment
without claiming cleanup. Rollback of source is a normal revert; runtime
rollback never restores an existing unit because collisions were refused.

## Required ordinary checks and finite mutants (future implementation)

No checks below were run in this design task. All runtime effects must be
fake-port/mocked in ordinary CI; the ignored acceptance test stays unselected.

- Python guard suite plus existing F0/F0b regressions: wrong platform/context/
  owner actor/ref/SHA, unsafe stage/artifact, collision/lock contention, build
  failure before unit mutation, exact packaged profile and closed override
  allowlist, direct exec argv/environment, bounded output/deadlines, both body
  and cleanup failures. Model start-failed-after-create and cancel-before-create;
  only resources actually owned may be stopped/removed. Assert call order and
  side-effect absence, not just source substrings.
- Rust ordinary location/setup fake-port tests: generated unit/control paths
  refused; wrong/extra supervisor PID refused before writes; missing/wrong
  marker refused; hardening/root/trust/domain/controller failures refuse;
  successful setup owns runs creation and S9; no child hook before Ready;
  failed setup never notifies/releases acceptance. Fake clock covers timeout
  and cancellation. Reuse existing parser and setup negative cases.
- Finite named driver mutants: bypass collision; build after start; fork instead
  of exec; execute Cargo inside unit; change TEST/argv; broaden profile; remove
  MainPID comparison; accept zero tests; remove deadline; cleanup another unit.
  Setup orchestration mutants: omit setup, create runs before setup, continue
  after Unavailable, skip A2 postcondition. Each must have one named behavioral
  assertion kill, not an error, syntax failure or source-string mismatch.
  Adapt the existing mutation runner's weak any-failure credit: require green
  baseline, exact-once anchor, successful compilation, zero test errors, named
  assertion failures and restored source fingerprint. No open-ended mutant set.
- Future commands: existing Python unittest discovery and bounded mutation
  runners; focused ordinary Rust hierarchy/fixture-planning tests, fmt and
  locked offline Clippy. Normal required hosted checks and different-provider
  source review apply before merge. Native results cannot be inferred from them.

## Technical gates versus owner-only disposition

Independent **Grok design acceptance** is the next gate for source preparation
only. Then Sol may implement the bounded patches and ordinary checks above;
independent source review and green required checks precede coordinator merge.
No implementation, tests, builds or runtime operations were performed here.

The accepted Stage F brief explicitly sequences design/execution after the
historical E_PROTECTION diagnosis. This document is a source-preparation
proposal under the user's narrower instruction, not closure of that gate or
an ADR amendment. Historical run 43091339273 remains unknown in refusal site,
mutation phase and cleanup; its unknown-cleanup environment is excluded.
No retry, reproduction or cause is claimed, and synthetic tests cannot supply it.

Future controlled execution requires a separately reviewed native proposal and
explicit owner disposition of the retained no-retry/diagnosis prerequisite,
on a new disposable environment. Existing manual owner-only workflow controls
remain unchanged: exact owner/triggering actor, main ref, workflow_dispatch,
matching source/workflow/reviewed SHA, hosted image, nonpersistent checkout
credentials and default-off reviewed-SHA variable. **NEW_SHA alone is not
permission.** No variable setting, dispatch, rerun, owner action or permission
request is part of this proposal. A future A1/A2 pass would close only those
cases; full Stage F, PA6, M2 and Stage G require their separate evidence/gates.
