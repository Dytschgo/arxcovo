# A10: accepted capability evidence join

2026-11-15; Sol implementation sub-agent; documentation only.
**A10 remains OPEN. Fresh independent Opus review is required before publication.**
This narrows the existing claim; it grants no execution or activation authority.

## Scope and pins

Parent `4847db066299efeea6634525f597bfcd367a6457` (PR139 checkpoint pending),
accepted main `2a1d5ad32cfb62cac3fb87c8a799b857e1a5f59c`.
The parent/main delta is five document paths only. Scope follows
[KICKOFF](../KICKOFF.md), [ADR0020 D2–D4/D8/D9a](../adr/0020-linux-execution-hierarchy-and-janitor.md),
[brief acceptance A10](../briefs/0020-linux-hierarchy-implementation.md), and the
[accepted A12 assessment's A10 handoff](../reviews/m2-a12-hosted-disposition-opus-assessment.md).
A10 asks whether cgroupfs writes **and migration** need any capability beyond
the packaged Standard bounding set. Partial operation coverage cannot close it.

The unchanged [unit](../../packaging/linux/arxd.service) delegates `memory pids`
and uses `DelegateSubgroup=supervisor`; the unchanged
[Standard drop-in](../../packaging/linux/50-arxcovo-standard-rights.conf) sets
`NoNewPrivileges=yes`, `CapabilityBoundingSet=CAP_SETUID CAP_SETGID CAP_CHOWN`,
`ProtectSystem=full`, `ProtectHome=yes`, and `PrivateTmp=yes`.
Thus the configured bound is exactly `0xc1`; it excludes `CAP_SYS_ADMIN`,
`CAP_DAC_OVERRIDE`, and `CAP_KILL`. R4 observes NNP=1 and exactly that bound,
not effective/permitted sets. Successful calls under the attributed package
support sufficiency for those calls on those hosts; they do not identify a
minimal capability set or prove that each listed capability was used.

## Accepted run/source bindings

All run links below are citations to committed records, not live rechecks.
Success credit joins exact selected fixture assertions, driver attribution,
completion and cleanup to the accepted review; green jobs or internal `absent`
fields alone are insufficient. Every listed attempt is attempt 1.

| Evidence | Exact executed main | Profile and accepted extent |
| --- | --- | --- |
| A1/A2 [43627472312](https://github.com/Dytschgo/arxcovo/actions/runs/43627472312), [record](linux-stagef-a1a2-run-43627472312.json), [review](../reviews/m2-a1a2-hosted-evidence-muse-review.md) | `1e9d8818de1e337b1402ca7e2b71ac02456b82f3` | Standard loaded; production setup before children, new `runs`, both controller writes and S9 checks; confirmed cleanup. |
| A3 Standard [43635131545](https://github.com/Dytschgo/arxcovo/actions/runs/43635131545), [record](../reviews/m2-a3-standard-hosted-record.md), [review](../reviews/m2-a3-standard-hosted-muse-review.md) | `beb7117b08aad68da790e05a85b335f9655f5236` | Whole Standard file loaded; real R4 returns Standard. No setup/migration credit from this selection. |
| A3 None [43636804545](https://github.com/Dytschgo/arxcovo/actions/runs/43636804545), [record](../reviews/m2-a3-none-hosted-record.md), [review](../reviews/m2-a3-none-hosted-muse-review.md) | `4bb15612e30ec5bbf0c019bab347c83fd60a9d0a` | Standard file entirely omitted, R4 returns None. Negative profile control only; no Standard operation credit. |
| A4 positive [43666413693](https://github.com/Dytschgo/arxcovo/actions/runs/43666413693), [record](linux-a4-baseline-run-43666413693.json), [assessment](../reviews/m2-a4-hosted-baseline-muse-assessment.md) | `a328060000b8cb7bb389d39f8785f96f6bcb0c25` | Standard loaded; positive baseline feedback, not standalone A4 acceptance. |
| A4 omitted kill [43674201597](https://github.com/Dytschgo/arxcovo/actions/runs/43674201597), [record](linux-a4-negative-run-43674201597.json), [assessment](../reviews/m2-a4-native-negative-hosted-muse-assessment.md) | `1958f9975d1620151ca9a182c5047dd8aeb43e65` | Standard loaded; joins positive baseline to close ONLY D9a present-residue mechanism and omitted-S7 negative. |
| A5 [43644661407](https://github.com/Dytschgo/arxcovo/actions/runs/43644661407), [record](../reviews/m2-a5-hosted-record.md), [review](../reviews/m2-a5-hosted-muse-review.md) | `cf3a2510fc45e0cb438ae14cc41115e492bc0f41` | Standard loaded; exclusive empty-orphan seed, one production setup, one removal, reused `runs`, confirmed cleanup. |

Profile attribution is per run, not borrowed from the separate A3 host:
at A1/A2 `1e9d881`, the historical driver's `install_runtime()` exclusively
writes the staged unchanged unit and Standard bytes plus the fixture override,
with no post-load DropInPaths/byte verification. From A3 `beb7117` on,
`standard_profile()` / `runtime_files()` select the Standard bytes except for
A3 None; `verify_runtime()` requires exact FragmentPath, DropInPaths, directory
contents and byte equality. The accepted reviews above retain this source/result join.
A1/A2, A4 and A5 additionally pass production setup's R4 gate; A4/A5 seed
preflight repeats it before mutation. A3 None supplies no positive A10 proof.

## Operation coverage

Source paths below are repository paths at the pinned parent unless stated.
`hierarchy/setup.rs`, `exec/cgroup.rs`, `exec/unix.rs`, and both package files
have zero diff from A1/A2 `1e9d881` through this parent. The adapter's later
A4 failure-cleanup helpers are test-only; historical fixture claims refer to
the exact executed commits above, not retroactive current-fixture behavior.

| Required operation / source call | Accepted real runtime coverage | Remaining limit |
| --- | --- | --- |
| Trusted descriptor traversal, `openat`, `fstat/fstatfs`, bounded reads of type/controllers/procs/events; `fgetxattr(user.delegate)`; directory enumeration (`hierarchy/linux.rs`, setup D2/S9) | A1/A2 and A4/A5 call the real adapter, require marker `1`, root ownership, cgroup2 and supervisor identity; re-read postconditions. | Not every defensive branch is observed: e.g. `DT_UNKNOWN` fallback `fstatat` is source-only unless that entry type occurred. No privileged-writer/mount-adversary proof. |
| `mkdirat(unit, "runs", 0700)` (`setup::body` S6, `LinuxFs::mkdir_runs`) | A1/A2 requires `runs_created=true` and root-owned exact 0700. A4/A5 create it in the fixture, then production setup observes Exists/reuses it. | Real setup creation demonstrated; systemd's creation of unit/supervisor is not a service capability operation. |
| Write `+memory +pids` to unit `cgroup.subtree_control` (S5), then to `runs/cgroup.subtree_control` (S8), with readback | A1/A2 requires both production writes and controller postconditions; A5 repeats both on reused runs. A4 positive completes setup after fixture controller preparation. | Exact two-controller, empty internal-node layout only; no arbitrary controller/hierarchy claim. |
| Write `1` to `runs/cgroup.kill`, poll `populated 0` (setup S7) | A1/A2/A5 exercise empty-subtree write; A4 positive kills one placed live helper. Accepted omitted-S7 case retains live helper/populated leaf, exact KillTimeout, 1 attempted/1 omitted/0 forwarded, zero setup removals. | Negative run does **not** perform the S7 kernel write. Combined A4 establishes the D9a mechanism, not cross-build-UID kill or crash provenance. |
| `unlinkat(AT_REMOVEDIR)` on verified empty leaves, fresh empty listing/events (S7) | A5 removes exactly one seeded empty leaf. A4 positive removes the populated-then-killed leaf; omitted case removes only during separately owned failure cleanup after reap. | Production janitor removal demonstrated; no native nonremovable-leaf or systemd stop-directory claim. |
| Open `runs/{memory.max,memory.swap.max,pids.max}` read-only (S9) | A1/A2/A5 require all three opens after enablement. | Openability is not a limit write or a child admission proof. |
| Exclusive run-leaf mkdir, leaf control opens and limit writes (`Group::create`, `exec/cgroup.rs`) | A4/A5 fixture `LinuxFs::seed_a5` performs exclusive literal leaf mkdir. A4 `hierarchy/a4.rs::limits` writes/reads back memory=67108864, swap=0, pids=1. | Real equivalent operation kinds under Standard, but fixture code/values only. `Group::create` dynamic leaf and configured-value writes remain source-only/unobserved on an accepted Runner path. |
| Move a task via leaf `cgroup.procs` | A4 fixture parent `LinuxFs::place_a4` writes numeric helper PID; `ChildOps::live` checks sole membership before setup. Positive and omitted cases share placement. | Demonstrates one real parent-driven helper migration under Standard. It is not Runner child self-migration or build-account execution. |
| Runner `pre_exec` writes `0` to held leaf procs fd (`exec/unix.rs`) | **No matching accepted runtime proof.** Source places this before per-child NNP, identity drop and capability clearing (service NNP already applies). | Real Runner child self-migration, subsequent identity transition and completion are unobserved here. Numeric-PID fixture migration cannot substitute. |
| Runner leaf `cgroup.kill`, events polling, removal and create-error cleanup (`Group::kill_and_confirm`, `release_leaf`, Drop) | Janitor/A4 evidence demonstrates related kill/read/remove operations, not these Runner calls. | Exact normal/cancel/error/unwind Runner lifecycle and residue correctness remain source-only; no accepted positive A12 join. |

Source references: [setup state machine](../../crates/arxd/src/exec/hierarchy/setup.rs),
[descriptor adapter and seed/place helpers](../../crates/arxd/src/exec/hierarchy/linux.rs),
[A4 limits/oracle](../../crates/arxd/src/exec/hierarchy/a4.rs),
[A5 preflight/postconditions](../../crates/arxd/src/exec/hierarchy/a5.rs),
[Runner cgroup operations](../../crates/arxd/src/exec/cgroup.rs),
[Runner migration/order](../../crates/arxd/src/exec/unix.rs).
Historical A4 placement/oracle is `crates/arxd/src/exec/disposable/a4.rs` at
`a328060` and `1958f99`; A1/A2 entry/observations are `exec/disposable.rs` and
`exec/hierarchy/fake.rs` at `1e9d881` (paths under `crates/arxd/src/`).

## Exclusions and remaining work

[A12 record](linux-a12-run-43741300777.json) and
[independent assessment](../reviews/m2-a12-hosted-disposition-opus-assessment.md):
sole run `43741300777` on `2a1d5ad` failed in 67 s, first `E_SERVICE_OR_TEST`,
final `E_COMMAND_FAILED`, lower bound `cgroup`, cleanup **unconfirmed**,
internal service/fixture-check/harness absent. No normal/cancel, migration,
residue, account-removal or clean-host credit follows. All three pins' recorded
HTTP404 responses are not cleanup proof. No retry, diagnostic, grant or reuse.
[A7/A11 hosted FINAL NO-GO](../reviews/m2-d7k-final-hosted-nogo-opus-assessment.md)
is unchanged and is not reopened by this join.

Accepted observations cover fresh disposable GitHub-hosted Ubuntu 24.04 units,
root service identity, delegated cgroup v2 and exact packaged hardening plus the
reviewed fixture override. Required systemd >=254/kernel >=5.14 are eligibility
floors, not proof across all such versions, distros, LSMs, namespaces or hosts.
No installed/dedicated-host, arbitrary permissions, effective/permitted-set
inventory, minimum-capability proof or whole-OS policy claim is made.
Uncovered Runner paths and installed/dedicated paths are **unverified on real hardware**.

A10 stays open for the unmatched Runner operations above. M2, crash/A9/U9,
full F/G and real-Runner PA6 remain incomplete; production remains inactive.
Next independent source/doc item: trace existing stop/drain wiring and accepted
A4 records against ADR0020 D9/U9, recording which process-stop versus delegated
directory-removal obligations have accepted proof and which remain unobserved.
Keep that a bounded documentation join, with no native proposal or new policy.
Then proceed to independent M3 source/doc work as the user steers; neither step
requires reopening A7/A12 or treating M2 as complete.

Checks for this note: committed-source/review reads, historical source joins and
Git diffs only; local link resolution, UTF-8/LF and whitespace checked before
commit. No tests/builds or new runtime observations; no network revalidation.
