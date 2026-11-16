# Linux S8 attribution preparation

Local implementation only, 2026-11-16. OpenAI / gpt-6.1-sol / high,
implementation sub-agent; no children. Independent Opus source/content and
exact-head review is pending and required before coordinator publication.
No push, PR, dispatch, rerun, merge or Linux execution was performed.

## Pins

- Exact prepared base: `f0128d6376de8d5de6a2208a91f82145cee1676e`.
- Source commit: `70d9d06094bfcc7b255eb9a8d21932f78f6c98ff`.
- Opus disposition: `2a8f92bddbad5bdad52e98a76553d143cf48d475`;
  prepared-base disposition blob `6b6cd2fea4f4d547973ce32bb0a534cb09ace0f2`.
- Binding Sol clarification blob at prepared base:
  `33926a1f3f9563adb1e0d6e855f2627babc25ad1`.
- Recorded green main: `e601caa594f591cc585007be8a455c07282cde8b`,
  CI `43862021319` (historical evidence, not rerun).
- Blocked PR159: `3d702d17a6175c054c73292aca0385bd8740f2a2`,
  CI `43865758301`, Ubuntu job `133293758711`. Expected `exit_nonzero`,
  observed `unknown` in S8 mode 3. Windows engine reds remain independent;
  no TLS files were touched.

## Change and boundaries

Only the two owned Linux shim/test source files changed. The shim additions
are cfg(test): a separate Copy thread-local Attribution, ten closed sites
plus none, bounded timing and saturating polls, closed read/status/wait
classifications, and a parent timing guard. Reset occurs at run entry before
the anchor. No Observation identity field is an emitter input. No measurement,
attribution update or added call occurs in the fork child. Child/drop and
fork-child source bytes are unchanged, as are production returns and deadlines.

S8's four mode iterations and S9c's two swap iterations each evaluate the
original operation once, capture its outcome, emit one bounded ASCII line,
then assert the same result and expectation. The S9c rename hook and all
other existing assertions are retained. Case, mode, outcome and diagnostic
enum spellings are closed. Numeric output is bounded by u32; durations clamp
to 60000 ms. Marker/pidfile values use exists() only; ELF size uses metadata
length only. fixture_ms measures fixture setup and copies before the call;
elf_mib is the base ELF length in whole MiB. No raw error, errno, path, ID,
environment, argv, absolute timestamp or free text enters the line.

The new ordinary Linux control skips root using the existing ordinary()
convention. It checks sleeping-mode unknown/deadline/wait >= deadline, permits
zero polls, populates all record fields, then proves a missing anchor resets
the complete record (including polls=0). It does not assert mode A/3/K outcome
mappings. The separate pure formatter test checks exact keys, ASCII/512-byte
bounds, forbidden separators, saturation, all case/mode/outcome/site labels,
and the closed read/status/wait labels. Neither existing deadline (fixture
200 ms; production 5 s), scheduling, nextest flags nor workflows changed.

## Actual checks

- `rustfmt --edition 2024 --check crates/arxd/src/unix_shim_rights_linux.rs crates/arxd/src/unix_shim_rights_linux_tests.rs`: exit 0.
- `git diff --check`: exit 0 before the source commit.
- Static source comparison against the full prepared base: PASS for
  insertion-only cfg(test) hunks; unchanged Child/drop, fork child and fixed
  return strings; unchanged other existing test bodies; S8 downstream
  assertions; S9c hook and downstream assertions; reset before anchor;
  emitter identity-field audit; saturating poll increment.
  The ad hoc audit needed two harness corrections: its initial function
  slicing included newly inserted definitions after e4_helper; its initial
  forbidden-token expression rejected the literal pid filename used only for
  exists(). These were audit false positives, not shim/control failures.
- Source-extracted pure formatter/privacy check in a unique D: temporary
  directory: `rustc --edition=2024 --test <formatter.rs> -o <formatter.exe>`,
  then `<formatter.exe> --exact s8_attribution_formatter`: compilation exit 0;
  1 passed, 0 failed, 0 ignored, 0 measured, 0 filtered out, 0.01 s.
  The harness contains the source attribution module, pure formatter/types,
  and formatter test only. This is a host-only privacy/format check, not a
  Windows shim proxy, Linux Rust validation, or Linux runtime credit.
- One permitted compile-only attempt with unique D: target/temp directories:
  `cargo check --tests --target x86_64-unknown-linux-gnu -p arxd --locked --offline`.
  Exit 1 after 8.52 s in ring 0.17.14's native build:
  `ToolNotFound: failed to find tool "x86_64-linux-gnu-gcc": program not found`.
  Stopped there; no retry, install or cross-compiler setup. This native tooling
  failure provides neither Rust source validation nor Linux execution evidence.
  Raw build output/environment values are not retained in this document.

