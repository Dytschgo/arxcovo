# M3 custody slice 3b: adopted pre-build pins

2026-11-20. Sol 6.1 / high, read-only architect. Coordinator adopted these pins, the hosted-only capped ext4 fixture and the two-line module declaration exception on 2026-11-20, with the independent review corrections incorporated below. This design preserves ADR0037 and contract section 9; it supplies hosted process evidence, not live daemon/executor integration or crash-durability qualification.

## Provenance and readiness

Repository baseline: 85eccce8a917446efc1ea9df6d9bd4b0c256c128 (main after PR197). Repository content was read only through git show at that hash; metadata through Git object commands. The object was absent locally, so it was fetched by exact SHA into D:/Temp/arx-custody-3b/objects.git; the shared checkout/index/branches were not changed. Read pinned AGENTS, CI administration, ADR0031/0036/0037, custody brief including section 9, workflow, dev-scratch, custody.rs/codec.rs/tests.rs, and both requested PR197 review reports. Some combined outputs truncated; decisive source and contract ranges were reread. No build, tests, container, checkout, commit or push ran.

PR197's exact-head reviewer closed the missing filesystem/fault coverage for 3a. Complete bytes readable after a final sync failure may authenticate on restart: there is no persisted failure latch. Neither that observation nor this proposed fresh-process test proves persistence across power loss. Preserve that limitation explicitly.

Before the single hosted image build, the coordinator must record the actual candidate source HEAD (including the completed probe/launcher/Containerfile/step), hashes of those inputs, and persist this adopted revision and its adoption record in the repository. The baseline above is an immutable design input, not the nonexistent hash of future builder output. Do not build an old binary from 85eccce8 and credit it as containing the new probe. Missing pins fail closed; no owner interview is needed for routine coordinator pins.

## 1. Immutable image and executable

Use the Docker Official Image ubuntu:24.04, explicitly linux/amd64:

    docker.io/library/ubuntu@sha256:9a04f13b3f80ef056361e30868593b1cfd2279fe19e4081cb93a90bf56819d6a

Verified by anonymous read-only Docker registry requests on 2026-11-20. The then-current 24.04 OCI index was sha256:9c7f200c07e3d4a03e7249d2c16bf66a51120fb876aa2242cf31451412543289; its linux/amd64 child is the pinned digest above. A direct GET of that child returned the identical Docker-Content-Digest. Child config sha256:2b147973fd863c93ced60f325461fe0e262b91317b008e6a2d961f13e81af0e4; single compressed layer 29,793,297 bytes, sha256:8e8df94085bf9af8de500d2042195f1819a4c381b7f3be2a6b920cab65039050. No credential/private data was sent; the public registry's anonymous bearer value was never printed or saved. Do not substitute the mutable tag. Re-fetch this digest, not latest, at execution; absence is a block, not permission to replace it.

Use the arxd *library unit-test executable* already produced by the unchanged cargo nextest run --workspace --locked in the existing Ubuntu worker: package arxd, lib target, debug/test profile, x86_64-unknown-linux-gnu. No new cargo invocation, cross-compilation, release build, Rust toolchain image, apt, pip or runtime dependency download. Fail on another runner architecture/ABI. Ubuntu-latest compatibility must be observed, not assumed from its label.

Fixed probe name:

    cluster::pki::custody::hosted_probe::hosted_custody_probe

The probe is #[test], Linux-only and cfg(test), inert in ordinary workspace tests unless its closed hosted-mode input is present. It never starts Docker itself. Launcher discovers at most 16 regular non-symlink executable ELF candidates named arxd-* directly in the effective CARGO_TARGET_DIR/debug/deps, bounded --list --exact invocation for each (2 seconds each within preparation budget); require exactly ONE binary listing precisely this test. Reject zero/duplicates, oversized paths/files, changed inode/hash during staging and build/runtime target ambiguity. No stale wildcard-first selection. Copy it to a fresh bounded context as arxd-tests, hash it, and assert listing again inside the image. Require selected=1, passed=1 for each completed operational invocation, rather than exit zero alone. The inert hold PID1 is a lifecycle keeper, terminated during cleanup and never counted as an operational probe pass.

Exact Containerfile recipe, stage/build target custody-runtime:

```Dockerfile
FROM --platform=linux/amd64 docker.io/library/ubuntu@sha256:9a04f13b3f80ef056361e30868593b1cfd2279fe19e4081cb93a90bf56819d6a AS custody-runtime
COPY --chown=0:0 --chmod=0555 arxd-tests /opt/arx/arxd-tests
USER 21001:21001
WORKDIR /
ENTRYPOINT ["/usr/bin/env","-i","PATH=/usr/bin:/bin","LANG=C","TZ=UTC","ARX_CUSTODY_HOSTED_MODE=hold","/opt/arx/arxd-tests","--exact","cluster::pki::custody::hosted_probe::hosted_custody_probe","--nocapture","--test-threads=1"]
```

