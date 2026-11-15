# A7/A11 D9c controlled C2-view source preparation (Opus)

2026-11-15, Anthropic claude-opus-5-5, high. No subagents. Built fresh on
branch `t3/d9c-hosted-empty-mount` from parent `b709982` (atop `5ed2b4e` with
decision `9a08d57`, atop `1702c1a`, atop main
`b3798371f93f733fe407f0d6ec1a6a657434d199`). Sol partial `b0599dee` was not
read, adopted or discarded.

This is source preparation only. It grants no native run, pin, bind, grant,
dispatch or acceptance. A7/A11, M2, full F/G and real-Runner PA6 remain
**unverified on real hardware**. The ONE remaining Opus-owned hosted attempt
still needs independent Sol review, all five required gates, coordinator merge
and a separately reviewed one-dispatch disposition.

## What changed (ADR0020 D9c, controlled fixture only)

**Fifth closed view.** The controlled fixture now has exactly five read-only
mappings: the four D9b policy views plus
`<stage>/fx/c2-empty:/usr/local/bin`. `controlled_override` emits exactly
five `BindReadOnlyPaths=` lines with no optional `-` prefix. Driver
`controlled_mapping_set` and Rust `controlled_mappings` both require the exact
five-set (Rust compares the `systemctl show` effective property,
`src:dst:rbind`). Rust `controlled_mounts` requires exactly one `ro` mountinfo
line at each of the five mount points. It rejects any other `/etc/*` line and
any submount below `/usr/local/bin/`. ProtectSystem's own `/usr` and `/etc`
lines stay allowed.

**Owned empty source.** `stage_controlled_sudo_nnp` creates `fx/c2-empty`
exclusively (`mkdir` raises on an existing name; nothing is adopted). It opens
the new directory root-down, captures the entry while pending, then runs
`fchmod(fd, 0755)` on the held new inode so umask cannot change the mode, and
pins it. The `controlled_pin(view=True)` pin requires root:root, a directory
and mode exactly 0755. Policy directories stay 0700 and files 0440, and a
0755 directory is never accepted as a policy directory. Recheck requires
seven owned entries, the exact `fx` entry set (now including `c2-empty`) and
an exactly empty `fx/c2-empty`. Cleanup is reverse-owned: policy files, then
`fx/c2-empty`, then `fx/sudoers.d`, then `fx`. Before each deletion it
reopens the path and checks the pin. If a pending, partial, colliding,
non-empty or replaced C2 source is found, nothing is deleted and the run
quarantines.

**Release before deletion.** New `a7_uids_released(accounts)` runs in
`cleanup_a1a2` after the unit stop and the `cgroup absent` check, and before
any fixture deletion (policy, C2 source, accounts, delivery). It quarantines
if a created identity was never captured or a task of a captured fixture UID
is still live. It is read-only and bounded (20000 `/proc` entries).

**In-unit proof before Runner and before each call.** Rust `controlled_view`
runs from `prepared_delivery` and from `recheck_delivery`. That covers
before `Runner::new` and before every `run_phased`. It now also opens the
owned source `fx/c2-empty` and the in-unit `/usr/local/bin` through
`delivery_open`, so every ancestor is root-owned with no 022. It reads
bounded entry sets from the held descriptors and `fstatvfs ST_RDONLY` on the
held view. It runs `fstatat(sudo, AT_SYMLINK_NOFOLLOW)` on the held view,
reopens both, and runs `fstatat` again on the reopened view. Pure
`empty_view` then requires:

- all four nodes are root:root `040755`;
- all four share the source's dev and ino (a bind keeps both);
- both entry sets are empty;
- the view is read-only;
- C2 is `Absent` (ENOENT only) on the held and the reopened view.

Present and any other errno (`Unknown`) refuse. In-unit `runtime_package`
now requires C2 to be exactly ENOENT in the view. The old present-and-equal
branch is removed, because the closed view must be empty.

