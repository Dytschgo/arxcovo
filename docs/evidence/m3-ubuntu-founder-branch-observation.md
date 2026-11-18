# M3 Ubuntu founder observation: local source evidence

Sol 6.1 / high implementation sub-agent, 2026-11-18; no children.
Assigned isolated base: bbe9a4433cadfe981662c9fafbd859c26a818498.
Source commit: 84522a3b729e036ff700efc15cd575c6dee160d1.
PR176 source pin: f1bf99bcbb3e18656879d6ce5cbf24e367c6056a.
Last-known green parent main: 05b9c82930cd3812614fc20f5d03d0a5fa05952a
(supplied/retained evidence, no hosted query here).

Corrected brief blob: 47d7c460933f433c300385e9189dbdfb513948f4.
Astra contract-review blob: 8d6c0b4d86f2b26715c44c896fd8d4306924d42b
(review references 2be92556/13ad3a35 supplied by root).
AGENTS, KICKOFF, ADR0005, query/founder briefs, prior stuck review and
Engineering/Debug skills read. Historical reproduction/fix phases remain deferred.

**Review candidate; contract not fully closed.** Source is frozen after the
authorized correction batch. Exact-source independent Astra review remains
required before publication. No publication or hosted-run authorization used.

## Implemented and remaining review gaps

Only the five allowed source files changed. Test-only observer has 48 packed
Release/Acquire slots, immutable typed lanes, optional task-local capture,
capture-return sequential snapshot and fixed ASCII output sinks. The original
24-event capacity and F6 formatter/capture output remain; no recorder holds a
store, permit, key, Raft handle or callback. Only the three specified real calls
opt in; the fourth body receives an entry/exit marker pair.

The original founder guarded Ok(r) and wildcard remain under cfg erasure;
test-only late-Ok selection records the actual founder branch. Business clock,
closed-check and explicit-drop counts are unchanged in the shared helper ranges.
Borrowed store results and initializer/linearizer errors become closed scalars.
Caller MissingRaft is marked without acquiring another Raft handle.

Returned founder refusals preserve the primary error through existing bounded
cleanup. Proof success flow is retained. F10 still releases/joins its gate,
drains reads and checks uninitialized state before the final call; its successful
leaf is discarded at that match statement. Handles are not claimed to be joined.
Startup failure, later panic, outer timeout and cancellation retain their original
cleanup/emission limitations.

Known contract gaps for root disposition, with no further edits attempted:
- Leader-wait ReplyError is derived from the mapped phase result, rather than
  classifying a borrow of its original error payload before collapse.
- Guard late-ready and elapsed-wait decisions both use CallerExpiry/Expired;
  they are not independently distinguishable in the packed scalar.
- Successful preflight/proposal caller replies and successful blocking-helper
  replies remain absent; absence cannot be treated as failure or a join result.
  Some early stopped checks occupy terminal slots with Unknown. Their slot
  position alone must not be interpreted as evidence that a helper ran.
- The erasure audit is manual plus token/count evidence, not a compiler-expanded
  equality proof. Added tail-expression blocks and closure bodies need the
  independent lifetime/drop review. No final-source fmt/static-suite rerun occurred.

## Actual checks and budget

Commands used the installed 1.95.0 toolchain. Cargo used a fresh task-private
CARGO_HOME registry copy, fresh target and TEMP/TMP, offline dependencies and
CARGO_BUILD_JOBS=2. Registry descendants and private cache directories were
checked for reparse points; no installation, network, cache deletion or global
process interference. Logs remain in the task-private
%TEMP%/arxcovo-founder-bbe9-observer directory.

| Actual invocation | Outcome |
| --- | --- |
| cargo fmt --all | exit0, once, before mechanical batch |
| python -I -B -m unittest discover -s tools/ci -p test_windows_cluster_quarantine.py | 8 PASS, once, before batch |
| python -I -B -m unittest discover -s tools/ci -p test_windows_diag_sample.py | 60 PASS, once, before batch |
| cargo clippy --workspace --all-targets --locked --offline -- -D warnings | exit101,83.347s: unreachable APIError pattern; three literal-Err expect lint errors |
| same Clippy, one authorized recheck | exit0,9.866s, final source |
| cargo test -p arxd --lib --locked --offline -- FOUR_NAMES --exact --test-threads=1 | exit0,127.663s including compile; exactly4 PASS,641 filtered, final source |
| scope/UTF-8 without BOM/LF and git diff --check | PASS, final source, exact five-file scope |

