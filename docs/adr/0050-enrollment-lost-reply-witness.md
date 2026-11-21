# ADR 0050: Witness a lost enrollment reply after owned completion

## Decision

The Linux hosted J4 case arms one listener-local, test-only observation.
The session joins the admitted Redeem and encodes its successful Result
before consuming the observation. It records the full reply position, node,
and the accepted request and CSR key digest. It then drops that socket before
writing the Result or sending TLS close-notify. A refused or unencodable
reply leaves the observation armed. No live listener has this capability.

J4 keeps the original destination and compares its reservation, staged key,
wrapping objects and contended outer lock before and after the loss. The
issuer's retained log and applied state identify the original Redeem and its
single allocation. A separate authenticated connection sends one Query for
the consumed code. The issuer must refuse before continuation, with the
allocation and applied log unchanged.

P1 and J4 retain the first observation or assertion failure while releasing
the test hold and observing one owned close under its own bound. P1 keeps its
existing successful in-flight close and certificate assertions.

## Consequences

One hosted J4 observation uses one issuer Engine and owned close, one Found,
one fixture Mint, two actual AnchorQuery dispatches and one Redeem. The
enrollment log has three applied proposals and one new binding. The
destination starts no Engine. An unexpected frozen-path refusal ends the
attempt after the bounded close.
