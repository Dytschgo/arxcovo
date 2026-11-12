# Stage D test correction

OpenAI / gpt-6.1-sol / medium coordinator, 2026-11-12. Base 8afbcbc retains
the Opus review 85da90f of target 3b95a1b / source d3615be.
Only unix_bridge/tests.rs changes under crates; production remains unchanged.

- T1 uses parse_inventory and unwrap for every invalid build-name/GID case,
  so every case reaches derive and cannot silently skip its assertion.
- T3 builds its expected RunSpec independently of the production spec helper
  and requires Completed/Discovered after the bridge's catch. A failed fake
  assertion can no longer disappear into a discarded Failed report.
- T8 records whether the fake saw the marker and checks it outside the catch.
- T10 checks that the numeric identity marker is absent from combined output.

The first T3 correction added the report assertion alone. Its timeout mutant
still passed (6053c69d-e4b8-4b06-83e8-8d8e2e04b9ac), because the expectation
used the same mutated production helper. This survivor earns no kill credit.
The final independent expectation closes that second masking issue.

Final compiled assertion kills, each with exact production bytes restored in
finally and checked for equality:

| Mutant | Run |
| --- | --- |
| Timeout plus one | 7d6ee0de-1ede-42f2-88b5-fe71d0feb5c5 |
| Output limit plus one | 677d1b8f-e001-4406-84ff-1d7ea021f4b4 |
| Remove GID-zero refusal | 93941ae5-db84-4b71-95a5-059dd61ab4da |
| Remove account length/character refusals | ccfcfca9-5f6f-40d1-b6fd-0f66a05a4f3f |

Final fmt and locked workspace/all-target Clippy passed. Focused bridge and
Stage B tests passed 24/24, run 4dffa45b-e63d-4bf6-ae4c-da23d3a00c06.
No full workspace repeat for this test-only correction. The builder's 323
passed / 3 skipped remains credited to d3615be, not a new coordinator run.
All commands ran in the canonical coordinator checkout; no child wrote it.

No actual runner, resolver, native owner/service/account, fixed system path,
VM/Lab Manager or experiment was exercised. Unix compilation remains pending;
this uses pure/fake bridge paths and the existing purpose-built panic child.
Stage C remains an explicit unmerged dependency. Production stays inactive.
CI investigations remain parked under the user's product-first direction.
Native, installed and dedicated paths remain **unverified on real hardware**;
M2 incomplete. ExecutionFailed/E_PROTECTION remain unknown without waiver or
retry. Fresh independent test-delta review remains required.