**Why 0755.** The probe runs as the low-UID account. It opens the C2
directory `O_RDONLY|O_DIRECTORY` and maps EACCES to `unknown`, which
refuses. 0700 would therefore never yield `absent`. The root controller's
own `fstatat` cannot observe that difference, so the exact-mode predicate
stands in for it. Pure tests reject 0700, 0711, 0750, 0775, 0757, 0777,
setgid/sticky, non-directories, foreign owners and any non-fixture inode,
including a writable raw host directory.

**Host split (controlled fixture only).** `a7_admission` now calls the new
`a7_alias_controlled()`, which checks protected `/` and `/usr/bin`, the
bounded `/bin -> usr/bin` alias and alias-target identity. It never opens or
stats host `/usr/local`, `/usr/local/bin` or C2, and makes no host C2 safety
or absence claim. The package/C1/plugin/util comparator and `cg_preflight`
floor are unchanged and still run before effects. Path refusals keep the
existing closed roles `a7_alias_root` and `a7_alias_target`; the record
schema is unchanged. The strict `a7_alias_c2` gate is byte-unchanged, and
the D7j diagnosis still calls only it.

**Canonical profile.** The new label is
`99e7f5b4b3b63ff13955432bf9b7cde0436c9ab57afbc93fbe99bb45eb17017e` in both
the Rust and driver `A7_PROFILE`. It replaces the four-view label `51d4fb5f…`.
It is SHA256 of the UTF-8 JSON below: sorted keys, compact separators, no
trailing newline, 1066 bytes. `<stage>` and `<case0>` are templates filled only
by the validated run stage and generated account. It labels reviewed static
facts. It is not a host attestation, an observed-host digest, or a substitute
for runtime checks or native oracles. `test_canonical_profile_label_matches_five_view_facts`
rebuilds this JSON from the driver mappings, literals and C2 entry contract and
checks it against both constants and this file.

```json
{"architecture":"amd64","c2_view":{"entries":[],"mode":"0755","owner":"0:0","source":"<stage>/fx/c2-empty","sudo":"absent","target":"/usr/local/bin"},"defaults":["/etc/sudo.conf","/etc/sudoers","/etc/nsswitch.conf","/usr/libexec/sudo/"],"mappings":["<stage>/fx/sudoers:/etc/sudoers","<stage>/fx/sudo.conf:/etc/sudo.conf","<stage>/fx/nsswitch.conf:/etc/nsswitch.conf","<stage>/fx/sudoers.d:/etc/sudoers.d","<stage>/fx/c2-empty:/usr/local/bin"],"payloads":[["/usr/bin/sudo",282032,"6ae6665cc51295ea8aaba94a3377dafa563f950aafc6296c0704fb33401a31ff"],["/usr/libexec/sudo/sudoers.so",588080,"151d681d51293a040dc479cb0175d1a6d461a40ff455661cf6ad485adf305d4c"],["/usr/libexec/sudo/libsudo_util.so.0.0.0",133944,"aae167b9da2436a31eba12d9fb20b36120fd204b2c720d62881d09e89eecd26d"]],"policy":{"nsswitch.conf":"passwd: files\ngroup: files\nsudoers: files\n","sudo.conf":"Plugin sudoers_policy /usr/libexec/sudo/sudoers.so\n","sudoers":"#includedir /etc/sudoers.d\n","sudoers.d/10-a7-case0":"<case0> ALL=(root) NOPASSWD: /usr/bin/true\n"},"revision":"1.9.15p5-3ubuntu5.24.04.4"}
```

**D7j wrapper pin refresh only.** `tools/ci/a7-admission-readonly.py` changes
only `_MODULE_SHA256` and `_MODULE_SIZE`, to the new driver bytes
(`a92ab1ee…99c7`, 86286), because it loads the shared driver. Its workflow,
gate, port, output and tests are unchanged. D7j stays consumed, and this
refresh grants no D7j run.

## What did not change

These are unchanged: production Rust, rights/shim, package, helper, Cargo,
workflows, ADR, PROGRESS and the strict host gate (`a7_alias_c2`). Also
unchanged: PATH `/usr/bin:/bin`, artifacts, accounts, schema, actual NNP, the
C1 `execveat(O_PATH)` descriptor oracle, C3 alias skip, the case0 rule and
case1 no-grant policy literals, and the 55/10 RunSpecs. The 60/15 call, 15
check, 90 case and 10 join envelopes, READY/ACK, two scoped workers,
cancel-both and join-both, markers, strict terminal summary oracles,
120/140/360 budgets and the 10-minute workflow are unchanged too. `TEST`
stays fixed to the A7 literal. No grant, fence or timeout was weakened. No
host is mutated outside the existing staged and owned fixture.

