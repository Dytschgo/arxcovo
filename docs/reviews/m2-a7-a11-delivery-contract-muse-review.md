# M2 A7/A11 delivery contract review - Muse (independent architecture/security/practicality)

Scope: contract commit `b4e45ec3dd00d94abe3f183c840ecb92f8accc60`
(`docs(m2): define fixed A7 A11 delivery contract`, 1 file, 90 lines)
on parent `7dff7c43041f2d9f116ef883cee6d9a6c3b8a83`.
Reviewed at `54902b08a6711bb952394d9a70e448d5d2593` worktree clean except this review.
Base: PR118 merge `17a2cb47306f73730c2ce990f4661094c4d01f3b` (5Muse PASS, hosted compile).
Read: AGENTS.md, `docs/KICKOFF.md` M2/routing, accepted plan
`docs/reviews/m2-a7-a11-sudo-nnp-plan.md` + Muse plan review
`docs/reviews/m2-a7-a11-sudo-nnp-plan-muse-review.md`, source review
`docs/reviews/m2-a7-a11-source-muse-review.md`, evidence
`docs/evidence/linux-a7-a11-source-preparation.md`, `crates/arxd/src/exec/disposable/sudo_nnp.rs`
(`Prepared` shape, `prepared_delivery()->None`, controller, discriminators),
`crates/arxd/src/exec/mod.rs` (`OwnerConfig`/`RunSpec`/`RunResult`),
`tools/unix-exec-validation/driver.py` (`prepare_sudo_nnp`, `cleanup_sudo_nnp`,
`gate`/`identity`/`plan`, `copy_tree`/`cargo_env`, `run_body` build/harness/monitor/cleanup,
`protected`/`same_object`/`call`/`cg_scan`).
No implementation, build, test, dispatch, account, sudo, probe, push, or native operation ran.
A7/A11, M2, full F/G and real-Runner PA6 remain **unverified on real hardware**.

## Standards - PASS, no findings

- Docs-only single-file commit, small conventional message, UTF-8 LF, Markdown only.
- No source, ADR, PROGRESS, workflow, packaging, test, path, command, identity,
  policy text, secret, or owner data bytes. No product approval flow, publishing
  default, checklist, or host-audit framework added.
- Consistent with settled style: concern-per-file reviews, hardware-unverified
  labelling, closed qualification language, `agent-review` before merge.
- Smell baseline: no new abstraction to flag; the fixed JSON/staging vocabulary
  (run, source_sha, host_profile_sha256, accounts[2], artifacts[2]) is the
  requested contract surface, not speculative generality.

## Spec - CONDITIONAL PASS (contract is the right minimal surface; 4 wording pins block source prep)

The contract correctly carries the brief: fixed `<=4096` JSON
run/source/profile/accounts/artifacts with no paths/commands; derived stage/self
exe/run identities; root-owned 0755 staging outside private 0700 stage with
hash/inode admission; real Runner 2 cases (probe + E4 reentry) per account;
captured downed accounts/workspaces; existing build/runtime/cleanup bounds;
qualification stays False/closed TEST with a separate bounded root-read-only
capture under a new grant before any exact-byte allowlist, and no semantic parser.
Feasibility against the callgraph is confirmed: `Runner::run_phased` is directly
callable in tests, `prepare_sudo_nnp`/`cleanup_sudo_nnp` already implement
owned-only provisioning/cleanup with collision refusal and quarantine, and the
dormant `Prepared` shape (two `OwnerConfig`, production arxd path, E4 libtest path)
matches the described replacement target.

Blocking wording pins (source prep must include all four verbatim; no new ADR):

1. JSON-to-gate binding. Schema list alone does not bind the JSON. Require:
   JSON `run` equals `gate`/`identity` (`GITHUB_RUN_ID-GITHUB_RUN_ATTEMPT`);
   `source_sha` equals `GITHUB_SHA == ARX_REVIEWED_SHA == ARX_WORKFLOW_SHA`;
   `host_profile_sha256` equals the independently reviewed attested profile
   digest for this exact host/image. Any mismatch refuses before effects.
2. Reentry-to-harness identity. "Libtest copy is both E4 caller and the same
   build as controller" is ambiguous across the two cargo invocations.
   Require: the staged libtest artifact comes from the single
   `cargo test --locked --lib --no-run --message-format=json` invocation that
   also produced the harness binary, and its bytes equal the harness artifact
   bytes (hash equality checked before copy and rechecked after copy);
   the staged arxd artifact comes from the single
   `cargo build --locked --bin arxd --message-format=json` invocation as the
   one non-test artifact. State the copy direction (held-fd read, `O_EXCL`
   create, destination byte reverify) for both.
