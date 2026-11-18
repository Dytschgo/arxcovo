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
