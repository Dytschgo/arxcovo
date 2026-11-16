# Linux S8 hosted outcome failure

Status: unresolved; PR159 blocked. Root Sol coordinator, medium effort,
2026-11-16. No fix or quarantine attempted.

PR159 CI43865758301 Ubuntu job133293758711 on3d702d17a6175c054c73292aca0385bd8740f2a2
failed `unix_shim::rights::linux::tests::s8_outcomes_and_reap` at
unix_shim_rights_linux_tests.rs112: expected exit_nonzero, observed unknown.
The test ran0.645s; overall nextest28.524s,528pass/1fail/22skipped/50unrun.
All15TLS pure/engine tests passed in the same job. The Linux shim and test files
are byte-identical to main e601caa (empty targeted git diff), but this does not
prove the failure is unrelated to changed load or dependency/build size.

The symptom is distinct from earlier S9c main failure43757400254 (expected
exit_zero/unknown at237), although both are unresolved Linux outcome failures.
No shared cause is claimed. Main e601caa CI43862021319 remains green; this is a
red PR, not a new red-main event. No feature merge or blind rerun is permitted.

## Actual reads and feedback boundary

`gh run view ... --log-failed` could not retrieve a job while the run was still
active. The completed-job logs endpoint succeeded:
`gh api repos/Dytschgo/arxcovo/actions/jobs/133293758711/logs`.
Local log: D:/Temp/arx-pr159-ubuntu-job.log. Only the fixed assertion signal is
recorded here; raw runner paths/logs are not committed.

Read-only local command inventory found wsl.exe and ssh.exe, no docker/podman.
`wsl.exe --list --quiet` reports WSL is not installed. No install, service or VM
startup was attempted; no remote host/credential was supplied. Prior S9c evidence
already records the bounded Multipass inventory failure; it was not repeated.
Linux executions/repeats:0. Reproduced locally:0. Fix attempts:0/3.

Debug phase1 cannot yet supply an agent-runnable local Linux feedback loop.
No cause is asserted, and phases2-6/fix are not claimed. Static source facts:
the fixture configures200ms; production config remains5s. `unknown` collapses
several failure branches, so the hosted assertion alone cannot name a phase.
Changing either deadline, retrying the assertion or skipping the test is not
justified by this evidence. Opus will settle a narrowly bounded way to obtain
actual Linux failure attribution before a source fix is attempted.

M2 remains open; A7/A11 final NO-GO and A12 consumed/no retry unchanged. No
native M2 acceptance, admin, old Lab Manager, protected VM or hardware action.
Hardware paths remain **unverified on real hardware**.
