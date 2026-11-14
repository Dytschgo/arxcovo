# A5 source preparation evidence

2026-11-14; OpenAI / gpt-6.1-sol / high; implementation sub-agent.
Base `fd7fe94`. Read AGENTS/KICKOFF, engineering safeguards, accepted ADR/brief
0020, corrected plan `2aec267`, prior review `6cbc7e6` and recheck `dbd8447`.
This commit implements A5 source preparation only; fresh Muse review required.

`LinuxFs::seed_a5` is cfg(test), inherent, with private descriptor access.
Read-only preflight copies S0–S4 and unit/supervisor checks before either
mkdir. It requires newly Created runs, trusted root-owned cgroup2 0700,
then exclusive descriptor-relative mkdirat of literal canonical `arx-0-0`.
Both handles stay held through setup/postconditions. Empty procs/events and
bounded directory classification reject population, nesting and unsafe entries.
No raw fd export or port change. The separate ignored fixture repeats A1/A2's
four environment gates and real/effective root checks; unchanged Standard
package is a precondition. Its sole setup call is inside `a5::finish`, requiring
one removal, runs_created=false, both controller writes, fresh S9 and literal
leaf absence before READY and the existing identity hold. Historical A1/A2
assertions are unchanged.

## Executed feedback (Windows, offline)

Command template (each filter individually, no ignored selection):
`cargo test -p arxd --lib <filter> --locked --offline -- --exact`.
Each of these 15 filters ran exactly one test and passed on the final source:

```text
exec::hierarchy::fake::a5::empty_orphan_removed_before_acceptance
exec::hierarchy::fake::a5::preflight_refuses_before_mutation
exec::hierarchy::fake::a5::seed_observations_refuse_population_nested_and_foreign
exec::hierarchy::fake::a5::populated_and_empty_ordering
exec::hierarchy::fake::a5::refusal_never_accepts_residue_or_wrong_evidence
service::unix_bridge::tests::e6::a5_pending_through_janitor_and_s9
exec::hierarchy::fake::h4_deadline
exec::hierarchy::fake::h6_all_names_before_removal
exec::hierarchy::fake::h18_verify_fresh
exec::hierarchy::fake::h19_verify_populated
exec::hierarchy::fake::h20_postconditions_fresh
exec::hierarchy::fake::a1a2_production_setup_before_acceptance
exec::hierarchy::fake::a1a2_refusals_never_release_acceptance
exec::hierarchy::fake::a1a2_cancel_timeout_and_postcondition_refuse
service::unix_bridge::tests::e7::w8_production_and_domain_source_pins
```

Callgraph inspected before selection: hierarchy tests use Fake/Manual and pure
parsers/setup only. The readiness test uses existing StartupDomain, synthetic
config/FakeResolver, fake Port, and Activity in its owned test temp directory.
It checks Pending and zero runner calls at every setup observation, explicitly
observes kill/events/removal/S9, and admits one fake call only after success.
Timeout and shutdown during janitor events polling refuse with zero calls.
No selected path calls LinuxFs, real Runner, native probes, subprocess fixtures,
service/account tools or networking.

Red before green: the new empty-orphan test initially called unchanged
prepare_a1a2; it failed its acceptance assertion (0 passed/1 failed). Separate
A5 evidence expectations made it pass. Six focused, temporary test-only mutants
each compiled, failed an assertion, restored byte-for-byte and passed afterward:

| Mutant | Assertion-sensitive exact filter suffix |
| --- | --- |
| Fake omits kill effect | a5::populated_and_empty_ordering |
| Fake completes kill without wait | a5::populated_and_empty_ordering |
| Fake omits removal effect | a5::empty_orphan_removed_before_acceptance |
| Fake stale listing hides residue after removal | h18_verify_fresh |
| A5 accepts zero removals | a5::refusal_never_accepts_residue_or_wrong_evidence |
| Test setup publishes readiness early | e6::a5_pending_through_janitor_and_s9 |

Credit is assertion sensitivity of these test-only faults, not production-line
mutation coverage or hardware evidence. No production setup mutation was made.
After tightening explicit phase/shutdown observations, the readiness mutant was
repeated: Pending assertion failed, exact bytes restored, same filter passed.
No retained mutation harness. `cargo fmt --all --check`,
`cargo clippy -p arxd --all-targets --locked --offline -- -D warnings`, and
`git diff --check` passed.

## Limits and remaining work

`cargo check -p arxd --tests --target x86_64-unknown-linux-gnu --locked --offline`
failed in libsqlite3-sys: missing `x86_64-linux-gnu-gcc`. Linux-only additions
were therefore not compiled here. No compiler installation or native run tried.
The ignored A5 fixture remains unselected and **unverified on real hardware**.
Driver selection remains A3 None; closed A5 mapping/profile/summary/hold/cleanup
driver tests are deferred with that separate slice, without arbitrary inputs.
No production setup/parser/trait/behavior, package, Cargo, workflow, ADR or
PROGRESS changes; no native, owner, service/account, network, GitHub, CI,
dispatch or sub-agent action. D7c is consumed and grants no execution authority.
A4 remains open and independent, unimplemented. M2 A4/A5, remaining F/G and
real-Runner PA6 stay open, **unverified on real hardware**.
