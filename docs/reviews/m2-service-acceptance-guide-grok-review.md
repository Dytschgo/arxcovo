# M2 service acceptance guide review

Disposition: the retained owner-guide correction is **acceptable as documentation**.
It does not perform acceptance, grant publication, or authorize install,
registration, launch, or activation. One retained wording note is recorded
under Spec and is not a blocker.

Reviewer: xAI / grok-4.7, one pass, 2026-11-14. Standards, Spec, and security
stay separate. This is a single-agent review, independent of the Sol guide
preparation. No subagents. No tests, build, native call, API, CLI, filesystem
admission, provenance, process, owner, account, VM, install, or dispatch.

M2 is **incomplete**. Real-Runner PA6 is **OPEN**. Installed and dedicated
paths are **unverified on real hardware**. The cause of main run `43545244266`
attempt 1 remains **unknown**. No retry or fix is credited. No bootstrap or
diagnostic extension is authorized by this review.

## Pins

The review worktree was clean. The guide bytes were read from Git objects, not
from a switched branch. Supplied `220aeddf33afe015ab067112da83688d266fece2`
and retained `9117b61362ebdffd6f2c4995a82ca74ebd0d1d48` have the same
`docs/service-install.md` blob, `7a7a7bba1861e9a4c57651cb03255f4ccfbd2ea7`.
Their trees differ only by `docs/PROGRESS.md`. That file was not edited.
The retained parent diff is `docs/service-install.md` only, +169/−73.

| Item | Identity |
| --- | --- |
| Supplied guide | `220aeddf33afe015ab067112da83688d266fece2` |
| Supplied parent | `fff9ede5845b1fb5f31de25bad0bd37e6dcd55e2` |
| Supplied tree | `acd37c36d6dff53f09fa0755dbec62136ecf8fec` |
| Retained guide | `9117b61362ebdffd6f2c4995a82ca74ebd0d1d48` |
| Retained parent | `d5f700b4618c8322e84629f37ee59dd9ff0861f4` |
| Retained tree | `e71aef99d3203340abe21fb96c1e84c0d285ad79` |
| Guide blob | `7a7a7bba1861e9a4c57651cb03255f4ccfbd2ea7` |
| Source domain | `aa49ce1e99475d07d63c6b0d23fa94bdb30bab74` |
| Source tree | `6a8e649b378753204e4a496c233519359375a78b` |
| Source subject | `test(windows): correct scoped replay observations (#74)` |
| Later main | `b34110a4b18799097deb771e4636a6ce0c5e36b1` |
| Later main parent | `aa49ce1e99475d07d63c6b0d23fa94bdb30bab74` |
| Later main tree | `e4c33da81e0dd0a925ce3caf9374a0138fce94e5` |
| Recovery supplied | `9a8e63dfe188f935ea63cebc48be4c6e926afde3` |
| Recovery retained | `ae6a6ad045c061b8357d48f32accef2adce14131` |
| Recovery document blob | `184d2c77dcaf12e777587a04df298afad895a99d` |

`git diff --name-only aa49ce1 b34110a` does not touch `crates/arxd/src/service`,
`mcp.rs`, `local.rs`, `http.rs`, `security.rs`, `exec`, or `packaging`. The
arxd change on that main commit is `crates/arxd/src/development/windows.rs`.
The other changes are docs, evidence, reviews, and
`tools/windows-native-replay/`. Service and MCP blobs used below are identical
at `aa49ce1`, `b34110a`, and the review HEAD before this document.

The recovery document is the same blob at both recovery commits. Its matrix
and manual recipe were audited against the same source pin. It does not
authorize native execution.

## Checks performed

Local only: guide diff, link resolution, fenced JSON, and source reads at the
pins above. Fifteen relative links resolve at both `aa49ce1` and `9117b61`.
Twenty-two code fences are balanced. Three JSON envelopes parse. No GitHub
write belongs to this guide review.

## Standards

Rules read: `AGENTS.md`, `CONTRIBUTING.md`, `docs/KICKOFF.md`, and
`docs/ci-administration.md`. The smell baseline was applied to the guide diff.

No documented-standard breach. The guide labels itself as preparation, says M2
is incomplete, keeps PA6 open, and marks installed and dedicated paths
**unverified on real hardware**. It does not claim a check that this review
did not run. It removes the old instruction to record a SID in
`docs/PROGRESS.md`. It adds no product approval flow, workflow change, or
bootstrap proposal. The retained commit stays one documentation concern.

Judgement: none that overrides the stop-before banner. The file still contains
future owner steps, and it marks them as future and unverified.

## Spec

The originating recipe is `docs/reviews/m2-acceptance-path-recovery.md` at the
blob above, checked against KICKOFF milestone 2 and the source at `aa49ce1`.

Implemented and matched:

- Stop before install on every OS. `Execution::configured` builds `Native`.
  After a successful config check, `Native::preflight` still returns
  `MechanismUnavailable`. `Native::run` returns that same report and does not
  launch. `Execution::capability` returns `available: false` and
  `service_hardening: null`. `unix_bridge_for_test` is test-only.
- macOS `owner_query::launchd` returns `Query::Unknown` for every input.
  Owner operations map that query to `service-manager`. ADR 0015's endpoint
  foundation is present: the socket path is
  `/Library/Application Support/Arxcovo/run/arxd.sock`. The ADR states that
  this `run` directory keeps a stale socket across reboot and that the owner
  removes it by hand. `/private/var/run/arxcovo` is ignored.
