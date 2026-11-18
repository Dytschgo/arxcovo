# 0030: Checked host clock at enrollment boundaries

Status: design accepted after independent Sol 6.1/high review on 2026-11-18;
implementation and required gates pending.

Enrollment currently receives an infallible timestamp before proposing redemption,
before signing, and before returning a certificate. An unavailable or visibly
anomalous host clock needs a refusal path at each observation. The advisory
anchor query needs the same fallible input. Deterministic replicated apply still
uses proposal timestamps and performs no live clock reads.

Use a private `CheckedClock::sample_ms() -> Result<u64, ClockError>` and a small
host adapter. Keep the existing founder `SignerClock` interface. Change the sole
enrollment API and anchor-query clock argument to the checked trait; add explicit
implementations to their existing logical test clocks. There is no blanket legacy
conversion, alternate issuer pipeline, configuration switch or production fallback.
The founder fixture receives only its explicit trait implementation; its existing
setup, assertions, timing and behavior remain unchanged.

The security premise is trusted host OS UTC within the existing 30,000 ms skew
bound at every sensitive observation on each issuer, including restart/resume.
This is a conditional trust premise, not measured UTC qualification. A new NTP,
attestation or quorum-time service is not required by this slice. Unknown initial
offset and errors hidden from both local clocks remain outside its guarantee.

The host adapter owns an `Instant` anchor, recovered local high-water timestamp
and a mutex containing its initial and last accepted observations. It retains no
store, engine, task, callback, queue or I/O handle. Acquire with `try_lock` before
sampling. Contention refuses transiently without sampling or changing state;
poison refuses. Synchronous sampling and OS scheduling have no hard wall-time
guarantee.

Bracket the UTC read with two elapsed reads. Use the later elapsed value and the
ceiling of the raw bracket duration as uncertainty. Refuse invalid/overflowing
conversions, elapsed reversal, UTC outside `0..=253402300799999`, or uncertainty
greater than 30,000 ms. Initialization requires a valid recovered floor and
checked `wall + 30000 >= floor`; failure creates no usable clock. Against the
fixed initial baseline, require:

```text
abs((wall - initial_wall) - (elapsed - initial_elapsed))
    <= 60000 + initial_uncertainty + current_uncertainty + 2 ms
```

Use signed wide arithmetic after validating inputs. Equality passes. The skew
terms cover opposite trusted UTC errors; uncertainty covers sampling brackets;
2 ms covers integer conversions. Never advance the initial baseline on each
sample: that would let repeated small observations hide a frozen wall clock.
Any wall decrease from the last accepted value latches rollback, even a small
legitimate correction. Other read/range/elapsed/anomaly failures latch their first
static refusal. No automatic reset, rebaseline or retry service is supplied.

Enrollment samples immediately before its redemption proposal, after the first
fresh guard before signing, and after the second fresh guard before credential
release. Checked rollback maps to existing `ClockRollback`; other failures map to
`Unavailable`. Preserve both existing expiry checks, fresh guards and the
post-sample-versus-pre-sample rollback guard for arbitrary checked implementations.
Anchor-query failures map to `Unavailable`; malformed input still refuses before
sampling. It remains an advisory queued local read, not an issuance reservation.

Keep the 900,000 ms code TTL, skew, replay, pruning, proof lifetime, capacity and
budgets unchanged. A request queued before expiry may consume replicated state
later while fresh signing refuses it; no successful late join is inferred and no
new atomic no-late-consumption guarantee is added. Controlled-clock tests do not
close the real fifteen-minute acceptance gap.

Suspend accounting differs by platform. If elapsed time excludes suspension,
correct wall advancement can conservatively latch refusal. If both sources hide
an interval, this detector cannot prove or repair it. Restart establishes a new
instance only under the same UTC premise and actual recovered-floor check; it
does not independently requalify the clock. Startup, recovery-floor acquisition,
Mint/Found/Remove callers, TLS time and supported-host qualification remain
separate integration work. This module does not activate live enrollment.

Validation must cover deterministic floor/range/arithmetic boundaries, the fixed
baseline, skew endpoints, exact envelope and +1 ms, rollback/frozen/forward time,
elapsed reversal, uncertainty, first-error latches, contention then recovery and
poison. Issuer tests must cover refusal at all three observations using existing
seams, without weakening existing expiry, replay or freshness checks. Local
execution is restricted to pure clock tests; engine-backed behavior is compiled
locally and awaits ordinary required hosted gates.

Independent design reviews settled the conditional model and the one explicit
founder-fixture compatibility adaptation. Exact implementation still needs its
own independent review. PR182 remains HOLD/NOFIX; engine Sol3/3 exhausted and
historical Opus0/2 are unchanged. No founder diagnostic/fix, timeout, quarantine,
CI policy or runtime authority follows. M2 OPEN / M3 UNCHECKED;
unexercised paths remain **unverified on real hardware**.
