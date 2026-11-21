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
   observation, never a deadline change. A later connection on A, with a
   different key and attempt ID, is refused at Query and releases no proof.
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
