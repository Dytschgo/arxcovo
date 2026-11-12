# ADR0019 contract clarification disposition

Author: Anthropic / claude-opus-5-5 / high, architecture and security design
sub-agent, 2026-11-12. Documentation only. No subagents were used.

This is **not** an independent review. The same provider and model wrote the
original proposal, so no independence is claimed for either. The status stays
**proposed**. A fresh independent Sol high review of the clarified ADR and
brief must pass before any implementation or publication.

## Pins

- Parent: `d9b6fa3067e1e74b25fcd2bebbdd74e289503d63` (docs only), on actual
  main `1d828ca7f4cddc74495f2d8eeca36bb4553a7ad9` after #46.
- The following were carried byte-identical in the first commit of this
  branch, verified by blob hash:
  - ADR `75097fe5` and brief `efb0c3de`, from proposal
    `5ed795d9d2c3f20438c5f17e89ffc9c2815428dd` (base `07ae3c89`);
  - the Sol review `080ac243`, from `6a6faae7a43831fa58a896a4b0cc722107ef9195`.
- The Sol review keeps its own pins and verdict: **PASS** for the inactive
  design, with advisories B-A1 and B-A2. Both findings, and the rationale for
  keeping fixed payloads and no per-run hook swapping, are credited to it.
- Source re-pin: `git diff --stat 07ae3c89 1d828ca7` over every source and ADR
  the proposal cited shows changes only in:
  - `service/executor.rs` (`configured_rights`, plus one new
    `Execution::native` parameter);
  - `service/application.rs` (store exit classification);
  - ADR 0017 (status checkpoints only; the validator merged as #42
    `4c2264d`).

  `exec/`, `development/`, `main.rs`, `packaging/`, the Cargo manifests and
  the toolchain are unchanged. So the proposal's line citations hold at
  `1d828ca7`, and ADR 0017's `Report`/`Creation` contract needs no recheck.

## B-A1: legacy error compatibility — resolved by preservation (ADR 0019 C1)

**Contradiction.** D1 promised that `run` maps `RunError` back to `ExecError`
"so existing callers and tests keep their meaning". `RunError` carries no text
or errno. The legacy values are `Invalid` and `Unavailable` with distinct
`&'static str` text (three differ at the spec check alone, `mod.rs:225-241`)
and `OperatingSystem(errno)` (`mod.rs:101-104`). A reverse mapping loses
data. D4's move would also reorder legacy refusals. D5's blanket allowance
would have reached legacy `validate_spec`, and with a short target it would
have let the agent tail exceed 65,536 bytes.

**Decision.** Preserve, rather than accept a lossy deviation.

- One private `RunFailure { phase, legacy }` sits behind a shared `run_core`.
  It has no `From` impls, so each site names its class explicitly.
- `run` returns `.legacy` after the unchanged `validate_spec`.
- `run_phased` returns `.phase` after `validate_phased_spec`. That validator
  allows only the exact four-item `exec-child` prefix, and the tail keeps the
  ADR 0007 bound. No string, errno or ID crosses `run_phased`.
- The early check is kept and the final recheck is added (C3), so the legacy
  order is unchanged.

There are exactly three labelled legacy deviations:

1. the race-window refusal, which reuses an existing literal;
2. the merged `Cancellation` type identity, which is additive;
3. the panic payload text at six unreachable sites; the panic itself is kept.

Brief §3a gives the full site table: the legacy value preserved and the phase
class.

**Why this is the smallest option.** The core already exists and is shared, so
there is no duplication. The only new type is private. The three alternatives
are each worse:

- A labelled lossy mapping would change public `ExecError` values for no
  safety gain.
- Duplicating the runner would fork the fences.
- Redacting the `Debug` of ADR 0007 types would change the public API.

**Tests.** L1, L1b, L2, L3, the R2 extension and the mutations listed in brief
§9. L1b kills the reverse-mapping mutant: three distinct legacy `Invalid`
texts map to one phase class. Root-only sites are limited to construction
review until the disposable experiment, and the brief says so.

