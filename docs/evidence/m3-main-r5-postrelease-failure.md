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
