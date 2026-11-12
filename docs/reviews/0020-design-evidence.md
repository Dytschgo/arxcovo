# ADR 0020 design evidence and disposition

Author: Anthropic / claude-opus-5-5 / high, design sub-agent, 2026-11-12.
Sections up to "Review limits" are the original proposal's evidence at
`fdea1c6`; the final section adds the correction 1 evidence at `d44035d`.
This is **design evidence, not a review**. The same provider wrote
[ADR 0020](../adr/0020-linux-execution-hierarchy-and-janitor.md) and the
[brief](../briefs/0020-linux-hierarchy-implementation.md). A fresh
independent Sol high review is required before acceptance.

Docs only. No code, build, test, mutation, CI run, push, GitHub write,
production call, provisioning, or account, owner, service or cgroup
operation was performed. The Lab Manager, its VM and its state were not
touched.

## Base and isolation

- Isolated T3 worktree at HEAD `fdea1c6a5d9954e0ff9d20f10e7d8d9a7f930b28`,
  which is distinct from the canonical checkout. The tree was clean before
  any write.
- The base is PR55 Stage D on the **unmerged** PR53 Stage C, on main after
  #52 `1604db1`. Stage C and Stage D stay unmerged. Production stays
  `MechanismUnavailable`.

## ADR number check

ADR 0020 was unused before this commit:

- No file `docs/adr/0020*` or `docs/briefs/0020*` exists at HEAD.
- No path whose basename starts with `0020` appears in any commit reachable
  from any ref (`git log --all --name-only`, 1,477 commits).
- No commit message mentions `ADR 0020`, `ADR-0020` or `adr/0020`.
- The only grep hit was a checkpoint commit whose abbreviated hash begins
  `002075c`; its file list contains no `0020` path.

## Repository sources read (blob ids at `fdea1c6`)

| Blob | File | Used for |
| --- | --- | --- |
| `f6c65008a7bb` | `crates/arxd/src/exec/cgroup.rs` | `Group::create` preconditions, ignored `unlinkat`, leaf names, `kill_and_confirm`, bounded `text` |
| `d3ec3e7e26a0` | `crates/arxd/src/exec/unix.rs` | root check, cgroup join in `pre_exec`, NNP/capabilities, `ChildGuard`, `control`/`put`/`trusted_root` |
| `e94549e81bd4` | `crates/arxd/src/exec/mod.rs` | `OwnerConfig`, `Runner::new` admin refusal, phased API |
| `00f8f2c0aae6` | `crates/arxd/src/service/unix_bridge.rs` | `ready: bool`, preflight order, `cgroup_parent` parser, failure mapping, discovery parse |
| `9c538ee25bf0` | `crates/arxd/src/service/executor.rs` | `Native`, `configured`, preflight, `capability`, `dispatch`, `complete` |
| `05dafc6752e5` | `crates/arxd/src/service/application.rs` | `Hooks`, preflight, no-op `start_discovery`, drain order |
| `c0488d356ca7` | `crates/arxd/src/service/mod.rs` | `Host::run` start order, 30/10/5 s budgets |
| `d396250f3cf7` | `crates/arxd/src/service/config.rs` | `execution_preflight`, rights mismatch |
| `fb1bb275b4e9` | `crates/arxd/src/service/native_unix.rs` | admin groups `sudo`/`wheel`/GID 0 |
| `534625eeba35` | `crates/arxd/src/service/preflight.rs` | fixed `ExecutionUnavailable` vocabulary |
| `7f2bdf656525` | `crates/arxd/src/store.rs` | start helpers, daemon actor CHECK, schema |
| `6850fb59469a` | `crates/arxd/src/mcp.rs` | `list_hosts` constant tool fields |
| `ca4a7729992d` | `crates/arxd/src/unix_shim.rs` | `tool-probe` output, PATH resolution, hook/catch |
| `a83727d168be` | `crates/arxd/src/main.rs` | shim routing before runtime |
| `0b337773c60e` | `packaging/linux/arxd.service` | `Delegate`, `DelegateSubgroup`, `KillMode=mixed`, `RestartPreventExitStatus` |
| `93a3abcce978` | `packaging/linux/50-arxcovo-standard-rights.conf` | `NoNewPrivileges`, `CapabilityBoundingSet`, `Protect*`, `PrivateTmp` |

