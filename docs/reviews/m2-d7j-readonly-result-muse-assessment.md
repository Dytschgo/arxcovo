# M2 D7j read-only admission result — Muse independent assessment and fixture-local remedy decision

Muse Spark 1.3 high, 2026-11-15. Independent review sub-agent, no subagents. Read-only: no builds, tests, host reads, root, sudo, helper, proc/sys, native, network, dispatch, variables, push, PR, merge, or watch. Reused the bounded closed record and the retained prior review where identical; nothing was re-fetched or re-run here.

## Scope pinned

- Parents: `412f7ef` atop `8867ab7` atop main `7a26fcd` (verified via local `git log`; PR131 review `489a5d4` / tree `c2d56d5` / all-5-green is coordinator-supplied evidence, not re-queried here).
- Consumed leg: ADR0019 D7j, accepted run `43720496074` attempt1/job `132831225756` on exact `7a26fcd`, `workflow_dispatch` on main, actor/trigger Dytschgo, failed in 6 s (`2026-11-15T13:43:43Z–49Z`), 5 steps green plus one diagnosis step nonzero.
- Read: [AGENTS.md](/C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-c9fbbfb7-d2893c6726d73e4f/AGENTS.md), [KICKOFF](/C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-c9fbbfb7-d2893c6726d73e4f/docs/KICKOFF.md), ADR0019 D7j (`985-1068`) in [0019](/C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-c9fbbfb7-d2893c6726d73e4f/docs/adr/0019-unix-native-execution-bridge.md), ADR0020 D9b (`669-713`) in [0020](/C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-c9fbbfb7-d2893c6726d73e4f/docs/adr/0020-linux-execution-hierarchy-and-janitor.md), [next-execution disposition](/C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-c9fbbfb7-d2893c6726d73e4f/docs/reviews/m2-a7-next-execution-disposition.md), [delivery contract](/C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-c9fbbfb7-d2893c6726d73e4f/docs/reviews/m2-a7-a11-delivery-contract.md), [run JSON](/C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-c9fbbfb7-d2893c6726d73e4f/docs/evidence/linux-a7-admission-run-43720496074.json), [readonly result](/C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-c9fbbfb7-d2893c6726d73e4f/docs/evidence/linux-a7-admission-readonly-result.md), [readonly preparation](/C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-c9fbbfb7-d2893c6726d73e4f/docs/evidence/linux-a7-admission-readonly-preparation.md), the D7j wrapper [a7-admission-readonly.py](/C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-c9fbbfb7-d2893c6726d73e4f/tools/ci/a7-admission-readonly.py), driver path logic (`a7_alias_c2`, `controlled_mappings`, `controlled_mapping_set`) in [driver.py](/C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-c9fbbfb7-d2893c6726d73e4f/tools/unix-exec-validation/driver.py), the earlier D7i closed record/disposition ([run](/C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-c9fbbfb7-d2893c6726d73e4f/docs/evidence/linux-a7-a11-run-43715436515.json), [disposition](/C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-c9fbbfb7-d2893c6726d73e4f/docs/evidence/linux-a7-a11-hosted-disposition.md), [Muse D7i assessment](/C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-c9fbbfb7-d2893c6726d73e4f/docs/reviews/m2-d7i-a7-a11-hosted-disposition-muse-assessment.md)), and the retained [D7j source/security review](/C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-c9fbbfb7-d2893c6726d73e4f/docs/reviews/m2-a7-admission-readonly-muse-review.md). Public systemd semantics confirmed against [systemd.exec](https://www.freedesktop.org/software/systemd/man/latest/systemd.exec.html) (`BindReadOnlyPaths=` creates unit-specific read-only bind mounts, invisible in the host mount table, implying a mount namespace).

## Standards — PASS, 0 findings

- The closed record is complete and bounded: run/attempt/job, exact source==workflow SHA, event/branch, actor/trigger, failure conclusion, 6 s window, 5/1 step split, and the fixed-schema diagnosis object with four closed predicate rows. UTF-8, LF, sanitized public metadata only; no raw log, unknown digest, identity, mode, inode, or payload retained.
- D7j consumed-status recording is exact: ADR0019 D7j carries the consumed-by-run banner, the run JSON marks `native_credit: false`, `retry_authorized: false`, and all three pins absent HTTP 404 after finally-clear.
- Smell baseline: no new abstraction, no duplicated logic, no env-dependent branches beyond the documented gate; the prior review's fixed allowlist and closed output schema are unchanged by this assessment (this file adds no source).

## Spec — PASS with stated limits

- Source-to-feedback binding is truthful: `source_sha == workflow_sha == 7a26fcd` in the closed JSON matches the dispatched main; run/attempt in the diagnosis binding match the observed run. Credit claimed is only what the feedback entails.
- The record correctly reports refusal at the `usr_local_bin` row (`owner_is_root: true`, `type_accepted: true`, `group_other_write_absent: false`) with the adapter stopping before opening/traversing the unsafe component. Root, `usr`, and `usr_local` rows are all-true; no further child state was captured. No account, policy, mount, Runner, or sudo probe occurred — the leg's read-only scope was kept.
- This explains THIS host's refusal only. It proves nothing about the historical D7i cause (role-less code-only `E_PATH`, no path role retained) and no systematic image claim: one fresh-host predicate observation cannot establish history or fleet behavior.
- No A7/A11 admission, controlled-mapping, NNP-case, oracle, completion, or cleanup credit of any kind is taken: `qualified: false`, `native_credit: false`, exit nonzero by intentional diagnosis design.

## Security — PASS, 0 blockers

- No secrets, owner data, identities, policy bytes, digests, modes, inodes, or payload in the retained record or this assessment. No new authority is created: all three pins verified absent 404 after finally-clear, host excluded, no retry or redispatch authorized.
- No host mutation, permission relaxation, ownership adoption, or production-rights change flows from this record: no `chmod` of the host, no relaxation of no-`022`/ownership, no `sudo`/`account`/`policy`/`mount`/`Runner` probe.
- Residual trust unchanged: D8/A1 privileged-writer and OS-dependency assumptions stand as before; whole-host, root, and concurrent-writer trust is not attested by this leg. This run adds no observation for or against them.

## Credit granted

- Truthful closed accounting only: one accepted D7j dispatch on the exact reviewed main failed fast (6 s) at the read-only admission predicate gate with `E_PATH` on the `usr_local_bin` row, stopped before unsafe open, left no mutation, and cleared all pins to independently verified 404.
- Guard-leg signal preserved from the failed run: mocked tests plus the five green steps succeeded before the diagnosis refusal; this credits the guard/transport legs only, never A7/A11 admission or execution.

## Expressly NOT granted

- No A7/A11 admission, controlled mapping, NNP case, oracle, completion, cleanup, host qualification, native authority, production activation, M2 milestone, or full-F/G/PA6 credit.
- No cause attribution beyond the observed row (no failed conjunct, ancestor, account, policy, or runner-environment claim), no systematic-image claim, and no retry/redispatch/host-reuse authority. Any later execution needs a new reviewed disposition on a NEW host.

## Remedy evaluation (fixture-local only)

Alternatives considered and rejected:

1. Relax root-ownership or no-`022`/no-group-other-write, or adopt the host path — rejected: changes the security contract without evidence, and the D7i diagnosis already ruled this out as unjustified.
2. `chmod`/remediate the host — rejected: host mutation is out of scope; the host is excluded and ordinary hosted images stay as they are.
3. Skip or weaken the C2-ancestor check on unprotected hosts — rejected: weakens production rights logic for every future host instead of fixing the fixture.
4. A new diagnostic leg or whole-OS audit — rejected: D7j already localized THIS host's blocking predicate; no material gap remains that another diagnostic leg would close. Priority stays working E2E with proportionate fences.
5. Original-host policy reconstruction or unknown-policy capture — rejected: already superseded for the controlled fixture; source literals supply case0/case1 closure.

Smallest concrete remedy (GO): a fixture-only fifth read-only bind mapping. In the disposable unit namespace only, bind a root-owned empty `0700` source directory over `/usr/local/bin` as `BindReadOnlyPaths`, keeping the existing four D9b policy binds untouched. `systemd.exec` semantics support exactly this: unit-specific read-only bind mounts, invisible in the host mount table, inside the unit's mount namespace. Actual installed C1 `/usr/bin` + plugin + real NNP/descriptor/C3 oracles stay unchanged; `PATH=/usr/bin:/bin` excludes `/usr/local`, and C2 must be absent inside the fixture, so the masked target cannot supply executables — the mask removes a writable ancestor from the in-unit view rather than granting anything. The in-unit view must prove itself: source/target dev+ino, exact (empty) entry set, and read-only status checked inside the unit BEFORE actual Runner admission and rechecked per call. The driver outside the unit must no longer require the host C2 ancestor to be protected; split host-side checks (C1/package/`/usr/bin` alias/floor, before effects) from the in-unit exact closed C2-view proof. No claim is made that the host C2 path is safe or absent, and production rights logic is not weakened: the split is scoped to the controlled fixture, and the strict host gate stays for every non-fixture path.

Proof still required (identified, not assumed): the D9c source slice must demonstrate the actual systemd target-binding behavior (effective mapping property, mountinfo, statvfs read-only, source/view inode equality) and the protected-source/namespace proof (root-owned empty `0700` source, exact entry set, per-call revalidation) in-unit before Runner admission — reusing the existing D9b check shape extended to the fifth mapping.

## Decision: GO with bounded D9c amendment (no implementation or run in this assessment)

GO on the single smallest fixture-local remedy above, with this exact bounded amendment wording to carry into ADR0020 D9c, the A7 brief, and the delivery contract (implementation waits for normal independent review, all gates, merge, and a separate native grant):

- ADR0020 D9c: "Controlled A7 fixture amendment D9c (fixture-only, after independent exact-head review, all required checks, and coordinator merge): the disposable override may add exactly one further read-only bind mapping of a root-owned empty `0700` source directory to `/usr/local/bin`, in addition to the existing strict four D9b policy mappings (making the exact unit mapping set FIVE). No other mapping, missing-source optional prefix, production use, or host-policy edit is permitted. Source/target dev+ino, exact empty entry set, and read-only status must be checked in-unit before Runner admission and rechecked per call. The driver outside the unit must not require the host `/usr/local/bin` ancestor to be protected for this fixture path; host-side C1/package/`/usr/bin`-alias/floor checks stay before effects, and the in-unit exact closed C2-absent view is proved separately. C2 must be absent inside the fixture; `PATH=/usr/bin:/bin` excludes `/usr/local`. Actual installed C1/plugin/NNP/descriptor/C3 oracles, collision/owned-cleanup/quarantine fences, and production rights logic are unchanged. No claim about host C2 safety or absence. Canon profile facts update later on the fresh execution host."
- A7 brief / delivery-contract amendment: replace the exact-four mapping set with the exact-five set above for the controlled fixture only; extend the existing `controlled_mapping_set` length/set check, in-unit view checks, and per-call revalidation to the fifth mapping with identical strictness; keep every delivery pin (schema/layout, OwnerConfig/RunSpec fields, budgets, identity/artifact/discriminator checks) unchanged.

Concrete blocker that would flip this to NO-GO: if the D9c source slice cannot demonstrate, through the unit's own namespace, the effective fifth-mapping property plus mountinfo/statvfs/inode proof in-unit before Runner admission and per call — then the smallest alternative is to hold D9c and run the D9b combined fixture on a fresh host whose `/usr/local/bin` already satisfies the protected predicates by nature, with no mask and no contract change.

Standing limits: existing strict FOUR mapping text must explicitly amend to FIVE if D9c is adopted; canon profile facts update later, not here. No implementation, test, or run until normal independent review, required gates, merge, and a separate native grant. D7a–j are consumed; no retries or host reuse. A7/A11, M2, full F/G, and real-Runner PA6 remain **unverified on real hardware**.

Completion wakes the coordinator for checkpoint publication and the concrete D9c amendment with Sol review.