## Call-graph audit (before tests)

- Driver: `run_body` calls `a7_admission` (package read, then
  `a7_alias_controlled`, then `cg_preflight`) before root/host probes,
  reservation or staging. After that it runs `stage_sudo_nnp`, then
  `prepare_controlled_sudo_nnp` (`stage_controlled_sudo_nnp`, then
  `sudo_nnp_accounts`), then `handoff_sudo_nnp`, then `install_runtime` with
  five `BindReadOnlyPaths` lines, then `verify_runtime` (exact drop-in
  bytes), then start and `monitor_a1a2`. The finally block runs
  `cleanup_a1a2`: stop, `cgroup absent`, `a7_uids_released`,
  `cleanup_controlled_sudo_nnp`, `cleanup_sudo_nnp`, delivery, runtime, then
  stage.
- `a7_alias_c2` callers: only D7j `_diagnose` (byte-unchanged) and tests.
  `a7_alias_controlled` callers: only `a7_admission`.
- Rust: `disposable_linux_a7_a11` calls `prepared_delivery`
  (`read_fixed_delivery`, then `runtime_package`, then `controlled_view`)
  before any LinuxFs/Runner work. Per account, `recheck_delivery` runs
  before `Runner::new`. Per step, `a7_case` calls `recheck_delivery` before
  `run_phased`. `recheck_delivery` repeats `read_fixed_delivery`,
  `runtime_package` and `controlled_view`. The non-ignored
  `a7_a11_delivery_stays_closed` still refuses at the environment gate
  before any host read.
- Tests and mutants run only pure extracted Rust, mocked Python ports, and
  stdin `rustc` builds. No host `/proc`, `/sys`, root, account, policy, mount,
  sudo, query, ignored fixture, Runner, LinuxFs, helper, native, network,
  dispatch or variable was used.

## Checks actually run (Windows 11 host, Python 3.12.10, rustc 1.95.0)

All TEMP/TMP and `CARGO_TARGET_DIR` were under `D:\Code\arxcovo-d9c-opus\`. No
install, cache or DB deletion.

- `python -I -B -m unittest discover -s tools/unix-exec-validation -p 'test_*.py'`:
  **180 tests OK** (baseline before changes: 173 OK). The seven new tests are
  view pin, nonempty/replaced cleanup, partial/collision creation, live UID
  release, controlled split, controlled alias fake ports and canonical
  profile. Existing tests were adapted to five mappings, seven entries and the
  split. Strict-gate coverage moved to direct `a7_alias_c2` calls with all
  four historical roles and all five ancestry paths; it was not deleted.
- `python -I -B tools/ci/test-a7-admission-readonly.py`: **13 tests OK**
  against the new driver bytes and the refreshed pin.
- Mutation harness. Every in-memory mutant is assertion-killed with zero
  test errors, and source bytes are unchanged after each run. **Total 202
  killed** (baseline 171):

  | Function | Killed | Baseline |
  | --- | --- | --- |
  | `main` | 40 | 40 |
  | `rust_mutants` | 8 | 8 |
  | `controlled_policy_mutants` | 31 | 11 |
  | `a7_integration_checks` | 15 | 12 |
  | `a7_case_mock_checks` | 5 | 5 |
  | `sudo_nnp_delivery_checks` | 24 | 24 |
  | `sudo_nnp_source_checks` | 45 | 45 |
  | `execution_preparation_checks` | 9 | 9 |
  | `admission_path_mutants` | 15 | 11 |
  | `execution_admission_mutants` | 10 | 6 |

  The pure Rust tests `a7_controlled_policy_discriminators` and
  `a7_c2_empty_view_discriminators` pass via stdin `rustc --test`. Updated
  anchors: `mappings.len() == 5`, `seen == [1; 5]`, the Python five-set and
  view-aware pin. New anchors cover:
  - submount and `/etc` rejection;
  - the C2 mapping target;
  - each `empty_view` conjunct: mode, uid, gid, dev, ino, both entry sets,
    read-only, C2 and same-inode;
  - view pin mode, `mkdir` mode, `fchmod`, entry count and empty entry set;
  - the cleanup reopen pin;
  - the live-UID, release-call and uncaptured-identity fences;
  - the controlled alias root/target role, attribution, target open and the
    two alias identity fences;
  - controlled versus strict selection in admission;
  - the profile label.
- `cargo fmt --all --check`: PASS.
- Linux type-check of the actual non-test `sudo_nnp.rs` items: `rustc
  --target x86_64-unknown-linux-gnu --emit=metadata -D warnings` against the
  locked offline linux metadata for `libc`, `sha2 0.10.9`, `serde` and
  `serde_json`, with `OwnerConfig` copied from `exec/mod.rs`: **rc 0**. A
  deliberately mistyped copy failed as expected. Full `cargo check --target
  x86_64-unknown-linux-gnu` stopped at `libsqlite3-sys`, because no
  `x86_64-linux-gnu-gcc` is installed. The full Linux compile, Clippy and
  nextest of this module are therefore **pending hosted CI**.
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`
  (Windows): **PASS**. This module is Linux-only, so it is not part of that
  compile.