Only Containerfile and arxd-tests are build-context inputs; exclude workspace, target tree, credentials, environment files, fixtures and runtime storage by staging this two-file context under fresh RUNNER_TEMP. Containerfile is copied from the pinned source and hash-compared. Build exactly once using argv: docker build --platform linux/amd64 --network none --target custody-runtime --file CONTEXT/Containerfile --iidfile ROOT/image.id CONTEXT. Digest-pinned FROM fetching may need registry access; build network none concerns build execution, and there are no RUN instructions. One pull/build, no retries. Reject binary >128 MiB, context >129 MiB and resulting image >256 MiB. Verify ELF interpreter/DT_NEEDED against this base using readelf and actual exact-probe listing. No ldd on an untrusted executable, host library copies, LD_LIBRARY_PATH or loader overrides. An unmet dynamic dependency/ABI is a blocking finding for coordinator disposition, never an unpinned package install.

After compilation and the single image build, record executable SHA256, built image ID sha256, Containerfile SHA256, source SHA and runtime image inspect identity BEFORE creating keys. Runtime uses the image ID, not a tag. Output hashes cannot be supplied now.

## 2. Identities, fixed launch and technical fences

Daemon UID:GID 21001:21001; arbitrary-build UID:GID 21002:21002. No supplementary groups, no shared private-root group, no root in either runtime identity. Check all real/effective/saved IDs, groups, CapInh/Prm/Eff/Amb=0, NoNewPrivs=1, RLIMIT_CORE=0 and executable root ownership/nonwritability. No group-add. The trusted disposable hosted runner controls Docker; neither container identity receives Docker access.

For each of founder, learner1, learner2 create a distinct container, sequentially, with private PID/mount/IPC namespaces. UID values may repeat; storage never does. Exact create argv template (ROOT/NAME are ownership-checked launcher-generated identifiers, not caller strings):

    docker create --name NAME --label arxcovo.custody.run=RUN_ID --platform linux/amd64 --user 21001:21001 --workdir / --read-only --network none --ipc private --shm-size 1m --cap-drop ALL --security-opt no-new-privileges=true --ulimit core=0:0 --ulimit nofile=128:128 --cpus 1 --memory 256m --memory-swap 256m --pids-limit 64 --log-driver local --log-opt max-size=1m --log-opt max-file=1 --log-opt compress=false --tmpfs /tmp:rw,noexec,nosuid,nodev,size=8m,mode=1777 --tmpfs /workspace:rw,noexec,nosuid,nodev,size=8m,mode=0700,uid=21002,gid=21002 --tmpfs /control:rw,noexec,nosuid,nodev,size=1m,mode=0700,uid=21001,gid=21001 --mount type=bind,src=ROOT/fs/NODE-wrap,dst=/wrap,bind-propagation=rprivate --mount type=bind,src=ROOT/fs/NODE-store,dst=/store,bind-propagation=rprivate IMAGE_ID

Then docker start NAME, no attached interactive stdin/TTY. Default entrypoint is fixed hold mode, an inert bounded PID1 without keys. All actual test invocations use the following argv, with only a closed MODE substitution:

    docker exec --user UID:GID --workdir / NAME /usr/bin/env -i PATH=/usr/bin:/bin LANG=C TZ=UTC ARX_CUSTODY_HOSTED_MODE=MODE /opt/arx/arxd-tests --exact cluster::pki::custody::hosted_probe::hosted_custody_probe --nocapture --test-threads=1

Modes: hold, create-founder, create-learner, deny-build, same-uid-control, release, restart-founder, restart-learner. UID is 21002 only for deny-build; otherwise 21001. No generic command/path/UID/argv input in the Rust probe. NODE and role are fixed launcher choices; learner modes cannot create CA. Mode selects only fixed operations. hold self-terminates within the runtime deadline (130 seconds from its own start at most). release writes a bounded fixed /control release marker; it does not signal arbitrary PIDs. create modes hold real owners/open key-file descriptors until release so live memory/FD probes have actual targets. Expected public contexts and target PID/FD integers use a bounded schema in /control, daemon-only; the trusted runner relays only fixed numeric target values through fixed probe input if needed. Pin the only additional exec environment fields as ARX_CUSTODY_TARGET_PID (decimal 2..4194304), ARX_CUSTODY_TARGET_FDS (exactly three comma-separated decimal FDs 3..127 for seal-key/ca-key/member-key), and ARX_CUSTODY_PUBLIC_CONTEXT (at most1024 ASCII hex characters of fixed-schema public expected installation/cluster/root/SPKI context). Supply them only to modes needing them, after env -i and before the executable. No optional path/command/loader fields; reject unknown ARX_CUSTODY_* fields. The trusted runner relays these public values through bounded capture of a fixed ready record, never Docker inspect environment. Public context is retained by the runner and supplied to learner/restart modes; it is never derived exclusively from the envelope being opened. Control-file writes stay mode0600 and bounded; no key bytes in control files.