ADRs read: 0009 (layout, lifecycle, Linux unit, preflight table, cleanup
attestation, bounds and quarantine, discovery, slices 7 and 9), 0017
(rows A–N, retention rules 5–6, discovery rule), 0019 (D1–D7, C1–C3, open
facts) and brief 0019 (§1, §4, §5, §10 E, open facts). The kickoff and
AGENTS rules were followed.

## Runner and packaging audit (summary)

- The runner requires a prepared, empty, controller-enabled, root-owned
  cgroup2 parent, and its module comment forbids enabling controllers. Setup
  must enable `memory pids` on both the unit and `runs/` (ADR 0019 already
  noted `runs/`).
- The runner's precondition cannot see populated leftover leaves (processes
  sit in leaves, not in `runs/`). This makes the janitor-before-availability
  ordering a containment requirement, not a convenience.
- The bounding set lacks `CAP_KILL`, `CAP_DAC_OVERRIDE` and `CAP_SYS_ADMIN`.
  Consequences: kill(2) on build-UID processes fails, so `cgroup.kill` is the
  only cleanup primitive. `trusted.*` xattrs are unreadable, so only
  `user.delegate` can serve as the delegation marker. Root-owned cgroupfs
  files are writable as owner (*inference*).
- The unit sets neither `ProtectControlGroups=` nor `ProtectKernelTunables=`,
  and both would make `/sys/fs/cgroup` unwritable (systemd.exec). The brief's
  pins keep them absent. No packaging change is proposed.
- `KillMode=mixed` is the crash backstop for leaf processes. Setup failure
  must never exit, because exit 7 is in `RestartPreventExitStatus`.

## Primary documents consulted (public fetch, 2026-11-12)

Fetched to a session scratch directory with plain `curl`. No repository, owner
or account data was sent. These are current upstream heads, not the target
distribution's versions. API-reported heads at fetch time were systemd `main`
`92e51dd8` and linux `master` `ee838993`. These may differ by minutes from the
fetched bytes.

| SHA-256 prefix | Source | Facts used |
| --- | --- | --- |
| `0778b37810a75c6b` | kernel `Documentation/admin-guide/cgroup-v2.rst` | multi-token `subtree_control` writes are all-or-nothing; top-down and no-internal-process constraints; interface files are owned by the parent's enablement; `cgroup.events` `populated` covers descendants; `cgroup.kill` is recursive, fork- and migration-safe, `EOPNOTSUPP` in threaded cgroups; an empty cgroup (zombies only) can be removed; migration needs write access to the common ancestor's `cgroup.procs`; frozen tasks die from fatal signals |
| `7a542a153328ea1a` | systemd `docs/CGROUP_DELEGATION.md` | controllers made available but not enabled; delegatee writes `subtree_control`; single-writer rule; `.control/` subgroup; `DelegateSubgroup=` since v254; `user.delegate` xattr `1` (kernel 5.6+, systemd 251+) |
| `3a1ceeb24069b330` | systemd `man/systemd.resource-control.xml` | `Delegate=` ownership boundary; extra controllers may be available; `DelegateSubgroup=` semantics |
| `869f268cb01eb86d` | systemd `man/systemd.exec.xml` | `ProtectControlGroups=`, `ProtectKernelTunables=`, `ProtectSystem=full` scope |
| `448322830dbbd02c` | systemd `man/systemd.kill.xml` | `KillMode=mixed`: SIGTERM to main, SIGKILL to remaining processes after main exits |
| `7d953c559068736b` | man7.org kill(2) | signal permission requires `CAP_KILL` or matching real/effective vs real/saved UID |

## Inference (not verified against a primary source or hardware)

- Ownership of files and directories created by a root `mkdir` in cgroupfs
  (root, so only root can populate an orphan leaf).
- No extra capability check on cgroupfs writes beyond file permissions under
  the drop-in's bounding set (shared with ADR 0019).
- Whether systemd removes delegated subdirectories at unit stop (U9). Both
  outcomes are designed for.
- That `rmdir` of an empty, never-populated leaf does not fail in practice
  (the D6 cancel residual).
