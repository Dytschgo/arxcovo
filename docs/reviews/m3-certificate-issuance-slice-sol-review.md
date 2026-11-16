# Sol review: certificate issuance first-slice contract

Target: Opus29ea8a4 retainedb4e29b7, on main6e9edd5. Reviewed2026-11-16 against the actual consensus read path and cached OpenRaft0.10.0 source. No build or test ran for this design review.

Standards: PASS on scope and three-PR separation. No dependency or implementation is approved by this review yet.

Spec: CONDITIONAL; corrections required before issuance implementation.

- C1 (blocking): decision4 gives the ephemeral CA a one-day lifetime while decision5 requires a30-day leaf and refuses leaves past the CA expiry. Every happy-path signing attempt necessarily returns CaExpiring. Set a test-CA lifetime that contains the specified leaf interval, keep the leaf30-day bound and retain a separate deliberately near-expiry CA negative control. This is test lifetime arithmetic, not production CA policy.
- C2 (required limit): decision1 says a dropped linearizer request stays in RaftCore for at most READ_BUDGET, but its own verified facts say the wait timeout starts only after dequeue. An outer deadline bounds caller waiting, not queue residence or non-abortable spawn_blocking closure completion. Remove that unsupported guarantee, specify ownership/draining consequences for timed-out store reads, and require the adapter not to queue repeated orphaned work without a bound. The read result must remain unavailable after deadline, even if work later completes. Distinguish deadline responsiveness from hard task termination.
- C3 (required precision): decision5 claims an isolated verifier stops accepting within at most30 days while explicitly assuming clocks can differ by30 seconds and the leaf is backdated60 seconds. State the derivation and exact assumption-bound interval, or use the conservative bound30 days plus the clock-skew allowance. This test-only lifetime decision does not establish production revocation freshness or renewal.

Security: no source finding; conditional on the above and future independent implementation review. CSR algorithm-pair/original-SPKI checks improve the settled handoff. The private issue ticket accurately narrows trusted code rather than claiming unforgeable commit evidence. Keep fresh pre/post issuance time checks at actual guard boundaries, not a timestamp cached before queue waits; no private key bytes in logs/state/evidence.

A0 may proceed only after C2 wording and ownership are settled. A/B remain pending the revised brief. M2 remains incomplete; A7 final NO-GO and A12 no-retry are unchanged. All hardware paths remain **unverified on real hardware**.
