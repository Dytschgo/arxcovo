# M2 A7/A11 dormant delivery source review - Muse (independent source/security)

Scope: base `17a2cb47306f73730c2ce990f4661094c4d01f3b` (PR118) to head
`852fadfc5d1ee8491d22dbf33e257c2c0073e9b6` on `t3/m2-delivery-security-review`
(retains Sol `7cfb816` as `6b2d213` and evidence `1e356dc` as `24217a3`;
empty diffs confirmed for both retentions). Reviewed
`17a2cb4...852fadf`: 4 source files + evidence, ~1065 lines.
Read: AGENTS.md, `docs/KICKOFF.md` routing, accepted contract
[delivery-contract](/C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-6fb54260-6f4f6489bf984d77/docs/reviews/m2-a7-a11-delivery-contract.md)
incl. 4 verbatim binding corrections, prior contract reviews `2eb1810`/`050466c`,
and evidence
[linux-a7-a11-delivery-source](/C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-6fb54260-6f4f6489bf984d77/docs/evidence/linux-a7-a11-delivery-source.md).
No Linux FS/native/proc/helper/ignored/account/sudo/admin/network/install/dispatch/
variable/push/PR/subagent operation ran. One focused pure-mock rerun only (below);
builder's 126-guard / 22-E4 / 69-kill / inventory / fmt / Windows Clippy report is
taken as supplied evidence, not rerun.
A7/A11, M2, full F/G and real-Runner PA6 remain **unverified on real hardware**.

## Standards - PASS, no findings

- One-concern docs-only review commit scoped as instructed; no PROGRESS/source/ADR
  edits. Small conventional message expected; LF/Markdown only.
- No secrets, owner data, policy bytes, identities, product approval flows,
  publishing defaults, or host-audit framework added. Closed-qualification language
  (`False`, `None`, `E_POLICY_UNATTESTED`) and hardware-unverified labelling kept.
- Smell baseline: no new abstraction to flag. Fixed `Delivery`/`DeliveryAccount`/
  `DeliveryArtifact` shapes, `sudo_nnp_artifact`/`build_sudo_nnp`/`sudo_nnp_open`/
  `sudo_nnp_digest`/`stage_sudo_nnp`/`handoff_sudo_nnp`/`cleanup_sudo_nnp_delivery`
  and `delivery_schema`/`delivery_open`/`delivery_account`/`delivery_workspace`/
  `read_fixed_delivery`/`recheck_delivery` are the contracted surface, not
  speculative generality. Duplicated per-case/per-artifact loops are the existing
  two-case shape. No message chains, middle men, or refused bequests introduced.

## Spec - PASS (all 4 binding corrections implemented verbatim)

1. JSON-to-gate binding: [sudo_nnp.rs:88-120](C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-6fb54260-6f4f6489bf984d77/crates/arxd/src/exec/disposable/sudo_nnp.rs:88)
   rejects `len > 4096`, requires `source == workflow == reviewed` (40-hex) and
   `profile` 64-hex, then `d.run == run`, `d.source_sha == source`,
   `d.host_profile_sha256 == profile`; mismatch refuses before effects.
   [sudo_nnp.rs:308-333](C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-6fb54260-6f4f6489bf984d77/crates/arxd/src/exec/disposable/sudo_nnp.rs:308)
   binds `run` from `GITHUB_RUN_ID-GITHUB_RUN_ATTEMPT`, `source` from `GITHUB_SHA`,
   workflow/reviewed from `ARX_WORKFLOW_SHA`/`ARX_REVIEWED_SHA`.
   `prepared_delivery` ([sudo_nnp.rs:11-16](C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-6fb54260-6f4f6489bf984d77/crates/arxd/src/exec/disposable/sudo_nnp.rs:11))
   and `recheck_delivery`
   ([sudo_nnp.rs:462-480](C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-6fb54260-6f4f6489bf984d77/crates/arxd/src/exec/disposable/sudo_nnp.rs:462))
   both use literal `expected_profile: None`, so both refuse before any
   observation. Driver mirrors with `sudo_nnp_host_attested() is False`,
   `sudo_nnp_eligible(..., False)`, and `expected_profile = None` literals at
   [driver.py:173-178](C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-6fb54260-6f4f6489bf984d77/tools/unix-exec-validation/driver.py:173),
   [driver.py:444](C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-6fb54260-6f4f6489bf984d77/tools/unix-exec-validation/driver.py:444),
   [driver.py:501-503](C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-6fb54260-6f4f6489bf984d77/tools/unix-exec-validation/driver.py:501);
   closed-before-effects is test-pinned
   (`test_closed_before_build_or_handoff_effects`).
