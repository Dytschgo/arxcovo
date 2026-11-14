# M2 stdio setup argument diagnostics

Builder: GPT-6.1 Sol, high effort, 2026-11-14. Baseline `4ff5909`;
isolated branch `t3/fix-mcp-client-setup` started clean.

The service guide documents `client-config new --principal ... --out FILE`,
then registration with `mcp-stdio --client-config FILE`. Inspection traced
`owner_cli` -> `client_config::create` -> private JSON and
`main::run` -> `ClientConfig::load` -> `local::bridge`. MCP contracts and
transport framing were read without edits.

## Reproduced defect and correction

At baseline, `arxd mcp-stdio --client-config` exited 1 and printed
`arxd: transport-frame-or-io`, although its missing argument prevented any
config load or transport connection. The new binary regression failed before
the fix (actual exit 1, expected 2); a direct baseline binary invocation also
confirmed that diagnostic.

Malformed stdio invocations now exit 2 before runtime startup and print only
`arxd: usage: arxd mcp-stdio --client-config FILE` on stderr. The accepted
grammar is unchanged. No arguments or file contents enter the diagnostic.
The regression exercises the actual binary with a missing option, missing
value, misspelled option, extra argument, then the documented argument shape
with a path containing a space in an owned temporary directory. The corrected
shape reaches the existing loader and reports `arxd: client-config` (exit 1)
for its absent file. All cases leave stdout empty and create no config.

## Validation actually run

- Before fix: `cargo test -p arxd --locked --offline --test cli stdio_setup_reports_argument_errors_before_loading_client_config -- --exact`
  failed as described above.
- After fix: `cargo test -p arxd --locked --offline --test cli`: 2 passed,
  including the existing missing-server-config regression.
- `cargo clippy -p arxd --locked --offline --bin arxd --test cli -- -D warnings`:
  passed.
- `rustfmt --edition 2024 --check crates/arxd/src/main.rs crates/arxd/tests/cli.rs`:
  passed; `git diff --check`: passed.

This is a binary-level setup error regression, not an authenticated config-to-MCP
roundtrip or installed acceptance. No daemon, command executor, native fixture,
owner setup, external network, GitHub or CI operation was run. File protections,
token handling and endpoint selection were not changed. Linux/macOS execution
was not tested. Installed service and dedicated execution remain **unverified
on real hardware**. M2 is incomplete; PA6 and Linux F/G remain OPEN. Optional
Windows harness and launchd profile evidence remain parked.

Fresh independent Grok/Muse review and normal required gates remain pending
with the coordinator before publication. This sub-agent started no agents and
did not publish or merge. PROGRESS/ADR updates belong to the coordinator.
