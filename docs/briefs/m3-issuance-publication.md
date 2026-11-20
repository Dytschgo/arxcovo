# M3 step 4a: checked publication storage

Provider: GPT-6.1 Sol / high. Base: be197a25928d70fd48fe4658d297393f95680d1c
(last merged PR198). Coordinator reviews/merges separately. No PROGRESS edit by
this builder. ADR0038 records adopted D2. This slice provides storage consistency
only, never issuance authority. Synthetic crypto fixtures are not Found evidence.

Exact tests assigned before execution (prefix cluster::issuance_publication::tests):
- files::publication_linux_real_file_roundtrip (Linux)
- files::publication_durable_boundaries (Linux; 24 adapters x error/unwind)
- files::publication_retained_artifact_must_match (Linux)
- files::publication_restart_partial_and_missing_history (Linux)
- files::publication_wrong_context_and_origin (Linux)
- files::publication_original_owner_collision_and_unsafe_objects (Linux)
- publication_non_linux_refuses_without_io (non-Linux)
- files::publication_lock_probe (Linux independent-process helper)

Affected selections: cluster::startup_plan::tests, cluster::join_receipt::tests,
cluster::pki::. No local consensus suite or Engine experiment. Linux-gated code
compiles as part of hosted Ubuntu all-target checks; actual Linux test execution
and filesystem qualification remain pending until hosted evidence is inspected.
This Windows host cannot compile/run that code. Non-Linux refusal runs locally.

Adapter faults refuse create, second write, sync, rename, recheck/readback/delta at
their concrete adapters, or unwind after the actual operation. Partial-write
errors retain an actual written prefix. They model unknown effects, not failed physical
flushes or power-loss persistence. No deletion or rollback follows. Scope and
acceptance gaps must be independently assessed; no CI-fit claim before measuring
the first normal hosted run under ADR0031. Hosted evidence and costs pending.

The implementation adds no production callers. Original-owner state and retained
guards prevent wrapper recreation from resetting an attempt. Generic recovery
inventory alone is not receipt/key validation or trusted history. Later integration
must use the actual retained job and independently trusted expected identity.

Remaining: 4b, 4c, 4a-W, trusted restart-history source, custody 3b and full retained
restart/operational ownership. M3 unchecked; **unverified on real hardware**.

Recovery tests use real zero-length state inventory fixtures, not valid SQLite
stores or Engine startup. Key/receipt validation is mechanism evidence only.
Local static gate passed with 28 vendor members/four patched hashes and 33
negative controls. One initial static invocation omitted metadata; corrected.
A second verification round found the new read fixture missing required state
inventory; fixed without changing generic acquisition. Three failed rounds used; builder stopped.
Local final counts/durations and hosted gaps are recorded in the task report.

STOP: final receipt selection failed at
cluster::join_receipt::tests::non_authorizing_assembly_preserves_v1_and_bounds:
whole-record equality compares assembler artifact member-key against the existing
fixture artifact sealed-key. No fix was attempted after this third failed round.
Static gate/fmt/Clippy passed; publication 1/1 and startup 5/5 passed; receipt 3/4
passed. PKI 38 listed but not run. Linux six test entries remain uncompiled and
unexecuted locally. Branch is WIP, not acceptance-ready. Coordinator must route
the evidence to a fresh independent Sol reviewer; this does not grant a fourth
builder attempt or reset any historical counters.

## Scoped stuck-review follow-up (2026-11-20)

The owner authorized a separate Sol 6.1/high builder with a fresh three-failure
budget for ST1/ST2/SP1/SP2 from the stuck review. The original builder remains
3/3 exhausted; Engine Sol 3/3 and historical Opus 0/2 are unchanged.

ST1 now adjusts only the assembly test's expected artifact to member-key after
asserting the shared fixture remains sealed-key. ST2 gates both facade layers'
member_copy imports to Linux tests, retaining production MemberCopy visibility.
SP1 requires member-key in retained fixed-layout validation; the new completed,
reopened test changes only the artifact field with healthy controls.

SP2 adds isolated role/anchor edits with valid-role controls, exact per-boundary
trace/inventory/byte-prefix/descriptor identity witnesses and original guard/state
controls, explicit reopen outcomes and full retained validation of complete
tuples, root/ancestor substitution before and after reservation, and an independent
process release probe after owner drop. These Linux oracles are source-reviewed,
not locally compiled or executed.

This follow-up passed Windows static gate (28 vendor members/four patched hashes,
33 negative controls), fmt, locked/offline all-target check and Clippy -D warnings,
and 48 affected tests: publication 1, PKI 38, receipt 4, startup plan 5. No failed
verification rounds consumed the new budget. The earlier STOP above records the
original builder's historical outcome, not an additional attempt by that builder.
Seven Linux publication test entries now await qualified compile/runtime evidence.
Exact commands and counts are in D:/Temp/arx-m3-step4a-2/report.md.
Independent exact-head review, hosted checks/costs and all prior acceptance gaps
remain pending. Step 4 and M3 remain unchecked.
