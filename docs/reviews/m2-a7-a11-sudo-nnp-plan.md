# M2 A7/A11: minimal combined acceptance source plan

Planning only, OpenAI / gpt-6.1-sol / high, 2026-11-15; independent review pending.
Main after PR117: `7113a420140de8a45a17c82a25305f8794c3adb8`.
Prepared parent: `f8b4131e6937c58b729abbf78da972ff25fb0095`; only PROGRESS differs.
Read AGENTS, KICKOFF M2/routing, brief0020 E4/A7/A11, ADR0019 D2/D7 and
ADR0020 D8/R5/D10, probe/fixtures, runner/account checks, setup and package.
A7/A11/M2/full F/G/real-Runner PA6 remain **unverified on real hardware**.

## Existing coverage first

- E4 S8 tests exit/signal/exec-error/deadline/reap using copied test ELF;
  S9 rejects unsafe candidates before execution; S9b accepts both exact aliases;
  S9c preserves the validated inode across file/directory swaps; S10 ignores PATH.
- S11 checks parsed/actual NNP (independent PR_GET precondition); S12 pins defaults.
  S6 integration checks TOOLS order, final bounded rights line, NNP=true and
  sudo!=exit_zero. It allows absent/untrusted/unknown: insufficient for A7.
- Hosted E4 run43504012486 recorded 11 assertion kills with restored sources
  (docs/evidence/linux-rights-probe-e4-native.md); owned ELF evidence, not A7/A11.
- Runner::run_phased -> unix::run already sets NNP, drops all UID/GID identities,
  clears capabilities/groups and verifies identity before exec. Account policy
  checks groups, not named sudoers grants. No production change is needed there.
- Production setup requires Standard c1/NNP, root, delegated arxd.service layout;
  current disposable A1/A2/A4/A5 fixtures exercise it but never installed sudo.

## Fixed source shape and host preconditions
One ignored controller `exec::disposable::sudo_nnp::disposable_linux_a7_a11`,
plus one literal ignored build-account reentry in E4 tests. Never select either
in ordinary CI. Reuse driver admission/staging/unit/MainPID/ack/finally cleanup,
actual LinuxFs::system setup once and actual Runner::run_phased.
Keep production activation unavailable; no UnixBridge/config/CLI selector change.
Use unchanged packaged arxd.service and Standard drop-in (NNP=yes, CapBnd=c1).
Require fresh GitHub-hosted Linux, systemd>=254, kernel>=5.14, cgroup2,
memory+pids delegation, user.delegate=1; refuse unsupported floor, never relax it.
Require root-owned installed ELF C1 sudo, protected ancestors, C2 absent or trusted,
and root-owned /bin -> exactly usr/bin or /usr/bin. No installation fallback.

Before service start, extend only the driver's owned fixture preparation to create
TWO exclusive generated accounts/private roots/workspaces, no supplementary groups.
Pin captured UID/GID/inodes; fixtures may consume only those newly owned identities.
One account has a root-owned 0440 exclusive sudoers.d file granting ONLY
`<generated> ALL=(root) NOPASSWD: /usr/bin/true`; the other has no applicable rule.
Require the image's bounded local sudoers configuration to include that file and
exclude other applicable user/group/default grants; refuse ambiguous/external policy.
Pin the closed image's policy/include bytes; use fixed visudo syntax checks only.
Do not infer effective policy from syntax alone or execute unfenced sudo.
Do all /etc preparation outside the Standard service: ProtectSystem=full forbids
in-service sudoers edits. No new account product flow or general policy parser.

## Exact native oracle
For EACH account, use its pinned non-admin OwnerConfig, prepared runs parent,
64MiB/swap0/pids32 limits and <=20s RunSpec deadline; no owner identity fallback.
Run the staged production arxd with literal [tool-probe] through the real Runner.
Require observed identity={uid,gid,daemon_was_root:true}, exit0, no truncation,
TOOLS in exact order plus exactly one final rights line, <=4096 bytes total,
rights exactly {no_new_privs:true,sudo:exit_nonzero}, confirmed tree cleanup and
cleanup_residue=false. absent/untrusted/unknown/exit_zero/skip are failures.

For A11, run the fixed E4 reentry through that SAME Runner/account configuration;
use Settings::default(), real / anchor, owner0, actual installed sudo and unchanged
probe/rights observation. No fake ELF, owner/argv/env override or copied sudo.
Add only cfg(test) fixed scalar observation at the existing selection/execute
sites: C1 selected, exact alias accepted/C3 skipped (never opened), O_PATH and
FD_CLOEXEC present on the selected regular inode, and calling UID/GID match pins.
Observe before fork, never allocate/log/callback in its async-signal-safe child.
Require normal nonzero exit with EOF on exec-error pipe, not exec failure127;
require reentry rights also true/exit_nonzero and runner identity/cleanup as above.
Use fixed bounded reentry output (<=4096 bytes), captured privately and reduced to
booleans; never publish usernames, paths, numeric identities or raw tool/policy text.
Installed sudo executes via production fork+execveat(AT_EMPTY_PATH), bound by
source pins to the held O_PATH FD; the test ELF is its caller, not sudo evidence.
This extra call supplies A11 observation; retain actual A7 shim evidence.

## Bounds, verification and delivery gates
Future source ceiling: exec/disposable.rs + new exec/disposable/sudo_nnp.rs;
unix_shim_rights_linux.rs/tests.rs; tools/unix-exec-validation/driver.py/test_driver.py.
Only separate reviewed delivery may stage/pin production arxd alongside libtest;
no helper image, package, production runner/setup/bridge or workflow changes here.
Reuse <=120s harness/360s build/1MiB capture bounds and <15min CI budget; fail closed.
Pure/model tests must reject wrong identity, either zero/unknown/absent result,
exec error, alias opened/not skipped, missing O_PATH/CLOEXEC, truncated/extra records,
missing policy case and unconfirmed cleanup; prove each discriminator by mutation.
Mock driver collisions, partial provisioning, policy failure and finally cleanup;
no mocks earn native credit. Preserve E4 regressions/inactive inventory and gates.
Finally stop/confirm unit/cgroup disappearance and no live fixture UID, then remove
only matching owned policy file/accounts/group/workspaces/artifacts; recheck absence.
On uncertainty retain reservation/quarantine, fail, never adopt/delete foreign state.

Independent Opus5.5 high design/security review (grok4.7 high only on limits), Sol
implementation and other-provider source/exact-head review plus all required gates
precede coordinator merge. D7a-g are consumed: obtain a NEW narrowly reviewed native
grant naming both accounts/cases, artifact/source/workflow pins, one dispatch and
cleanup/variable lifecycle before selection or execution; user already authorizes
coordinator disposable actions through those gates, no extra human approval step.
No source blocker requires Stage G: direct Runner is already callable in tests.
Remaining choices: attest a qualifying installed-sudo/local-policy host image and
settle exact new grant/delivery pins; current evidence does not attest that host.
If unavailable, deliver only ignored source + pure rejection tests, credit neither
A7 nor A11. M2 MCP run/activity acceptance and Stage G stay separate pending gates;
no owner steps, later milestones, diagnostic framework or weakened acceptance.
