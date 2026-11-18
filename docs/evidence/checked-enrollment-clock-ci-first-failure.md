# PR184 first CI: missing mandatory test inventory

Head `1cd91ef62cf74945b18f4c56efbfc7c613474760`, normal PR CI
[44368904104](https://github.com/Dytschgo/arxcovo/actions/runs/44368904104),
attempt 1. Independent Sol 6.1/high review found one P1 integration omission:
the new anchor refusal test was absent from the exact required-test inventory.
Standards passed; no additional material clock algorithm finding was identified.
No success attestation was posted.

| Job | Observed conclusion | Duration | Execution evidence |
| --- | --- | ---: | --- |
| Windows 135030022723 | Failure | 52 s | Inventory guard: 9 tests, 1 failure; later Rust validation skipped |
| Ubuntu 135030023154 | Failure | 22 s | Same guard failure; later Rust validation skipped |
| macOS 135030023302 | Queued at metadata read | Unmeasured | No execution credit |

The guard rejected an unlisted `tokio::test` in
`OwnedQueryTests.test_query_required_inventory_and_helpers`, through
`assert_owned_query` and `assert_fixture_quarantine`. This is not a clock runtime,
founder, engine or timeout failure. No hosted Rust, Node or smoke pass is inferred.
The macOS row is an observation, not a later terminal outcome.

Bounded per-job logs are retained under
`D:/Temp/arx-pr184-inventory-correction/`, each with an 8 MiB cap and 45-second
download deadline. Both downloads completed without truncation:

- Ubuntu: 42,610 bytes, SHA256
  `61e48e6329bc6e9795bf2ae1403483f37a2284c530a21fd53a58e681499f0188`.
- Windows: 40,818 bytes, SHA256
  `b22e2840214c377b3f85fa0eded5874c1554a939e1f85cb21df183288b54e1da`.

`first-run.json`, `first-jobs.json` and `log-manifest.json` pin the attempt,
head, job metadata and logs. The independent report is at
`D:/Temp/arx-pr184-independent-review/review.md` with its separate pin proof.

Coordinator Sol 6.1/high reproduced the same failure once with the normal pure
guard command before editing. Nine tests ran, one failed (0.729 s harness).
The correction adds only
`checked_clock_refusal_has_no_submission_and_malformed_has_no_sample` to
`QUERY_LOCAL_REQUIRED` and updates its exact count from four to five. Existing
entries remain mandatory; the guard's per-entry rename/ignore/platform-filter
negative checks automatically include the new case. The full nine-test guard
then passed, 0.133 s harness / 0.324 s command. Logs and results preserve both runs.

No Rust source changed after the previously compiled and reviewed candidate.
No test was suppressed, no workflow changed and no quarantine annotation, timeout,
capacity, founder algorithm or assertion was changed. The guard still rejects
unlisted tests and hidden/ignored required cases. No rerun or dispatch was used.
The corrected source needs independent exact-head recheck and its own normal
required CI; local guard success is not hosted acceptance.

PR182 HOLD/NOFIX; engine Sol3/3 exhausted and historical Opus0/2 unchanged. This
mechanical inventory correction does not grant another founder/engine repair or
runtime diagnostic. M2 OPEN / M3 UNCHECKED; unexercised paths remain
**unverified on real hardware**.
