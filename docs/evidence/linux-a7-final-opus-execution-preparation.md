# A7/A11 final Opus-owned hosted attempt preparation (D7k)

2026-11-15, Anthropic claude-opus-5-5, high, implementation sub-agent. No
subagents. Branch `t3/d7k-final-opus-disposition`, parent
`ee6a45ed0214d8e8da2cb4e8b0f165096e034719` (coordinator PROGRESS hand-off, tree
`d780bdfe7b6c8c16b0bf842c26fc35110ef94d5b`) atop main
`52ce2a5d2443de018b06965dfca43fdc626b5f36`. This builder edits only ADR0019 D7k
and preamble and this file. It does not edit PROGRESS.

Docs-only preparation. It grants no run, pin, dispatch or acceptance. D7k becomes
effective only through the ADR0019 D7k gates. A7/A11, M2, full F/G and real-Runner
PA6 remain **unverified on real hardware**.

## Read before writing

AGENTS, KICKOFF, PROGRESS (current state and A7/A11 attempt ceiling), the
[Opus hosted-achievability decision](../reviews/m2-a7-a11-opus-hosted-achievability.md),
the [cross-provider correction handoff](../reviews/m2-a7-a11-cross-provider-correction-handoff.md),
ADR0019 D7h/D7i/D7j (all consumed), ADR0020 D9b/D9c, the
[D7i preparation](linux-a7-a11-execution-preparation.md) for the delivery and
budget contract, the
[Opus D9c source evidence](linux-a7-d9c-opus-source-preparation.md), the
[independent Sol source review](../reviews/m2-a7-d9c-sol-source-review.md), and
the closed failure records for D7i (`43715436515`) and D7j (`43720496074`).

## Identity verified locally (git only)