Every exec begins env -i; Docker exec otherwise inherits container environment. Only the listed constants and bounded nonsecret probe controls survive. No inherited key FD: new Docker exec process, stdin closed, stdout/stderr only bounded pipes; Rust verifies inherited FDs before opening its own files (allow stdio and a transient inspection directory FD only). No socket activation, shared process handles, exec inheritance from the key-owning process, env-file, host caller environment or workspace loader search. Daemon sets umask 077, disables dumpability before opening secrets, verifies restrictions and opens only fixed absolute roots. Do not clear supplementary groups by giving runtime CAP_SETGID: launch correctly and fail if observation disagrees.

Keep Docker's default seccomp/masked proc policy; no seccomp unconfined, privileged mode, capability additions, host PID/IPC/network, published ports, Docker socket, owner mounts or host paths beyond the synthetic fixture. No network interfaces except loopback under network none; bounded AF_INET/AF_INET6 connection probes show external/metadata/gateway failure without transmitting data. No bridge/full-cluster network qualification is claimed. Ptrace/process_vm_readv/proc denial may be enforced by several kernel/runtime layers: record the actual denial class, never infer DAC-only protection from EPERM.

## 3. Bounded storage and lifecycle

Use ONE disposable 64 MiB ext4 filesystem image shared only by the trusted host preparer, with six separate subdirectories mounted pairwise as above. This makes a real block/filesystem capacity cap without relying on unsupported overlay2 --storage-opt size, and avoids calling tmpfs crash-persistent storage. Create ROOT with mode0700 and an unguessable generated basename under RUNNER_TEMP; create-new disk.img, ftruncate to 67,108,864 bytes; hosted-only sudo -n mkfs.ext4 -q -F -m 0 -E lazy_itable_init=0,lazy_journal_init=0 ROOT/disk.img; sudo -n losetup --find --show --nooverlap ROOT/disk.img; sudo -n mount -t ext4 -o nosuid,nodev,noexec LOOP_DEVICE ROOT/fs. Validate canonical tool paths on the hosted Ubuntu runner before use; never auto-install missing tools. Verify loop association and mount major/minor match the newly created file. Make NODE-wrap/store daemon-owned 0700, containing no precreated custody files; no shared ancestor is exposed in containers. Trusted hosted-only chown prepares synthetic roots; no account creation/owner-PC provisioning.

This bounded host fixture is explicitly adopted by the coordinator as hosted-only technical preparation; no fixture work was performed here. No block devices/loop mounts enter containers; no owner/static VM/backups are involved. If hosted sudo/loop/ext4 is unavailable, stop qualification. Do not switch silently to tmpfs or uncapped Docker volumes. Runtime private roots are actual ext4 directories. Explicit application tmpfs mounts are disposable /workspace, /control and /tmp; Docker also supplies /dev/shm and runtime-generated /dev and other virtual/device mounts. Inventory them as specified below. Disk/fsync and process restart observations still do not qualify power-loss/host-crash durability or a native filesystem.

Limits: at most three containers total and one active node at a time; per-node 1 CPU/256 MiB/64 PIDs/128 FDs, swap disabled via equal memory/memory-swap. Custody data is hard-capped by the 64 MiB filesystem. Explicit application tmpfs capacity is 17 MiB (/tmp8 + /workspace8 + /control1); private /dev/shm is explicitly capped at 1 MiB (1,048,576 bytes) by --shm-size 1m, making that subtotal 18 MiB per node. This is not a whole-container tmpfs total. Inventory runtime-generated /dev tmpfs and require its capacity <=64 MiB: the resulting nominal capacity ceiling for these writable tmpfs superblocks is 82 MiB (18+64), with actual usage charged within the same 256 MiB cgroup, not added to it. At that full-capacity ceiling at most174 MiB remains for other charged memory; allocation can fail sooner because process/kernel charges also consume the cgroup. Record actual capacities and usage, never infer free RAM from capacities. Image <=256 MiB, staged binary/context <=129 MiB, captured evidence <=1 MiB, Docker log <=1 MiB per node. Launcher streams and caps pipes, never buffers unlimited output or dumps inspect/env/secret files. Runtime image root is read-only; inputs and ext4 file give task scratch an attributable bound below256 MiB excluding Docker's pinned immutable image cache. Check available host disk >=1 GiB before pull/build. Reject size overflow; don't prune shared Docker caches/images. Record actual image/scratch high-water bytes.