- *Superseded by correction 1:* that `sudo` under `PR_SET_NO_NEW_PRIVS`
  cannot elevate. The kernel no-new-privileges document is now fetched (see
  below); what `sudo` itself prints or returns when it starts without root
  remains inference, and A7 is the acceptance item.
- Kernel version facts (`cgroup.kill` in 5.14, `NoNewPrivs` status line in
  4.10) are from memory, not from fetched documents.

## Disposition of ADR 0019 stage E items and open facts

| Item (brief 0019 §10 E / open facts) | Disposition |
| --- | --- |
| Linux hierarchy setup: unit and `runs/` controllers, create `runs/` | ADR 0020 D2–D3 (S5, S6, S8, S9); brief E2–E3 |
| Janitor | D4; leftover kill via `runs/cgroup.kill`; unknown names refused; brief H3–H9, H16–H17 |
| Startup discovery wiring | D7; daemon row after Ready; ADR 0017 rows; cache rules 5–6; brief E6 |
| ADR 0009 slice 9 rights checks | D8 R4/R5 plus pure table; per-run cadence deviation labelled |
| "Whether startup discovery should run before the janitor confirms an empty `runs/`" | **Decided: no.** Strictly after (D7) |
| Empty orphan leaf after failed unlink (`cgroup.rs:85`, `:109`) | D6: containment latch for observable sources; inert residual for the cancel path; next-start janitor |
| `memory` and `pids` enabled on both unit and `runs/` | S5 and S8, each verified by re-read; S9 proves the interface files exist |
| `ProtectHome`/`PrivateTmp` reach children | Unchanged open fact (ADR 0019); not part of stage E |
| Root-only runner sites, `cgroup.kill`, leaf removal | Still Stage F; brief A1–A10 |

## Review limits and unresolved decisions

- Same-provider authorship. Nothing was executed, and no Linux build or test
  ran; the host was Windows. Line citations are at `fdea1c6` only.
- Unresolved decisions U1–U9 are listed in ADR 0020. The reviewer should
  give particular weight to:
  - U1, the rights cadence and the window between Ready and the first
    discovery;
  - U3, the latch scope against a runner residue signal;
  - the ADR 0019 amendments A1–A4, which change Stage C/D code after those
    stages merge.
- Native, installed and dedicated paths remain **unverified on real
  hardware**. M2 remains incomplete.

## Correction 1 evidence (2026-11-12)

Author: Anthropic / claude-opus-5-5 / high, design sub-agent. Still design
evidence, not a review; same-provider authorship. Docs only: no code, build,
test, mutation, CI run, push, GitHub write, native/root/owner/account/
service/cgroup/VM operation, experiment, Lab Manager access or private-state
read. No `E_PROTECTION` or S4U retry.

### Base and isolation

- Runtime-prepared isolated T3 worktree; `git rev-parse --show-toplevel`
  named the worktree, distinct from the canonical checkout. HEAD
  `d44035dee79403b6386215d4aa8ca5254f37dc35`, tree clean before any write.
  No checkout was created or switched.
- `git diff --stat fdea1c6 d44035d` lists only docs (the proposal, the Sol
  review and `PROGRESS.md`), so every code citation at `fdea1c6` still holds.
- Retention check by blob id: the three proposal files are identical in
  `e3928f5` and `54032bf`; the review file is identical in `8ff4196` and
  `8ff3a54`. The review file was not edited by this correction.

### Additional repository sources (blob ids at `d44035d`)