| Item | Commit | Tree |
| --- | --- | --- |
| Baseline main, PR #133 squash | `52ce2a5d2443de018b06965dfca43fdc626b5f36` | `923842bae41f48ec2be53b2f3a1372b0ede61e88` |
| Reviewed exact head | `caba0fe0635ad00784d9f2006cbf225b032f4ad5` | `923842bae41f48ec2be53b2f3a1372b0ede61e88` |
| Opus D9c source | `f41af38f570c3ceeaeff87ec7decfcc6f9b0d9c7` | `12effb39a669856a5389a35f386a866b056283b5` |
| Retained source | `c596f50cf79ec1ac51bae7d0ba615a0a2b0ef37e` | `12effb39a669856a5389a35f386a866b056283b5` |
| Squash parent | `b3798371f93f733fe407f0d6ec1a6a657434d199` | (PR #132) |

- Reviewed head and squash trees are exactly equal.
- `f41af38` and `c596f50` share one tree.
- `f41af38` to `52ce2a5` differs only in `docs/PROGRESS.md` and the added Sol
  review, so every source blob Sol reviewed is the merged blob.
- `b379837` to `52ce2a5` non-doc delta is exactly the five reviewed paths:
  `sudo_nnp.rs`, `driver.py`, `test_driver.py`, `mutation_check.py`,
  `tools/ci/a7-admission-readonly.py`.
- `52ce2a5` to `ee6a45e` changes only `docs/PROGRESS.md`. The non-doc inventory
  of `ee6a45e` is byte-equal to `52ce2a5`.

Supplied coordinator facts, not re-queried here: PR #133 merged
2026-11-15T14:53:14Z; independent Sol source and exact-head PASS comment
6896280064; status 58964602017; all five required checks green, including the
hosted Linux compile of the changed controller.

## Fixed source facts the attempt depends on (read, unchanged)

- `driver.py` `TEST = 'exec::disposable::sudo_nnp::disposable_linux_a7_a11'`.
  `A7_PROFILE` in driver and Rust is
  `99e7f5b4b3b63ff13955432bf9b7cde0436c9ab57afbc93fbe99bb45eb17017e`. That is a
  label for reviewed static facts. It is not a host attestation, and it does not
  replace the in-unit five-view proof or the native oracles.
- `driver.py` gate: `GITHUB_SHA == ARX_REVIEWED_SHA == ARX_WORKFLOW_SHA`,
  checkout `git rev-parse HEAD` equality, and `GITHUB_RUN_ATTEMPT == '1'` for A7
  before `a7_admission`, which runs before root/host probes, reservation or
  staging.
- `unix-execution-experiment.yml`: manual dispatch only;
  `Dytschgo/arxcovo`; actor and triggering actor `Dytschgo`; `refs/heads/main`;
  `github.sha == github.workflow_sha == vars.UNIX_EXECUTION_REVIEWED_SHA`;
  `ubuntu-24.04`; 10-minute job; `contents: read`; pinned checkout without
  persisted credentials. It has no inputs.
- That workflow does not read `SUDO_PACKAGE_REVIEWED_SHA` or
  `A7_ADMISSION_REVIEWED_SHA`. Their absence is a coordinator precondition
  checked before and after, not an in-run check. `a7-admission-readonly.yml`
  requires the native pin absent, so the two legs cannot both pass their gates.

## Key runtime blobs (baseline `52ce2a5`)

| Path | Git blob | Bytes | SHA256 |
| --- | --- | ---: | --- |
| `crates/arxd/src/exec/disposable/sudo_nnp.rs` | `6a3031a579d9fb1d1f499a3b4639c216e496d14f` | 64906 | `46b0de072f4e7fb8feb47e50ef670ac3d7678a4266baa9b991f4ce73693f31d8` |
| `tools/unix-exec-validation/driver.py` | `acfd9c61aaa10bc065f43d80a15be0cc9e733291` | 86286 | `a92ab1ee64b8a3979d436a2d35e9baae355cb3a74a762a5efaf62acce0e599c7` |
| `tools/unix-exec-validation/test_driver.py` | `6504c3135b4b2f6df1fd55e196100ccccbf6efdb` | 147302 | `869092ae27ece2d78cc651a0a33ec9dbe7560ad36b9694c814c26f1da76ddd22` |
| `tools/unix-exec-validation/mutation_check.py` | `7bf0b5e61305543caeabf9a4ae2ee8002657402e` | 56157 | `f1c35726f459cd0e32f8d32a8261b32d32db4272dcf51e9bfc19530ef9129402` |
| `tools/unix-exec-validation/sudo_host_qualification.py` | `41ec1325a77669d34c20f00c4b04d40c9d8086a4` | 8528 | `b6a1891c14acfacfe2bbd2df1aefa1e79736c6be1e0592f353dea5ea55c3c5c9` |
| `tools/ci/a7-admission-readonly.py` | `856c8b9b5dc9beaad2689f66ffb414eb4a042cf7` | 9172 | `b4f3b3d42aa492219e0b52bff17d580482c2255ede166e42d9c99730f11cceec` |
| `crates/arxd/src/unix_shim_rights_linux.rs` | `daa2144283fea0d2b4bd859c796fb86a827b631d` | 14519 | `3fc3e949dc0725d22d466c12f1ef3fa13d395fea586a84546c8cb4e4be8ceef4` |
| `packaging/linux/50-arxcovo-standard-rights.conf` | `93a3abcce9782cc7fddf4c2c1dcc5fc74ec18a74` | 134 | `3afb57ea49b5004d9e83293ad424ca1542a4d732d8642ccddf26e2668f49d67a` |
| `tools/unix-exec-validation/a4-helper/build.py` | `5412b0cf345d3bd5dd0106e972a6b10e12827b7b` | 10727 | `6db176037849bdce663c25b79dc34025fae4539580ef832ff9f6f9592d12b45e` |
| `.github/workflows/unix-execution-experiment.yml` | `dbcca2276307357301719ed68a09631b8ab7a10a` | 2232 | `7b5370f46179ed8709d8ce7946968c735194ade55b3eb893e3c43c33c6a3efd3` |
| `.github/workflows/a7-admission-readonly.yml` | `5568feb19d9391ace8c6434909d4f19e9a4e36e8` | 2342 | `7878e894b9487183dd8d9cdc4788b0ee73ad100b75b5410617b22ab23b5bbc33` |
| `.github/workflows/sudo-package-readonly.yml` | `fb4f0a5e28304e8dfc61cd3770c2ddff4f462ea3` | 1929 | `fe7e423573e43299c794f58c3dff0a9b9cbc69512654a1c973a32c90e84f94a1` |
| `.github/workflows/ci.yml` | `8fe814e48a7cc5f88561c64da8eb9c0b19877d33` | 3701 | `07b8c8c350cb28e7f60ed293e4565d1f250d0f2c2bd2604bec35e6dda6d6df72` |
| `Cargo.lock` | `1c5c0fcf2b577f89ed998826ddf6608338b4b523` | 51052 | `1ebaf3af25f462da12eb8e8f7cfff3803deb180dd4c1f294be67bef3f3a7e4cc` |
| `Cargo.toml` | `343f7301c8744ba3ee1b8ebfb5422c70a04479f9` | 177 | `a71354fd4e463f4ef6c3f02ca3a76ce03902c01f4d497f9863fa55b12f0cec2b` |
| `rust-toolchain.toml` | `3417ee590341edc47a046d94e77ee9bee2b33bf4` | 86 | `97aab3493c1fb14fe9a21c760433577f434714f5725bbb176e67048dde581afe` |

The `driver.py` SHA256 equals the D9c evidence wrapper pin (`a92ab1ee…99c7`,
86286 bytes).

## Strict docs-only candidate predicate

Compare the candidate main squash with baseline `52ce2a5` as recursive full trees
including modes. There must be no deletions, renames, type changes or mode changes.
Only these regular 100644 paths may differ:

- `docs/adr/0019-unix-native-execution-bridge.md`
- `docs/evidence/linux-a7-final-opus-execution-preparation.md` (new, this file)
- `docs/reviews/m2-d7k-final-opus-disposition-sol-review.md` (new Sol review)
- `docs/PROGRESS.md` (existing coordinator checkpoint only)

Then require:

1. The candidate's non-doc inventory equals the full inventory below
   byte-for-byte. Its SHA256 is
   `04a7f684092042c0001598725d6c9d3e6890e2d9a5d8dc5d7842ab3557d749b3` (222
   records, 19629 bytes, LF only) from
   `git ls-tree -r <candidate> | grep -v $'\tdocs/'`. Every runtime, source,
   test, workflow, helper, package, Cargo/toolchain and policy blob is therefore
   byte-identical to the baseline.
2. The reviewed head and the candidate squash have exactly equal full trees.
3. Live main read back immediately before pin and dispatch is still the exact
   candidate.

Any miss stops D7k. It needs a new reviewed disposition and is never normalized.

## Operation contract (coordinator only, after all gates)

1. Independently confirm all three pins absent: `UNIX_EXECUTION_REVIEWED_SHA`,
   `SUDO_PACKAGE_REVIEWED_SHA` and `A7_ADMISSION_REVIEWED_SHA`.
2. Set `UNIX_EXECUTION_REVIEWED_SHA` once to the candidate full SHA and read back
   equality.
3. Dispatch `unix-execution-experiment.yml` once on `main` with no inputs, onto
   one fresh hosted `ubuntu-24.04` amd64 host, attempt 1.
4. Capture the accepted run URL directly from dispatch output. Monitor that exact
   run and attempt through terminal. Keep the pin even if ancillary lookups fail.
   Reconcile uncertain identity against that URL only. No early clear and no
   second dispatch. A rejection counts only once it is confirmed that no run was
   accepted.
5. Finally delete the pin and independently verify HTTP404. Confirm all three
   pins absent.

Any accepted dispatch consumes D7k, including success, failure, infrastructure
failure, skip, cancellation and unknown. A confirmed rejection consumes it too.
No retry, rerun, image retry, diagnostic read, new grant name or extra owner
approval exists. Excluded hosts: every D7a–j and earlier experiment host,
including `43091339273`, `43627472312`, `43635131545`, `43636804545`,
`43644661407`, `43666413693`, `43672720190`, `43674201597`, `43693642693`,
`43715436515` and `43720496074`.

## Pre-registered falsifiable predictions

Host admission (before effects; unchanged source):

- Package `candidate_match` for installed C1 sudo, plugin and util against the
  reviewed Noble `1.9.15p5-3ubuntu5.24.04.4` amd64 digests.
- Protected `/` and `/usr/bin`, `/bin -> usr/bin` alias and target identity.
- systemd >= 254, kernel >= 5.14, cgroup2, controllers and delegation floor.
- Host `/usr/local/bin` is never consulted.

In-unit, before `Runner::new` and before every call:

- The effective property is exactly the five mappings.
- mountinfo has five `ro` lines and no extra `/etc/*` or `/usr/local/bin/*`.
- `fstatvfs` reports `ST_RDONLY`.
- Each source/view pair shares dev and ino.
- Policy bytes and entry sets match.
- `fx/c2-empty` and the in-unit `/usr/local/bin` are root:root 0755 and empty,
  and C2 is exactly ENOENT on the held and the reopened view.

Both cases (case0 has the NOPASSWD `/usr/bin/true` rule; case1 has no grant):

| Prediction | case0 | case1 |
| --- | --- | --- |
| `no_new_privs` | true | true |
| Candidate | C1 chosen | C1 chosen |
| C3 | skipped (alias) | skipped (alias) |
| C2 | absent (ENOENT through view) | absent (ENOENT through view) |
| `sudo` | `exit_nonzero` | `exit_nonzero` |

All identity, descriptor (`execveat(O_PATH)`, CLOEXEC), record, residue,
live-UID and cleanup oracles pass. CASE0, CASE1 and COMPLETE each appear exactly
once, terminal cleanup is `confirmed`, and the job ends within the merged budgets
(60/15 call, 15 check, 90 case, 10 join, 120 monitor, 140 runtime, 360 build,
10-minute workflow).

## Outcome rules

- **Security finding:** `exit_zero` with `no_new_privs=true` in either case.
  Stop, escalate, no further hosted attempt.
- **NO-GO hosted (any other miss):** preflight, package or digest drift, view
  proof, mapping, mount, mode, emptiness, C2, time, envelope, budget, ACK,
  cancel/join, terminal summary, cleanup, live-UID, quarantine, `unknown`,
  `untrusted`, unattributed, skipped, cancelled or infrastructure failure.
  A7/A11 stay **unverified on real hardware** pending a dedicated Linux host
  (systemd >= 254, kernel >= 5.14, cgroup v2, no writable `/usr/local/bin`). That
  host is the owner's and is not an agent admin step. Work advances to the
  remaining M2 items (crash/A9/U9, A10, A12, full F/G, real-Runner PA6) under
  their own dispositions, then independent M3. M2 is not marked complete.
- **Matching run:** still not acceptance until a mandatory independent post-run
  source/run/oracle assessment. Credit covers only the exact case proof shown.
  Source feasibility is not acceptance.

Retain bounded sanitized evidence in every branch, and exclude the host afterward.

## Checks actually run (Windows 11, Git Bash, local repository only)

- `git rev-parse` of every commit/tree in the identity table, and
  `git diff --name-status` for `f41af38..52ce2a5`, `b379837..52ce2a5` (non-doc)
  and `52ce2a5..ee6a45e`.
- Non-doc inventory equality `52ce2a5` versus `ee6a45e` (`cmp`: equal). Its
  SHA256, record count, byte count and CR count (0).
- `git cat-file` sizes and `sha256sum` of each key blob above.
- `grep` of `TEST`, `A7_PROFILE`, the attempt/SHA/HEAD gates and the three
  workflows' pin expressions.
- After writing: branch diff paths against `52ce2a5`, `git diff --check`,
  UTF-8/LF/no-CR checks on both edited docs, and relative link targets exist.

Not run: no build, test, Cargo, host, root, `/proc`, `/sys`, sudo, account,
mount, policy, Runner, LinuxFs, helper, native, dispatch, variable, network,
push, PR, merge or watch.

## Full non-doc source inventory at `52ce2a5`

Every tracked path outside `docs/`, with mode, type and blob. Docs are bound by
the predicate above and by reviewed-head/squash tree equality.

```text
100644 blob d0d6371de329ffb232ba2fb9b79732165a702be7	.gitattributes
100644 blob de7a8c79940b870926acec71eeeb7707e2f09886	.github/agent-authors.json
100644 blob 575540057aee74d979b86ff6c086bf8ae465dd09	.github/main-protection.json
100644 blob 5568feb19d9391ace8c6434909d4f19e9a4e36e8	.github/workflows/a7-admission-readonly.yml
100644 blob 8fe814e48a7cc5f88561c64da8eb9c0b19877d33	.github/workflows/ci.yml
100644 blob 30167418c0993d83d8d447f5f21ab3f57dd94835	.github/workflows/e3-native-evidence.yml
100644 blob 2be8e3226b0b07ce9791bbf2f63bc51863c52d3a	.github/workflows/e4-native-evidence.yml
100644 blob 1898f78e1f4f529ab575af6d5a1db3961631b6ea	.github/workflows/policy-signal.yml
100644 blob 4925b7f12bdebcd624024e64af5ce9e000743b67	.github/workflows/policy.yml
100644 blob 8b01486932f9eb7346a21f3bbf1c5aa2844c4daf	.github/workflows/s2b-native-evidence.yml
100644 blob fb4f0a5e28304e8dfc61cd3770c2ddff4f462ea3	.github/workflows/sudo-package-readonly.yml
100644 blob dbcca2276307357301719ed68a09631b8ab7a10a	.github/workflows/unix-execution-experiment.yml
100644 blob 30027d353dc7e7dd99038b7dc88ed3786b2d2aa2	.github/workflows/windows-identity-probe.yml
100644 blob 3c554ae03483f7a5d0e772215db0c29dc38d671e	.gitignore
100644 blob ca5c350055ccb6dacb495a786f60c8232abed8f2	.node-version
100644 blob 145d3fa25b8c8cdd495ab40849109408792fee0d	.npmrc
100644 blob c2ae25947916672b13180778265354907b75e295	AGENTS.md
100644 blob 483614be7811d25ae6cac786711a3f40f669f605	CONTRIBUTING.md
100644 blob 1c5c0fcf2b577f89ed998826ddf6608338b4b523	Cargo.lock
100644 blob 343f7301c8744ba3ee1b8ebfb5422c70a04479f9	Cargo.toml
100644 blob a028880c724b3ad2c8ec7a867dc9ce3bb2539c2b	LICENSE
100644 blob c338651dd7ba58c4344677f866fbe22039306286	README.md
100644 blob c100d23cd621428fa205584da64974c642c281d7	SECURITY.md
100644 blob 3ad7c587de0344adc7b401f2f1d36cca022d9309	crates/arx-guest/Cargo.toml
100644 blob fc1bbf91cb5052277a0f3752db4b89b328008e00	crates/arx-guest/src/main.rs
100644 blob 7d5212c545400f926a8fd07d49786deaaf063f04	crates/arx-guest/tests/cli.rs
100644 blob 055372c98d6e3f42a87a30725145cded1d7a3531	crates/arxd/Cargo.toml
100644 blob 0d3859e6a5e9011217819c6a8efd7d00a512c940	crates/arxd/README.md
100644 blob 87e3d6a50fc68cbecc6428cfc075bb4f708f0842	crates/arxd/src/client_config.rs
100644 blob dc83fd3fde74fe355d0709e44d4dd901914bdd52	crates/arxd/src/client_config/tests.rs
100644 blob fdc351273e641cf416591895bd50e3155581ddb7	crates/arxd/src/client_config/unix.rs
100644 blob 4e53527cf95e20c4602ae88c5dc66fd38c30abd2	crates/arxd/src/client_config/windows.rs
100644 blob 3ed5899d35cd6030e80a0be9274197834a0dee5c	crates/arxd/src/config.rs
100644 blob 03be8eea75463f52e4cd4f3855921a69ed7a68c6	crates/arxd/src/development/mod.rs
100644 blob f6962c26355fb817dc24bfd9e684f3b030f1ce92	crates/arxd/src/development/tests.rs
100644 blob 754070e807a69f7e0f20de0a8813f2aa55e469e6	crates/arxd/src/development/types.rs
100644 blob 491b1cfe9b2b23a6d1b814b51e8a6853f011b7f1	crates/arxd/src/development/windows.rs
100644 blob b01bd021d3bb1529cd50a93049cb4e9d17e73453	crates/arxd/src/exec/cgroup.rs
100644 blob 252a60449e1217a055129c4aed9d1ed7050912b5	crates/arxd/src/exec/disposable.rs
100644 blob b17a961996d0e8184da0262ea21e20d5773d484d	crates/arxd/src/exec/disposable/a4.rs
100644 blob 6a3031a579d9fb1d1f499a3b4639c216e496d14f	crates/arxd/src/exec/disposable/sudo_nnp.rs
100644 blob e47ced34b2f516a59bfc662949040d5d65e6d2a6	crates/arxd/src/exec/hierarchy.rs
100644 blob 1c7192626056ea4aac5f63f6fea3c71ee9d408a3	crates/arxd/src/exec/hierarchy/a4.rs
100644 blob 71f064ccfb78ff06c75a180a61974d57552ea102	crates/arxd/src/exec/hierarchy/a5.rs
100644 blob 86f4e509814cae28618c84dfc65ec8e3a97b3b6a	crates/arxd/src/exec/hierarchy/fake.rs
100644 blob 7c4196333f4938499188c7f99e5476c955b8e9d0	crates/arxd/src/exec/hierarchy/linux.rs
100644 blob 145a44d4912fc28c39b0832d7decb331b64b465e	crates/arxd/src/exec/hierarchy/parse.rs
100644 blob b33bed7cd5d7022f30c039e761c544567304d28c	crates/arxd/src/exec/hierarchy/setup.rs
100644 blob 2d04f70c3310bd1a5b55d246a8123963ff02a081	crates/arxd/src/exec/mod.rs
100644 blob 1c383aafb66da08693850fe07878737eadea3bf0	crates/arxd/src/exec/output.rs
100644 blob 91c52516fba2b4b1740309c7b986759e6aa60fc4	crates/arxd/src/exec/residue_tests.rs
100644 blob 966bdaa194f5b6c6ad31cf1fec0c50f9072b95d3	crates/arxd/src/exec/stage_b_tests.rs
100644 blob c2b413036247da906d61d5691249a2a2173a2647	crates/arxd/src/exec/tests.rs
100644 blob 0d087e37f31f3e17db652a87f2a0364eb398479e	crates/arxd/src/exec/unix.rs
100644 blob 09ffc537c198eaa47063eaa41b1d8d1d0b039be1	crates/arxd/src/http.rs
100644 blob d7e1917e8f331ae3d2277b10d617d492e72fbeac	crates/arxd/src/lib.rs
100644 blob 72feb0b22335c674848c6fc9742ebf776fe4140b	crates/arxd/src/local.rs
100644 blob 0d23f272d20f516a5269e5b08b09e4f0bb229f12	crates/arxd/src/local_execution_tests.rs
100644 blob 44bdb0be221e5e3220837e65a5e0fc89df529b7b	crates/arxd/src/local_unix.rs
100644 blob b685e90a502896f87121d741622ab563a3014ef1	crates/arxd/src/local_windows.rs
100644 blob cae709bba9d106f0494fec656b356edfe7602634	crates/arxd/src/main.rs
100644 blob f83acbc8b5da33577296a797c2dbfcefd18f396f	crates/arxd/src/mcp.rs
100644 blob f92b7a2f37c59627faee2704a99004cc5fcc1e48	crates/arxd/src/mcp/activity_tests.rs
100644 blob ccf3e75733aba9ab07e25ac8032f152693be8f21	crates/arxd/src/mcp/discovery_tests.rs
100644 blob 4c911a1ad59deff98f029dccd710deb31b0a6523	crates/arxd/src/security.rs
100644 blob 7976171bd2a67d194770e71f33b3f0a347e94a18	crates/arxd/src/service/application.rs
100644 blob 541a8230b1b590a20bb338fa3ff37e8380ed0401	crates/arxd/src/service/application/tests.rs
100644 blob d396250f3cf78ffda14929e2a39102a769b504a0	crates/arxd/src/service/config.rs
100644 blob 648302034a22f5a49e76c9b198e30617f306f20b	crates/arxd/src/service/executor.rs
100644 blob 168d86fe25ec0a729d0c40a8523331e7b69c782b	crates/arxd/src/service/executor/tests.rs
100644 blob b636acea3df84e2c717ae7df3586486ffc1dfe91	crates/arxd/src/service/layout.rs
100644 blob c0488d356ca7b133d7c056c2273da2af2517a824	crates/arxd/src/service/mod.rs
100644 blob 3f9264f670cbf9b53c1e7212ae3d181a4fca4708	crates/arxd/src/service/native.rs
100644 blob acfd3c11e285440604bd6fdcbb3f8278b13dc7b0	crates/arxd/src/service/native_macos.rs
100644 blob 372a62949fe618138b599152ce300939a0f6c346	crates/arxd/src/service/native_macos_policy.rs
100644 blob fb1bb275b4e9f28e2f49648a10a43725347a9a51	crates/arxd/src/service/native_unix.rs
100644 blob 2f4b6fe21a8d0dc5676427b263c8614f31cbdb40	crates/arxd/src/service/native_windows.rs
100644 blob 933330993d4176d123fd5d87d6219e24cb342a27	crates/arxd/src/service/owner_cli.rs
100644 blob 6627ffdc89693ca467d6e2ec04dc4c432d701585	crates/arxd/src/service/owner_cli/tests.rs
100644 blob d1956de63f80a9a2138128e2d9de19f6d4c33fe1	crates/arxd/src/service/owner_install_windows.rs
100644 blob 6334aac5b9d86be697ca32d2392b08820809ebb3	crates/arxd/src/service/owner_install_windows_tests.rs
100644 blob af209f870bd9c63110a74ef8b677342aee3c5de9	crates/arxd/src/service/owner_launchd_capture.rs
100644 blob 32a36454578553d7f8256be98e837bdb3f8885a1	crates/arxd/src/service/owner_plan.rs
100644 blob 20c51c17ba24e5d3b5cecfad2af5d2b662445888	crates/arxd/src/service/owner_query.rs
100644 blob 6254f1dd7fb03682b904315a76c064624458067f	crates/arxd/src/service/owner_scm.rs
100644 blob f9a3b747bce4bf17505d2c34c61e9aea8dd4ae9c	crates/arxd/src/service/owner_scm_tests.rs
100644 blob f006648ade2270b95dd0e5d0983c8e446bc7b431	crates/arxd/src/service/owner_scm_windows.rs
100644 blob cb7e5de582436d44e8015b2b3ea92732b1cd212f	crates/arxd/src/service/owner_scm_windows_tests.rs
100644 blob 433b70381753569661aadc9c06ae1c653415e9df	crates/arxd/src/service/owner_transactions.rs
100644 blob 0f3fd140bcdda0891f0ea2e331def17609cb81c7	crates/arxd/src/service/owner_transactions_tests.rs
100644 blob bd991dd11c75f079bd3f689ec936baf8f3371084	crates/arxd/src/service/owner_unix.rs
100644 blob 4c89e9af853190f92c1f9a18bcbf80f8237fa25b	crates/arxd/src/service/owner_windows.rs
100644 blob 5fd38201a80a766c4779391f7aec70ce4bc190f7	crates/arxd/src/service/owner_windows_tests.rs
100644 blob 534625eeba3569119f1c4e49fc1ef35f1ca3f072	crates/arxd/src/service/preflight.rs
100644 blob 0b9022e8f809fa6b298564b6bd713b97394e556e	crates/arxd/src/service/tests.rs
100644 blob 89f2a922d2f5c5c630f07fb77c34bae062617578	crates/arxd/src/service/unix.rs
100644 blob 01c626dddbbb43ac43f4d332a79c15dcc1bc02d6	crates/arxd/src/service/unix_bridge.rs
100644 blob 1ff55b22f810e1907e21a2edc9c9df07be868f92	crates/arxd/src/service/unix_bridge/readiness.rs
100644 blob f5136186085e03c4225249b569e99debdad6aaa9	crates/arxd/src/service/unix_bridge/tests.rs
100644 blob 781b3b36577794c60a12175e2e843296d9e3ece8	crates/arxd/src/service/unix_bridge/tests/a6.rs
100644 blob 7514f353217fe015974228af9d92fa07cb249095	crates/arxd/src/service/unix_bridge/tests/e6.rs
100644 blob 1263f1c1bd5752c6ba981d928e4bfd36eb14919c	crates/arxd/src/service/unix_bridge/tests/e7-no-cache-golden.txt
100644 blob 0f3df557475c2a496b47de3dffe8527303cc45bf	crates/arxd/src/service/unix_bridge/tests/e7.rs
100644 blob 7e098e3dc8f9f8141c8e7f27b489eeb9731d2e8b	crates/arxd/src/service/unix_bridge/tests/s2b.rs
100644 blob 8059646740cb4b3479c22420e750a5daaa98e355	crates/arxd/src/service/windows.rs
100644 blob f223e99c6bdb57569557e384bec828cf5c40a3ab	crates/arxd/src/store.rs
100644 blob 84531963eeae9e8b31dc8f0c83220f852e640783	crates/arxd/src/store/crash_tests.rs
100644 blob e70d0881964fdfc18f8a51b4b51875e569b374b1	crates/arxd/src/unix_shim.rs
100644 blob b269e00622d48a2bb7dfd8143e1cdec7f25b5177	crates/arxd/src/unix_shim_rights.rs
100644 blob daa2144283fea0d2b4bd859c796fb86a827b631d	crates/arxd/src/unix_shim_rights_linux.rs
100644 blob d02fd13090d0e778100a6fd9a8a8da28b5b60e92	crates/arxd/src/unix_shim_rights_linux_tests.rs
100644 blob d8d879e795965650ec7a38fee896d0b45c2b8dfb	crates/arxd/src/unix_shim_tests.rs
100644 blob f65a1b502295d7d97adb9d2714d7fc1f2ababf95	crates/arxd/tests/cli.rs
100644 blob e92ef2669e6777cede83859d383190c59aef421d	crates/arxd/tests/inventory.rs
100644 blob 40a3c40fd8879a415b0aade734cb7f2ca8312bca	crates/arxd/tests/inventory/activity_v2.rs
100644 blob 4471b3a63696fa16b0957b545bc7d2b5fcbf2063	crates/arxd/tests/service.rs
100644 blob 4108b5c47238e5ef9be9e6c23dad1351cda90c9f	crates/arxd/tests/unix_shim.rs
100644 blob d9c9c78153f07acdb18cdca3b74e62fa2f99c5c5	desktop/main.cjs
100644 blob 2974885dd37ed0e2cfe328fac2e57c9a293dcf06	desktop/package.json
100644 blob 5b1dff857e4931d1d1ffb0bea6ed5b2bbcaa97ab	desktop/playwright.config.ts
100644 blob c612e12d02eeb4ff8b1c9c9c7a7037080b2f72f2	desktop/tests/sandbox.spec.ts
100644 blob 5e17b60985b1efc8ad6598722e040b390d7b105c	eslint.config.mjs
100644 blob 8e9cff2aa19a35fe08264ad6d0c0e48f32b88c99	package-lock.json
100644 blob ed4f1214dcfced8d7814028350de959a7f4cdbe8	package.json
100644 blob fc4812328f8d0e6ebc49d50162cfa873fc30e297	packaging/README.md
100644 blob 93a3abcce9782cc7fddf4c2c1dcc5fc74ec18a74	packaging/linux/50-arxcovo-standard-rights.conf
100644 blob 0b337773c60e49fe84de76db018d2235ae076254	packaging/linux/arxd.service
100644 blob cbb41a5785ca2a0261539a2cf8d48584310af84f	packaging/macos/com.arxcovo.arxd.plist
100644 blob 68eb6494a2142383009bcb8e994e95e09dcd424d	playwright.config.ts
100644 blob 3417ee590341edc47a046d94e77ee9bee2b33bf4	rust-toolchain.toml
100644 blob 61ac6e9c78821011afc30ec119d046c7c5c10a4b	scripts/ci-budget.mjs
100644 blob 34e1dba90848c91d3318f738955b1681b1217f9a	scripts/ci-budget.test.mjs
100644 blob 2b8154b5097c331b4df55c134560857c13d5fb00	scripts/ci-workflow.test.mjs
100644 blob 7683fca9dfb722a6565253bceece8189f8db4159	scripts/enable-auto-merge.mjs
100644 blob 2ddb2aa529dceb2d8d73a7d19a8546ca5b17983e	scripts/enable-auto-merge.test.mjs
100644 blob 75c21e6d3342f73ed22c34b40a7262d7a10ae385	scripts/evaluate-budget.mjs
100644 blob df92db35466aecf9938c4f75e34b605194aaf03a	scripts/evaluate-budget.test.mjs
100644 blob 4fb2e5c7a8458a220238d8c5175d242a29c9c6e0	scripts/fixtures/ci-budget-buffer.mjs
100644 blob 3586a49bd349f882ca535967ea988a326636476f	scripts/policy.mjs
100644 blob 5720ce0b745e8e811eede997ca9a0ddeb85cabed	scripts/policy.test.mjs
100644 blob 54b0baf3bfc9a9262a1d9deb7a0b46a2bfcd5437	scripts/test-service-artifacts.py
100644 blob c88069c63f879fa96fb1191aa52f54e7b450d503	scripts/test_discovery_drain_mutations.py
100644 blob 1ac80cdf093fda548fe641ba6933824fcfb0c233	scripts/test_discovery_e7_mutations.py
100644 blob 962e9e4f0d06e84755b5a87ef3332c1e265c30a9	scripts/test_discovery_followup_mutations.py
100644 blob 4a6ecf102b68e22752b099cc4f8638e87cc1c21d	scripts/test_readiness_e6_mutations.py
100644 blob c18dd8d83ceed1806b50b0aaa46beb7e335fff13	tools/ci/.gitignore
100644 blob 2acc3dcf6aa55653378efcbd410cc9a65a1c1be0	tools/ci/README.md
100644 blob 856c8b9b5dc9beaad2689f66ffb414eb4a042cf7	tools/ci/a7-admission-readonly.py
100644 blob fbbb32d8c70a4b58ab705582885b28f210a04233	tools/ci/e3-native/driver.py
100644 blob 6cff8e824c687d5e4bd3cb9e07a284f40b4369af	tools/ci/e3-native/mutations.jsonl
100644 blob ead690c51d712d032c28ef5c0ca4af9467e2d482	tools/ci/e3-native/test_driver.py
100644 blob 1551ab3f6106883c3234198c766f5642fc59ce99	tools/ci/e4-native/driver.py
100644 blob 6891576ab16ee8561f984949cbd727e5083dd320	tools/ci/e4-native/mutations.jsonl
100644 blob 971a21d6477002e20330ab59a482be5ddc265ff3	tools/ci/e4-native/test_driver.py
100644 blob e0120128e2e6da7339e6f214597f9b1ad5e9a810	tools/ci/observe_dns.py
100644 blob 923b0374d28d3623c4cbb3a4d12c645721b8fe7d	tools/ci/observe_launchd_absence.py
100644 blob c7104fe1c827f992a90fa3d40668ada3455a0efa	tools/ci/observe_manager.py
100644 blob 9c86162e26b49e6341e8a5f80011f833f0ab57a9	tools/ci/s2b-native/driver.py
100644 blob 1c04b068c8a46e037580f2c25945f42d283c4696	tools/ci/s2b-native/mutations.jsonl
100644 blob 93f3bfcad6b89bcc84b31d2b1abc5f94edf35f5a	tools/ci/s2b-native/test_driver.py
100644 blob 40f9ea1ffd60ec9c31cf8507ade3000a2dca0eb4	tools/ci/sudo-package-readonly.py
100644 blob e70649566ce39d0840df85696c6479d3ff1586ff	tools/ci/test-a7-admission-readonly.py
100644 blob 61a204201ab7af88e621b543487049b3500478f9	tools/ci/test-sudo-package-readonly.py
100644 blob f73515ddcc550f2959ae1d8ee10abda3b916a3b4	tools/ci/test_observe_dns.py
100644 blob d78684c1e6f4be95b9787fe12610cd2372e2c2c3	tools/ci/test_observe_launchd_absence.py
100644 blob 2f76bab053e72481895ddcc55d9b9c9bf68939ff	tools/ci/test_observe_manager.py
100644 blob 1521cb29637b5afb62b779af8bac725ad4b7b42b	tools/ci/test_ubuntu_mirror.py
100644 blob e800270f2f0d714e884b9460ecc174ec25e6db46	tools/ci/ubuntu-mirror.md
100644 blob 79b583e57b313eb5c8f968b1f2d441cf548720dc	tools/ci/ubuntu_mirror.py
100644 blob c18dd8d83ceed1806b50b0aaa46beb7e335fff13	tools/unix-exec-validation/.gitignore
100644 blob 6e4b36c937c16aca2681b1ab1bc00acd8b3cda71	tools/unix-exec-validation/README.md
100644 blob 71e92a1fd2ea41385c1b23960a377bd1f547977e	tools/unix-exec-validation/a4-helper/.gitignore
100644 blob 10ab0391abde27e45558c2ceda61bf5e0ed830d0	tools/unix-exec-validation/a4-helper/README.md
100644 blob 5412b0cf345d3bd5dd0106e972a6b10e12827b7b	tools/unix-exec-validation/a4-helper/build.py
100644 blob 5a02cf43d8abaa29e221be4ac54221e2ae05354f	tools/unix-exec-validation/a4-helper/helper.rs
100644 blob 0554592aff7cc97e869f7f8a4dbf25b254aac640	tools/unix-exec-validation/a4-helper/image-pin.json
100644 blob 43e23c1b8597eead804e78cdf69c58139a788bc0	tools/unix-exec-validation/a4-helper/linux-image-pin.json
100644 blob 91ee0a9075a87447bf531d6089c08547e2767fac	tools/unix-exec-validation/a4-helper/recipe.json
100644 blob 1a9d6829e6bb2e40c379b8b3fc208b8440b4ea0c	tools/unix-exec-validation/a4-helper/test_build.py
100644 blob 27f880a5fd4881a08ab777f0e28de66fbc5cfeee	tools/unix-exec-validation/a4-helper/tests.rs
100644 blob acfd9c61aaa10bc065f43d80a15be0cc9e733291	tools/unix-exec-validation/driver.py
100644 blob 7bf0b5e61305543caeabf9a4ae2ee8002657402e	tools/unix-exec-validation/mutation_check.py
100644 blob 41ec1325a77669d34c20f00c4b04d40c9d8086a4	tools/unix-exec-validation/sudo_host_qualification.py
100644 blob 520d341633c67dc52ec483e9b3fcc92131a3da1f	tools/unix-exec-validation/sudo_package_profile.py
100644 blob 6504c3135b4b2f6df1fd55e196100ccccbf6efdb	tools/unix-exec-validation/test_driver.py
100644 blob ce2357d1e3c14013f13eac52d72c8e5983814b39	tools/unix-exec-validation/test_f0_blockers.py
100644 blob 294568ab0cadaec82a7497f9a29858d605ccff04	tools/unix-exec-validation/test_f0b_transport.py
100644 blob ee0967be3ef09a256fc4867f81bcab0b9af2c7a4	tools/unix-exec-validation/test_sudo_host_qualification.py
100644 blob 00dbeb7b637df485492168e9199139f1dca85db1	tools/unix-exec-validation/test_sudo_package_profile.py
100644 blob cd42ee34e873b4ddf17cdeeed2e6c0d4e094c62b	tools/windows-identity-probe/.gitignore
100644 blob c62acaeac5f111acaaa1c4de17745e9a214b242d	tools/windows-identity-probe/CiValidation.psm1
100644 blob 4014b7d74ab0f7345f0264397aeba17ac6d15241	tools/windows-identity-probe/DEBUGGING.md
100644 blob 87dbc89ac7d56e08fd998d65df7eca3056f9eac5	tools/windows-identity-probe/IdentityProbe.csproj
100644 blob f1d923809bf9e6ab80ce6f34c6d2472f18385cb7	tools/windows-identity-probe/Invoke-DisposableCi.ps1
100644 blob 612d007b4a704c19b0abe2c0e173e8a06f9e92ce	tools/windows-identity-probe/Native.cs
100644 blob 50cbdaef70ede61f22ec442fbb0c4ba8d64ee3fb	tools/windows-identity-probe/ProbeBatchRight.cs
100644 blob e75f03dae5bb5c8087c981da378ac9b165040393	tools/windows-identity-probe/Program.cs
100644 blob 797ef395ceca74ef56cff8e6b8b8aa7430f588b0	tools/windows-identity-probe/Test-CiValidation.ps1
100644 blob d4d3537d21e70141d78c7d587b7bb30d3deee60d	tools/windows-identity-probe/Test-GuardMutations.ps1
100644 blob df191a56506c00ba9183762efad38283b75c7b30	tools/windows-identity-probe/Test-SetupDiagnostics.ps1
100644 blob f26127281e627d3cca76312846a18243206cd063	tools/windows-native-replay/Admission.cs
100644 blob 6c1185276c27dc71453d40a1ea2bd10bf3f52170	tools/windows-native-replay/Check-Source.py
100644 blob e1c83ed2e3701add6a9feddea9d2e94b13cbfe9a	tools/windows-native-replay/Controller.cs
100644 blob 9526c04b368664ee23e6a6efbb27e554cf5b10b9	tools/windows-native-replay/HeldFiles.cs
100644 blob a4c88570d6012ae356c9b4f1e71b2f0d0d76acba	tools/windows-native-replay/Invoke-BoundedReplay.ps1
100644 blob d9a0fd2e78586b16c3f625cc73fe0928f932cc80	tools/windows-native-replay/NativeJob.cs
100644 blob 2cd40a0e13bb680a9d59136dd1dff0c0942f4518	tools/windows-native-replay/PINNING.md
100644 blob cbc1ddc275b5543741b7f517699d7e7a8a3120a9	tools/windows-native-replay/Prepare-Provenance.py
100644 blob 1a39056c9e5298c6d520794e843eb031d0350ced	tools/windows-native-replay/Test-PureController.ps1
100644 blob dbf05ddef24026e55c6e87b9a38a7c7899412b9d	tools/windows-native-replay/child/Cargo.lock
100644 blob 6d36690a7b1d4406339de7b63d753de4db44a091	tools/windows-native-replay/child/Cargo.toml
100644 blob cd5b7c59da062c25118665b2f164769265545e7c	tools/windows-native-replay/child/src/main.rs
100644 blob 41a9499660e8b0b40b21bd021f56ba45d760474a	tools/windows-native-replay/selectors.json
100644 blob 20ab11d2a18157132247baa727940661c2b592ca	ui/index.html
100644 blob 1799eb63a4394d847182abeda9fab5875909d41b	ui/package.json
100644 blob 78ddaccc4262c91ae9db53ab7b04d312875252c9	ui/src/App.test.tsx
100644 blob f6dc928d2bb381dd2662dab5084236e0ef5d2bc3	ui/src/App.tsx
100644 blob 1c773831afd7ba399ce6d31fa71fdecedcc2ebfe	ui/src/main.tsx
100644 blob 903b1d97e221e4f42900ac4e02b02851b114a652	ui/tests/smoke.spec.ts
100644 blob b212ae2d17ec1ec67ad3fd41b3514573c27bcd7c	ui/tsconfig.json
100644 blob 041854f74501ecbb719e93f24f73e471596ab572	ui/vite.config.ts
100644 blob b916fd8d80a9ae1434207a621d8cfc15291c271a	ui/vitest.config.ts
```
