# E4 native harness: B1 integration-label narrow Opus recheck

Reviewer: Anthropic / Opus 5.5 / high. This was a single-agent narrow
recheck in a review sub-agent with no delegates. The builder was Sol
(OpenAI) medium, so the review comes from a different provider.
Date: 2026-11-13.

**Verdict: PASS. B1 is closed. There are no new blockers.** This PASS
allows only the coordinator to push for the first hosted native E4
evidence. It does **not** allow a merge.

## Pinned target and checkout

- Runtime cwd and Git top-level:
  `C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-00bcb894-dc30486baec0e6c8`.
  This is an isolated worktree on `t3/00bcb894`, outside canonical
  `D:/Code/Arxcovo`. Status was clean at the start and after every check.
- Reviewed HEAD: `c1fd1eb1a795e32ac8d65641c6172125805d6d93`.
  - Prior reviewed head: `e9b00c4`. The prior review was `23dee78`, retained
    as `f78be2f` (`docs/reviews/0020-e4-native-harness-opus-review.md`).
  - Sol medium fix: `d06e541` (`fix(ci): distinguish nextest integration
    labels from Cargo targets`). It changes only `tools/ci/e4-native/driver.py`
    (+9/−2) and `tools/ci/e4-native/test_driver.py` (+19).
  - Docs: `c1fd1eb` adds one PROGRESS line and the appended evidence section.
- `git diff --exit-code e9b00c4 HEAD` passed for `.github`, `crates`,
  `Cargo.lock`, `Cargo.toml`, `tools/ci/e4-native/mutations.jsonl` and
  `tools/ci/e3-native`. **Production, source pins, manifest and workflows are
  unchanged.** The driver `PINS` and `MANIFEST_SHA` lines are untouched.
- Hashes recomputed from both the working tree and `git cat-file blob HEAD:`:

  | File | SHA-256 | Matches evidence table |
  | --- | --- | --- |
  | `driver.py` | `9b1dc47f83909fe493ed4910ddb9656c2656babe0e775b4e6cf1079daef4d658` | yes |
  | `test_driver.py` | `38e14eb4fc92fff62a304d53473dffc80cbb7465a25b31b65084f613d3104eff` | yes |
  | `mutations.jsonl` | `15980053ce88457578407686d309af95a6dd18cb175fd777d82ac0e70b850671` | yes (unchanged pin) |
  | `e4-native-evidence.yml` | `48122e681f227be7e7b2b6acc091c10782dd8685e9d71c81ffd4dcbb377834f8` | yes (unchanged) |

  All three harness files are tracked `100644` at stage 0.

## Checks actually run (this recheck)

| Check | Result |
| --- | --- |
| `python -I -B -m unittest discover -s tools/ci/e4-native -p test_driver.py`, Windows Python 3.12.10 | **22/22 pass**. Tree still clean. |
| Old-driver control: `git archive HEAD` of `.github`, `tools/ci/e4-native` and `crates/arxd/{src,tests}` into the scratchpad. The new tests passed 22/22 with the new driver, then ran with the `e9b00c4` driver swapped in. | Old driver: **6 failures and 1 error**, including `test_real_integration_binary_label_and_cargo_selector`. The others fail because the shared `status()` helper now emits the qualified label. |
| In-memory adversarial probe (`adv.py`, 29 cases, no subprocess) | **0 bad.** Details below. |
| Real nextest probe. Owned, dependency-free scratch crate (`package arxd`, lib test, `tests/unix_shim.rs` with the S6 name). Lockfile generated `--offline`. The driver's exact `command()` argv was used, with only `--offline --manifest-path` added and a scratch `CARGO_TARGET_DIR`. cargo-nextest 0.9.151, Windows stable toolchain. | New driver: S6 pass gives exit 0 and `survived`; S6 assertion failure gives exit 100 and `killed`. The lib pass/fail cases give `survived`/`killed`. Real line: `PASS [   0.016s] (1/1) arxd::unix_shim s6_probe_bounded_order_root_cwd_and_deadline`. The old driver classifies both S6 runs `invalid`, which reproduces B1 against real output. |

This independently reproduces Sol's reporter probe. It is **reporter-format
proof only**. It is not project Cargo, not the native harness and not E4
behavior.

Not run: project Cargo, rustc or nextest on the Arxcovo workspace; the
driver's `main`/`execute` on real sources; any Linux/WSL/container process;
fmt/clippy; hosted CI; push, dispatch, watch, merge, retry or settings
changes. **All 11 native mutation kills remain UNRUN.**

## Spec

Source: prior review B1 (required fix and required tests), the E4 slice in
`docs/briefs/0020-linux-hierarchy-implementation.md` §E4, and ADR 0020
D8/R5 as carried.

### B1: closed

- **One mapping.** `BINARIES = {"arxd": ("arxd", ("--lib",)), "unix_shim":
  ("arxd::unix_shim", ("--test", "unix_shim"))}`. `statuses()` uses
  element 0, the nextest binary ID. `command()` uses element 1, the Cargo
  selector. The parser and the selector can no longer drift apart.
