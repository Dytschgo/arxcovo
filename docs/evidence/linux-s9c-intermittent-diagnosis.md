# Linux S9c intermittent test: reproduction blocked

Date: 2026-11-16. Investigator: OpenAI / gpt-6.1-sol / high.
Local starting HEAD: `388573ed2f0d07c12c99053aec4c089e412e934e`.
Branch: `t3/diagnose-linux-s9c-flake`.

## Reported symptom

The task brief reports main Ubuntu CI run `43757400254` at
`38750cf1e179e14da6fa871972027b6d66028ec9` failed
`unix_shim::rights::linux::tests::s9c_bound_object_survives_file_and_directory_swap`
at `unix_shim_rights_linux_tests.rs:237`: left `unknown`, right `exit_zero`,
duration 1.584 s. The brief also reports green main run `43768671593` at
`0ab5038`. These are supplied observations, not CI logs independently retrieved
in this diagnosis. The later green run does not establish that the flake is fixed.

## Read-only environment checks actually run

The checkout is on Windows. `Get-Command wsl.exe` resolved the Windows system
WSL executable. Both inventory commands completed with exit code 1:

```text
wsl.exe --list --quiet
wsl.exe --list --verbose
The Windows Subsystem for Linux is not installed.
```

The output also suggested installation; that suggestion was not executed.
WSL output contained NUL characters, removed only for readable display. Native
exit codes were captured separately rather than inferred from PowerShell's
overall command status.

`Get-Command` found no `docker.exe`, `podman.exe` or `nerdctl.exe` on PATH.
It did find `multipass.exe`. A read-only
`multipass.exe list --format json` inventory was attempted using a hidden,
redirected child process with an 8-second wall-clock bound. It did not complete
within that bound. Only that inventory client was terminated; no guest was
started, stopped or configured. This timeout establishes no guest availability
or absence. No usable ordinary Linux test environment was established.

Drive D: exists and had 3,007,846,260,736 free bytes at inventory time. No build
target or test temporary directory was created because no Linux execution was
available. Inventory output was held in memory. Any resumed build/test must use
fresh task-specific target and temporary directories on D: (mounted into Linux
as appropriate), separate from concurrent builders.

## Debug phase and limits

Read `AGENTS.md`, `docs/KICKOFF.md`, the installed dytschgo-debug skill and
its active profile, and the relevant Unix bridge/Linux execution ADRs (0019
and 0020). This bounded sub-agent task does not create or alter oversight.

Phase 1 is **blocked**: zero actual Linux test executions, zero repeats, zero
local passes and zero reproduced failures. No red-capable local feedback loop
was established. Following the skill's Phase 1 stop rule, phases 2–6 were not
entered: no source-cause hypotheses, instrumentation, fix, quarantine or timeout
change. Fix attempts: **0 of 3**. No Linux/native acceptance is earned; native
hardware paths remain **unverified on real hardware**.

No ignored/native M2 fixture, owner/admin step, infrastructure installation or
startup, old Lab Manager resource, protected VM, native grant, Cargo/workflow,
consensus, registry or PROGRESS change was made. The sole repository change
for this task is this evidence document.

## Narrow handoff

Resume on an already available, authorized ordinary Linux environment. Pin and
record the tested revision and toolchain first; the current local HEAD differs
from both supplied CI revisions. Build/list the ordinary `arxd` library test
and confirm its exact name before repetition. Set unique target/temp locations
on D: where this Windows host is involved.

The intended focused invocation is below; it was **not run** here:

```sh
cargo test -p arxd --lib unix_shim::rights::linux::tests::s9c_bound_object_survives_file_and_directory_swap -- --exact --nocapture
```

After a separately bounded build, cap the repeat loop at 100 serial executions
or 5 minutes total, whichever comes first, with a 15-second external bound per
invocation and stop at the first failure. Capture revision, invocation, repeat
index, exit status, elapsed time and the assertion output; count an external
timeout separately from the reported assertion. Confirm every invocation
actually executes one test (a zero-test success is not evidence). These are
proposed diagnostic harness bounds, not changes to the test's own timeout.
If all repeats pass, retain the measured rate and unresolved status rather
than claiming a fix. If the exact assertion reproduces, minimise it before
ranking and testing causes under dytschgo-debug. This handoff requests access
to existing execution capacity, not permission to install or start infrastructure.
