# PR183 first hosted observation

Exact head `bfd8aeb48b48fe354d58b927db1bd000f9978497`, CI
[44334571677](https://github.com/Dytschgo/arxcovo/actions/runs/44334571677),
attempt 1: **FAILURE**. No rerun or dispatch. Main remains the separately verified
`cd890750` baseline; this is a failed PR run, not a main health result.

| Job | Result | Recorded interval | Execution evidence |
| --- | --- | ---: | --- |
| Ubuntu 134907640337 | failure | 66 s | all-target Clippy rejected unused test-only Hold::new/release and hold_fixture; Rust/Node/browser/Electron tests unrun |
| Windows 134907640414 | failure | 275 s | nextest 412 passed, 1 failed, 36 skipped, 279 unrun; test body 66.258 s; later Node/smoke steps unrun |
| macOS 134907639729 | cancelled | 902 s lifecycle interval | runner ID 0, empty runner name and no steps; no test execution |

The Windows failure is the unchanged service response-only reason-table test at
application/tests.rs:419: expected activity user_version 3, observed 4 after this
feature's schema upgrade. All three new SDK shutdown regressions passed on hosted
Windows, as did 28 of the 31 focused cases. The three store-focused cases were
unrun after fail-fast. The real-Job dropped-before-first-poll and real execution
bridge/SDK tests also passed on this hosted observation; their policy fixtures do
not establish production token qualification or hardware acceptance.

GitHub's macOS annotations state that no hosted runner acquired the job, with
macOS arm64 capacity notices. Its job-log request returned BlobNotFound (215-byte
error document), not a job log. No macOS runtime duration or passing test count
can be inferred from the 902-second queue/lifecycle interval. No runner labels,
workflow settings, timeouts or capacity policy were changed to address it.

Bounded raw Ubuntu/Windows logs (78,055 / 174,014 bytes) and their SHA256 values,
run/job/check metadata, macOS annotations and parsed exact Windows case names
are retained at `D:/Temp/arx-pr183-first-ci`. Fetches were capped at 8 MiB per job
and 45 seconds; no output was truncated. `log-manifest.json` distinguishes the
unavailable macOS log from successful downloads. No source failure is called
pre-existing or intermittent without base evidence.

The two demonstrated integration corrections are deliberately narrow:

- Compile Hold constructors/release and hold_fixture only on Windows, where the
  real pipe tests use them. Keep the shared test checkpoint/type and production
  admission code unchanged; do not suppress dead-code warnings.
- Expect schema 4 in the response-only test, retaining every behavior/privacy/
  column assertion. Do not change the schema or fixture permissions.

A related-assertion search found the same stale expected 3 after successful
hot-journal reopen in store/crash_tests.rs. It is also updated to 4, with recovery,
journal, privacy and integrity assertions unchanged. That native child/recovery
case is UNRUN locally and must execute in the normal hosted suite. The remaining
expected 3 in damaged-v3 rollback is intentional and retained.

Coordinator Sol 6.1/high applied those test-only corrections. Local focused
response-table plus three SDK regressions: **4 PASS** (22.636 s command / 7.87 s
body). Windows all-target workspace Clippy with -D warnings PASS (6.078 s), fmt
and diff checks PASS. Linux/macOS validation is pending hosted CI, not claimed
from Windows Clippy. Evidence is `D:/Temp/arx-pr183-ci-integration`.

No production function body changes. The existing normal binary remains SHA256
`f889367307c8561cd813b115b6a6fb908631bf736f233f2d2ca8f7f7d6224b48`; it was not
rebuilt or re-observed for these test-only corrections. The earlier reviewed v2
production observation remains evidence for its unchanged production behavior,
not a new binary execution claim.

Fresh independent review and all required checks must cover the corrected head.
A genuine source update may trigger its normal first CI; that does not turn the
original failed run green or repair the macOS infrastructure. M2 OPEN / M3
UNCHECKED; exhausted engine counters, PR182 HOLD/NOFIX and host/data/admin fences
remain unchanged. The ten original ACL-setup integration failures remain locally
unrun after their recorded failure; no fixture bypass or new native attempt.
