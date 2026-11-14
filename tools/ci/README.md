# Read-only Unix manager observations

`observe_manager.py` collects evidence for the still-unimplemented native
launchd parser in ADR 0014. It never installs, starts, stops, enables, disables
or removes anything. The only manager command vectors are:

- macOS: `/bin/launchctl print system/com.arxcovo.arxd` and the fixed Apple
  `system/com.apple.syslogd` read-only control.
- Linux: `/usr/bin/systemctl show` for `arxd.service`, requesting only
  `LoadState`, `ActiveState`, `FragmentPath` and `DropInPaths`.

[ADR 0014](../../docs/adr/0014-unix-owner-lifecycle-corrections.md) is now on
main through merged PR #29 (`aafaa3c`). This diagnostic collects evidence without
enabling the production parser.

Each child has a six-second polling deadline after process creation, a combined
16 KiB stdout/stderr cap, then a kill and bounded one-second reap if needed.
No shell or inherited environment is used. Queries run sequentially, at most
two per supported platform. Failure is recorded without retries. The diagnostic
entry point returns zero; every existing application/install/smoke check still
runs and remains authoritative.

Only fixed labels, field classifications, booleans and numeric exit codes reach
the JSON output (at most 2048 bytes). Raw manager output, PIDs, paths,
environment values and errors are discarded. Missing-service phrases and exit
codes are observations, never an assertion that an arbitrary manager error
means absence. The existing production launchd parser remains fail-closed.

The Apple control may be unavailable on a future image. That is an observation,
not permission to create a control service or to substitute another target.
This cannot establish start/stop, reboot, installed-service or M2 acceptance.
Capture the exact source SHA, job, image version and results before using these
observations to implement a parser. Actual installed paths remain
**unverified on real hardware**.

The initial source `533ec14` macOS observation is retained separately in
`docs/evidence/macos-manager-observations.json`: the absent job returned 113;
the Apple control returned zero, but a broad state scan reported duplicate
fields. The current diagnostic limits launchd state/path/PID fields to the
one-tab outer job level, excluding nested dictionaries. The refinement passed
hosted macOS at historical source `50a5303` (run 43056868647); the current parent
integration still needs its own hosted checks. The original record is never
rewritten to match a later diagnostic.

Run `python -m unittest discover -s tools/ci -p test_observe_manager.py`.
Three tests use ordinary Python child processes on Unix to verify output caps,
timeout and exit/EOF handling; they skip on Windows. None invokes a manager.

## Prepared fixed macOS absence observation (not wired into CI)

`observe_launchd_absence.py` is a separate preparation for ADR 0014's absence
evidence gate. Fresh independent Grok source review is required before any
publication or hosted execution. This preparation runs no collector CLI.

Its only command is `/bin/launchctl print system/com.arxcovo.arxd`, with null
stdin, no shell, cwd `/`, and a cleared environment containing only fixed
PATH, LANG and LC_ALL. It never queries an Apple control or another service.
Two seconds of query polling and at most one second of cleanup replace this
diagnostic's older six-second/combined-cap design; production is unchanged.
Separate stdout/stderr limits are 65536/4096 bytes, aggregate 69632. Both EOFs
and a real exit code are necessary; failure is never absence.

Only fixed facts, the real numeric exit code, EOF/truncation flags, and strictly
formatted public OS/runner/source metadata are emitted. No diagnostic bytes,
private-output hashes or artifacts are emitted. One exact synthetic candidate
is represented by its fixed ID; it is not a compatibility fixture. Every
observation remains `Unknown`, with `fixture_credit: false`. Unknown or loaded
output is unsupported. See the evidence document for the exact allowlist,
provenance limits and proposed replacement of the existing macOS step.

Pure tests (all transport/time/host dependencies mocked):
`python -B -m unittest discover -s tools/ci -p test_observe_launchd_absence.py`.
Do not run broad discovery for this preparation: the older manager suite has
ordinary-child tests. No native capture or installed acceptance is claimed.
