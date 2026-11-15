# Independent Opus delta recheck of A12 delivery blockers B1/B2

2026-11-15, Anthropic Opus 5.5 / high, review sub-agent, no subagents.
Correction author: coordinator. Original source author: OpenAI gpt-6.1-sol / high.

## Target and scope

- Prepared head `8b12e9efedae3dd233d65fdba30dd57611040dd4` on
  `work/m2-a12-delivery-source`. The correction base is `975f08b`, which holds
  my prior review `docs/reviews/m2-a12-delivery-source-opus-review.md` (from
  review head `d2ee8e8`). Main `cbab8b69f7204c81e4490da107dc1bca3282cf0c` is
  an ancestor.
- `git diff 975f08b..8b12e9e` touches exactly seven paths, 54+/5-:
  `a12.rs`, `driver.py`, `test_driver.py`, `mutation_check.py`,
  `a7-admission-readonly.py`, the evidence file and PROGRESS. There is no ADR
  or workflow change.
- I reviewed only this delta. My earlier full content and security review
  still holds for everything outside it. This recheck does not repeat that
  review.

## Pins and invariants I recomputed

I recomputed each blob from `git cat-file` at `8b12e9e`, then took its
SHA-256 and size.

| Path | Blob | SHA-256 | Matches evidence |
| --- | --- | --- | --- |
| `crates/arxd/src/exec/disposable/a12.rs` | `da5cd128…` | `72601ed1…38e0` | yes |
| `tools/unix-exec-validation/driver.py` | `5a715359…` | `a8a01bc5…1abf5`, 96781 B | yes |
| `tools/unix-exec-validation/test_driver.py` | `eb3c8430…` | `8d8061c5…71b0ab` | yes |
| `tools/unix-exec-validation/mutation_check.py` | `3095efd9…` | `35383821…6adaa` | yes |
| `tools/ci/a7-admission-readonly.py` | `f354bab7…` | `6be7c142…9ddf80` | yes |
| `crates/arxd/src/exec/mod.rs` (unchanged) | `b150f543…` | n/a | yes |
| `crates/arxd/src/exec/unix.rs` (unchanged) | `0d087e37…` | n/a | yes |

- The consumed D7j wrapper changes only `_MODULE_SHA256` and `_MODULE_SIZE`.
  Both equal the recomputed `driver.py` hash and size. No other non-doc file
  pins either the old or the new driver hash.
- `exec/mod.rs`, `exec/unix.rs`, `tools/ci/s2b-native/**` and `.github/**` are
  unchanged against `975f08b`. `mod.rs` and S2b are also unchanged against
  main, so the `#[cfg(test)] #[path] mod a12;` declaration and the S2b pin
  still hold.
- The driver diff is 3 lines: one added `build_env` line, plus the `Popen`
  `env=` line changed from `cargo_env(stage)` to `build_env`.
- Both gates stay closed and unchanged: `DELIVERY_ENABLED = false` in Rust,
  `A12_DELIVERY_ENABLED = False` and `A12_EXECUTION_ENABLED = False` in the
  driver. `TEST` is unchanged. The native fixture stays `#[ignore]`,
  unselected and not run.
- The changed files are `i/lf w/lf eol=lf`.

## B1 (Standards): CLOSED

The new `wired` predicate in `a12_fixed_targets_and_closed_source_gate` does
three things:

1. It cuts `include_str!("a12.rs")` at the first
   `"\n#[test]\nfn a12_acceptance_counterfactuals"`. That marker occurs once,
   at line 792. Everything before it is the top-level helpers plus
   `mod native` (lines 260–790). Every portable test, including this one,
   comes after it.
2. It extracts the actual `prepared_delivery` body up to the first `}` and
   requires it to equal `closed_read(read_delivery)`. The real body is at
   line 272.
3. It requires `read_delivery` to occur exactly twice in that prefix: the
   definition at line 499 and the one reference in the body.

**Self-match check.** In the source text, the marker literal is written with
`\n` escapes, so it cannot match the real newlines it looks for. The expected
body string, the replacement controls and the `hidden_bypass` text all sit
after the marker, so none of them reaches the prefix. This removes the
self-match defect from my original B1.

**Built-in controls.** Both controls change the source before checking it,
using `assert_ne!`:

- `replacen("closed_read(read_delivery)\n", …)` first hits line 272, the real
  body, because the test's own literals are not followed by a newline.
- `replacen("    fn read_delivery()", …)` first hits line 499, the real
  definition.

Both controls must then fail `wired`, so neither is vacuous.

**My own pure in-memory check.** I emulated the predicate in Python on the
`8b12e9e` blob, compared it with the old `source.contains` guard, and added
counterfactuals of my own. Nothing was written to the repo and nothing was
compiled. The script is in my session scratchpad.

| Mutant | New guard | Old guard |
| --- | --- | --- |
| current source | wired | wired |
| body replaced by `read_delivery()` | rejected | wired |
| extra native caller | rejected | wired |
| body `closed_read(read_delivery).or_else(read_delivery)` | rejected | wired |
| second `#[cfg(not(test))] fn prepared_delivery` bypass defined later | rejected | wired |
| `static R: fn() -> _ = read_delivery;` alias in native | rejected | wired |
| expected literal moved before the marker | rejected | wired |
| marker test renamed or missing | rejected (fails closed) | wired |
| new portable test inserted before the marker | rejected (fails closed) | wired |
| CRLF checkout | rejected (fails closed) | wired |

