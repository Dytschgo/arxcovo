# M2 S2b native harness pins: narrow Opus recheck

Reviewer: Anthropic / claude-opus-5-5 / high, review sub-agent, 2026-11-14.
Single-agent review, no subagents. It reviews an OpenAI / gpt-6.1-sol /
medium delta, so it is a different-provider review. Standards and Spec are
reported separately. Scope is limited to new findings in `b7a6dba..945b33b`.

**Verdict: PASS, no blockers on either axis.** The coordinator may publish
this harness to obtain the **first hosted evidence only**. This grants no
merge, native credit, activation, A13 acceptance or M2 acceptance. The native
baselines and both native kills remain **UNRUN**. Real-Runner PA6 remains
**OPEN**. M2 is incomplete and the hardware path is **unverified on real
hardware**.

## Isolation and pins

- Cwd and Git top level were the same isolated T3 worktree
  (`<isolated-worktree>`), branch `t3/review-native-pins-1`. Status was clean
  at HEAD `945b33bfe666681ebaf0c4aef4688ea1219c6cfa` before and after the
  test run.
- Prior review `7d20762` is retained as `b7a6dba`; both have stable patch-id
  `410785ffcc3265a7b9f65f74e810a0e924ad66d6`.
- Delta commits: `ed1b74c` (pin bridge, fix paths), `271bfa7` (append pin),
  `945b33b` (explicit mutation paths). Changed paths: the S2b workflow,
  `driver.py`, `test_driver.py` and the S2b evidence file only.
  `git diff --stat b7a6dba 945b33b` is empty for `crates`, `Cargo.toml`,
  `Cargo.lock`, `ci.yml` and `mutations.jsonl`. `git diff --check` is clean.
- Working-tree SHA-256 equals the driver `PINS`/constants and the evidence
  file (all `100644`, `eol=lf`):
  - `exec/cgroup.rs` `64d4cb8c48e516d5fdcd92f85fe2c9c9abb028ee55251929e95824c16211d121`
  - `exec/mod.rs` `f8d425f7a57a309b7e10c29b4609f2c0c75002a33bc1eb0c00e04cd47dd3f9b4`
  - `service/executor.rs` `45da7396dc926ad40b18a5fab1ff2fd898c3d0577b8cd13d91eba1a7205cf5ed`
  - `unix_bridge/tests.rs` `b2e0c0efc24feb46555fee12413edf79c8afe22d0221908fde59238a597a780d`
  - `unix_bridge/tests/e6.rs` `f26d1ed0778c5b24495f46bde26b8248f046aedfbcae07440857453bf17dabb1`
  - `unix_bridge/tests/s2b.rs` `6eb93098dfda74ae52e751a07f69fd9252417184d65d6064cb6a26270643e089`
  - `service/unix_bridge.rs` (7th pin) `323fa965a041c4ad53c99d149f810e031d886cc09e83089d26c82d27b49026d1`
  - `mutations.jsonl` `28f05948ca4aace6944fbe8c77676e8f702575042cf77d9c9c176096ac5b0f94` (unchanged)
  - workflow `3097257cbc463e1b85a4845fc529f3df8af5c0db768db00d1b3a259aa81ad418` (= `WORKFLOW_SHA`)
  - `driver.py` `2009ce4a8bd2b7ab1a4593224eff930f6a05a91959cb22bdcc2f86788820d6d2`
  - `test_driver.py` `9eff03de41890654c0cdb6f4f1d80b0fb54b6f1cebe73e5bea507cbf56be4057`
- Intermediate driver hashes in the evidence match history: `ed1b74c`
  `f7ed1df…` (bridge pin prepended), `271bfa7` `c6556fe…` (appended).
- Each fixed mutant's `old` text occurs exactly once in its pinned target and
  differs from `new`: `drop_skips_store` in `exec/cgroup.rs`,
  `group_forgotten` in `unix_bridge/tests/s2b.rs`.

## Checks run

`python -I -B -m unittest discover -s tools/ci/s2b-native -p test_driver.py`
(Python 3.12.10, Windows): **11/11 passed**, tree clean afterwards. No Cargo,
nextest, native, install, GitHub, push or CI action was run.

## Standards

No new findings.

- The workflow path filter now names the real evidence file
  `docs/evidence/linux-unwind-local-s2b-native.md` and the dependent
  `service/unix_bridge.rs` (the parent of the `tests` module). It matches the
  seven `PINS`.
- `load_mutations` names the two reviewed target paths as literals rather
  than `TARGETS[0]`/`TARGETS[-1]`. This removes the hidden coupling between
  pin order and mutation targets that caused both earlier `manifest_schema`
  failures. Possible smell (judgement only): the two paths now appear in both
  `PINS` and the loader. That duplication is intentional and safer than
  index coupling; no change requested.
- The removed `status` helper had no callers; no test still references it.

## Spec

No new findings.

- Authority is not widened. Mutation writes are still limited to manifest
  rows whose `target` must equal one of the two literal paths, and the
  manifest stays SHA-pinned. Adding `unix_bridge.rs` to `TARGETS` only adds
  it to the guarded read set, the tracked/`100644` check, the source pin, the
  restore loop (rewrites pinned original bytes only if changed) and the
  fingerprint. No product source changed.
- Preserved unchanged: symlink/reparse/hard-link and traversal guards,
  bounded reads, run deadlines and output bounds, process-group kill,
  clean-checkout/top-level/HEAD checks, workflow/source/manifest pins,
  in-memory applicability before writes, `mutated` restore and verify,
  final restore with fingerprint check, before/after baselines, and the
  classifier (`killed` only on exit 100 with the single named test FAIL and
  only `error: test run failed`).
- The evidence states the failure history accurately: two failed portable
  checks (prepend broke `TARGETS[0]`; append still broke `TARGETS[-1]`),
  neither given credit; the third passed 11/11, which I reproduced.
- The evidence correctly says the classifier fixtures are hand-written
  nextest-format samples, not captured output.
- Accepted limits are recorded without false credit: six guard-removal test
  gaps, named-test-only failure attribution (no inner assertion credit),
  process-group cleanup edge cases bounded by the job timeout, and
  cleanup/interrupt reporting limits. These are accepted for a bounded
  disposable hosted job only. No native execution or kill is claimed.

Standards: 0 findings. Spec: 0 findings.