Writable-mount acceptance: inspect actual mountinfo/statvfs/ownership/modes and permitted operations under BOTH 21001 and21002, plus filtered trusted Docker inspection. Require HostConfig.ShmSize=1,048,576, private IPC, and /dev/shm capacity=1 MiB with nosuid/nodev/noexec; verify memory.max=268,435,456, memory.swap.max=0 and the adopted CPU/PID controls (or equivalent observed cgroup limits). Inventory /dev tmpfs separately (<=64 MiB); distinguish its device nodes, devpts, mqueue and proc/sys pseudo-filesystems and Docker-generated /etc/hosts,/etc/hostname,/etc/resolv.conf mounts from application tmpfs. Do not double-count masked read-only tmpfs submounts or alias mounts of one superblock. Permit only the fixed Docker runtime device/virtual mounts and generated resolver/hostname/hosts sources, plus the two attributed synthetic ext4 sources; reject unexpected writable host sources or unrecognized writable tmpfs. Root-owned /dev cannot be used by either UID to create arbitrary files; only required standard device-node operations and the observed runtime virtual interfaces are allowed. Record writable runtime virtual interfaces separately rather than inventing a byte capacity for them. /workspace is build-only, /control and private roots daemon-only, /tmp is shared sticky scratch and carries no secrets. Verify those permissions as each identity. Default masked/read-only proc/sys fences remain intact; no mount-policy widening is authorized to make inspection pass. If capacities, limits, source attribution or writable-mount inventory cannot be verified, fail closed and clean up. Pure argv/accounting tests must reject an omitted/wrong --shm-size and incorrect17/18/82 MiB arithmetic or unexpected writable sources; no real Docker execution in those tests.



One monotonic 150-second step deadline starts at launcher entry: total_deadline=t0+150s; work_deadline=t0+130s, reserving the final20 seconds for cleanup. Phase ceilings remain pure launcher tests/preflight/discovery20s; base pull/image build40s; ext4/container preparation15s; aggregate create-readiness waits15s; aggregate deny/same-UID probes15s; aggregate release/holder-terminal-wait/fresh-process restarts15s; final evidence10s; cleanup20s. These sum150s and are ceilings, not retry allowances. Per-node create-ready -> live deny/control -> release -> holder terminal/exit/reap -> restart -> remove-node sequences are INTERLEAVED; phase ceilings are cumulative counters across all three nodes, not instructions to keep three holders alive concurrently. At most one active node/holder; remove the prior node before activating the next. Holder waiting overlaps the live probe work and is not a second sequential phase allowance.

Asynchronous create-exec ownership: run.py starts each create-founder/create-learner Docker exec with Popen and retains its process-group/pipe ownership until reaped. Let ready_remaining=15s minus prior cumulative readiness waits. Readiness deadline=min(exec_spawn+min(5s, ready_remaining), work_deadline). A nonpositive remaining budget fails before spawn. A valid bounded ready record proves only readiness, never selected=1/passed=1 or terminal success. Missing/malformed readiness, pipe overflow or early exit before readiness latches the first failure (ReadyTimeout, Protocol or HolderExit as appropriate), cancels further work and enters cleanup. The holder must remain alive after readiness while denial/control probes execute, including beyond five seconds; there is NO five-second lifetime timeout on this asynchronous exec.

Holder lifetime deadline=min(exec_spawn+30s, work_deadline), regardless of whether readiness has arrived. Rust create mode also self-expires after30s without release, returning failure rather than a successful test result. The launcher monitors holder liveness, both capped output pipes, holder deadline and work deadline throughout every probe/control wait (polling/event-loop interval <=20ms). The asynchronous holder exception does not widen any other command bound. Probe failure, unexpected holder exit before deliberate release, holder expiry or work expiry latches the first failure BEFORE teardown (HolderExit, HolderTimeout or WorkTimeout if no earlier failure), stops issuing probes/restarts, and enters cleanup. Killing the local Docker CLI alone is insufficient: remove the positively attributed node container to terminate its daemon-side holder, then kill/reap the local exec process group and drain capped pipes. Container/mount/loop cleanup still follows the ownership checks below; inability to attribute a partial acquisition is UnknownCleanup, never permission for a broad delete.

Normal release is a short fixed release-mode exec (<=3s), followed by a separate holder-terminal wait/reap deadline=min(release_completion+min(5s, release_restart_remaining), holder_deadline, work_deadline), where release_restart_remaining is15s minus all prior cumulative release/terminal/restart time including this release. A nonpositive remaining budget fails immediately. Require a terminal selected=1/passed=1 record, successful Docker exec exit, closed/drained pipes and actual reap AFTER deliberate release, all before launching restart. Readiness alone cannot supply terminal credit. Missing terminal evidence, nonzero exit, release failure or reap timeout latches the first failure, forbids restart and enters forced cleanup; cleanup errors remain secondary. Completed restart exec is <=5s and must supply its own selected=1/passed=1 and exit/reap. All short probes are <=3s and ordinary Docker control operations <=5s; pull/build has its40s phase ceiling. Every bound is additionally clipped by its remaining aggregate phase budget and work_deadline. Cleanup starts immediately on failure, or by work_deadline on success, and uses at most20s and never exceeds total_deadline. Cleanup cannot extend holder/work deadlines, grant a retry, erase a first failure or turn a timed-out holder into a pass. Missing records, zero-selected tests, truncation, OOM/137 or leftovers fail the step. The outer150s deadline and unchanged12-minute worker limit remain binding.