- **Unknown binary fails closed.** `statuses()` returns `[]`, so `classify`
  gives `invalid` and `baseline_result` raises `baseline_missing_test`.
  `command()` raises `GuardError("binary_guard")`. Passing the label string
  `arxd::unix_shim` as a key, or an empty string, also fails closed.
  `load_mutations` still accepts only `arxd` and `unix_shim`, so a manifest
  row cannot reach the unknown path anyway.
- **The real format is accepted.** The qualified S6 line is accepted with
  the `(1/1)` counter, the padded `( 1/11)` counter and no counter. It
  yields `["PASS"]` for a baseline and `killed` for exit 100 with exactly
  one `FAIL` plus `error: test run failed`.
- **Other forms are rejected for S6.** The probe confirmed each of these:
  - the bare `unix_shim`
  - the prefix `x::unix_shim`
  - the outer prefix `x::arxd::unix_shim`
  - the suffix `arxd::unix_shimx`
  - the library label `arxd`
- **The library is unchanged.** `arxd <name>` is accepted. `arxd::unix_shim
  <lib name>` and `arxd::arxd` are rejected for the library.
- **The Cargo selector is retained.** The S6 command is `-p arxd --test
  unix_shim` and contains no `arxd::unix_shim`. The library command keeps
  `--lib`.
- **The other truth rules are unchanged:** a duplicate `FAIL`, any `SKIP`,
  exit 101, and an unknown binary each give `invalid`.
- **The fixture now uses the real format.** The explicit real-format test
  covers baseline and kill acceptance, bare-label rejection for both
  `baseline_result` and `classify`, selector retention and `binary_guard`.
  The shared `status()` helper now emits `arxd::unix_shim`, which closes
  the root cause of the defect (prior S-A1).

### New blockers

None.

### Advisories (non-blocking)

- **R-A1. One required rejection case is untested in the committed suite.**
  B1 asked for rejection tests of "other prefixes such as `x::unix_shim`".
  The committed test covers only the bare label. The anchored regex rejects
  every prefix and suffix variant (probe above), so this is a coverage gap,
  not a defect. A one-line case can be added later.
- **R-A2. Prior A2 now matters more (carried).** Fixing B1 makes the S6
  baseline reachable. The final S6 cap of 20 s must cover the integration
  rebuild after the S6 and S11 mutants, and that rebuild time is unmeasured.
  If it runs out, the run fails closed and wastes it; no false credit is
  possible. The first hosted run should report the S6 baseline seconds for
  sizing.
- **R-A3. Prior A1, A3, A4 and A5 are carried unchanged.** For A4: the
  earlier "Updated committed manifest" hash table, which lists `7226a7df…`
  and `57f33fe4…`, still reads as current. Only the appended section
  supersedes it.

## Standards

Sources: AGENTS.md (meaningful tests, honest claims, bounded reads, safe
paths, no secrets, small conventional commits) and the Fowler smell
baseline. Smell findings are judgement calls.

### Blocking

None.

### Advisories

- **S-A4 is resolved.** The prior Repeated Switches smell is gone: the
  parser and the command share one map. A residual third literal remains in
  `load_mutations`, which checks `row["binary"] == "arxd"` and
  `"unix_shim"`. It is consistent with the `BINARIES` keys, but a future
  binary would need edits in two places. This is a minor judgement call.
- **S-A5. Test-helper indirection (possible).** `status()` rewrites
  `"unix_shim"` to `"arxd::unix_shim"` inline instead of reading
  `d.BINARIES[binary][0]`. Keeping the label literal in the test is
  arguably better, because it pins the real format independently of the
  driver. No change is needed.
- **S-A3 is repeated (readability).** The new doc text again drops spaces,
  for example "review23dee78", "actualnextest0.9.151", "driverflags",
  "Portable22" and "nativeharness".
- The commits are small and conventional (`fix(ci)` for code, then
  `docs(evidence)`), with one concern each. There are no secrets or private
  data, and none of the new code reads files or runs subprocesses.

## Gates, limits and carried state

- **Push.** This PASS allows only the coordinator to push for the first
  hosted native E4 evidence.
- **Merge** still needs all of the following:
  - real ordinary-UID hosted baselines with explicit pass-or-skip results
  - all 11 actual kills
  - an exact-head different-provider integration review
  - the five required checks green
- **Limits.** This was a Windows-only recheck. No Linux path, compile of the
  project, or kill was exercised. Native paths remain **unverified on real
  hardware**. M2 is incomplete, and production remains
  inactive/MechanismUnavailable.
- **Unresolved, not retried or diagnosed:** the privacy flake,
  `E_PROTECTION`, `ExecutionFailed`.
- **Not touched:** canonical; the old Lab Manager installation and its
  state; the `agent-windows-test` VM; owner infrastructure; private state;
  ignored fixtures; toolchains, apart from reading the installed
  cargo-nextest. PROGRESS and product files were not edited.

**Count:** Spec has 0 blockers (B1 closed) and 3 advisories; the worst is
R-A2, the carried unmeasured S6 final cap. Standards has 0 blockers and 3
advisories; the worst is the residual `load_mutations` binary literal.
