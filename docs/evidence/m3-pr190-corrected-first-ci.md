# PR190 corrected-source first CI: held

Exact head `8216987b4f11e228fc560e85f8f2a8029f8a478d`; normal source-push run
44484921164, attempt 1, completed FAILURE on 2026-11-19. This is the first
execution of the corrected source, with no rerun or dispatch. Prior head14ace
failure and its independent disposition remain preserved separately.

| Worker | Job | Seconds | Observed result |
| --- | --- | ---: | --- |
| Windows Rust | 135425876109 | 411 | 675/800 selected run: 673 pass, 2 fail; 36 skipped, 125 selected unrun; nextest body 58.437s |
| Ubuntu Rust | 135425876244 | 193 | 786 pass, 22 skipped; nextest body 60.583s |
| macOS Rust | 135425876145 | 212 | 754 pass, 8 skipped; nextest body 46.527s |
| Ubuntu UI | 135425875737 | 60 | 60 Node cases, browser smoke and Electron smoke pass |
| Windows UI | 135425876140 | 72 | 60 Node cases, browser smoke and Electron smoke pass |
| macOS UI | 135425876231 | 40 | 60 Node cases, browser smoke and Electron smoke pass |
| Windows verify | 135429085391 | 6 | failed closed |
| Ubuntu verify | 135429085401 | 4 | failed closed |
| macOS verify | 135429085527 | 4 | failed closed |

All ten Windows and nine-per-Unix binding cases passed exactly once in normal
nextest. All three Rust workers ran the eight loader-preflight tests and passed
the inventory verifier with 33 pure negatives. macOS passed its existing
stock-system-directory observation test. Separately, the patched SQLite build
passed the actual compiled host guard. The directory test supplies no
stock/unpatched-library refusal evidence. These observations close
neither the whole native qualification nor the required CI gates.

Windows failures:

- `service::owner_windows::tests::n1_create_new_and_existing_refusal`: line104,
  first `publish(staged).unwrap()`, `Io`.
- `service::owner_windows::tests::n2_exact_hash_replace_and_conflict_cleanup`:
  line153, second publication after the expected hash-conflict and preservation
  assertions, `Io`. This is a later failure site than line132 on old14ace.

No native error code is recorded. Successful earlier statements do not identify
which internal publication operation failed or its final file state. The cause
remains unknown; the fresh independent Astra disposition confirms HOLD. No local
reproduction, native probe, fix or retry was performed. Existing TMP exports were
not changed by the macOS/Node corrections. PR189 founder hold remains separate.

Policy measurement: 425s wall, 420s raw charged, 5s potential idle credit,
`withheld:not-green`. No duration-history append or acceptance credit.
Independent exact-head Sol attestation7003529631/status59343063652 remains valid
within source-review scope; agent-review and ci-time-budget success cannot
substitute for failed required aggregate gates.

All nine logs were fetched once with 8MiB/45s per-job bounds; none truncated.
Retained root: `D:/Temp/arx-pr190-821-first-ci/`, with raw terminal run/jobs,
`log-manifest.json`, `execution-proof.json`, `policy-measurement.json` and
`evidence-manifest.json` (15 pinned inputs, SHA256
`f0ae52f7c1e8c0e1ec2eca9622f672c77853fb05eee5e83ee94f3350ce809ff5`).
Earlier partial snapshots are unchanged. Local extraction parser mistakes were
corrected on the saved logs only; no test reran. C: free was 689313525760 bytes;
all coordinator scratch/cache paths were explicitly D:.

Provider/effort/outcome: Sol6.1/high coordinator, retained first failure;
independent Astra6.2/high disposition HOLD, source findings unchanged. No counter reset or fix grant.
Engine Sol3/3 exhausted; historical Opus0/2 unchanged. Merge, next-store and
operational gates remain held. M2 OPEN / M3 UNCHECKED;
**unverified on real hardware**.

Independent disposition: `D:/Temp/arx-pr190-821-astra-disposition/report.md`
SHA256 `870c476fa80861a718aebd1762bf15ee7075f3ceeb4bfd5657879915b19c533e`.
Coordinator reverified six review refs and all 37 corrected/historical manifest
inputs. The reviewer recorded CI's PR merge object `9193ba05b788dcdb4569745781d86b1cc5449a1b`;
its tree was unavailable locally for comparison. Source boundaries use exact
committed head821; no stronger checkout-equivalence claim follows. The later N2
boundary proves its preceding assertions passed once, without cause, repeatability
or final replacement-state evidence. No execution or fix authority follows.