try/finally owns cleanup, also on SIGTERM/SIGINT; reserve cleanup time proactively. Latch the first failure before effects/cleanup; later failures are secondary records and cannot replace its class/code. If work succeeded but cleanup failed, step fails as Cleanup. Kill/reap launcher process groups; forcibly remove only recorded container IDs after verifying run label and image ID; do not remove by broad name prefix. Verify no container holds the mounts, unmount the exact recorded filesystem, detach only the verified loop-device/file pair, then remove only attributable ordinary fixture files under checked ROOT. No recursive traversal through links/reparse points; verify ownership, device/inode, containment, mount identity, empty mountpoints and single-link files. If verification fails, retain artifacts and report unknown cleanup without traversing/deleting. Do not delete any static VM, backup, old manager state or unrelated cache. SIGKILL/runner loss cannot guarantee cleanup: report this limit, rely only on disposable runner lifetime, never claim verified cleanup after loss. Preserve sanitized summary/hashes outside the removable fs; don't upload key artifacts or filesystem image.

## 4. Required behavioral oracles

Each listed case is proposed, NOT RUN. Every operation invokes the production Custody/codec/key implementation. Synthetic test_ticket is permitted for sign verification, never as Found evidence.

1. Founder create: actual create_fresh_installation/create_founder_ca/create_member, durable return, valid CSR, actual sign and check_leaf. Open the three explicit test-only key/envelope FDs with O_CLOEXEC and retain them solely as targets of FD denial; never hand them to another process. Hold owners and explicit CLOEXEC seal/envelope FDs while reporting only ready/PID/FD integers. Save only public expected contexts/original cert identity to control. No ephemeral CA constructor on this path.
2. Build deny, on live healthy founder: workspace create/write/read succeeds; UID/groups/caps facts match. open/read/write/traverse attempts fail for /wrap, /store, seal-key, install-context, custody-lock, ca-key, member-key. Root-owned executable/configuration cannot be altered. Try /proc/PID/mem, /proc/PID/fd listing/readlink/open of each known real custody FD, process_vm_readv and ptrace attach; all fail. Signal with kill(PID,0) returns EPERM; also use harmless SIGCONT and require EPERM, then prove daemon alive. Try setresuid/setresgid to daemon identity and require failure. No byte output from unexpected successful access; latch failure immediately. Record errno/status only. Paired daemon read/load/sign and workspace positives prevent an always-failing harness from passing.
3. Same-UID negative control: trusted runner launches fixed same-uid-control as21001 against the disposable founder fixture. Actual wrap/envelope reads succeed, and it can authenticate the envelope with the wrapping key using private codec helpers without exporting bytes. This demonstrates failure of the intended protection premise under shared identity. It is an EXPECTED access success, not an isolation pass. Yama/dumpability may still deny same-UID ptrace; don't weaken them to manufacture a debugger success.
4. Fresh-process restart: release causes original create process to drop owners, close all FDs and exit; require exit/reap before restart exec. Restart uses only persisted authenticated envelopes and separately retained expected public contexts. Same original DER/fingerprint, CA SPKI and member SPKI; real CSR/sign/matching leaf verification succeeds. Wrong expected-context reload refuses unchanged, then correct reload succeeds. No inherited key/owner, regeneration, repair or image rebuild. This proves process restart, not power loss.
5. Two learners: create-learner creates only its own installation and member envelope with founder public root context; restart-learner reloads real member and validates a matching leaf using public bytes if supplied. Neither learner receives CA PKCS#8, encrypted CA, founder wrapping key, private FD or founder directory mount. Inspect actual mountinfo/FD targets in learner, plus trusted Docker inspect filtered mount sources and host inode/device separation. Each learner mounts exactly its own two roots; inventories exclude ca-key; fixed founder path/handle probes fail/are absent. Root/image layer has no CA. Negative fixture with CA presence is already covered by 3a refusal tests; retain those, don't copy a CA into an actual learner to claim absence. No transfer/promotion or generic member-to-CA conversion.
6. Report process identity/caps/limits/mount/network facts, selected test counts, per-case statuses/timings, first failure and cleanup status. Never dump private bytes, environment, certificate bodies, filesystem image or arbitrary OS diagnostics. Redact to fixed enums and numeric metadata.