2. Reentry-to-harness identity: staged libtest comes only from the single
   `cargo test --locked --lib --no-run --message-format=json` invocation and the
   arxd binary only from the single `cargo build --locked --bin arxd` invocation
   ([driver.py:343-392](C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-6fb54260-6f4f6489bf984d77/tools/unix-exec-validation/driver.py:343),
   exactly-one-artifact + fixed-path pins in
   [driver.py:323-340](C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-6fb54260-6f4f6489bf984d77/tools/unix-exec-validation/driver.py:323)).
   Copy is held-fd read -> `O_EXCL` create -> destination rehash + source rehash
   ([driver.py:471-494](C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-6fb54260-6f4f6489bf984d77/tools/unix-exec-validation/driver.py:471)).
   Rust binds controller bytes to the staged reentry hash independently of the bin
   build ([sudo_nnp.rs:397-426](C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-6fb54260-6f4f6489bf984d77/crates/arxd/src/exec/disposable/sudo_nnp.rs:397)).
3. Exact `OwnerConfig`/`RunSpec` fields: derived (never from JSON)
   `account=name`, `uid`/`gid` captured `>=1000` distinct,
   `workspace=/var/lib/<name>/workspace`,
   `protected_directories=[stage]`, `environment={PATH:/usr/bin:/bin}`,
   `admin=false`, `cgroup_parent=/sys/fs/cgroup/system.slice/arxd.service/runs`,
   `memory_bytes=64MiB`, `pids=32`
   ([sudo_nnp.rs:337-362](C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-6fb54260-6f4f6489bf984d77/crates/arxd/src/exec/disposable/sudo_nnp.rs:337));
   `RunSpec.cwd=workspace`, `timeout=20s`, `output_bytes=4096` per Runner call
   ([sudo_nnp.rs:787-808](C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-6fb54260-6f4f6489bf984d77/crates/arxd/src/exec/disposable/sudo_nnp.rs:787));
   owned base/workspace dev/ino + owner + `0700` rechecked before EACH call via
   `recheck_delivery` at both call sites
   ([sudo_nnp.rs:794](C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-6fb54260-6f4f6489bf984d77/crates/arxd/src/exec/disposable/sudo_nnp.rs:794),
   [sudo_nnp.rs:809](C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-6fb54260-6f4f6489bf984d77/crates/arxd/src/exec/disposable/sudo_nnp.rs:809)).
4. Executable admission TOCTOU: recheck is `O_NOFOLLOW|O_CLOEXEC` open + `fstat`
   dev/ino/mode/uid/nlink match against pinned JSON immediately before admission
   ([sudo_nnp.rs:427-451](C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-6fb54260-6f4f6489bf984d77/crates/arxd/src/exec/disposable/sudo_nnp.rs:427));
   `delivery_open` validates every ancestor owner-0/no-group-other-write including
   `/usr`, `/usr/libexec` and the exclusive
   `/usr/libexec/arxcovo-a7-a11-<run>-<attempt>` dir (root-owned `0755`, no
   pre-existing adoption via `mkdir` collision failure +
   `owned['created']`/`E_FIXTURE_COLLISION` guards); load-bearing
   root-writable-ancestry assumption is stated at
   [sudo_nnp.rs:132-134](C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-6fb54260-6f4f6489bf984d77/crates/arxd/src/exec/disposable/sudo_nnp.rs:132)
   and in `delivery_workspace`.
   Dormant build/cleanup bounds hold: single libtest + single arxd, shared 360s
   deadline, 4MiB private capture, `O_EXCL` exclusive 0755 staging outside the 0700
   stage, held-fd copy with dest+source rehash, no sudo copy, no unknown-artifact
   admission (`set(listdir) == images`), owned partial-copy retention, stop-gated
   (`unit_gone is True`) quarantine. `run_body` has no edge to
   `build/stage/handoff/cleanup_delivery` (only dormant comment at
   [driver.py:171](C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-6fb54260-6f4f6489bf984d77/tools/unix-exec-validation/driver.py:171)
   and docstring at
   [driver.py:343-348](C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-6fb54260-6f4f6489bf984d77/tools/unix-exec-validation/driver.py:343));
   TEST stays `a4_omitted_kill`, `standard_profile`/`a4_selected` unchanged.
   Non-blocking consistency note (not a defect, no fix required): driver
   `plan()`/`identity()` accept run attempts up to 20 digits while Rust
   `delivery_names` and driver `sudo_nnp_names` cap attempts at 6; both sides
   fail closed (Rust refuses, driver handoff refuses), so no bypass exists.