- `clippy-driver -D warnings` on the same Linux non-test slice: **rc 0**.
  On the pure Rust test slice (`--test`): **rc 0**. A planted
  `bool_comparison` probe was rejected in both, so the lints were active.

## Pre-recorded predictions for the one later attempt

Not authority; for the separate disposition. On one fresh hosted Ubuntu 24.04
x64 runner with the merged exact head:

- Host admission passes: package `candidate_match` with reviewed digests,
  protected `/`, `/usr`, `/usr/bin`, the alias, and the systemd >= 254 /
  kernel >= 5.14 / cgroup2 / controllers floor. Host `/usr/local/bin` is
  never consulted.
- In-unit, before Runner and before each call:
  - the effective property is exactly the five mappings;
  - mountinfo has five `ro` lines and no `/etc/*` or `/usr/local/bin/*`
    extras;
  - every view is read-only;
  - each source/view pair shares dev and ino;
  - the policy bytes and entry sets match;
  - `fx/c2-empty` and in-unit `/usr/local/bin` are root:root 0755, empty,
    and C2 is ENOENT.
- In both accounts the probe reports `no_new_privs=true` and C1 is chosen.
  C3 is skipped by the alias. C2 is absent through the view, so
  `sudo=exit_nonzero` in both the case0-rule and case1-no-rule cases.
  Identity, descriptor, records, residue and cleanup oracles pass. CASE0,
  CASE1 and COMPLETE markers each appear once, and cleanup is `confirmed`.

## Miss limits (any one ends hosted A7/A11; NO-GO, dedicated host)

- Any pre-mutation refusal: package or digest drift, unsafe `/`, `/usr` or
  `/usr/bin`, an alias mismatch, or a floor or delegation failure.
- Any in-unit view-proof refusal: a mapping-set, mountinfo, statvfs,
  dev/ino, mode, owner, emptiness or C2 miss, or unsafe in-unit
  `/usr/local` ancestry.
- Any envelope, budget, ACK, cancel/join, terminal-summary, cleanup,
  live-UID or quarantine uncertainty.
- Any `unknown`, `untrusted` or unattributed result. There are no
  diagnostic sub-legs, image retries, new grant names or relaxed fences.
- `exit_zero` with NNP true is a product security finding: stop and
  escalate.

## Remaining limits

The kernel/systemd behavior of the fifth bind (applied after ProtectSystem
and visible to Runner children), the actual hosted `/usr/local` ancestry, and
the probe's ENOENT-to-`absent` path are reasoned from source and documentation
only. They are **unverified on real hardware**. Linux compilation of the full
crate, Linux Clippy and nextest are pending hosted CI. Source review by Sol,
the five gates, coordinator merge and the separate one-dispatch disposition
come next. No new bind, grant or diagnostic leg comes before that.
