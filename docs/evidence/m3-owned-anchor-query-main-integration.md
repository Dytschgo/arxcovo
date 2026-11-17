# Owned anchor query: pinned main integration checkpoint

Sol 6.1 high, local integration sub-agent, 2026-11-18; no children.
Prepared HEAD: `e5fca6bfe1268c3f115c3eb345ca8ed52d7c1963`.
Merged exact main: `10aa891f27906e0c73e48396c5c812cab7d4523a` (PR163).
Reviewed feature: `f9d0a413d02737214af62601aa345c90634a4e26`.
Retained builder evidence: `15610c3335a0411ca45f5e9c7de6c8aa71dcbadd`;
independent source PASS: `89efaaf5` (retained at `0c9efc0a`).
No identity/admission implementation or source workaround. This is a local
checkpoint; supplementary provenance validation stopped as recorded below.

## Merge and health accounting

Only conflict: docs/PROGRESS.md. Resolved to pinned main byte-for-byte, without
custom PROGRESS edits as instructed. Branch history remains in the first parent;
owned-query evidence, reviews, ADR0005 and identity prerequisite brief/review
remain retained. PR175's pending documentation is not integrated or depended on.
Historical hold statements in retained documents describe their original dates.
The following supplied coordinator facts supersede those historical holds; no
live CI, protection, PR status or remote inventory was fetched by this agent:

- Exact main CI44177880021 attempt1 green: wall632s, charged629s, credit3s,
  Windows628s. Automatic policy44179754154 succeeded.
- Owner-approved PR163 bootstrap completed; original five protections restored;
  budget hold cleared. PR175 documentation checkpoint pending.
- Root inspection and independent integration/exact-head reviews still precede
  publication. All five gates, first hosted query-engine proof on every OS,
  Windows<=660s and fresh exact-main green remain required. No merge acceptance.

## Actual local verification before stop

No feature source changed. The complete binary feature diff from source main
3022d601 to f9d0a413 equals pinned main to the integrated working tree for all
five paths. Diff SHA256:
`6bd0c42ec03c2f4b49254f9592f7f6c5c30f4f1212e36601b33519dcd1ae5e72`.
Each working file equals its reviewed Git blob byte-for-byte:

| Path relative to repository | Blob at f9d0a413 | SHA256 |
| --- | --- | --- |
| crates/arxd/src/cluster/consensus.rs | 4d0a4d3d191a72179b3b8b80980cd5ac6a875255 | 30bedbf0740c1e41be8bf461ad10694ec858361f45cf49b0c2bf78fe0a44b5b9 |
| crates/arxd/src/cluster/consensus/anchor_query.rs | 318d1e9cb6b67b412e2bf804961da237a892f3cc | 350b634354c1d12de617dadbce3c828bf215efb77f7ed9ddaed99ba6ecef234a |
| crates/arxd/src/cluster/consensus/anchor_query_tests.rs | baeecae9ae332901e86305ac300cf38721f79f06 | 0f38c589307d047707950e24ef2517b1e9ffe4ed4043bc62655dd8c60389b8fb |
| crates/arxd/src/cluster/enrollment/anchor.rs | d8d09285e75114857d3caee90751fe31e01c87be | 0f0f185d2f4b604f879738dcb84b046a0f282af6ad81ccfd7bf0d1ca3a65195b |
| tools/ci/test_windows_cluster_quarantine.py | bbe983656891f430505f68fb0a5ab331e55efeeb | 406e75e0b74d6600ec389c59c5c5c68f225c6350c8d40bd10f514bf9852d4e62 |

All remaining260 non-document main paths match pinned main. This includes all
nine policy implementation/test/fixture files, workflows, CI history, sampler,
nextest settings and complete main quarantine guards except the reviewed additive
query guard already included above. Six selected retained branch evidence,
review and prerequisite documents match prepared HEAD. Source review document
also equals89efaaf5, blob `7b39c9d5fa61836d4f7671f5f15990990ee2e837`.
UTF8/no-BOM/LF passed for31 changed/merged paths before this new document.

