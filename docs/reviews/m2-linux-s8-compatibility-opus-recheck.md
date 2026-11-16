# Opus recheck: Linux S8 attribution compatibility correction

Status: **PASS, no blocking finding, 2026-11-16.** Reviewer: Anthropic /
claude-opus-5-5 / high, an independent review sub-agent checking the root Sol
coordinator's correction on separate Standards, Spec and Security axes.
Single reviewer, no children. Docs only. No source, PROGRESS, evidence, brief,
tools, workflow or Cargo edit. No push, PR, status, comment, merge, watch,
rerun, dispatch, install, VM, admin, native M2 or native E4/A7/A12 execution.

## Pins

| Item | Value |
| --- | --- |
| Reviewed head | `2ae20f2ed9303f34af9159851c214206352fe2a2` (evidence only) |
| Correction source | `9e5667f2695ac373b4c19b459f84f3c6c4ea54c8` |
| Parent (PR161 push1 head) | `93f07d1318cc716fda430345f8f302352fe74d8c` |
| Reused attribution review | `c52e982` (Opus source PASS of `70d9d06`) |
| Spec | Sol contract compatibility amendment (`m2-linux-s8-feedback-sol-contract.md` at 2ae20f2) |
| Standards | AGENTS.md, the existing P/E4/E7 source guards and the smell baseline |

Scope: `93f07d1..9e5667f` touches three files. The shim has +98/-20, all inside
`#[cfg(test)] mod attribution`. The E4 driver changes 2 lines. The manifest
changes 1 row. `9e5667f..2ae20f2` changes only PROGRESS (1 line), the evidence
file (+35) and the contract amendment (+19). `70d9d06..93f07d1` is docs only.
Observed later: root `D:/Code/Arxcovo` has `eee5e07` on top of 2ae20f2. It adds
9 lines to the evidence file only, which is outside this pin and not reviewed.

## Checks actually run by this reviewer (host Windows, no cargo build)

1. **Enum equivalence (script).** I parsed the four `closed_enum!` invocations
   at 93f07d1 and the explicit enums at 9e5667f. Site/Read/Status/Wait have
   identical variant order, `#[default]` variant and label strings. Each
   explicit `label` match arm order equals its variant order.
2. **Rest of file byte-identical.** Excluding only the macro and enum region,
   the shim is byte-identical between 93f07d1 and 9e5667f. So the production
   (cfg-stripped) source, the fork child, `Child`/`Drop`, the `"unknown"`
   returns and every attribution site are the same as `70d9d06`. I reused the
   c52e982 attribution logic and fork-child findings unchanged. The new source
   has no `macro_rules` and no CR bytes.
3. **Real source guards, standalone.** I extracted `stage_b_tests.rs` lines
   232-990 unchanged and only rewrote `include_str!` and the
   `CARGO_MANIFEST_DIR` paths to absolute paths. I compiled with
   `rustc 1.95.0 --edition=2024 --test` and ran 8/8 pass:
   `p_scan_checked_nonempty_inventory` and the 2 p_scan controls, `l6_e3`,
   `s13_e4_tokenized_source_and_bypass_controls`,
   `s12_e4_portable_production_pins`, `w8_e7` inventory and braces.
4. **Red control.** The same harness pointed only at the 93f07d1 shim fails
   exactly 2 tests: `p_scan_checked_nonempty_inventory` (expect at the
   inventory scan) and `s13_e4` (it also calls `scan`). Hosted push1 stopped
   at p_scan, so it never reached the `s13_e4` failure. Both have the same
   `macro_rules` cause, and the correction fixes both. The scanner file is
   unchanged.
5. **Module compile.** `mod attribution` (shim lines 147-299) compiles alone
   under `rustc --edition=2024 --test` with 0 errors and 0 warnings.
6. **E4 pins (script).** All five PINS equal sha256 of their 9e5667f git blobs.
   Only the `unix_shim_rights_linux.rs` and `unix_shim_rights_linux_tests.rs`
   values changed. Both old values were already stale against 93f07d1, since
   `70d9d06` changed both files. The other three values are unchanged and
   still match. `MANIFEST_SHA` equals sha256 of the new manifest (LF, no CR).
   The driver has the same line count, and only the `PINS =` and
   `MANIFEST_SHA =` lines differ.
7. **Manifest (script).** It still has 11 rows, and only row index 8
   (`S8_timeout_nonzero`) differs. Its mutation, assertion, binary and blocker
   fields are unchanged. The new `old` fragment includes the
   `#[cfg(test)] attribute(|a| a.site = Site::Deadline);` line. Replacing
   `"unknown"` with `"exit_nonzero"` in `old` gives `new` exactly, so the
   mutation is the same single outcome flip. Every row's `old` fragment
   occurs exactly once in its pinned target. The previous S8 fragment now
   occurs 0 times, which caused the stale exact_count failure.
8. **Pure E4 harness.** I ran
   `python -I -B -m unittest discover -s tools/ci/e4-native -p test_driver.py`:
   22/22 OK in 0.46 s. This includes
   `test_fixed_manifest_and_production_fragments_read_only`, which applies
   all 11 edits in memory. The checks used mocks only, with no native run.
9. `git diff --check 93f07d1 2ae20f2` exit 0. `rustfmt --check --edition
   2024` on the shim exit 0. The JSONL parses. The new doc lines are ASCII,
   and the longest line is 133 characters.

## Reused root checks (not repeated)

These are the exact Windows cargo p_scan red/fixed/controls logs, the first
failed and final pure harness logs, and the root's extracted formatter
harness (1/1). Labels and the rest of the file are byte-equal, so the
c52e982 worst-case line of 245/512 ASCII still holds. No Linux cross-check
was attempted (gcc is known missing).

## Standards

0 findings. The four explicit enums repeat the same shape (Duplicated Code
smell), but the repo's payload scanner forbids `macro_rules` in scanned
sources, so the documented guard wins and the smell is dropped. There is no
guard weakening, quarantine, assertion or deadline change.

## Spec

0 findings. The amendment asks for these, and all are met:

- Replace the macro with explicit enums that keep all variants, defaults and
  labels. Checks 1-2.
- Leave the guard code and its expectations unchanged. Checks 3-4.
- Change only 2 source hashes and the manifest hash in the driver. Check 6.
- Change only the S8_timeout_nonzero context, with the deadline-site line in
  both halves and the same unknown-to-exit_nonzero flip. Check 7.
- The pure harness applies every fixed edit in memory. Check 8.

No native run, grant or kill claim is made.

## Security

0 findings. The change is test-only: the cfg-stripped production source is
byte-identical. It adds no syscall, environment read, exec path or payload
text. The E4 pins bind the actual reviewed bytes, and the driver guards and
policy are unchanged. Evidence paths are local `D:/Temp` logs with no secrets
or owner data.

## Limits and non-blocking notes

- 9e5667f has **no Linux compile, Clippy or test result**. The hosted
  compile/Clippy credit is only for 70d9d06. S8/S9c runtime, the new
  attribution controls and the S8_timeout_nonzero native kill are unrun.
  Hardware paths are **unverified on real hardware**.
- The evidence could note that `s13_e4` was a second, latent symptom of the
  same scanner cause. This does not change the disposition.
- Budgets are unchanged: Linux fix 0/3 and observability 1/3 used. Push 2/3
  still requires a fresh exact-head review and all five gates.

Verdict: **PASS** for the coordinator to proceed to push 2/3.