## B-A2: panic diagnostics privacy — resolved by a payload boundary (ADR 0019 C2)

**Facts.** These were read from the primary std docs as served at
doc.rust-lang.org (Rust 1.99.1). They were not checked against the pinned
1.95.0 source, because no local `rust-src` exists.

- The hook runs before the panic is caught.
- `catch_unwind` does not catch aborting panics.
- The default hook prints to stderr, and the hook is global.
- `Result::expect` includes the content of the `Err`.
- `NulError` derives `Debug` over its bytes.
- The str char-boundary message includes one character.

From repository source:

- No hook is installed anywhere.
- Linux stderr goes to the journal by default (*source inference*), and macOS
  stderr goes to a log file.
- A worker panic today leaves the row `started`.
- `unix.rs:22`, `160` and `167` use `Result::expect` on `NulError`. They
  would print private path or cwd bytes, and they are unreachable only while
  `clean_path` refuses NUL.

**Decision.** The boundary is a fixed payload where the payload is produced
(P1–P7). There is no service-process hook, and no hook is swapped per run.
The shim, a separate single-threaded process, gets one silent hook and exits
`125` on a panic. The catch covers the bridge's whole per-run body. This is a
labelled deviation from brief §3.

Evidence obligations:

- **P-SCAN:** a reviewed inventory with a self-test and a non-vacuity check.
- **T10:** a subprocess stderr test with markers, run with and without
  `RUST_BACKTRACE`. It requires each fixed literal exactly once, and its
  negative control requires the hook to visibly print a marker payload.
- **T4b** and **S7**.

**Service-level policy.** No such policy is needed for the bridge itself. Owned
code can meet P1, and P-SCAN checks it. Activation gate G-P1 audits the rest of
the dispatch path. G-P3 stops activation, and requires a separate reviewed
diagnostic ADR within stated limits, only if that audit finds a data-bearing
payload this repository cannot fix. G-P2 requires an unwinding release
profile.

**No absence claim.** These checks do not show the absence of private data in
std, tokio or dependency payloads, in implicit or aborting panics, in real
root-only paths, or in real journal and log contents.

## Retained core and gates

- Unchanged: D1's typed phases; `spawn` `Err` mapped to `Unknown`; the fixed
  installed shim running as the build account; shared cancellation; the final
  recheck; `Runner::drop` never reaching `Stopping`; the bounded shim
  overhead; Linux standard as the only target; every other platform
  `MechanismUnavailable`; a `cfg(test)`-only constructor; staging A–G.
- The only labelled deviations are the wrapper sentence in D1, "moves" in D4,
  the blanket allowance in D5, and the catch scope in brief §3.
- Not authorized: an experiment retry, a third S4U attempt, a password
  fallback, account creation, an approval flow, or any cgroup, rights or
  owner-CLI redesign.
- Experiment `43091339273` stays failed with `E_PROTECTION` before acceptance.
- The original native Linux and macOS facts remain *source inference*.
- Installed, native and dedicated paths remain **unverified on real
  hardware**. M2 is incomplete.

## Work and checks

Actual work:

- Git object reads and diffs.
- Source reading of `exec/*`, `development/types.rs`, `service/executor.rs`,
  `main.rs`, the packaging units and ADRs 0007, 0009 and 0017.
- One fetch of the public Rust docs. The URLs held no repository or owner
  data.
- Text edits to exactly four files, then local commits.

Checks run:

- blob-hash identity of the three carried files;
- `git diff --check`;
- a scan for CR, tab and trailing whitespace;
- strict UTF-8 decoding;
- relative-link targets.

Not run:

- any build, test, mutation, native, owner, admin, SCM, LSA, account,
  privilege, service, pipe, system-path, VM, Lab Manager, private-state or
  experiment operation;
- any GitHub operation.

`PROGRESS.md`, ADR 0018, source and workflows are untouched. PR47's files are
untouched. The coordinator owns the ledger.

Next: a fresh independent Sol high review of the clarified contract.