FOUR_NAMES, each prefixed
cluster::consensus::issue::found::observation::observer_tests:::
immutable_lane_isolation_and_optin,
slot_packing_absence_saturation_cutoff,
formatter_cap_ascii_primary_newline,
error_cleanup_scalar_precedence.
One test invocation only. No semantic test red occurred.

The one mechanical batch removed the unreachable observer pattern and replaced
literal-Err expect plus its unreachable return with a direct primary-error panic.
Budget used1/1; no further correction or check retry authorized.
Other tooling errors before checks: PowerShell brace syntax, one JavaScript
string parse, missing optional .cargo/config.toml/incorrect adapter read path,
and Windows rg glob syntax. They caused no source mutation or test execution.

Owned exec sessions completed: cache-copy73154 exit0; Clippy23990 exit101;
recheck29844 exit0; pure-tests75457 exit0. These are session IDs, not OS PIDs.
No global process-name ownership inference or process killing was performed.
No claim about an independently inspected descendant process inventory.

## Output and interpretation limits

Three fixed8192-byte sinks plus one fixed512-byte sink bound the new target
records to25088 bytes, including emitted newlines and truncation footer.
Each write reserves13 bytes; overflow/non-ASCII is rejected and sticky.
Primary tag/result/reason precede optional rows. Existing F6 output is separate.
Tests exercise cap/ASCII/footer, scalar precedence, immutable lane isolation,
absent/saturated packing and frozen-array cutoff behavior, without Raft.

Cutoff is capture return, after the business decision and before cleanup:
48 sequential Acquire loads are not an atomic worker snapshot. Missing, racing,
saturated or ambiguously ordered edges remain unknown.

No proc/PID/boot/host/file reader was added. Checked epoch milliseconds and
process-local elapsed microseconds are endpoint markers only. External same-job,
attempt and unique-case provenance is required. With all four visible unique
pairs, only max(start+1ms)<min(end) can support recorded-wall intersection,
conditional on a common stable wall clock. Regression, wall/elapsed mismatch,
missing output/provenance or insufficient margin leaves overlap Unknown.
Agreement cannot rule out intervening clock steps. Verified monotonic
cross-process overlap remains Unknown; scope overlap establishes no phase overlap,
CPU use or historical cause.

No local Windows engine, old F6/F10, whole-cluster, nextest, mutation or target
failure was run. Hosted checks, exact-source independent review and new-revision
normal full CI are UNRUN and require root's separate grant. Historical Ubuntu
refusals and parent green are retained evidence, not reproduced here.

PROGRESS proposal (not edited): Sol6.1/high local observer candidate84522a3b;
Clippy final PASS and pure4 PASS; guard8/sampler60/fmt PASS before mechanical
batch; contract gaps and independent exact-source review outstanding.
C invalid/T unrun; PR176 held; Sol3/3 exhausted/historical Opus0/2 unchanged;
M2 open/M3 unchecked; hardware **unverified on real hardware**.
Babysitter unchanged.

## Retained log SHA-256

| Log | SHA-256 |
| --- | --- |
| fmt.log | e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855 |
| guard.log | 09c9201146f8ae80b5598cffe9664227b5f7368851b23d221e49d4a611c5425b |
| sampler.log | 694f1106068c15e78ae39945b3a98e0499201c2da10cfa1e06449064aabb034d |
| scope-audit.log | 154cf2c32c12f33e102955ea401028e9fe4d5a4a70633a858e2bdb0c11cc6811 |
| final-scope.log | 2707f214659b31b646c24a32a00a87574e5b89a57bd26e58bcce6e2964179fef |
| clippy.log | 31543fb98af2a1a188b61008f7b6fb0d6f3127f272bab2b4edb15d0f962dad48 |
| clippy-recheck.log | 0487c871ddf4674261b5fa3ba86cc94895dc277a87deaf1508180e6331a3ac78 |
| pure-tests.log | 24b5994eb38b0f0a28addd4437def34b66aa1daa1a4ade050159a6138afdb0bf |
