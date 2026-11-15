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
Require the fixture budgets amended below, existing 4096-byte, identity, exact
records, true/exit_nonzero, scalar
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
set/absence lifecycle. D7a-h remain consumed; no retries or host reuse. Coordinator
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
   `timeout=55s` for tool-probe / `10s` for E4 per the budget amendment below,
   `output_bytes=4096`; recheck owned base/workspace
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

## Binding budget amendment

Documentation correction only, 2026-11-15; independent amendment review, required
gates and merge pending. This supersedes only the old 20s RunSpec and phase/budget
wording above. Delivery pins 1-4 otherwise stand. The accepted
[Muse review](m2-a7-a11-integration-budget-muse-review.md) remains accepted except
its two blocking corrections, now fixed by the
[source-pinned proposal](m2-a7-a11-integration-budget-proposal.md) (pins 2 and 4).

1. Complete fixed admission and one production setup, send READY within unchanged
   packaged30s startup, then finish identity-ack before ANY Runner child. Driver
   uses `systemctl start --no-block`, then its 120s monitor polls `active`, verifies
   runtime bytes, MainPID/exe and exclusive supervisor cgroup membership, and
   writes `identity-ack`. Only afterward can terminal `inactive`/`failed` with
   `Result=success`, `ExecMainStatus=0`, both complete case oracles, fixed completion
   marker and exact successful libtest summary pass. READY/ACK alone never passes.
   Source: [driver.py:1282-1349](../../tools/unix-exec-validation/driver.py#L1282),
   [1455-1470](../../tools/unix-exec-validation/driver.py#L1455),
   [1571-1572](../../tools/unix-exec-validation/driver.py#L1571).
2. Exactly two scoped workers, one owned account/Runner domain each; production
   tool-probe then fixed E4 reentry per worker. Fixture-only RunSpec55s/10s;
   actual nominal probe stays 9*5s+5s=50s. Keep output4096, memory64MiB/swap0/pids32
   and per-call view/account/artifact checks. No generic framework or probe copy.
3. Allocate exactly **10s cleanup acceptance allowance per case (5s per call)**:
   normal cleanup has a 2s cgroup confirmation loop then a 2s reap loop, leaving
   1s margin per call. Cleanup and guard drops are inside synchronous
   `run_phased` return. Failed removal can retry for another 2s in Group::drop;
   failed/error/unwind paths refuse and retain cleanup, not a 5s completion claim.
   Source: [Runner:200-226](../../crates/arxd/src/exec/mod.rs#L200),
   [cleanup:422-449](../../crates/arxd/src/exec/unix.rs#L422),
   [guards:481-504](../../crates/arxd/src/exec/unix.rs#L481),
   [cgroup:124-163](../../crates/arxd/src/exec/cgroup.rs#L124).
   Enforce monotonic full-call elapsed <=60s/15s on return, cumulative
   admission/check/oracle elapsed <=15s, case elapsed <=90s; excess refuses.
   Thus 15+55+10+10=90, parallel cases do not sum. Full-call checks include
   pre-timer work and cleanup; they do not isolate cleanup time or prove a hard
   5s cleanup bound. Existing native loops bound polling, not syscall duration.
4. Monitor120 includes startup after `--no-block`: reserve15s startup/setup and
   remaining polling overhead +5s READY-to-ACK +90s cases +10s final join/completion;
   require terminal observation before120s. Refuse phase excess. A7/A11 must
   enforce the 5s ack envelope; shared `hold_identity_ack` currently waits120s.
   Three 5s property-query polling deadlines plus local work/overshoot must fit
   measured 15s case admission/checks; final child.wait() is not hard-bounded
   ([query:542-613](../../crates/arxd/src/exec/disposable/sudo_nnp.rs#L542)).
   Package30s, Runtime140s, build360s, workflow10min stay.
5. Parent observes either worker failure/panic/excess, cancels caller/shutdown
   tokens for BOTH and joins BOTH even if either join reports panic. Catch panic
   inside scoped work to cancel before implicit joins. Retain all process-tree,
   cleanup/residue and final driver unit/cgroup/UID checks; uncertainty quarantines.
   Measure/refuse join/completion >10s after joining. Never detach cleanup, abort
   a run thread or abandon a join to meet a clock. Cancellation is cooperative;
   synchronous OS blocking can exceed envelopes, so this guarantees refusal of
   late returned results, not hard native completion. Outer monitor/runtime
   refusal/stop and owned-only driver cleanup remain required.

No implementation before this amendment's independent review/gates/merge; D9b
source/selection gates remain. No execution before a separate native grant for
both cases/source/artifacts/workflow/profile/bounds on a fresh host. D7a-h remain
consumed; the stale A1/A2 owner reply is closed. Source ceiling and pure rejection/
scheduling/cancellation tests are as fixed in the proposal. A7/A11, M2, full F/G
and real-Runner PA6 remain **unverified on real hardware**.

## Binding C2-view amendment (ADR0020 D9c)

After this amendment review/gates/merge, the controlled fixture uses exactly FIVE
read-only mappings: four D9b policy mappings plus `<stage>/fx/c2-empty:/usr/local/bin`.
Exclusive root-owned empty source0755, captured identity, no022, exact empty set;
no adoption, optional source or extra mapping. Earlier 0700 remedy wording is
superseded: the actual low-UID probe opens candidate directories O_RDONLY and
must observe sudo ENOENT, not EACCES/unknown. No product rights logic changes.
Verify effective mapping property, mountinfo/statvfs read-only, source/view dev+ino,
ownership/mode/empty/C2-absence inside unit before Runner and per call. Extend
existing fixed mapping/held-FD/owned-cleanup/quarantine shape only to this source.
Outside-unit admission keeps package/C1/plugin/util, protected /usr/bin/alias and
floor checks; host C2 ancestry is not claimed safe or absent. Only this controlled
fixture proves its closed C2 view in-unit instead. Ordinary strict host checks
stay. PATH=/usr/bin:/bin, artifacts/accounts/schema, NNP and all real oracles,
case budgets and both-worker cancel/join stay. Future source updates canonical
profile facts for this exact view. No implementation before amendment merge,
no native run before separate source review/gates/merge and a new exact grant.
D7a-j consumed; hardware paths remain **unverified on real hardware**.