Executed once each, all exit0:

- `cargo fmt --all -- --check`: wall0.916s.
- `python -m unittest discover -s tools/ci -p test_windows_cluster_quarantine.py`:
  8/8 PASS, runner0.152s, wall0.509s.
- `python -m unittest discover -s tools/ci -p test_windows_diag_sample.py`:
  60/60 PASS, runner0.173s, wall0.441s.

## Reused execution and retained inventory provenance

Read retained artifacts from
`C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-6959c072-46da01a5cc9eee99/target/m3-owned-query-6959-evidence/`.
All nine log/inventory SHA256 values recomputed and matched the retained evidence:
fmt.log, clippy.log, clippy-correction1.log, natural.log, guard.log, sampler.log,
inventory.json, M1.log and M2.log. Hashes remain in m3-owned-anchor-query.md.
Saved inventory SHA256:
`e752a24cf5577be7d99ce6530871dc8b27a5f5364fd30ae92f20a2e1fcd2344c`.
Parsed its first JSON object after the Cargo stderr prefix, rejecting trailing
content:681 tests/eight suites. All eight additions present and unignored:
three pending_query tables, four owned_query durable cases and sole engine case
owned_anchor_query_single_voter. This is the saved listing, NOT a fresh inventory.
Existing fixture28/sample8/founder13/TLS14/atomic1/anchored-proof1 classifications
are retained source-review evidence; current static guard passed above.

Reused builder execution:26 selected tests PASS, Clippy PASS after one mechanical
lint correction, once-only M1/M2 compiled designated failures and exact source
restoration. No Rust compilation, behavior, engine or mutant rerun here. Cache
last compiled M2; restored bytes do not prove a post-restoration behavior replay.
New owned-query engine remains hosted-first and UNRUN locally.

## Stop evidence and handoff

A supplemental provenance script printed the eight saved query cases and verified
review89efaaf5, then exited1 at its document equality assertion:
`expected = git show 15610c33:docs/evidence/m3-owned-anchor-query.md` versus the
whole current evidence document. This assertion was too strict: the existing
current document includes the retained root-inspection appendix added after15610c33.
This is an agent provenance-check failure, not a feature test or source mismatch.
The appendix was already read before this check; it is preserved without repair.
No repeat-to-green or further verification was run after that assertion.
Final `git diff --check` and final-document encoding validation were not run;
root must inspect these and settle the provenance objection before publication.
No encoding correction, source fix or exhausted attempt counter was spent.

No push, PR creation/status/watch/remote merge, admin/install/cache deletion,
root checkout edit, VM or old Lab Manager action. Invoked command sessions exited;
no background task/service was launched. Only local integration/evidence committed.
M2 open/M3 unchecked; Sol3/3 and historical Opus0/2 unchanged. Hardware paths
remain **unverified on real hardware**. No identity/admission runtime credit.

Proposed coordinator PROGRESS entry (not applied): Sol6.1/high local integration
of exact main10aa891f preserves reviewed five-path query sourcef9d0a413 and nine
policy files; fmt/guard8/sampler60 pass once; nine retained artifact hashes and
saved681-test inventory verified. Supplemental evidence15610c33 whole-document
comparison failed on retained root appendix; stopped without rerun. Root final
diff/encoding inspection and independent integration/exact-head reviews pending;
no publication. Supplied main632wall/629charged/Windows628 green and policy success
clear the old budget hold. M2/M3, counters and hardware gaps unchanged.

## Root disposition - 2026-11-18

The whole evidence document equals prepared e5fca6bf exactly, and the original
builder15610c33 bytes are an exact prefix. The additional root appendix is retained
provenance, not source drift. Root therefore closes the comparison objection.
All final changed Git blobs decode strict UTF8 with LF and no BOM. Root diff
whitespace check passed; no behavior, compilation, engine or mutation rerun.
Independent integration and exact-head reviews and hosted gates remain required.
