# Main R5 post-release wait failure

Exact main `c6121523417089bc0afaf307470183dd4a7f47ba`, after PR161.
CI43880904887, Windows job133346204427 failed at14:05:20Z; job duration372s.
Nextest:152/595 run,151 passed,1 failed,16 skipped,443 unrun; R5 took12.321s.
Raw job output is retained locally at D:/Temp/arx-main-c612152-windows.log.
No raw runner paths, full metric rings or private data are committed here.

The failure is `cluster::consensus::three_node::r5_shutdown_read_owners`:
`three_node.rs:1195`, Result::unwrap on `Elapsed(())`. This is the two-second
wait AFTER shutdown and AFTER opening the deliberately held read gate. Its
condition combines state/log weak upgrade success and available read permits.
The log does not identify which predicate remained true at the deadline.

Both preceding shutdown assertions completed:
- clean variant: Ok,1.5255ms;
- gated variant: expected Orphaned, first=Owners,engine=Joined,owners_waited=1,
  owners_left=1,read_held=1,propose_held=0,log_strong=0,sm_strong=2,total_ms=5004.
Thus the recorded red is neither a shutdown result assertion nor evidence that
its five-second budget failed to enforce the intended gated refusal.

D1: t=10300,lag=13ms,over=0,candidate=None,helper attr=None. Storage maxima:
Vote126ms,Append145ms,Apply192ms. These are bounded observations, not a cause.
The first node's metrics report shutdown; the other nodes reached term2.
The dump is truncated (8120bytes); no complete-lifecycle claim is made.

PR161 changed no cluster source; this R5 case was introduced in PR149 and
remains an ordinary required Windows test outside the eight quarantines.
No quarantine extension, rerun, timeout, helper or assertion change follows.
Sol shared-suite fixes3/3 remain exhausted; Opus recovery fixes0/2. Diagnosis
must distinguish remaining owners/permits from timing or instrumentation before
any correction proposal. The earlier negative append model supplies no cause.

Main is RED; no further feature or diagnostic merge until exact-main CI is
green or an independently reviewed repair restores it under existing steering.
Founder implementation and Windows phase publication wait. PR159 stays held.
M2 open,M3 unchecked,eight Windows quarantines unchanged; hardware paths are
**unverified on real hardware**.


## Final CI disposition

Run43880904887 completed failure on exactc612152. Ubuntu passed in228s,
macOS passed in333s (14:17:04-14:22:37Z), Windows failed in372s.
The Windows failure above remains the sole reported required-job failure.
No rerun was dispatched. The background read-only watch exited1 and is terminal.
Opus disposition9075daa records one local R5 control pass and one bounded probe
round: separate held-store and held-permit controls distinguish the predicates,
but natural hosted failure was not reproduced. No hypothesis or fix is credited.
Root contractc761aed authorizes a bounded observation implementation, followed
by independent Sol review and exact-head checks. Main-red merge authority is
unchanged; no feature or diagnostic merge has occurred after this red.


## Separate wall-clock budget blocker

PR159 ci-time-budget status59033483192 failed at14:23:39Z: latest completed
main CI43880904887 took1475s, exceeding the required15-minute wall clock.
Created13:58:02Z, final update14:22:37Z. The macOS job executed14:17:04Z to
14:22:37Z (333s), so most wall time preceded macOS execution. This is separate
from Windows R5. Existing policy deliberately includes the complete run wall
clock; queue time is not subtracted and the gate is not waived. No rerun or
ci-speed label was applied to unrelated TLS or diagnostic work. A successful
reviewed measured CI reduction is needed under the existing ci-speed rule.