## 5. Exact single CI insertion

Add ONLY the following step immediately after 'Preflight loader environment and run workspace tests', before 'Read-only service artifact checks', in jobs.rust.steps:

```yaml
      - name: Hosted distinct-UID custody qualification
        if: runner.os == 'Linux'
        shell: bash
        run: python3 -I -B tools/ci/custody/run.py
```

run.py's no-argument entrypoint first runs test_run.py via bounded isolated unittest discovery, then performs this fixed harness. Exit nonzero on tests/preflight/harness/cleanup failure. No additional CI step, job, timeout-minutes, continue-on-error, retry or always condition. Existing steps stay byte-identical and in order; Windows diagnostic anchor/sample, nextest configuration/quarantine, all three OS matrices, verify gates, ci-time-budget and agent-review remain unchanged. Windows/macOS have neither new Docker execution nor hosted probe compilation (Linux cfg gate); measure any source/module incremental overhead anyway. Python pure tests do not execute Docker/sudo: injectable command/clock/filesystem interfaces prove actual refusal/cleanup control flow.

Name pure tests test_rejects_unsafe_roots_and_context, test_requires_one_exact_probe_and_matching_hash, test_fixed_argv_environment_and_limits, test_deadline_reserves_cleanup_and_kills_children, test_first_failure_survives_cleanup_failure, test_success_requires_cleanup, test_rejects_unknown_container_mount_loop_ownership, test_zero_selected_or_missing_oracle_fails, test_holder_readiness_lifetime_release_and_deadlines, test_shm_and_runtime_mount_accounting. test_holder_readiness_lifetime_release_and_deadlines must use a fake clock/process control to prove ready by5s, healthy holder alive beyond5s during live probes, no pass at readiness, release then terminal/exit/reap strictly before restart, and forced container cleanup/local reap on both holder30s and work130s deadlines; include readiness5s timeout and cleanup-error first-failure preservation. test_shm_and_runtime_mount_accounting must assert the exact --shm-size 1m argv and17+1=18,18+64=82,256-82=174 arithmetic, rejection of wrong/omitted shared-memory bounds, and both-UID mount-source/limit verification failure paths. Implement meaningful fault inputs/side-effect assertions, not source-string mirror tests. Coordinator records exact discovered counts; no asserted count before implementation.

## 6. Cost and first hosted measurement

Estimate, not measurement: added Linux step typically35-75s (registry/image work dominates), hard ceiling150s; pure tests/discovery1-5s, prep/cleanup3-10s, image10-35s, actual process cases5-15s. Incremental Linux probe compile approximately2-10s, unknown until measured. Current Linux worker ~3-4min gives a planning estimate around4-6min, below unchanged12min; this is not a budget-fit claim. The worker timeout is a separate constraint from ADR0031's900 charged seconds for the whole valid green first-attempt run. Linux may overlap a longer Windows/UI tail, or extend charged union; job deltas cannot be summed as charged time. Historical Windows10m36s has only84s margin; no permission to consume that margin blindly.

On the normal exact-candidate first CI measure source/run/attempt identity; full wall, trusted charged-v1 or conservative wall fallback, credited idle and longest execution; all six workers and three gate timings; each Rust worker compile/lint/nextest durations and counts; added probe compilation, Python helper/discovery, pull/build, filesystem prep, create-readiness/holder-lifetime/deny/release/terminal-reap/restart/cleanup durations; binary/image/context sizes/digests, selected=1 per invocation, protocol case totals, real UID/cap/mount/limit observations and first failure. Compare to current normal baseline, acknowledging noise. Observe macOS/Windows all-target compilation and unchanged feature limits. No local Engine experiment, trigger-only run, dispatch, rerun-to-green or fabricated budget status. If margin fails, preserve evidence and narrow/split under coordinator disposition. Never raise budgets or modify scheduling/quarantine. Gates require complete successful workers and exact-head independent review.

## 7. Fence, adoption and excluded work

Builder owns only custody/hosted_probe.rs; tools/ci/custody/Containerfile, run.py, test_run.py; and the one ci.yml step. Adopted mechanical exception: custody.rs receives exactly the private declaration #[cfg(all(test, target_os = "linux"))] mod hosted_probe; (two lines). There is no existing declaration at the baseline; without that exception the proposed module cannot compile. No production behavior change or tests.rs workaround. Coordinator owns docs/PROGRESS.md (provider=Sol6.1, effort=high, design/build/review outcomes, exact checks/costs, last merge, gaps/blockers) and durable adopted pins/ADR clarification if needed. Coordinator adoption is taken; this scratch artifact still must be persisted as the durable repository pin record before image execution.

