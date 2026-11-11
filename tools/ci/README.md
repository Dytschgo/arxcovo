# Read-only Unix manager observations

`observe_manager.py` collects evidence for the still-unimplemented native
launchd parser in ADR 0014. It never installs, starts, stops, enables, disables
or removes anything. The only manager command vectors are:

- macOS: `/bin/launchctl print system/com.arxcovo.arxd` and the fixed Apple
  `system/com.apple.syslogd` read-only control.
- Linux: `/usr/bin/systemctl show` for `arxd.service`, requesting only
  `LoadState`, `ActiveState`, `FragmentPath` and `DropInPaths`.

ADR 0014 is currently on [PR29's pinned contract](https://github.com/Dytschgo/arxcovo/blob/1e1e48b5636eb1d171affa8784fee6dc1a48534e/docs/adr/0014-unix-owner-lifecycle-corrections.md),
not on main. This diagnostic does not require that branch at runtime.

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
one-tab outer job level, excluding nested dictionaries. That refinement still
needs hosted confirmation; the original record is never rewritten to match it.

Run `python -m unittest discover -s tools/ci -p test_observe_manager.py`.
Three tests use ordinary Python child processes on Unix to verify output caps,
timeout and exit/EOF handling; they skip on Windows. None invokes a manager.
