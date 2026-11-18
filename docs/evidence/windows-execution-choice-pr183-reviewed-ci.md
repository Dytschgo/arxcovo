# PR183 reviewed-source first CI: partial matrix checkpoint

Exact head29a3317f4fa2e7b12783b4906053f3f7a6663a89,
[CI44349045277](https://github.com/Dytschgo/arxcovo/actions/runs/44349045277)
attempt1. Fresh independent Sol6.1/medium source review gives Standards/Spec
PASS0material; exact-head agent-review success and
[attestation](https://github.com/Dytschgo/arxcovo/pull/183#issuecomment-6984948836).
The review verified19source/9artifact identities, closed the golden/coverage
findings and inspected unchanged production bodies. This is not hosted acceptance.

On the subsequent independent M3 design-review completion, one job read found:

| Job | Result | Job seconds | Rust PASS | Skip | Test body seconds |
| --- | --- | ---: | ---: | ---: | ---: |
| Ubuntu134959287041 | SUCCESS | 291 | 680 | 22 | 51.365 |
| macOS134959287697 | SUCCESS | 464 | 648 | 8 | 72.620 |
| Windows134959286418 | RUNNING | pending | pending | pending | pending |

Both completed jobs passed all Rust cases selected by existing policy, all48Node
tests, UI unit checks, browser smoke and Electron smoke. Both executed the complete
ten-case activity_v2 integration family and the corrected inventory corruption
case. Earlier local ACL failures remain recorded; hosted passes do not imply that
the local setup or optional dedicated backend was repaired. Windows evidence,
successful feature<=660s gate, full matrix, fresh exact-main health and merge
remain pending. No native/VM/hardware acceptance or stability credit.

Full bounded logs/metadata/distinct test names retained in
`D:/Temp/arx-pr183-29a-first-ci`,8MiB cap/45s deadline, exit0/no truncation:

- Ubuntu255457bytes SHA256
  `eab7830748cf549dfb07c45b646f6ee35bc6316d03b7bd715e8541113f22e7e1`.
- macOS235507bytes SHA256
  `1618a65ac020635ee5562444827eca64fc2fe86ada2b5c1940251db7b5a298a3`.

Coordinator Sol6.1/high retained and inspected these execution artifacts; it did
not rerun tests or dispatch a workflow. Prior bfd/f5/d30 failures and local fixture
correction failures remain preserved. No source, timeout, capacity, quarantine,
runner, engine or founder repair. M2 OPEN / M3 UNCHECKED.

## Terminal CI and merge

The subsequent merge-readiness check found all five required gates SUCCESS at
exact29a. CI44349045277 attempt1 completed SUCCESS in594s workflow wall.
Windows134959286418 SUCCESS589s (feature gate<=660s):694RustPASS/36skip,
184.200s test body,48NodePASS, UI unit/browser/Electron smokePASS. No Rust cases
were unrun under the existing selection. All10activity_v2 cases and corrected
inventory corruption test passed on Windows too. All three distinct case lists
and terminal metadata retained at the same D: root.

Windows bounded log284747bytes SHA256
`6fab35257445865d19fb8b5859f3a913620f69c509bd4533e862ce26c89ee3e5`,
exit0/no truncation. Original partial snapshots remain unchanged.

The optional Windows diagnostic sample was4PASS/4FAIL: T1/R6c/R6s/R6p PASS,
T2/T3/T4/U6 FAIL. U6 Stop/Orphaned report owners_left49, owners_waited55,
engineJoined; one diagnostic row was truncated (2362bytes). Raw bounded output
and structured summary preserved. No stability, re-enable, root-cause or fix
credit. Required checks remain green under existing policy; no quarantine,
deadline or diagnostic change was made.

Immediately before merge, coordinator verified live main exactlycd890750 with
CI44304025778 attempt1 completedSUCCESS, then rechecked PR head/base/CLEAN and
all five green gates. Proof timestamp2026-11-18T16:48:36.7070154Z in
`immediate-premerge-proof.json`. No stale-main shortcut or protection bypass.

PR183 squash-merged as08c4b999c23f9b516b3a942039ebc3c3f1a6dd04 with exact-head
guard29a. GitHub merge result and merged PR state verified; merged crate source
equals reviewed29a. Existing source branch/unpublished docs checkpoint0ffafc39
preserved locally. PR183 unwatch completed; no merged-PR polling/watch remains.

Resulting exact-main normal push CI44351538977 attempt1 is RUNNING at the first
read, all runners acquired: Windows134968209866,Ubuntu134968210365,
macOS134968210476. Metadata at `D:/Temp/arx-pr183-merge-main`. No main-green,
M2 acceptance, owner-launcher run or fullv1 completion claim until actual evidence.
Existing main duration history retains the prior verified626s/624charged entry;
no pending result is inserted as green. No rerun/dispatch.
