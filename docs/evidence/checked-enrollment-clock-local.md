# Checked enrollment clock: local evidence

2026-11-18; Sol 6.1/high builder, coordinator integration. Implements the bounded
prerequisite in [ADR 0030](../adr/0030-checked-enrollment-clock.md). Independent
exact-source review and hosted acceptance are pending.

The private clock module adds fallible observations, conservative local anomaly
refusal and explicit recovered-floor checks. Enrollment samples at its existing
three boundaries; anchor queries refuse clock errors before submission. Founder
production code and its legacy clock remain unchanged. Its private logical test
clock receives only the separately reviewed explicit trait implementation.

| Executed locally | Result | Command duration |
| --- | --- | ---: |
| Workspace/all-target locked offline Clippy, warnings denied | PASS | 16.196 s |
| Workspace/all-target locked offline tests, `--no-run` | Compile PASS | 38.456 s |
| Isolated `checked_clock::tests::` | 14 PASS, 0 fail, 0 ignored | 0.766 s; harness 0.01 s |
| Format check | PASS | 0.735 s |

The pure tests cover recovered-floor/range/conversion boundaries, bracket
rounding/ceiling, fixed-baseline progress, trusted skew endpoints, exact envelope
and +1 ms, frozen/forward/rollback time, elapsed reversal, first-error latches,
poison and same-thread contention with no sampling or mutation. They start no
helper thread and do not observe real UTC or run an engine.

The issuer's existing test retains its original six cases and adds 15 subcases:
five static errors at each sampling position. They assert proposal/sign/release
boundaries, error mapping, admission return and consumed replay. A new anchor
case checks malformed input without sampling and valid-query failure without
submission, admission or store retention. These integration paths are locally
**UNRUN**. Existing quarantine annotations and fixture budgets are unchanged.

Coordinator source inspection found that the 15 new grants could exceed the
unchanged 16-member registry limit. Before any engine execution, the test was
corrected to remove and verify removal of only each newly consumed synthetic
member after its assertions. This changes test cleanup only. The builder's
pre-correction source/log snapshots remain preserved.

After that correction, format check passed in 0.748 s, all-target Clippy passed
in 4.741 s and all-target `--no-run` compilation passed in 18.186 s. The pure
module and its tests are hash-identical to the 14-pass source; those tests were
not repeated. No compiler or test failure occurred. The capacity issue was found
by source review, not runtime evidence.

Local files under `D:/Temp/arx-checked-clock-build/` retain the original source
and artifact manifests, bounded logs, per-command environment/results and static
proof. `D:/Temp/arx-v1-continuation/final-source-proof.json` pins all ten final
Rust files, identifies the one test file changed after the builder checks, and
links the post-correction results. It also verifies the exact founder-fixture
addition and unchanged quarantine/budget counts.

All scratch, Cargo target/home and tool caches were explicitly D:. C: stayed
above the 20 GB build threshold. Test compilation rebuilt the shared target's
ordinary binary to SHA256
`df79751b36b5465ff3098212ec02818edacb919ce6f41919699b59c101c0b862`;
that artifact has not been observed or accepted. The separate owner preview
copy remains the reviewed/observed `f8893673...6224b48` binary; its launcher and
authenticated Claude Code acceptance remain UNRUN. Do not apply the earlier
runtime evidence to the rebuilt target binary.

No local issuer, anchor, founder, engine, daemon/native, network, VM or container
runtime was executed. No startup clock integration, healthy UTC, real fifteen-
minute expiry, live transport, or full v1 acceptance is claimed. PR182 HOLD/NOFIX,
engine Sol3/3 exhausted and historical Opus0/2 remain unchanged. No founder repair,
timeout, quarantine, policy or capacity adjustment. M2 OPEN / M3 UNCHECKED;
unexercised paths are **unverified on real hardware**.