No Linux shim compilation success is claimed. S8, S9c and the new Linux
runtime control were not run. No Windows broad suite or cluster/disk workload
was used as a proxy. CI overhead under 1 s is a pending Linux acceptance gate,
not a measured result here. WSL is not installed and docker/podman are absent
according to the supplied root inventory; no new runtime provisioning was tried.

## Remaining gates

Debug phase 1 remains unfinished. No cause, hypothesis, behavioral fix or
quarantine is claimed. S8 and S9c remain distinct unresolved outcomes. Local
Linux executions/reproductions: 0. S8 fixes: 0/3. Observability publication
pushes: 0/3; its runtime observation budget starts only after the coordinator
publishes an independently reviewed PR. An attributed S8/S9c red is evidence:
stop rather than push for green. The root coordinator owns any publication,
required Ubuntu observation and merge disposition; all ordinary required
checks and different-provider review gates remain.

M2 open; M3 unchecked; A7/A11 FINAL NO-GO; A12 consumed, no retry.
Native M2, VM, admin, old Lab Manager and protected hardware paths were not
exercised. Hardware paths are **unverified on real hardware**.


## Source approval and first publication

Opus5.5 high reviewc52e982 passes Standards, Spec and Security with0findings.
It independently checked all18inserted production-file blocks are cfg(test),
unchanged non-test/fork-child/Child bytes and assertions/hooks, enum/errno/reset
semantics and245byte maximum output. Its extracted host formatter test passes
with rustc-Dwarnings; those are host-only checks. No Linux compile, Clippy or
runtime result is claimed before the first normal Ubuntu PR job.

Only the two shim files change in source. The PR also retains the originating
failure evidence, Opus disposition, root contract, source evidence/review and
current PROGRESS checkpoint. No source changed after70d9d06. First publication
is observability push1/3 (no behavioral fix; Linux fix0/3). An attributed red
must be recorded and stops pushes for green. Required Ubuntu acceptance includes
s8_attribution_controls, s8_attribution_formatter, S8 and S9c execution/durations;
five exact-head gates and independent publication review remain pending.
Fresh main at publication: e601caa CI43862021319 success. PR159's Windows reds
are separate and not waived. No install, native M2, VM or owner action occurred.


## Push1 compatibility failure and local correction

PR161 head93f07d1, CI43870536050 Ubuntu job133310209058: Clippy and fmt passed,
then p_scan_checked_nonempty_inventory failed with forbidden diagnostic at514.
262pass/1fail/22skip/303unrun. S8/S9c/newcontrols were not reached, so no
ARX_S8_ATTR red or runtime acceptance exists. This is the first hosted Linux
compile/Clippy credit for70d9d06, not a local Linux run or a behavior fix.
The optional native-evidence pure harness also failed stale pins. The Opus
publication reviewer posted no PASS or status and identified both issues.

Root Sol coordinator medium reproduced the exact scanner failure on Windows:
`cargo test -p arxd --lib --locked --offline exec::stage_b_tests::p_scan_checked_nonempty_inventory -- --exact`
exited101 with forbidden diagnostic (D:/Temp/arx-pr161-scan-red.log). Source
9e5667f replaces only the helper macro with explicit enums/label matches. All
four sets of labels/defaults and the formatter output are preserved; the scanner
is unchanged. The exact test then passes (D:/Temp/arx-pr161-scan-fixed.log), and
all3p_scan controls pass (D:/Temp/arx-pr161-scan-controls.log).

Root refreshed the two changed LF source hashes in the E4 driver. The first
pure harness run then exposed one stale exact-match fragment, so its
S8_timeout_nonzero context now includes the attribution line in BOTH old/new
halves and retains the same unknown-to-exit_nonzero flip. Manifest hash updated;
other10rows, challenge assertions/IDs, driver logic and3other source hashes
unchanged. Final pure harness22/22 pass (D:/Temp/arx-pr161-e4-pure-fixed.log).
The prior failed pure run is D:/Temp/arx-pr161-e4-pure.log. These are in-memory
patch/schema/pin checks with mocks, not native experiment execution or kill credit.

A source-extracted host Rust formatter harness for final manual enums compiles
and passes1/1 in0.01s (D:/Temp/arx-linux-attr-compat/formatter.rs/.exe). Format
and diff checks pass. No Linux cross-check was repeated: native gcc is still
missing. New source9e5667f has no hosted Linux result yet. Independent recheck
pending before push2/3; no same-head rerun. Linux fixes0/3, observability1/3used.
M2/M3/hardware and native-execution limits remain unchanged.


Windows job133310209022 in the same first-push run43870536050 also failed
p_scan_checked_nonempty_inventory at514 with forbidden diagnostic. It ran
259pass/1fail/16skip/334unrun in58.674s test execution. This independently
confirms the same all-platform scanner incompatibility; no second cause or
new behavior failure was observed. Log D:/Temp/arx-pr161-windows-job.log.
No rerun or extra observability push occurred. Correction9e5667f remains in
independent review at2ae20f2; this later checkpoint is docs-only.
