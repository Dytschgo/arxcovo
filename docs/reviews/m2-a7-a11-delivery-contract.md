# M2 A7/A11 fixed delivery and host qualification contract
Design sub-agent: OpenAI / gpt-6.1-sol / high; 2026-11-15. Documentation only.
Baseline: PR118 main `17a2cb47306f73730c2ce990f4661094c4d01f3b`;
reviewed `cae125a` tree equals `3f56e0`, five green gates and hosted Linux compilation
are supplied coordinator evidence, not rerun here. No host evidence is available.
Read AGENTS/KICKOFF, accepted sudo-nnp plan and Muse plan/source reviews, brief0020,
ADR0019/0020, controller, driver, Runner/account checks, E4 probe and staging policy.
A7/A11, M2, full F/G and real-Runner PA6 remain **unverified on real hardware**.

## Smallest fixed delivery
Keep `crates/arxd/src/exec/disposable/sudo_nnp.rs::Prepared`'s existing shape:
two owned OwnerConfigs (rule, no-rule), production arxd path, E4 libtest path.
Replace `prepared_delivery()->None` only with a fail-closed test-only fixed reader;
never accept caller paths, environment artifact overrides, CLI selectors or activation.
Derive run/attempt from the validated controller executable's exact existing
`/var/lib/arx-ci-<run>-<attempt>/source/target/debug/deps/arxd-<hex>` layout.
Read only `<stage>/a7-a11-delivery.json`, root-owned 0600, nlink1, <=4096 bytes;
reject duplicate/unknown/missing keys, wrong types, overflow and trailing records.
Version1 schema: run, source_sha, host_profile_sha256, accounts[2], artifacts[2].
Each account contains uid/gid and base/workspace dev+ino; array order fixes policy.
Each artifact contains size, sha256, dev+ino; array order fixes arxd then reentry.
No path/name/command/env/limits fields: derive names with `sudo_nnp_names(run)`;
derive `/var/lib/<name>/workspace`, require captured identities >=1000 and distinct.
Require nologin, account-named sole primary group, no supplementary groups, admin=false.
Set memory=64MiB, pids=32, environment={PATH:/usr/bin:/bin}, private roots=[stage],
cgroup_parent=/sys/fs/cgroup/system.slice/arxd.service/runs; existing code sets swap0.
Recheck owned base/workspace inode, owner and 0700 mode before each Runner call.
Executables live outside the 0700 stage: exclusive root-owned 0755 directory
`/usr/libexec/arxcovo-a7-a11-<run>-<attempt>/`, literal `arxd` and `reentry`, 0755.
No pre-existing directory/file adoption. Root-down descriptor-relative no-follow
opens must validate every ancestor owner0/no group-or-other writes; no canonicalize.
Require final regular ELF, owner0, nlink1, exact size/hash/dev+ino; cap each at 128MiB.
Hold validating descriptors through admission; recheck named identity before use.
Root-only writable ancestry prevents build-account replacement during Runner exec.
Input hashes bind staged bytes to the reviewed build; they cannot self-attest source
or host. Source/workflow admission and independently reviewed host profile supply trust.

## Build, connect and clean
Extend `tools/unix-exec-validation/driver.py`'s existing root-private offline/locked
copy/prebuild before accounts: staged cargo `test -p arxd --locked --lib --no-run
--message-format=json`, then `build -p arxd --locked --bin arxd --message-format=json`.
Use existing staged Rust1.95/cargo_env; require exactly one matching libtest and one
non-test arxd artifact at fixed target/debug locations. Share the 360s build budget,
4MiB private build capture; hash held files, copy exclusively, verify destination bytes.
Libtest copy is both E4 caller and the same build as controller; sudo is never copied.
After qualified host refusal checks, reserve unit, collision-check both accounts,
groups, roots and policy names before first provisioning; prepare outside service.
Call existing `prepare_sudo_nnp`, capture identities/inodes even on partial failure,
write fixed private delivery, install unchanged packaged Standard unit/drop-in plus
existing disposable ExecStart override. Reuse setup once, MainPID/runtime checks,
READY/identity-ack and 120s harness/1MiB private capture; no production bridge activation.
For each config retain two actual Runner calls: arxd `tool-probe`, then literal ignored
`unix_shim::rights::linux::tests::e4_sudo_nnp_reentry` with existing fixed arguments.
Require existing <=20s/4096-byte, identity, exact records, true/exit_nonzero, scalar
C1/alias/C3-unopened/O_PATH/CLOEXEC/inode/exec-EOF/nonzero and cleanup discriminators.
Finally stop/confirm unit+cgroup gone, no live fixture UID, then existing owned-only
policy/account/group/root cleanup plus matching executable staging and private stage.
Partial capture, changed inode or uncertain stop retains reservation/quarantine; fail.

## Qualification decision and exact remaining evidence
Current False host/policy latches plus fixed visudo syntax refusal are insufficient.
A reviewed exact-byte local-policy allowlist is sufficient later; no semantic policy
inference at runtime. It must cover policy plugin/routing, all includes and defaults,
exclude external sources/aliases/netgroups or other applicable grants for generated
sole-group accounts, and include the exclusive generated 0440 NOPASSWD /usr/bin/true
file only for case0. Case1 must have no applicable rule, not merely no owned file.
Because no such host bytes/pins exist, next host action requires a separate reviewed
root-read-only qualification grant, not provisioning or unfenced sudo execution.
Capture privately fixed /etc/sudo.conf, /etc/nsswitch.conf, /etc/sudoers and complete
/etc/sudoers.d entry set (<=32 regular files, <=16KiB each, <=128KiB total); refuse
links, unknown includes/plugins/routing or excess, never follow arbitrary policy paths.
Review exact bytes privately to establish a closed allowlist; release only fixed
results/digests, no raw policy/identities. Pin required local plugin bytes if applicable.
Also establish protected installed ELF /usr/bin/sudo, C2 absent-or-trusted, root-owned
/bin -> exactly usr/bin or /usr/bin; systemd>=254, kernel>=5.14, cgroup2 memory+pids
delegation/user.delegate=1 and existing protected hierarchy checks. Never install/relax.
Unresolved: does that fresh host's exact closed policy qualify? No future hash assumed.
Revalidate profile bytes/entry set on the later fresh execution host; drift refuses.

## Next source slice and gates
Dormant handoff/build/cleanup only: sudo_nnp.rs, driver.py, test_driver.py and
mutation_check.py; retain False qualification and closed TEST mapping until grants.
Add pure rejection/mutation tests for schema, source/hash/inode/path swaps, private
stage traversal, account/config mismatch, policy drift, collisions and partial cleanup;
retain existing S8-S14/inactive inventory, E4 pin checks and driver guard suites.
Then separate bounded qualification source/grant review; after evidence, execution
grant pins both cases, source/artifacts/workflow/profile, one dispatch and variable
set/absence lifecycle. D7a-g remain consumed; no retries or host reuse. Coordinator
automates authorized fresh-host actions through independent review and green required
gates including ci-time-budget; no new human approval. No ADR rewrite absent spec change.

## Binding review corrections (source preparation)

The following four exact pins supersede abbreviated wording above and bind the source slice:

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

## Controlled-policy amendment

ADR0020 D9b supersedes the complete original-host policy-byte qualification/private-capture route for the controlled fixture only, after its review/gates/merge. The four literal read-only mount mappings and in-unit view checks establish the policy cases by construction; installed binary/plugin/default-path/floor checks and all delivery pins remain. No host-original-policy claim or list-mode oracle. Existing false/None selection gates remain until separately reviewed source and native disposition.
