# PR183 schema-family follow-up

Normal CI44340307270, attempt1, exact `f5cee7b6ef9dde1345435be7046346fc06115387`:
**FAILURE**, all jobs terminal, 555 s workflow wall. No rerun, dispatch or manual
cancellation. All three platform failures are the same test's stale schema
expectation, not an observed production drain or engine defect.

| Platform/job | Job interval | Rust result | Test-body time | Unrun |
| --- | ---: | --- | ---: | ---: |
| Ubuntu134928130147 | 182 s | 434 passed,1 failed,22 skipped | 29.720 s | 243/678 |
| macOS134928130621 | 250 s | 412 passed,1 failed,8 skipped | 48.681 s | 233/646 |
| Windows134928130789 | 431 s | 424 passed (1 slow),1 failed,36 skipped | 164.357 s | 267/692 |

All fail `service::executor::tests::late_dispatch_after_drain_finishes_without_worker_or_poll`
at its schema assertion: expected3,actual4. Unlike the first bfd run, macOS did
acquire a runner on this observation. The earlier capacity cancellation is still
preserved; this observation does not establish capacity stability. Later
inventory binaries, Node and browser/Electron stages did not execute.

All three bounded logs and metadata/parsed exact case names are retained in
`D:/Temp/arx-pr183-f5-first-ci`. Ubuntu175547bytes, macOS172573bytes,
Windows176604bytes; SHA256 manifests retained, no truncation. No job was cancelled
to publish a replacement source; the original run was terminal before delivery.

Following the owner's conditional request, Opus5.5/high audited the whole schema
expectation family. Coordinator repeated the case-insensitive source search and
confirmed all33 hit lines: 22 activity-test references,4 production references,
7 unrelated cluster references. Every activity occurrence has its full-context
classification in [the audit table](windows-execution-choice-schema-audit.md).
Indirect metadata/schema helper expectations were also inspected.

Two test lines change:

1. Successful `open_service_fixture` must produce schema4, so the late-dispatch
   test's exact schema equality now expects4. Its worker-count/backend-call,
   shutdown/refusal, privacy and storage-failure assertions are unchanged.
2. `corrupt_identity_fails_closed_without_replacement` must use unsupported
   future version5. Version4 is now supported, so the prior version4 mutation
   no longer represented corruption. Error and byte-for-byte file-preservation
   assertions remain unchanged. Intentional legacy seeds/rollback values stay.

Opus reproduced the executor assertion once before editing (exit101,4vs3), then
ran seven safe store/fake cases: 7PASS,0FAIL (8.833 s command / 0.88 s body).
Windows all-target Clippy -D warnings PASS4.514s, fmt/diff PASS. Coordinator read
the actual before/after logs and verified2source pins and7artifact hashes/sizes.
No production function, schema, migration, identity/admission/drain, backend,
permission, timeout, capacity, SDK or workflow changed. No production binary
rebuild or new runtime observation occurred.

The inventory corruption case is UNRUN locally: its existing ACL setup fails
before behavior on this host. Its stale-input defect is source-proven, not a
recorded local execution. Hot-journal/native crash and the ten earlier ACL cases
also remain locally unrun after their preserved failures. No fixture bypass or
pre-existing classification. All required platform suites must execute these
cases on the corrected head, with fresh independent review and normal gates.

Raw audit/result/source diff/manifests and before/after logs stay at
`D:/Temp/arx-opus-activity-schema-ci-audit`. The original result's prose category
subtotals differ from its row table; the committed audit table's per-row labels
are authoritative (7 already-current checks,3 rollback checks,9 legacy seeds,
1 retained v0 corruption input,2 corrected stale cases). These sum to22 activity
test references. No source or runtime claim depends on the prose arithmetic.

M2 OPEN / M3 UNCHECKED, full v1 incomplete. PR182 HOLD/NOFIX, exhausted engine
Sol3/3, historical Opus0/2 and host/data/admin/babysitter fences remain unchanged.
This bounded test-family correction grants no further engine or native attempt.
