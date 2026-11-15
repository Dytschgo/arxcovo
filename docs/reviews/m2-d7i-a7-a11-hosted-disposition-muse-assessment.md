# M2 D7i A7/A11 hosted disposition — Muse assessment of failed run 43715436515

Scope: parent `8d58ed6` atop `4317c24` atop main `0011d23a0681c0acb717064166c399e194937869` (PR #128 merged reviewed tree `2cfe06d` = head `e8fb224`, independent PASS `6894433669` / all-5-green supplied evidence). Read: `AGENTS.md`, `docs/KICKOFF.md`, ADR0019 D7i (now-consumed form in `8d58ed6`), [disposition](../evidence/linux-a7-a11-hosted-disposition.md) and [run JSON](../evidence/linux-a7-a11-run-43715436515.json), preparation actual-dispatch appendix/source pins, retained runtime-candidate Muse review. Reused prior reviews/pins where identical; no tests, builds, network, host/root/native/sudo/helper/dispatch/variables/public writes, push, PR, merge, watch, settings, or subagents. No `PROGRESS`/ADR/source edit in this assessment.

Coordinator-observed run: attempt `1` / job `132815838689`, `workflow_dispatch` on `main`, exact SHA, actor/trigger `Dytschgo`, failed in 18 s (`2026-11-15T12:45:18Z–12:45:36Z`); 8 steps succeeded, guard stage/run/finally step failed. Safe guard tests, dependency fetch, and compile-only A4 helper comparison succeeded. Root driver closed final `E_PATH`; `mutation_lower_bound: none`, `cleanup: not_reached`, `internal: null`. Monitor held the native pin through the exact terminal outcome, then finally cleared it; both pins independently read back absent 404.

## Standards — PASS

- Closed record is complete and bounded: run/attempt/job, event/branch, exact source/workflow SHA equality, actor/trigger, failure conclusion, 18 s window, 8/1 step split, and the exact fixed-schema feedback object. UTF-8, LF, sanitized public metadata only; no raw log, unknown digests, private policy, identities, or payload retained (log read once under the 2 MiB bound per the disposition).
- D7i consumed-status recording is exact: ADR D7i carries the consumed-by-run banner, `PROGRESS.md` records the failure row, and the run JSON marks `native_credit: false`, `retry_authorized: false`, both pins 404.

## Spec — PASS with stated limits

- Source-to-feedback binding is truthful: `source_sha == workflow_sha == 0011d23` in the closed JSON matches the merged main the coordinator dispatched; run/attempt in feedback provenance match the observed run. Credit claimed is only what the feedback entails.
- The record correctly reports refusal **before the outer owned-stage path** and does **not** identify which protected path failed, does **not** prove an account/policy cause, and does **not** exercise A7/A11: first = final = `E_PATH` with no path role retained, so no phase localization beyond the driver's owned-stage gate is warranted.
- No actual A7/A11 acceptance or native credit is taken: mutation none, cleanup never reached, internal null. `A7/A11`, M2, full F/G, and real-Runner PA6 remain **unverified on real hardware**.

## Security — PASS with stated trust boundary

- No secrets, owner data, identities, policy bytes, or payload in the retained record, this review, or the consumed-ADR banner. No new authority is created: both pins verified absent 404 after finally-clear, host excluded, no retry or redispatch authorized.
- Call-graph note (read-only): `E_PATH` is a multi-site code in `tools/unix-exec-validation/driver.py` (checkout allowlist, root-down directory/ancestry ownership gates, stage-plan equality, `sudo_nnp_open` absolute-path gate, site-absolute gate, fixture/service plan gates, private-read size/decode gates). The exact call graph therefore cannot localize which phase refused from the closed record alone — the retained feedback carries no path role or runtime query — so no cause is inferred here. Failure localization belongs to the separate source/mock diagnosis, not to this record.
- Residual trust unchanged: D8/A1 privileged-writer/OS-dependency assumptions stand as before; this run adds no observation for or against them.

## Credit granted

- Truthful closed accounting only: one accepted D7i dispatch on the exact reviewed main failed fast (18 s) at the driver's owned-stage path gate with `E_PATH`, consumed D7i, left no mutation and no cleanup, and cleared both pins to independently verified 404.
- Guard-leg signal preserved from the failed run: guard tests, dependency fetch, and compile-only A4 helper comparison succeeded before the refusal; this credits the guard/fetch/compile legs only, never A7/A11 admission or execution.

## Expressly NOT granted

- No A7/A11 admission, controlled mapping, NNP case, oracle, completion, or cleanup credit of any kind.
- No host qualification, native authority, production activation, M2 milestone, or full-F/G/PA6 credit.
- No cause attribution (no path, account, policy, or runner-environment claim) and no retry/redispatch authority; any later execution needs a new reviewed disposition.

## Remaining limits and next step

- D7i is consumed regardless of outcome; the host is excluded. No retry, rerun, diagnostic redispatch, or host reuse.
- This assessment rests on the retained closed record plus read-only source/contract context; run/job APIs were coordinator-observed evidence, not re-fetched here.
- Next is the parallel source/mock diagnosis of first/final `E_PATH` (separate task); completion of that plus this review wakes the coordinator for the doc checkpoint publication.