| Blob | File | Used for |
| --- | --- | --- |
| `f6c65008a7bb` | `crates/arxd/src/exec/cgroup.rs` | every residue site: create-path `unlinkat`, `kill_and_confirm` false paths, `Drop`; owned-temp test pattern for E5 |
| `d3ec3e7e26a0` | `crates/arxd/src/exec/unix.rs` | `socketpair` (`:223-224`), final recheck (`:344-352`), spawn (`:353-355`), NNP and capability clearing (`:242-310`), `walk`, `trusted_root` |
| `9c538ee25bf0` | `crates/arxd/src/service/executor.rs` | preflight before `active.fetch_add` (`:337-352`), `AcqRel` slot operations, stopping serialization |
| `00f8f2c0aae6` | `crates/arxd/src/service/unix_bridge.rs` | body refusal shape (`:74`), failure mapping, `cgroup_parent` |
| `05d052e1650e` | `crates/arxd/src/service/unix_bridge/tests.rs` | Stage D T2 fixtures (one accepted, all others refused) |
| `ca4a7729992d` | `crates/arxd/src/unix_shim.rs` | path-based `Command` spawns, single-threaded entry, probe timing |
| `91ed14c574ca` | `crates/arxd/src/development/mod.rs` | `TOOLS` has 9 entries (probe time budget) |
| `c875ccab1d97` | `crates/arxd/src/exec/stage_b_tests.rs` | current P-SCAN inventory and negative controls |
| `05dafc6752e5` | `crates/arxd/src/service/application.rs` | `stop_accepting`, drain order |
| `c0488d356ca7` | `crates/arxd/src/service/mod.rs` | `start_discovery` after `Running` |

ADR 0009 (packaged drop-in, `rights_mismatch` row, slice 9 text) and ADR 0019
(D1–D7, C2/G-P1–G-P3) were re-read for the narrowed rights meaning and the
A5 delta.

### Primary documents fetched for the correction

Fetched with plain `curl` into a session scratch directory; no repository,
owner or account data was sent. Current public heads, not the target
distribution's versions.

| SHA-256 prefix | Source | Facts used |
| --- | --- | --- |
| `73cd16bb37fab267` | kernel `Documentation/userspace-api/no_new_privs.rst` (raw GitHub) | NNP is inherited across fork, clone and execve and cannot be unset; with it, setuid/setgid bits no longer change the uid or gid and file capabilities do not add to the permitted set; NNP "does not prevent privilege changes that do not involve `execve()`"; the loader treats `LD_*` differently for setuid programs |
| `6e15f3808cc32bae` | docs.kernel.org `admin-guide/cgroup-v2.html` | `cgroup.events` is flat-keyed with `populated` and `frozen`; `cgroup.kill` kills the cgroup and all descendants with SIGKILL, handles concurrent forks and is migration-protected, `EOPNOTSUPP` in threaded cgroups; a cgroup with no children or live processes can be removed; frozen tasks die from fatal signals; `cgroup.pressure` is a control file name |
| `d665489cadb85624` | man7.org open(2) | each `open()` creates a new open file description that records the file offset |
| `f0b886340b0471a1` | man7.org dup(2) | duplicates share offset and status flags; the duplicate's close-on-exec flag is off |
| `ec26d14cd175c865` | man7.org opendir(3) | after `fdopendir`, the descriptor is used internally and should not otherwise be used |
| `130e3c72c4f84ddf` | man7.org execveat(2) | `AT_EMPTY_PATH` executes the file referred to by the descriptor, which may come from `O_PATH`; with close-on-exec set, an interpreter script fails with `ENOENT` |

The raw GitHub fetch of `cgroup-v2.rst` was rate-limited (HTTP 429 body);
the rendered docs.kernel.org page was used instead. A `getdents(2)` page was
also fetched but is not relied on.

### Inference added or retained by the correction

- The release-sequence argument in ADR 0020 D6 (every `AcqRel`
  read-modify-write on `active` after a release synchronizes with it) is
  reasoning from the Rust/C++ atomics model, not a fetched document.
- That an agent cannot cause cleanup to fail at will (the residue latch is
  a host or kernel fault signal).
- That the dynamic loader does not use secure-exec handling for a setuid
  binary started under NNP, so `LD_*` would apply; the probe uses an empty
  environment regardless.
- That cgroupfs offers no user creation of regular files or symlinks; the
  D4.3 refusal rows are fail-closed classification either way.
- That kernel control-file names fit `[a-z0-9_]+(\.[a-z0-9_]+)+` within 64
  bytes (from the documented names; a future name outside it would refuse
  setup, residual decision R-3).
- What `sudo` returns when executed without root under NNP (A7).
- That `execveat` on an `O_PATH` descriptor of the installed ELF `sudo`
  works for the build account (A11).

### Limits

Nothing was executed beyond read-only git inspection, file reads and the
public fetches above. Future tests are requirements, not passes. Native,
installed and dedicated paths remain **unverified on real hardware**; M2
remains incomplete.