Coordinator decisions TAKEN (2026-11-20): adopted digest/platform/recipe/probe/UIDs/resource/time pins, explicitly including hosted-only capped ext4 preparation and exactly the two-line module-declaration exception; this revision incorporates the required SP1/SP2 corrections. Implementation obligations still pending: finalize candidate-source/input hashes and persist adopted pins before image build; record resulting binary/image hashes before key execution; assign independent Sol exact-head implementation reviewer and retain all normal gates before coordinator-only merge. Treat dynamic-link, runner capability, deadline or budget failure as a real block needing evidence/disposition. No owner decision is currently needed; native accounts/provisioning remain unrequested. No design authority here to add attempts to exhausted work.

Everything else stays excluded: Cargo/dependency upgrades, CA/codec production changes, startup/publication/credentials/receipt/provenance, Found/Redeem/founder/Engine, consensus, transport, clocks, services/config/main/MCP, actual executor/A-to-B, promotion/CA transfer, renewal/vault, complete three-node acceptance/expiry, native account/admin/VM work. No Docker socket mounts, ports, host networking, owner mounts or publishing. D6 Engine Sol3/3 exhausted and historical Opus0/2 unchanged; after three failed fixes stop and route evidence to a fresh independent reviewer without resetting counters. Never access LabManager or agent-windows-test; no babysitter changes.

Runtime evidence remains pending. Native and multi-host paths are **unverified on real hardware**; crash durability remains unqualified. This design task performed only pinned reads, public registry/documentation lookups and scratch document/hash inspection. Initial read commands set TMP/TEMP/scratch but omitted explicit target/observer/cache variables; later commands set all, and no build/cache tool ran. Scratch root/tmp inspected as ordinary non-reparse directories. No secret/private owner data was used or emitted.

Primary runtime flag references: https://docs.docker.com/reference/cli/docker/container/run/ ; https://docs.docker.com/reference/cli/docker/container/exec/ ; https://docs.docker.com/engine/storage/tmpfs/ . These document read-only/cap/resource flags, exec environment inheritance and tmpfs memory semantics; actual hosted observations remain mandatory.


## 8. Builder-ready handoff (adopted)

Implement only these paths:

- crates/arxd/src/cluster/pki/custody/hosted_probe.rs: new fixed cfg(test), Linux-only probe, exact libtest name and closed modes/controls from sections1–4; real custody operations only.
- crates/arxd/src/cluster/pki/custody.rs: exactly the two private module-declaration lines below, nothing else.
- tools/ci/custody/Containerfile: exact two-input, immutable-base custody-runtime recipe above.
- tools/ci/custody/run.py: fixed no-argument bounded hosted launcher, pure-test preflight, asynchronous holder monitoring/release/reap, corrected shm/mount accounting and ownership-safe first-failure cleanup.
- tools/ci/custody/test_run.py: meaningful injected command/filesystem/fake-clock tests listed in section5, including the two review regression cases.
- .github/workflows/ci.yml: only the step below in existing jobs.rust immediately after workspace tests and before Read-only service artifact checks.

~~~rust
#[cfg(all(test, target_os = "linux"))]
mod hosted_probe;
~~~

~~~yaml
      - name: Hosted distinct-UID custody qualification
        if: runner.os == 'Linux'
        shell: bash
        run: python3 -I -B tools/ci/custody/run.py
~~~

Coordinator alone owns durable adopted pins, docs/PROGRESS.md and any clarification record. No dependency/API/CA/codec/executor/startup/Engine change, new CI job, existing-step edit, scheduling/quarantine/timeout/gate change or native provisioning. Keep all original exclusion/stop rules.

Tests: pure launcher cases must prove fixed argv/env/identity/mount/resource pins, unsafe input refusal, unique exact probe/hash selection, asynchronous readiness/lifetime/release/reap/deadline behavior, selected nonzero terminal counts and cleanup/first-failure preservation. Hosted fixed probe cases must demonstrate actual founder create/load/sign, live distinct-build-UID file/process/FD/signal/escalation denial with workspace/daemon positives, successful same-UID access as the limitation control, separate-process restart identity preservation, and two CA-free learner mount/FD/inventory/reload fixtures. Readiness is not terminal test credit. Cases remain NOT RUN here.

Before the single image build, persist exact candidate source HEAD and input hashes; after build and before key creation record test-binary SHA256 and image ID. On normal exact-head first CI record full wall/charged-or-fallback/credited-idle/longest execution, all six workers/three gates, platform compile/lint/nextest counts/times, added compilation/helper/image/fixture and per-node readiness/holder/probe/release/reap/restart/cleanup time, exact selected/passed counts, actual image/context/scratch sizes, writable mount capacities/source/permissions under both UIDs, shm/cgroup facts, first failure and cleanup result. No CI fit claim until measured; unchanged12-minute workers,900 charged-second gate and independent exact-head implementation review still decide readiness.

## Revision and review record

