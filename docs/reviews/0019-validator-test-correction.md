# Unix bridge validator test-contract correction

OpenAI / gpt-6.1-sol / medium coordinator, 2026-11-12. Direct correction of
independent Sol review B1; no source implementation or behavior test ran.

T3 now names validate_phased_spec. New L2b calls the real legacy Runner::run
with both an item-overflow prefix/tail and a byte-overflow prefix/tail within
the item limit. Both require exact Invalid("argv bounds"), so changing only
the legacy entry point's validator fails an assertion. Direct L2 tests stay.

The mutation is safe only under ordinary non-root Unix identity (or on
non-Unix): the wrong validator reaches identity/unsupported refusal before
cgroup or spawning. Root Unix skips this entry-point mutation as L1 does;
no test is allowed to reach privileged native work. This adds no constructor,
runtime selector, authority or product approval flow.

Pinned Rust1.95 evidence is credited to the independent reviewer, including
the broader string-slicing excerpt risk. No new absence guarantee follows.
The proposal stays proposed pending an Opus recheck of this Sol correction.
S1/S2 owner CLI work is separate; no bridge implementation or retry begins.
