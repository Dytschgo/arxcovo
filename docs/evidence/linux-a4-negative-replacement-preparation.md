# Linux A4 negative replacement preparation

2026-11-15. Implementation sub-agent: OpenAI gpt-6.1-sol, high.
Docs only; D7g is proposed, with **no run authority yet**. Native omitted-kill
remains **UNRUN**. Root review and independent Muse exact-head review remain
pending, followed by five green required gates and coordinator merge.

- Actual main baseline: `994760c98c2816f37b1acdcaa9d9827693765b17` (PR #114).
- Baseline tree: `908abcf01c0969cb73d3486e92476f3c90c132f5`, locally verified
  identical to reviewed `d60665c81f4e83c10382fea170244ee04db98bf1`.
- PR114 five-green/Muse PASS are supplied coordinator evidence, not queried here.
- Checkout start: `0ff13465d665bc51de6a0925067a8bdc9a1b3896`. Its baseline delta
  is only coordinator PROGRESS, ADR0019 and the closed skipped-run record.
- Final independently reviewed head/tree and first eligible main squash SHA/tree:
  pending coordinator recording after review/merge; no future SHA is asserted.

Read AGENTS/KICKOFF, ADR0019 preamble/D7f, ADR0020 D9a, original run preparation,
accepted omitted-kill plan and Muse plan/source/D7f reviews, native source
evidence, and the [closed record](linux-a4-negative-skipped-43672720190.json).
Run `43672720190`, attempt 1/job `132686133651`, was accepted on exact baseline
main by `Dytschgo` (actor and triggering actor), event `workflow_dispatch`,
then completed **SKIPPED, zero steps**. The immediate run-list lookup found
none despite dispatch returning the run URL; monitor finally cleanup deleted
the pin before admission. Root independently confirmed HTTP 404 absence.
These are supplied closed-record/coordinator facts, not network checks here.
D7f is consumed; its skipped host ran no steps and earned no native credit.

[ADR0019 D7g](../adr/0019-unix-native-execution-bridge.md#d7g-one-replacement-native-a4-omitted-s7-kill-run-2026-11-15)
is the replacement disposition. The only edits in this task are that ADR and
this note. No executable delta versus `994760c` is allowed, even the three
selection blobs changed historically under D7f: all are now baseline bytes.
The whole candidate tree comparison, rather than a sample, binds every other
blob/mode outside the five exact document paths enumerated in D7g.

| Fixed execution path | Baseline and prepared Git blob |
| --- | --- |
| `tools/unix-exec-validation/driver.py` | `2180b594616acd3f811d6a2c0bd694b20fc29ce0` |
| `tools/unix-exec-validation/mutation_check.py` | `00740ee6085f01ab94c63835feb5e49999ef5168` |
| `tools/unix-exec-validation/test_driver.py` | `2684cc6a4125ae1e880e535950e4b4d606d9aebd` |
| `.github/workflows/unix-execution-experiment.yml` | `dbcca2276307357301719ed68a09631b8ab7a10a` |

Before use, coordinator must record the actual reviewed head/tree and main
squash/tree, prove tree equality and full baseline blob/mode identity outside
the exact allowed document set, and read back current main immediately before
dispatch. Only the first eligible normal main squash containing D7g qualifies;
advancing main or mismatch requires a new independently reviewed disposition.
Once effective: pin once, read back once, dispatch once; **capture the accepted
run URL directly from dispatch output**. Hold the pin until that exact run is
terminal even if run-list lookup fails. Never redispatch on lookup failure.
Then finally clear and independently confirm HTTP 404 absence. No repository
infrastructure change is needed for this coordinator monitor fix.

All native oracles, bounds, provenance and owned-only cleanup remain those in
D7f/D9a, explicitly retained by D7g. Acceptance still requires independent
native assessment. Owner-authorized fresh disposable-host execution needs no
extra approval once effective. All prior grants remain consumed and hosts
excluded. A4/M2/full F/G/real-Runner PA6 stay open; installed/dedicated paths
remain **unverified on real hardware**.

Local checks actually run: baseline/reviewed tree equality, full Git delta and
blob/mode comparison, strict UTF-8/LF/no-BOM validation and `git diff --check`.
No tests, native/helper execution, network, dispatch, variable operation, push,
PR, subagent or PROGRESS/source/workflow/helper/pin/Cargo edit was performed.
Final commit identity and check results are returned in the handback.
