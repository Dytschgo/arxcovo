# ADR 0051: Per-instance test clock for enrollment expiry evidence

## Decision

Linux test builds give each `HostClock` its own forward-only raw source
offsets for wall time and elapsed time. A sample reads both offsets once
and adds them to the host readings before the unchanged checked
arithmetic. The recovered floor, valid range, bracket ceiling, rollback
latch and wall/elapsed envelope still decide every sample. Production and
non-Linux builds have no offset field, switch, environment input or
caller. The field is boxed so test builds keep the existing inline layout
within the size lints that apply to frozen enums.

`advance_for_test` moves one instance's wall and elapsed sources together,
which models time passing on one healthy host. `shift_for_test` moves them
separately so that controls can show the envelope refusing resume-visible
skew and a frozen wall. Offsets are cumulative and checked. An overflowed
or poisoned source refuses with a latched failure. Other instances and the
host clock are unchanged.

## Wiring audit

The audit traced the checked clock that each sensitive consumer uses at
`61542cae`.

- **Virgin founder.** The offline journal and inventory check passes before
  the founder constructs `HostClock::new(0)`. Floor zero is the actual floor
  of proved untouched genesis. That instance feeds the Found bridge, the
  initial member TLS key, the founder TLS clock, and the retained issuer.
- **Recovered founder and recovered learner.** Process recovery constructs
  the clock from the recovered applied store's `committed_ms`. Strict
  joiner reopen does the same. The recovered issuer uses that instance.
- **Fresh published learner.** A learner whose join was just published has
  no applied state yet and uses floor zero. Promotion reuses the learner's
  instance for the promoted issuer.
- **Strict and peer admission.** The learner builds its admission gate from
  the same retained instance that issued its TLS credentials
  (`bound_start/learner.rs:463`, `LearnerGate::new(sm, expected, address,
  clock, ...)`). Each gate observation samples that clock before it reads
  the phase or applied admission state and refuses as unavailable on a
  clock failure (`peer_registry.rs:199`). Bootstrap and Strict phase
  decisions therefore never run on an unchecked or stale time source.
- **Issuer consumers.** One instance covers the TLS time provider of the
  no-client-certificate enrollment listener, local activation, owner Mint
  (proposal time and the post-commit reveal check), AnchorQuery, and Redeem
  at pre-proposal, pre-sign and post-sign. CA transfer context uses the
  same instance. Promoted-voter materialization takes the learner's
  instance from its voter context and hands it to the promoted issuer.
- **Fresh destination.** A production destination uses its own
  `HostClock::new(0)` for its TLS client and publication time.

The audit found no caller that uses a stale floor or a second unchecked
time source. `runtime.rs` and `mint.rs` are unchanged. Remove stays
internal and has no owner-facing path.

## C15 design

C15 is one retained founder issuer on hosted Linux with real TLS ingress.
The expected counts are one Engine start and owned close, one Found, three
owner-local Mints, five AnchorQuery dispatches and three Redeems. That is
seven enrollment proposals, two allocations and one exact replay at the
original E. Destinations publish only and start no Engine.

1. **Exact replay (R8).** After Mint A, a harness client opens and
   authenticates a held continuation while A is pending. An ordinary
   durable-join destination then sends its Query and the original Redeem.
   The issuer holds that reply while the campaign checks the retained log
   and applied store. It confirms the dispatched request ID, the CSR key, E
   and the single allocation. After release, the held continuation sends
   the same request ID and CSR once, computing the proof for its own
   challenge. The Redeem must actually be submitted. It must return the
   original node and E with the same key, add one replay log entry and
   allocate nothing. Redeem work is admitted only while its five-second
   work budget fits the 12-second socket budget. The campaign checks that
   window before it sends the replay. A missed window is a failed
   observation, never a deadline change. Before the held Redeem, the
   campaign also waits, inside that window, for destination A's real
   completion. From A's retained publication it requires the returned full
   E, node and key to equal the applied original. A later connection on A
   is refused at its Query and releases no proof. Its attempt ID and key
   are never sent, because the Query is refused first.
2. **Controlled expiry.** After Mint B the campaign reads the applied store
   directly. It requires B to be pending and usable at a checked sample.
   It then advances only the issuer instance's wall and elapsed sources
   together. The new checked sample must land past
   `expires_ms - 2*SKEW_MS` and before nominal expiry. The same direct
   read at that sample must refuse with the applied position unchanged.
   Next, a real Query over healthy TLS must reach dispatch and return the
   uniform refusal, with `Refused` rather than `Unavailable` or `Busy`.
   It must not change the Redeem count or allocations. The public refusal
   is never widened. Attribution comes only from that direct read.
3. **Fresh positive.** Mint C uses the advanced checked sample. A second
   destination redeems C through the same issuer, using the issuer's TLS
   clock as client time, and publishes. Issued certificates keep the
   unchanged validity arithmetic relative to that controlled time.

**Received refusal.** For the consumed reconnect and for expired B, a
test-scoped observation client replaces the production `anchor()` call. It
uses the production verifier configuration, the production Query frame and
the unchanged 12-second budget. It sends only the Query. It requires a full
TLS handshake with the retained issuer's certificate, then the exact
uniform Anchor refusal frame, then a clean TLS close. It reports a deadline,
EOF before the frame, a transport error, a wrong frame, or a close without
close-notify as distinct failures. The production client's error mapping
and the wire are unchanged. Stub-issuer controls cover each outcome and
confirm that no byte follows the Query.

