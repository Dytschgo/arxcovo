# M2 A10 capability evidence join — independent Opus source/content review

2026-11-15; Opus 5.5 high review sub-agent; documentation only.
Target: Sol docs-only note `docs/evidence/linux-a10-capability-evidence-join.md`.

## Pins and retention

- Reviewed HEAD `9dabcd5ef51ca9f6bdca221fdcae9bb4290ffb7b` (PROGRESS +2 only),
  parent `0d3d6cffa7c47ac6e3f8a98a0f15fb9c91c7f305` (the note).
- Builder `3ce8faf088048e19d0e1995d407b76aacf6199d6` and retained `0d3d6cf`
  share parent `4847db066299efeea6634525f597bfcd367a6457` (PR139 head) and
  tree `f7332f2101009850cd42b4a5da6569342961bb7e`: retention is exact.
- `0d3d6cf` adds one file, 115 lines, LF only, final newline, no trailing
  whitespace; `git diff --check 4847db0 0d3d6cf` clean.
- `git diff --stat 2a1d5ad 4847db0` is exactly five document paths, as the note states.

## Standards — PASS

One concern, one file, conventional subject. No source, ADR, package,
workflow or policy edit. No secrets, private data or new authority; it states
it grants no execution or activation and keeps **unverified on real hardware**
for uncovered Runner and installed/dedicated paths. No findings.

## Spec — PASS after one exact-join correction

Verified against committed source, historical blobs and records:

- **Bound.** Drop-in sets NNP and `CAP_SETUID CAP_SETGID CAP_CHOWN`; bits 7/6/0
  give `0xc1`. `parse::hardening` requires `NoNewPrivs:\t1` and
  `CapBnd:\t00000000000000c1`; setup S0 requires `Hardening::Standard`.
  The note correctly limits R4 to NNP plus bound, not effective/permitted sets,
  and claims sufficiency, not minimality.
- **Zero diff.** `setup.rs`, `cgroup.rs`, `unix.rs` and both package files have
  no diff and no touching commit in `1e9d881..4847db0`. Every adapter addition
  in that range (`seed_a4`, `place_a4`, `retained_a4`, `cleanup_a4`, `seed_a5`)
  is `#[cfg(test)]`. The note says only the A4 cleanup helpers, which is
  narrower than the truth but not wrong.
- **Run bindings.** All six run IDs, executed SHAs and attempt 1 match their
  records. A1/A2 `prepare_a1a2` at `1e9d881` requires `runs_created`,
  both controller writes, zero removals, uid 0, mode 0700, cgroup2 and
  controller readback; S9 opens all three files before `Ready`. A4
  limits (67108864/0/1), `seed_a4`→`seed_a5`, `place_a4` numeric-PID write
  and `ChildOps::live` are present at `a328060` and `1958f99`. A4 negative
  KillTimeout and 1/1/0 with zero removals match the accepted assessment. A5
  one removal and reused runs match its review. A4/A5 `a5::preflight` repeats
  the R4 check at all three SHAs.
- **Operations vs Runner calls.** The matrix keeps operation kinds separate
  from Runner calls. `Group::create` (limit writes, `cgroup.kill`/`procs`
  opens), `pre_exec` writing `0` to the held procs fd before
  `PR_SET_NO_NEW_PRIVS`/`setgroups`/`setresgid`/`setresuid`, and
  `kill_and_confirm`/`release_leaf`/Drop are correctly listed as having no
  accepted runtime proof. Fixture migration does not stand in for Runner
  self-migration.
- **Exclusions.** The A12 facts match the record exactly (`43741300777`,
  `2a1d5ad`, 67 s, `E_SERVICE_OR_TEST`→`E_COMMAND_FAILED`, lower bound
  `cgroup`, cleanup unconfirmed, internal absent, 404s not cleanup proof, no
  native credit). The A7/A11 FINAL NO-GO is not reopened. A10 stays OPEN.
- **Links.** All 26 relative targets resolve. The 6 run URLs are cited only,
  not fetched.

**S-1 (concrete, low impact, fix before publication).** The "Profile
attribution" paragraph credits all runs to the driver's `standard_profile()` /
`runtime_files()` / `verify_runtime()`. At A1/A2's executed `1e9d881`, none of
those functions exists. That driver's `install_runtime()` exclusively writes the
staged unit and Standard bytes plus `90-disposable-a1a2.conf`, but it checks
only pre-absence and has no post-load check of `DropInPaths`, directory
contents or bytes. The functions exist at `beb7117`, `4bb1561`, `cf3a251`,
`a328060` and `1958f99`. A1/A2's Standard credit still holds, through exact
staged install plus the production S0 R4 gate (NNP=1, CapBnd `c1`), so no
conclusion changes. Because the join must be exact, replace the sentence with:
"At A1/A2 `1e9d881`, `install_runtime()` exclusively writes the staged
unchanged unit and Standard bytes plus the fixture override, with no post-load
DropInPaths/byte verification; from A3 `beb7117` on, `standard_profile()` /
`runtime_files()` select the Standard bytes except for A3 None and
`verify_runtime()` requires exact FragmentPath, DropInPaths, directory contents
and byte equality."

## Security — PASS

No secrets, owner data, principals or logs. No grant, retry, diagnostic,
variable, dispatch or activation. Public publishing is unaffected. No findings.

## Eligibility

Standards PASS, Security PASS, Spec PASS conditional on S-1. After the
one-sentence S-1 replacement, the note is eligible. A diff-only recheck of
that sentence is enough; the matrix needs no re-review. PR139's gates and
merge stay with the coordinator.

## Recommendation: bounded A9/U9 join

It adds information, so it is more than a gap record. Every accepted
`cleanup: confirmed` run (A1/A2, A3 Standard/None, A4 positive/negative, A5)
went through driver cleanup. That cleanup ran `systemctl stop` and then required
`/sys/fs/cgroup/system.slice/arxd.service` to be absent. The check is present at
every executed SHA I inspected; `beb7117`→`4bb1561` changes one driver line.
That is accepted evidence that systemd removed the unit cgroup, including the
delegated `supervisor/` and an empty `runs/`, at unit deactivation on those
hosted Ubuntu 24.04 hosts.

The join must record these limits:
- On success, the driver waits for `inactive` with `ExecMainStatus 0` before
  cleanup. The removal therefore follows a natural main-process exit under the
  fixture `ExecStart`, not a SIGTERM/`KillMode=mixed` stop.
- `runs/` held no leaves at deactivation.
- The production arxd Drain path never ran.
- There is no crash or populated-leaf case.
- A12 adds nothing.

Keep it one note of about 80 lines or less, with one review. Then proceed to
independent M3 per user steering. If the budget is tight, a two-line PROGRESS
entry recording the observation and its limits is an acceptable substitute.

## Checks actually run

Git object, history, diff and blob reads; reads of source, records, reviews,
ADR, brief and KICKOFF; local link resolution; CR, whitespace and final-newline
checks. No tests, builds, native, host, root, network, Runner, dispatch,
variable, push, PR, comment or merge actions.