## Security - PASS, no new exposure

- Fail-closed: `None` profile, `False` host/policy latches, closed TEST mapping,
  `E_POLICY_UNATTESTED` before build/stage/handoff effects, owned-only cleanup with
  reservation/quarantine on partial capture, changed inode, or uncertain stop.
  No per-run sudo, no `/etc` mutation in this slice, no install/relaxation, no
  semantic policy inference.
- Fixed strict reader: `<=4096` JSON, `deny_unknown_fields` + struct-level
  duplicate rejection + trailing-record refusal (test-pinned), root-owned 0600
  nlink-1, root-down no-follow descriptor opens, regular-ELF/owner-0/nlink-1/
  exact size+hash+dev/ino (each `<=128MiB`), held descriptors through admission,
  per-call reopen+fstat. NSS lookups are bounded (`getpwnam_r`/`getgrnam_r` in a
  fixed 64KiB buffer with in-buffer pointer checks, `nologin` + pinned uid/gid
  + sole-group `getgrouplist == [gid]` + empty `gr_mem`).
- Staged executables live outside the 0700 stage; build accounts hold no ancestor
  write bit. Input hashes bind staged bytes to the reviewed build only and do not
  self-attest source/host — stated honestly in code comments.
- Linux-only types were read but not compiled here (Windows session, no cross
  toolchain per evidence): `getgrouplist` infers `[u32; 64]`/`i32` count matching
  `gid_t`/`c_int`, `open`/`openat` flag/pointer shapes match `libc` conventions,
  `MaybeUninit<passwd/group>` + `read_unaligned` null-check is bounded. Actual
  hosted-Linux compilation remains the publication gate (below), not a finding.

## Actual checks and limits

- Reran focused pure-mock suite after call-graph audit (filesystem/account/process
  ports mocked, no Cargo/native): `python
  tools/unix-exec-validation/test_driver.py SudoNnpDelivery` — 6 tests OK
  (0.017s, Windows). This corroborates single-output identity, shared budget/cap,
  closed-before-effects, digest/link rejection, quarantine-on-swap, and
  same-harness-bytes/partial-failure paths.
- Not rerun (builder evidence accepted, limits noted): full 126-guard discovery,
  22 E4 portable, 69 mutation kills, S8-S14/inactive inventory, E4 pin checks,
  `cargo fmt`/`git diff --check`, offline Windows Clippy. No native/helper/ignored
  fixture, LinuxFs, account, sudo, probe, network, install, dispatch, or variable
  operation ran here. Linux integration uncompiled in this session.

## Blockers and publication eligibility

- Blockers in this source slice: none. No minimal fix required.
- Eligible publication of this dormant slice: this independent review (this file)
  + hosted Linux compilation of the Linux-only reader/controller + green required
  checks (CI, `ci-time-budget`, `agent-review`) on the exact head. No new review
  requirements are added.
- Not eligible: any A7/A11, M2, full F/G, or real-Runner PA6 claim; any
  provisioning, selection, execution, or policy-byte release before the separate
  bounded root-read-only qualification source/grant review, the attested
  fresh-host evidence, and the execution grant pinning both cases,
  source/artifacts/workflow/profile, one dispatch and variable lifecycle.
  Qualifying host policy stays `False`; closed TEST mapping stays unchanged until
  those grants. E4 production/source pin inputs are unchanged; no pin refresh.

Findings: Standards 0; Spec 0 (4/4 binding corrections verified); Security 0.
A7/A11/M2/full-F/G/PA6 remain **unverified on real hardware**.