**Recorder scope.** The Redeem-intent recorder is opt-in. Only C15 builds
its observation with it, and it keeps at most four entries. Standing
campaigns that attach an observation record nothing extra.

Logs quote stage and relative millisecond offsets only, never a paste,
code, verifier or proof. The campaign keeps the first failure, records
bounded pre-close evidence and observes one owned close.

**Owner Mint path (amendment A13).** Owner-local Mint needs a
`service::cluster_owner::Operator`. Outside tests, only an authenticated
owner-socket connection constructs one. Amendment A13 adds a single Linux
test constructor, `Operator::for_campaign(uid)`, and makes no other change
to that file. A crate-wide source inventory test checks that the
constructor has exactly that one cfg-gated definition and exactly one
caller, the C15 owner Mint. Each of the three Mints still goes through the
real retained 13c caller, `Job::mint_owner`, including its leader-epoch and
listener checks, the actual proposal and the checked-clock reveal policy.
Only owner-socket authentication is bypassed. C15 therefore does not
exercise owner-socket authentication. C13's hosted run covers that path
with real distinct owner, daemon, agent and build UIDs. The C15 counts stay
"three owner-local Mints" through the real Mint caller.

**Replay window.** Destination A is prepared before the held continuation
opens. The campaign prints the held continuation's anchored, verified and
replay-send offsets and the remaining admission margin, all as relative
milliseconds.

This is controlled-clock real transport only. It is not elapsed-time, UTC
or suspend evidence, which remains with 15b and ADR 0030.

## Consequences

The checked-clock policy and every production deadline are unchanged.
Linux-only controls run the injected source through the real arithmetic.
They cover coherent advance, instance isolation, skew within the envelope,
resume-visible and frozen-wall refusal, floor and range after a shift, a
latched first refusal that survives advancing, overflow, source poison, and
contention. Installed-service and physical multi-host paths remain
**unverified on real hardware**.

## A16. Private production observation for elapsed qualification

The coordinator approved the R6 amendment after the source audit found that
owner status could count AnchorQuery dispatches but could not distinguish a
checked policy refusal from clock failure, permit pressure or timeout. The
Linux production issuer retains one last-Query observation at the existing
ingress call site. The slot holds the existing dispatch ordinal, a SHA-256
digest of the 32-byte Query body under the D5 domain
`arxcovo-d5-query-v1\0`, the captured checked sample and the typed outcome.
A forwarding CheckedClock returns the source result unchanged and captures
its first result. Slot publication and readback use try-locks. A busy or
stale slot withholds evidence while Query dispatch continues.

The existing authenticated owner socket accepts an empty request with tag
5 and replies on tag 16. Its version-1 body is 53 bytes. The first five
bytes are version, ordinal, outcome, Query sample status and checkpoint
sample status. Two big-endian 64-bit timestamp fields follow, then the
32-byte digest. Sample statuses are absent, accepted and failed. Outcome
codes are absent, accepted, policy refusal, busy and unavailable, in that
order. An absent or failed sample has zero in its timestamp field. The
request takes a checked checkpoint with the same retained issuer clock
without taking the node's admission lock. The harness serializes these
samples with client exchanges because they share the checked clock. Existing tag-4 status, public enrollment framing, TTL, clock
policy, proposal and worker code, and all deadlines stay unchanged.

The harness reads the fixture's bounded SQLite snapshots and retained log
using the C13 observation pattern. It correlates the acknowledged Mint to its
original full log position and committed expiry, then brackets the late
Query with equal applied views. Those views must show the same full applied
and membership positions, enrollment floor, pending record and allocation
counter. A16 must identify the late Query and its successful checked sample
past expiry with a typed policy refusal. The TLS client must receive the
uniform Anchor refusal and authenticated close before proof. Owner status
must show the additional Query and unchanged Redeem count.

Tests cover exact owner authentication, malformed requests, the legacy
status reply, checked source success and failure, ordinal and contention,
and the existing public framing. The A13 and E7 source guards and the
workflow and Windows quarantine guards pass with their existing allowlists.

## W1. Coordinator-approved hosted UTC reference

The two fixed reference origins are `https://www.google.com/generate_204`
and `https://www.cloudflare.com/cdn-cgi/trace`. This implementation uses
one verified HTTPS HEAD at each origin per checkpoint. A missing Date ends
the observation. Every request uses a generic User-Agent and sends no
repository, owner or personal data, cookies, credentials or query
parameters. The client follows no redirects, reads no response body and
performs no retries. The origins see ordinary HTTPS connection metadata.

Each Date needs a bounded, unambiguous fresh response. An Age header or an
indication of caching fails the check. Within each before/after bracket
from the retained issuer's checked clock, the complete offset range must
fit ±30 s after adding ±1 s for Date resolution and the approved ±1 s
source accuracy. Each origin passes separately. The five checkpoints cover
the initial Mint and Query, both sides of the dwell, the late refusal and
the fresh positive. The qualification artifacts contain relative clock
readings and reference offset bounds.

W1 supports the conditional premise in ADR 0030 for the sampled hosted
Linux profile. C13 supplies the restart evidence at the recovered applied
floor. C15 supplies the controlled rollback and resume-visible controls.
Physical suspend and other OSes remain **unverified on real hardware**.