Independent design review D:/Temp/arx-custody-3b-review/review.md was hash-verified as e771b2e451d3b16eb5aa22f4d0c633a5a0d8b50998a375f16369abb137da3f48. Verdict: GO with required changes. This revision addresses SP1/P2 with asynchronous five-second readiness, separate30s/work-clipped holder lifetime, interleaved shared budgets, release/terminal/reap ordering, deadline cleanup and fake-clock obligations; it addresses SP2/P2 with explicit1 MiB shm, corrected18/82 MiB capacity arithmetic, runtime-generated mount inventory/cgroup verification under both UIDs and pure regression obligations. Coordinator adoption of the pins, hosted-only ext4 fixture and two-line declaration is recorded above. Those decisions are taken; actual candidate/input/output pins, implementation, independent exact-head review and hosted evidence remain pending.

This revision changed only D:/Temp/arx-custody-3b/pins.md and performed scratch document/hash checks. No repository reads were needed for the corrections; no repository files were changed, no build/test/container/provisioning/push ran, and no new runtime or review pass is claimed. Crash durability remains unqualified; native/multi-host paths remain **unverified on real hardware**.


## Amendment A1: exact sudo-rs target (coordinator, 2026-11-20)

Hosted evidence (PR #199 runs on heads `2ebf9281` and `f6a535c4`): on GitHub-hosted
ubuntu-latest, `sudo` resolves to `/usr/lib/cargo/bin/sudo` (Ubuntu sudo-rs), uid 0,
gid 0, mode 4755, refused only by `canonical_system_directory`. Independent stuck-rule
review `D:/Temp/arx-pr199-amend-review/review.md` (SHA256 `e9f01cef…d9`): GO with
required changes. The owner authorized one further scoped builder (2026-11-20).

Adopted policy, sudo role only:
- Trust the exact resolved target `/usr/lib/cargo/bin/sudo` in addition to the existing
  canonical directories. No prefix broadening: other files in that directory, subpaths,
  sibling directories and arbitrary package-owned paths stay refused, and no other tool
  role may resolve to this exception.
- Validate BOTH the original route (the canonical `/usr/bin/sudo` or fixed-search entry
  and every symlink hop) and the resolved target. Every path component from `/` to each
  checked object is root-owned (uid 0) and not group- or other-writable; the target is a
  regular, root-owned, setuid file as observed, not group/other-writable.
- Execute through the validated absolute path with `shell=False`; tools run under sudo
  (mkfs, losetup, mount, umount, findmnt, mkdir, chown) are also passed as validated
  absolute paths so sudo's secure_path cannot select another binary.
- Every filesystem error during validation (missing path, dangling link, permission,
  loop) yields a sanitized tool/predicate refusal record; validation stays fail closed.
- The remaining time-of-check/time-of-use assumption is stated honestly: retained checked
  objects plus root-owned, non-writable ancestors prevent an unprivileged actor from
  substituting the binary; this is not atomic execution of the checked descriptor.
- All other adopted values (image digest, UIDs, caps, deadlines, fixture, CI step) are
  unchanged.

## Amendment A2: exact uutils multicall targets (coordinator, 2026-11-20)

Hosted evidence (head `c0679c7a`): with A1 in place sudo validates; preflight then refuses
`mkdir`, whose route through `/usr/lib/cargo/bin` (all ancestors uid 0, mode 0755) ends at
a regular root-owned 0755 file with 116 hard links (`regular_single_link`). The runner is
Ubuntu 26.04 with rust-coreutils (uutils) as the default core utilities. Independent review
`D:/Temp/arx-pr199-a2-review/review.md` (SHA256 `0d8a19fa…2677`): GO with required changes.

Adopted policy:
- A closed role-to-exact-target map for the coreutils roles the harness invokes (mkdir,
  chown, and any other fixed role listed explicitly in run.py). No category detector,
  basename bypass or prefix broadening; other roles and files stay refused.
- The multi-link exception is integrity assurance, not single-link proof: the route
  (canonical entry and every symlink hop) and the target pass every A1 ancestor check; the
  target is a regular root-owned file, not group/other-writable, with no setuid/setgid;
  `/proc/sys/fs/protected_hardlinks` reads exactly 1 at preflight (unreadable or 0 refuses).
  All other predicates stay conjunctive.
- Multicall dispatch is preserved: each role executes through its own validated entry name
  so the utility function is selected correctly; never one generic path without the
  function argument.
- Preflight evaluates every required tool and the sysctl, reports every refusal in one run
  with sanitized records, closes everything it opened, keeps the time limits and the first
  failure, and still fails closed.
- Python receives no A2 allowance; validate `sys.executable` under the existing rules and
  require Python >= 3.11.
- Hosted qualification (pins sections 4, 6 and 8) stays open until real cases run. All
  other adopted values are unchanged.