Every mutant that is not benign now fails, and every structural drift fails
the test instead of passing it vacuously. The Rust snippets type-check as
written: `Option<&str> == Option<&str>`, `String: PartialEq<&str>`, and
`&String` deref-coerces to the closure's annotated `&str`. The coordinator
reports the seven A12 Rust tests as green; I did not rerun them.

## B2 (Spec): CLOSED

- `build_a12` now builds its environment as
  `dict(cargo_env(stage), CARGO_PROFILE_DEV_DEBUG="0")`. This is a fresh copy,
  and that copy is the only one passed to its single `Popen`.
- `cargo_env`, the legacy key-set test, the A1/A2 `build_env = cargo_env(stage)`
  site (line 2004) and the A7/A11 build at line 926 are all unchanged.
- The 128 MiB cap is unchanged in both Rust `decode`/`pin` and Python `a12_pin`.
- The A12 path does not invoke Cargo again after `prepare_a12`. The pinned
  artifact goes straight to `monitor_a1a2`, so a later build with debuginfo on
  cannot rebuild over the pinned harness.
- **Profile inheritance.** The `test` profile inherits from `dev`. There is no
  `[profile.*]` in either Cargo manifest and no repository `.cargo/config`.
  The stage `CARGO_HOME` receives only the copied registry. So the override
  applies to `cargo test --no-run`. This is the same value `ci.yml` and the
  E3/E4 workflows already use. Pinned toolchain 1.95.0 also defaults
  `strip = "debuginfo"` when debug is off.
- **Test.** `test_one_bounded_build_and_clock` asserts that the shared
  `cargo_env` has no `CARGO_PROFILE_DEV_DEBUG` key, and that the `env` passed to
  `Popen` has the value `'0'`. The `spawn` side effect raises directly, because
  `build_a12` has no broad `except` around `Popen`. The `none` case therefore
  produces a real assertion failure, not a masked `RuntimeError`.
- **Mutant.** The eighth A12 mutant replaces the override with
  `build_env = cargo_env(stage)`. Its `before` string occurs exactly once,
  which the harness asserts, and it targets that test. The coordinator
  observed it red before the fix (None vs '0') and killed after. I accepted
  that observation and did not rerun it.
- **Size.** The Linux harness size is still **unmeasured**, and the hosted
  Linux compile is the first observation. The evidence correctly makes no
  bounded-size claim, no cap raise, and no causal claim about A7 history.

## Security: no change

The delta adds no host, account, path, privilege, network or gate surface.
The wrapper change is pin maintenance for an already-consumed D7j asset.

## Advisories (non-blocking)

1. **B1 compound evasion.** A two-part edit gets past the predicate: first
   make `read_delivery` `pub`, then call `native::read_delivery()` from a
   Linux-cfg item after the marker. My emulation confirmed it. The change
   needs a visibility edit that review would see, and the driver and Python
   gates stay independently closed. A one-line hardening is to also assert
   `!native.contains("pub fn read_delivery")` or no `native::` in the suffix.
   Today the suffix contains no `native::`.
2. **B2 env exactness.** The test asserts the one key, not the whole A12
   build env. My original fix said "exact". A mutant that dropped
   `CARGO_NET_OFFLINE`/`RUSTC`/`HOME` while keeping the key would survive.
   This env was also unasserted before the change, so it is not a regression.
   The optional fix is
   `self.assertEqual(kw['env'], {**driver.cargo_env(stage), 'CARGO_PROFILE_DEV_DEBUG': '0'})`.
3. PROGRESS and the evidence are coordinator-owned and consistent with the
   diff. I accepted the coordinator's reruns (7 Rust, 14 A12 mocks,
   8 mutant kills, 13 wrapper mocks, 194 full portable, 757/2447 maxima, fmt
   and diff) as reported, and did not repeat them.

## Checks I ran

- `git diff`, `ls-files --eol` and the ancestry check.
- A blob, SHA-256 and size recompute for all seven pinned paths, plus a grep
  for wrapper pins.
- An unchanged-path check for `mod.rs`, `unix.rs`, S2b and workflows.
- Call-graph reads of `build_a12`, `cargo_env`, `command`, `prepare_a12`, the
  `run_body` A12 branch, the mutation harness and the A12 build test.
- A pure in-memory Python emulation of the Rust `wired` predicate across 13
  cases. TEMP was the session scratchpad. There were no builds,
  installs, deletions, network calls, native, host, account, proc, sudo or
  Runner actions.

## Verdict

**B1 CLOSED, B2 CLOSED. No new blocker.** The prepared head `8b12e9e` is
eligible to publish as a PR. After that, it still needs a normal exact-head
independent review, the hosted Linux compile (Clippy and nextest), and all five
required gates. None of these has been observed yet.

This recheck grants nothing:

- no candidate, dispatch, account or execution permission;
- no A12, A7/A11, M2, full F/G or PA6 credit;
- no A7 retry or counter reset.

The hosted A7 NO-GO is unchanged. Delivery stays closed and unselected. The
Linux harness size is unmeasured. A12 remains **unverified on real hardware**,
and M2 remains incomplete.
