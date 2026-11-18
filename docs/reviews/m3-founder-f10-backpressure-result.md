# F10 backpressure local Windows model result

OpenAI / Sol 6.1 / high; research sub-agent; 2026-11-18; no children.
STOP after C: process-presence mismatch; T NOT invoked. No pair verdict.
One authorized nextest command compiled and executed C, actual exit 0.
The evidence wrapper later exited 1 on cp1252 console rendering, after saving
outcome/logs/hashes. This wrapper failure is distinct from nextest exit 0.
No correction, repeat, discovery/list/probe, extra experiment or post-restore test.

## Pins and admission actually checked

Assigned HEAD 23ab39ab6500ff6785bad0807670c8cb78def2c5.
PR176 source f1bf99bcbb3e18656879d6ce5cbf24e367c6056a.
All 132 active crate files compared byte-for-byte with that Git object.
Sorted POSIX path-NUL/SHA256/LF baseline manifest:
de6c0835a62b0786f96ad6022ec52984b1458e93b62f21b9ff2bb7cbe6d103a3
Base/restored found_tests.rs SHA256:
6c601f548eed3e02c43adc48a0bdec18f958d28e70d6a334a39ff2a4874408a5
C patch SHA256:
81f8869a530ea9c4a183bec278c7d73292fd4e57c329335998114c57b6311a49
C patched target SHA256, checked before and after invocation:
e65958bd34980c94dc0a07ec2d7929ddda8df635ad4678d96eaa1d990af44de2
T patch SHA256 (checked, never applied):
504f59fc6eadada2b13de77d4be1154a3a36304eecc06fa4f553c2f7fb521283
T target hash is design-only, not an executed artifact:
593466c22e64e0dea5b2d2a163be43a644a49f3301657b5cdbbd3cde799364ce
Both exact git apply --check --whitespace=error-all passed.
Retained review47e053a3/original3320f566 independently compared byte-identical.
Reviewed design/safety reasoning is reused, not a new independent review here.

Initial process admission found no cargo/rustc/nextest/arxd processes.
Cache registry copied into private storage; cache/tool/workspace paths checked
non-reparse. Evidence root ACL protected, owner-only full control, inherited
by private cache, separate C/T target/tmp/evidence directories.
No Cargo/nextest ancestor or user config found; no CARGO/RUST/NEXTEST env knobs.
Cached CLI source inspected for exact flags; no discovery invocation.
Only isolation env: CARGO_HOME, CARGO_TARGET_DIR, TMP/TEMP, pinned bin first PATH.
No install, network action, configuration override, retry, VM or admin action.
Actual tool versions: cargo1.95.0(f2d3ce0bd), rustc1.95.0(59807616e),
nextest0.9.151(6b3f994577a98874a2e0ea5bef77fbbab3434044).
Cargo executable SHA256:
dc19c8e6d66802d120bf0696b1924b748bd90f3ca16f21391e54a290ff12b7c5
Rustc executable SHA256:
e3ebbd547ea7b73c034d588ba569602b379f3b05ad1a3b5f8dcfab9d4478d74a
Nextest executable SHA256:
74d834a7a97655bc519b529b617444baa99b0f8a60d714089d9a8e0058187f39
Cargo.lock SHA256:
c899c5d793c4449e53992f70423936dcc80bdc464b97e828145cd3e9825081b6
Toolchain config SHA256:
97aab3493c1fb14fe9a21c760433577f434714f5725bbb176e67048dde581afe

## Single C command and observed outcome

Pinned absolute cargo executable, from assigned worktree:
```text
cargo nextest run --offline --locked -p arxd --lib
-E 'test(=cluster::consensus::issue::found::found_tests::f10_drop_bounded)'
--test-threads 1 --retries 0 --fail-fast
--success-output final --failure-output final
```
These wrapped lines represent one actual command, including compilation.
UTC start 2026-11-18T01:34:00.488180+00:00.
UTC end 2026-11-18T01:35:19.756246+00:00; whole command79.266s.
Cargo reports compile1m15s; nextest test0.140s, summary0.141s.
One test passed,640 skipped; no other behavioral cell executed.
14 ordered events, overflow=false: finaldeadline enter Pending;
preflight/initialize/leaderwait/proposal/guard1/guard2 enter then exit Ok;
finaldeadline exit Ok, pending=none, elapsed34021us, remaining4965978us.
All six completed phase spans <5000000us.
Markers: gate_acquired=1, gate_released_joined=1, drain_completed=1;
issue_free=4, read_free=8, primary_ok=true, primary_unavailable=false;
shutdown_completed=1, shutdown_ok=true. All result/trace/cleanup oracles pass.
Markers are late booleans/checkpoints, not release or completion timestamps.

Immediate post-command process snapshot instead recorded two cargo names and
one rustc name. IDs/ownership were not captured; attribution is unestablished.
This violates the no-competing-process/survivor condition: overall C INVALID.
A subsequent single cleanup inspection found no matching process names;
nothing was killed. That later absence does not repair admission or authorize T.
No arxd/nextest name appeared in the failing snapshot. In-test normal cleanup
is supported by Gate join/capacities/shutdown_ok, not by process names alone.

## Restoration and retained evidence

Before reversal every crate hash matched the saved C patched manifest.
Only exact C patch reversed after reverse-check; all132 bytes equal baseline;
F10 marker absent, existing DEBUG-F6-OBS baseline present. Git crate diff empty.
Restoration static check UTC2026-11-18T01:36:03.398862+00:00.
Original found/assertions/settings/clocks/Owners/earlier F10 body restored exactly.
Private retained evidence: D:/Temp/arx-f10-bp-23ab39ab.
Contains admission.json, baseline/patched manifests, C invoked/outcome/oracle,
command/sanitized logs, bounded complete-trace.txt, restoration.json and scripts.
No cache deletion, unrelated kill, source commit or public action.
C binary arxd-648de2072896f9ec.exe SHA256:
0f2f00bff6aa1d613f03f645942eeeb48b77bb9aa2985f6c2556821975ad5da5
C command.log SHA256:
25bea2487ee0a4300397cedfd4d9dd9af61dc5fe0ffa54eda44a6f4f47343fa1
C sanitized.log SHA256:
3d3751160d5a3b3af11beda6e9b9a28c879b48f7a3a4781ff9104b59684dd2d2
C complete-trace.txt SHA256:
698ace07b6cbd0aedcc80f721d948c8a1d52773ad1b38329f3b35fc36f21a181

Local Windows MODEL only under prior owner PC grant. Not Ubuntu acceptance,
natural-host failure reproduction, host cause, fix or reproduction-rate credit.
5s/2s/5s/20s are cooperative budgets, not hard wall caps; synchronous Gate and
printing are not preemptible. Preflight entry alone proves no worker progress.
Hardware paths **unverified on real hardware**. Debug natural-symptom phase1
remains open; no new hypothesis/fix/regression loop or phases2-6 claimed.
Sol3/3 exhausted; historical Opus0/2 unchanged. M2open/M3unchecked; PR176held.
PROGRESS untouched. Proposal: record single C exit0 and exact trace/cleanup,
but process-presence invalidation, T unrun, exact restoration, no retry authority;
retain all counters, holds and hardware qualifications above.