- Development acceptance is not M2. `docs/evidence/windows-development-acceptance.json`
  at `aa49ce1` has `dedicated_account_acceptance`, `installed_service_acceptance`,
  and `milestone_2_complete` all false. The guide says that record is not a
  service fallback.
- The optional replay harness is not an acceptance step. At `b34110a`,
  `selectors.json` has `registration_enabled` false and
  `Invoke-BoundedReplay.ps1` throws `GATE2_ONLY`. The recovery document parks
  that harness. KICKOFF milestone 2 does not require it.
- Owner grammar matches `owner_cli::parse`: `service install|start|stop|status|uninstall`,
  `client-config new --principal --out`, and `service configure` with
  `--build-account`, `--client-account`, `--rights standard|admin`,
  `--workspace`, repeated `--env KEY=VALUE`, and repeated `--agent PRINCIPAL=HASH`.
  Success prints `<principal>=<sha256>` and not the token. `client-config-partial`
  is exit 1. After exclusive create, the writer does not delete the file or
  its parent; the partial test keeps the file and reports no rollback.
- Previous-config refusal matches ADR 0018 and `configure_plan`: a stale old
  workspace is `service-workspace` exit 3 even when the new workspace is valid;
  a missing old account is `service-identity` exit 3 before the old workspace
  check; a changed build pin is refused. A missing new Windows account can be
  `service-account-missing`. The manual remedy is restore, or uninstall, delete
  the config by hand, install, and configure.
- Service identity is not the build account. SCM registration requires
  `LocalSystem`. The Linux unit has no `User=` and is a system unit. The macOS
  plist is a LaunchDaemon with no `UserName`. `service_identity` rejects
  Windows `S-1-5-18`, `S-1-5-19`, `S-1-5-20`, service SID prefixes, and Unix
  uid `0` as the pinned client or build identity. The guide says commands must
  not run as the owner or as the service. At this pin they do not run at all.
- Linux standard install writes the packaged drop-in. An admin plan removes
  that drop-in. Neither mode has a production executor. macOS
  `configure --rights admin` is refused before account lookup.
- Windows boot intent matches `owner_scm`: register leaves demand start; start
  selects automatic and then delayed auto-start; stop waits until stopped
  before selecting demand start. A failure after the boot write can leave the
  earlier intent. Installed behavior stays unverified.
- MCP shapes match `mcp.rs`, `executor::schema`, and `store::ListActivity`.
  `main` accepts exactly `mcp-stdio --client-config FILE` for the bridge.
  `list_hosts` and `discover_tools` take empty objects. Dedicated
  `run_command` is one of `tool` or `executable`, with `cargo` in the tool
  enum. Bounds are args 128 × 8192, cwd 4096, timeout 1–3600 default 60,
  output 2–8192 default 4096, and absolute executable length 4096. Unknown
  fields are rejected. `list_activity` limit is 1–50 default 20, `before_id`
  is positive, and `include_reads` defaults false. Rows are caller-scoped and
  ordered by id descending, so the last returned id pages backward.
- `list_hosts` exposes `hosts[0]` OS, arch, logical CPUs, RAM, and both disk
  totals, plus `execution.available`. Dedicated discovery uses `tools` and
  `tools_observed_at`. An empty or stale cache is not success. The guide tells
  the operator to stop while `execution.available` is false.
- A dedicated completion exposes top-level `activity_id`, `outcome`,
  `exit_code`, `rights`, `tree_boundary`, and `cleanup_confirmed`, plus
  `metadata.identity_mode` and `metadata.identity_matches_pin`. Assessment
  accepts Windows `windows_sid`, integrity 8192, not elevated,
  `post_switch`, and `windows_job`; Linux and macOS `unix_uid`, `pre_exec`,
  and `linux_cgroup` or `mac_uid_sweep`. The activity page flattens those
  stored fields. The guide's restart read is a future check, not a claim that
  it was executed. It forbids publishing raw SID, UID, GID, login, private
  path, credential, environment, or raw command output.

Retained note, not introduced by this diff and not a blocker:

The macOS section still says that a working `sudo -n true` makes every command
be refused with `rights_mismatch`. That probe is the Linux shim argument
vector and `classify_rights` in the Linux hierarchy parser. macOS admin
detection is membership of the `admin` group. Configure maps
`RightsMismatch` to `service-rights`. At `aa49ce1`, dedicated `run` does not
reach either check; it returns `MechanismUnavailable`. The following paragraph
now says the macOS containment contract is not a working executor, and the
acceptance table does not use this sentence as a pass condition. Do not add
the account to `admin` or give it a sudoers rule remains safe advice.

## Security

No reachable finding. The guide adds no approval prompt, secret, raw identity
output, private owner path, development fallback, or way around the inert
executor. Deleting the launchd and `Native` refusals would still leave
Windows S4U/shim/Job, macOS UID sweep, and Linux Stage F/G unproved. PA6 stays
open. The harness remains parked and is not an M2 prerequisite.

## Verdict

Standards: 0 hard findings.
Spec: 1 retained non-blocking note, the macOS `sudo -n true` sentence.
Security: 0 findings.

No blockers. Publication and merge still require the normal hosted gates,
a different-provider review on the exact published head, and a coordinator
merge. This document is not that publication review and does not grant one.
Native and installed paths remain **unverified on real hardware**. M2 is
incomplete. PA6 is OPEN. The main71 cause is unknown and no retry is credited.
