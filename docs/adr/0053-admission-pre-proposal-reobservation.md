# ADR 0053: Allow one pre-proposal learner admission re-observation

The M3 learner admission rule allows at most one additional fresh observation
before proposal handoff. This applies when the sampled membership or progress
view advances across the first read, or when a prior bound learner is behind
the sampled commit. The same owner task retains its membership intent and
proposal permit throughout.

An already advanced view allows immediate re-observation. For a lagging
learner, release the retained node and wait up to 250 ms, capped by the original
RPC deadline. Check the current Raft metrics when arming the wait. Progress
through the sampled commit permits a second full proof of identity, uniform
membership and exact watermarks. Stop cancels the wait and the retained-node
reacquisition. A cutoff or a second refusal ends the attempt. An exact replay
returns the observed membership watermark without a proposal.

The one-proposal rule, all three exact Raft guards (leader, membership and
tail), and the post-apply authorization check still apply. C10's stale-tail
refusal leaves zero new entries. Proposal handoff ends all re-observation.
The D6 freeze in [ADR 0037](0037-acceptance-custody.md) stays in force.
