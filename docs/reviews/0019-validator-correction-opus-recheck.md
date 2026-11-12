# ADR 0019 validator correction recheck

Coordinator-retained result from Anthropic / claude-opus-5-5 / high, 2026-11-12.
Target `eaf1bc062a09ec022ca9fe7c27187bc7ec78ab8e`, compared with
`b68a73eb1df52813e83eaff29fcc786109139ab9`. The reviewer returned report text,
made no file changes, and reviewed Git objects from an isolated checkout.
This is an attributed condensed record, not a new coordinator review.

Standards PASS; Spec PASS. B1 is closed: T3 names validate_phased_spec;
L2b invokes legacy Runner::run and independently exceeds the aggregate item
and byte limits, requiring exact Invalid("argv bounds"). The wrong-entry-point
validator mutation must compile and fail that assertion. Direct L2 remains.
Non-root Unix identity refusal precedes cgroup/spawn; root Unix skips this
entry-point mutation. Neither case authorizes native execution.

The unchanged C1-C3, D1-D7 and activation gates retain their earlier review
credit. Opus independently reviews only the Sol correction here, not its own
architecture. Pinned Rust 1.95 facts remain credited to the Sol reviewer;
there is no guarantee against dependency, implicit, aborting or real-log leaks.

Publication required a refresh of stale PROGRESS on current main; that is done
in this integration. The six imported contract/review files remain byte-equal
to eaf1bc0. Exact published-head review and all required CI remain mandatory.
The contract remains proposed until its publication disposition.

Non-blocking precision advisories: non-Unix refusal surfaces as legacy
Unavailable, not a public Unsupported error; marker wording could be clearer;
validator signatures must remain compatible for the mutation to compile;
L2b should use other fields from valid spec(); the old P4 Rust-version note is
superseded by appended pinned-source evidence. No source behavior changed.

Reviewer checks: Git/text, UTF-8/LF/whitespace and relative links only. No
build, behavior test, mutation, native operation or GitHub write for this target.
Native, installed and dedicated paths remain **unverified on real hardware**;
M2 is incomplete and ExecutionFailed remains unresolved.