3. Exact `OwnerConfig`/`RunSpec` field names. "Private roots=[stage]" and
   "cwd/rights config" do not name code fields. Require:
   `account=name`, `uid`/`gid`=captured `>=1000` distinct across cases,
   `workspace=/var/lib/<name>/workspace` derived (never from JSON),
   `protected_directories=[stage]`, `environment={PATH:/usr/bin:/bin}`,
   `admin=false`, `cgroup_parent=/sys/fs/cgroup/system.slice/arxd.service/runs`,
   `memory_bytes=64MiB`, `pids=32`, `RunSpec.cwd=workspace`,
   `timeout=20s`, `output_bytes=4096`; recheck owned base/workspace
   dev/ino + owner + `0700` immediately before each Runner call.
4. Executable admission TOCTOU scope. "Hold descriptors; recheck named identity"
   is insufficient unless the recheck is an `O_NOFOLLOW|O_CLOEXEC` open plus
   `fstat` dev/ino/mode/uid/nlink match against the pinned JSON immediately
   before each Runner call, and `protected()` covers every ancestor including
   `/usr`, `/usr/libexec`, and the exclusive
   `/usr/libexec/arxcovo-a7-a11-<run>-<attempt>` dir itself (root-owned `0755`,
   no pre-existing adoption). State the load-bearing assumption: only
   root-writable ancestry makes path-based exec after recheck safe; build
   accounts hold no write bit on any ancestor.

Practicality notes (not blockers): `128MiB` per-executable cap and shared `360s`
build / `4MiB` build capture / `120s` harness / `1MiB` private capture /
`<=20s`/`4096`-byte probe discipline reuse existing bounds and are implementable
in `run_body`; exclusive `0440` root-owned rule file for case0 only plus
"case1 has no applicable rule, not merely no owned file" is the correct
two-case fence; refusing links/unknown includes/plugins/routing/excess and
capping the capture set (`<=32` files, `<=16KiB` each, `<=128KiB` total) keeps
the later qualification capture bounded.

## Security - PASS WITH HARD CONSTRAINTS (no new exposure; pins above are load-bearing)

- Fail-closed posture retained: `False` host/policy latches, closed TEST mapping,
  `prepared_delivery()->None`, `E_POLICY_UNATTESTED` before effects, owned-only
  cleanup with reservation/quarantine on partial capture, changed inode, or
  uncertain stop. No per-run sudo, no in-service `/etc` mutation
  (`ProtectSystem=full`), no installation/relaxation, no policy inference.
- Staged executables outside the `0700` stage in an exclusive root-owned `0755`
  dir with no adoption, descriptor-relative no-follow opens, regular-ELF /
  owner-0 / nlink-1 / exact size+hash+dev+ino checks, and ancestry validation
  are the correct trust boundary, subject to pin 4.
- Input hashes bind staged bytes to the reviewed build only; the contract
  states honestly they cannot self-attest source or host. Source/workflow
  admission and the independently reviewed host profile supply trust, plus pin 1.
- Bounded output discipline holds end to end (`<=4096` delivery JSON and
  probe/reentry parsers, `96`-byte rights line shape, `1MiB` private harness
  capture reduced to booleans); later qualification capture releases only fixed
  results/digests, never raw policy/identities.
- Full policy/include/plugin/NSS scope is correctly deferred to a closed
  exact-byte allowlist (policy plugin/routing, all includes and defaults,
  no external sources/aliases/netgroups or other applicable grants for
  sole-group accounts, pinned local plugin bytes if applicable, protected
  installed `/usr/bin/sudo`, C2 absent-or-trusted, root-owned `/bin` alias
  shape, systemd/kernel/cgroup2/delegation floor). No semantic parser is
  introduced.

## Next source eligibility

- Eligible next: dormant handoff/build/cleanup source only (`sudo_nnp.rs`,
  `driver.py`, `test_driver.py`, `mutation_check.py`) incorporating pins 1-4
  verbatim; retain `False` qualification and closed TEST mapping until grants.
  Pure rejection/mutation tests for schema, source/hash/inode/path swaps,
  private-stage traversal, account/config mismatch, policy drift, collisions,
  and partial cleanup; retain S8-S14/inventory/E4 pin/driver guard suites.
- Not eligible: any A7/A11, M2, full F/G, or real-Runner PA6 claim; any
  provisioning, selection, execution, or policy-byte release before the separate
  bounded root-read-only qualification source/grant review, the attested fresh-host
  evidence, and the execution grant pinning both cases, source/artifacts/workflow/
  profile, one dispatch, and cleanup/variable lifecycle. D7a-g stay consumed; no
  retries or host reuse. Coordinator automates authorized fresh-host actions
  through independent review and green required gates including `ci-time-budget`;
  no extra human approval. No ADR rewrite absent a requirements change.

Findings: Standards 0; Spec 4 blocking wording pins; Security 0 new issues.
Worst: Spec pins 1-4 must land before source prep; strongest caveat is the
unstaged-executable TOCTOU assumption closed only by pin 4.
A7/A11/M2/full-F/G/PA6 remain **unverified on real hardware**.
