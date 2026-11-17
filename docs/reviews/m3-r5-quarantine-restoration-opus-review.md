# M3 R5 quarantine restoration: independent Opus source review

Reviewer: Anthropic / claude-opus-5-5 / high, single-agent review sub-agent in an
isolated worktree, no children. Implementer: OpenAI / GPT-6.1 Sol / high. This is a
different-provider review of Sol's change. It is not a merge authorization.

## Target

- Reviewed head: `5350d22fa9effca2a1d823473134cd999a9c7d59`
  (`c7d1367` code commit, `5350d22` docs commit).
- Prepared base: `8e662ecf625e8e7fb26a9dde5d5896ff694aacf7` (accepted contract root).
- Source base: main `c6121523417089bc0afaf307470183dd4a7f47ba`.
- Spec: `docs/briefs/m3-r5-quarantine-restoration.md` as accepted at `8e662ec`.
- Scope: `git diff c612152 5350d22` touches exactly 9 paths: `three_node.rs`, the
  guard test, `windows_diag_sample.py`, its test, ADR 0024, the sample brief, the R5
  brief, the R5 evidence file and `docs/PROGRESS.md`. Workflows, `ci-workflow.test.mjs`,
  nextest profile, `policy.mjs`, `ci-budget.mjs`, Cargo files and production code are
  unchanged. `git diff --check` is clean.

## Checks: actual versus reused

Actually run by this reviewer at `5350d22` (Windows, Python 3.12.10,
`PYTHONDONTWRITEBYTECODE=1`, CI-shaped commands, each once):

- `python -I -B -m unittest discover -s tools/ci -p test_windows_cluster_quarantine.py`:
  2 tests OK (includes all 11 mutant subtests and the patched-`EXPECTED` case).
- `python -I -B -m unittest discover -s tools/ci -p test_windows_diag_sample.py`:
  60 tests OK.
- SHA-256 of the four code files matches the evidence file's "final code" pins.
- Scratchpad-only extra guard mutants, run against the real guard function and real
  source (not committed): R5 gate moved to r4, bare `#[ignore]` on R5, `windows`
  shorthand predicate, reason typo, a second R5-reason gate on r4, `not(target_os =
  "windows")`, and one of the eight gates removed. All 7 raised `AssertionError`.
- Static review of the full diff, the unchanged `load_expected`, `check_expected` and
  `check_inventory`, the CI invocations in `.github/workflows/ci.yml` (lines 63, 101),
  and a search confirming no other workflow step runs ignored Windows tests.

Not run here, by assignment: any cargo, build, fmt, Clippy, nextest or engine test.
Reused from root, not re-observed: final guard 2/2, sampler 60/60 and `cargo fmt --all
--check` PASS. Root's Clippy, real `cargo nextest list -T json` inventory with the
sampler argv, and the ordinary R5 skip run were still running when this review was
written; this review neither reuses nor claims them. Those results must come from
root's new final report.

## Standards

No blocking finding.

- Scope and one-concern rules hold: code commit and docs commit are separate,
  conventional, and confined to the contract allowlist plus the evidence file.
- Every new behavior has a test that fails on its mutant. The fixture correction is
  sound: the original eight gates and R5 are rustfmt multiline, and the new
  `assertNotEqual(mutant, source)` makes any future no-op fixture fail loudly instead
  of passing vacuously. This is a routine test-fixture correction, not an engine fix.
- No secrets or private owner data. The evidence file names a local temp directory
  only.

Smell judgements (none blocking):

- Possible fragile test seam: `test_unsampled_filter_matches_verified_guard_inventory`
  patches `builtins.exec` globally. importlib also calls `exec` through builtins for
  uncached imports, so a future guard import of an uncached module would add calls and
  break `assert_called_once`. Today every guard import is already cached, and the
  failure mode is a loud test failure, not a false pass.
- The guard now uses bare `assert`. Under `python -O` the real-source check would
  no-op, but the mutant subtests would then fail, so the suite fails closed. CI does
  not use `-O`.
- Nit: the ADR 0024 `## R5 addendum` heading has no preceding blank line. It still
  renders under CommonMark.

## Spec

No blocking finding. Requirements checked against the brief:

- Source: exactly four added attribute lines before `async fn r5_shutdown_read_owners`
  with `REASON_R5` verbatim. Body, assertions, 2 s wait, retries, helpers and the
  original eight attributes are byte-unchanged; `ignore` occurs 9 times.
- Guard: `EXPECTED` and `REASON` lines byte-identical; `WINDOWS_UNSAMPLED` and
  `REASON_R5` added; pure `assert_windows_quarantine` enforces both inventories,
  disjointness, `count('ignore') == 8 + 1`, each named `async fn` exactly once, and both
  cfg bans. All nine brief mutants are present (R5 appended to `EXPECTED` via
  `patch`), plus duplicate-R5 and cfg-ban mutants. Issuance tests unchanged.
- Sampler: only `FILTER` changed, to the exact brief expression; argv remains built
  from constants. `IDS`, `ID_ORDER`, budgets, grammar and profile unchanged. The
  sampler's existing guard-mutation test still edits `EXPECTED` bytes and still gates.
- Sampler tests: the exclusion is compared to `WINDOWS_UNSAMPLED` from the namespace
  executed by the real verified-bytes loader; ignored R5 with `expression` mismatch
  passes and the same case with `matches` gates `inventory`. Existing 7/9-name
  rejections and step math pass unchanged.
- Coverage change is stated explicitly in the ADR addendum, sample brief and PROGRESS:
  Windows R5 runs nowhere; Linux/macOS R5 stays required. No unintended weakening
  found: the new guard is stricter than the old one (exact-once function count
  replaces a presence check).

Notes, not blockers:

- `check_inventory` ignores mismatch reasons, so the `expression` reason in the new
  test documents nextest's shape but is not what the code discriminates on. Proof that
  real nextest reports exactly 8 matched/ignored and R5 as an expression mismatch is
  root's pending real-inventory result.
- The PROGRESS health line says "This local change adds ... pending independent
  review" instead of the brief's "This PR adds a reviewed". That is accurate before
  publication; root should switch to the brief wording when a PR exists.
- The evidence file, ADR addendum and PROGRESS describe the initial stop and an
  untested correction. These are historical statements. Root will append the final
  verification checkpoint; that pending update is not a source blocker.

## Security

No finding. Production bytes are unchanged. The sampler loader's fixed path, reparse,
realpath and size checks are unchanged, and nothing is derived from the guard at
runtime. The guard's only new import is stdlib `unittest.mock`, run under `-I`.
No label, status, protection, workflow or policy change.

## Verdict

Source review: **PASS** at `5350d22` for the bounded quarantine exception. This is not
a fix and not cause attribution. Sol shared fixes stay 3/3; Opus stays 0/2.

Merge: **BLOCKED**. `ci-time-budget` cannot pass against known main `c612152` /
CI43880904887 (1475 s wall / 738 s charged, uncredited because it is red). ADR0025 is
accepted by the owner but inactive, and its bootstrap is unauthorized. No exception
label, manual status or budget bypass is permitted. A reviewed concrete PR with the
other four gates green must exist before root puts the owner budget question
(a)/(b)/(c). Also pending: root's Clippy, real nextest inventory and R5 skip results,
publication, and the hosted gates.

M2 is open, M3 is unchecked, and Windows three-node is **unverified on real hardware**.

Summary: Standards 0 blocking (3 smell judgements; worst is the global `builtins.exec`
patch seam). Spec 0 blocking (3 notes; worst is that the real nextest inventory proof is
still pending at root).
