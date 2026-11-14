# M2 F0b: bounded internal refusal transport (design only)

2026-11-14. Author: Anthropic / claude-opus-5-5 / high, design sub-agent
(single agent, no subagents). Status: **accepted for bounded local
implementation by coordinator, 2026-11-14.** Independent Sol high review
`9682af4` (retained `7d7aa2f`) and Opus high recheck of the Sol correction
`40d2d0d` (retained `dbcd578`) close all design blockers. Coordinator:
OpenAI / gpt-6.1-sol / medium. Opus architecture authorship remains original;
the delta recheck earns no independent Opus architecture credit.

Implementation and synthetic mocked/owned-temp unit verification are allowed
within this contract. Publication still requires fresh different-provider
source/security review and all normal gates. This grants no native command,
dispatch, historical retry, observation, activation or merge exception.
F2 stays undispatched.
ADR 0019 D7's no-retry restriction stays in force. A new source SHA
discharges nothing, and failure counters are not reset. M2 is incomplete.
Real-Runner PA6 and the real-record replay gate are **OPEN**. Native,
installed and dedicated paths are **unverified on real hardware**.

## Scope

This proposal covers one concern: carrying a structured `Refusal` out of the
three internal driver modes (`service`, `harness`, `fixture-check`) and
into the outer `run` record. The carrying is bounded, fixed and
fail-closed. It replaces the dead `account`/`cgroup` mutation tokens with
existence-only lower bounds.

Out of scope:

- workflow edits;
- any Rust source;
- `disposable.rs`;
- CI configuration (see the separate follow-up at the end);
- F2;
- any hypothesis or diagnosis of the historical `E_PROTECTION`;
- errno granularity (SP-A5);
- the F0 advisories ST-A1, A6–A8, A10 and A11, and SP-A1.

## Historical authoring isolation and exact pins

- The cwd equalled the Git top level, an isolated T3 worktree on branch
  `t3/f0b-refusal-transport-design`.
- The status was clean.
- HEAD was `f0baed412b82abf5ade88e3016853e4bf08b7f66`. This is the prepared
  PR70 head at authoring time. PR70 subsequently merged as `affaec9`;
  the complete trees match. PR71 merged as `a9ec0ac` and now includes the
  required-CI pattern change. Current local implementation base is merged
  PR71; the table below remains historical source credit.

| Object | Pin at `f0baed4` |
| --- | --- |
| F0 security review (contract source) | original `4220dd780d52a4008d077a3aab0bbc581c7bb504`, retained `9c64c6660a82956afff0cd1bff01055d227b7492`; same blob `99df914aa99ef4663869053815ad6ded01469204` at 4220dd7, 9c64c66 and HEAD |
| F0 blocker recheck / ST-B2 recheck | `373e8bcf…` / `397082ae…` |
| `driver.py` | `cf5a0ca64707ccf95036490a06c7dc3b4cb86573` |
| `test_driver.py` / `test_f0_blockers.py` | `0baeeb58…` / `6545f5d7245c9bd5dc0eee311a0245968a8d0f0d` |
| Tool README / `mutation_check.py` | `aaa39753…` / `fe5b976b…` |
| Evidence `docs/evidence/unix-protection-f0-feedback.md` | `6b2f2056…` |
| `unix-execution-experiment.yml` (unchanged) | `e5f1a96dbfa522bda602db45bf24db000ebddd5c` |
| `ci.yml` | `c84515f415c579c530c8c18e927f85c705760f37` |
| `disposable.rs` (unchanged, read only) | `586b30811b3af485f6b96f7a4a989b2ba64e3e70` |
| Proposal / ADR 0019 / AGENTS / KICKOFF | `8fd8bdc0…` / `cc36ce4e…` / `c2ae2594…` / `9dc49e72…` |

I read the following:

- AGENTS and KICKOFF;
- ADR 0019 D7 and C2;
- the accepted proposal's Stage F (F0/F1/F2);
- the full F0 security review, including its final section (the F0b
  contract);
- both F0 rechecks;
- the full current `driver.py`;
- the structure of `test_f0_blockers.py`;
- the tool README;
- the CI test patterns;
- `disposable.rs` lines 1–29 and 140–170.

**Checks actually run:** only Git reads (`rev-parse`, `log`, `diff --stat`,
`show`) and file reads. No build, test, Python, native command, network,
GitHub, CI, private state or subagent was used.

## Facts this design rests on (source, `f0baed4`)

1. **Internal sites.** Seven `protected` sites run only in internal
   processes:
   - `installed` (in `main`);
   - `service_stage`, `service_cg` and `service_runs` (in `service`);
   - `capture_base` (in `capture_fixture`, reached from the `service` loop
     and from `fixture-check`);
   - `harness_stage` and `harness_binary` (in `harness`).
2. **Where internal output goes.** No internal output reaches the outer
   process:
   - The outer process runs `service` through `systemd-run`, with stdout and
     stderr sent to `DEVNULL` (`driver.py:508-515`).
   - The harness's output goes to `test-output`, which is deleted with the
     stage.
   - `disposable.rs` runs `fixture-check` through `checked()`, which calls
     `Command::output()` and discards the captured stderr. The panic text
     names only the tool (`disposable.rs:21-25`, `:153-160`).
   - In the outer record, every internal failure is `E_SERVICE_OR_TEST`.
3. **Internal top level.** Internal processes never change `feedback_phase`,
   so every internal `Refusal` has phase `initial`. `entry()` prints a
   closed code to stderr for internal modes (`driver.py:597-598`). That
   stderr is discarded as described in fact 2.
4. **How the outer process stops the unit.** The outer `finally` calls
   `subprocess.run([... 'systemctl', 'stop', unit], timeout=20, ...)` and
   **ignores the return code** (`driver.py:526-527`). The only stop
   evidence is `require(not cg.exists(), 'E_QUARANTINE_CGROUP')` (`:528-529`).
   A `TimeoutExpired` from the stop escapes `finally`. That skips
   `fixture_cleanup`, `protected(stage,'run_cleanup')` and `rmtree`, so
   cleanup stays `unconfirmed`. This is the existing fail-closed behaviour,
   and F0b keeps it byte-identical.
5. **The stop return code tells us nothing about writers.** The unit
   starts with `--wait --collect`. On the normal path the unit has already
   finished and been collected when `systemctl stop` runs. `systemctl` then
   reports that the unit is not loaded, with a non-zero exit (documented
   LSB status 5). This is a *documentation inference*, **unverified on real
   hardware**. A zero exit would not prove the processes are gone either.
   F0b therefore never reads the return code as evidence, in either
   direction.
6. **Stage.** The outer process creates `/opt/arx-ci-<run>-<attempt>` with
   `mkdir(0o700)` as root. It checks the parent with
   `protected(stage.parent, 'run_parent')`. `service` runs with
   `UMask=0077`. The existing stage entries are `source`, `rust`, `cargo`,
   `runner`, `build-output`, `artifact`, `test-output`, `harness-pid`,
   `fixture-directory`, `fixture-identity` and `passed`. None of them
   collides with the four names F0b adds.
7. **Fixture identity.** `capture_fixture` writes `fixture-identity` only
   after `pwd.getpwnam('arxexec<pid>')` succeeds with uid and gid ≥ 1000. It
   uses a plain `write_text`, not `O_EXCL`. Both the `service` loop and
   `fixture-check` can write it. `disposable.rs` calls `fixture-check` once
   per test run, but F0b must not depend on that.

## Why `installed` stays intentionally uncaptured

`protected(Path(__file__), 'installed')` walks every ancestor of
`<stage>/source/tools/unix-exec-validation/driver.py`, including `/`,
`/opt` and the stage. A refusal there means one of those is a link, is not
owned by root, or is group- or other-writable. That makes the channel
directory itself untrusted. Writing a record into it would trust the very
thing that just failed verification. The record could land somewhere an
attacker controls, or be pre-planted or raced. The outer reader could not
tell such a record from a genuine one.

So the writer runs **only after `installed` has passed**, and only after
its own descriptor walk re-verifies the stage (writer step 2). A refusal at
`installed` writes nothing. The outer process then still sees only
`E_SERVICE_OR_TEST`, with that mode's status `absent`. This is a deliberate
gap. It is not a defect to fix later.

## Coverage claim (not all seven, not full capture)

| Internal site | F0b capture |
| --- | --- |
| `installed` | **Never** (above) |
| `service_cg`, `service_runs` | Captured when `service`'s escaping exception is exactly this `Refusal` |
| `capture_base` (service loop) | Same, **unless** a later exception masks it in `service`'s `finally` (`E_OUTPUT_DRAIN`, a kill or a wait timeout) |
| `capture_base` (fixture-check) | Captured when it is the escaping exception of the *first* fixture-check to create the file |
| `harness_binary` | Captured when it escapes before `execve` |
| `service_stage`, `harness_stage` | Writable only if the stage passes the writer's own walk after `protected` refused it. In practice that needs a change between checks, so it is rare. No completeness is claimed. |

Also uncaptured:

- any internal non-`Refusal` failure (it maps to `absent`);
- an internal `Refusal` that a later internal exception replaced;
- a writer killed before or while writing (it maps to `malformed`);
- SIGKILL or `BaseException`.

`absent` means "no validated internal record was found when the reader
ran". It **never** means the internal process succeeded or refused nothing.
An F2 proposal may claim at most this table. It must still limit itself to
the six outer sites plus `E_SERVICE_OR_TEST` and these per-mode statuses.

## Design

### 1. Channels (fixed)

`CHANNELS = {'service': 'feedback-service', 'harness': 'feedback-harness',
'fixture-check': 'feedback-fixture-check'}` is a closed literal mapping.
F0b looks names up by exact key and never formats one from argv. The
mapping is paired with these site sets:

```text
INTERNAL_SITES = {'service': ('service_stage','service_cg','service_runs','capture_base'),
                  'harness': ('harness_stage','harness_binary'),
                  'fixture-check': ('capture_base',)}
PRECEDENCE = ('service', 'fixture-check', 'harness')
```

`installed` is in no set. F0b uses no pipes, journal, stdout, stderr,
environment variables, `systemd-run --pipe` or sockets.

### 2. Stage descriptor (shared by writer, reader and progress)

`open_stage(run_id) -> fd`:

1. Derive `/opt/arx-ci-<run_id>` from `plan(run_id)` only. A run ID that
   `plan` refuses yields no fd.
2. Walk `/` → `opt` → `arx-ci-<run_id>` one component at a time. Each step
   calls `os.open(name, O_RDONLY|O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC,
   dir_fd=parent)`. Every intermediate fd is closed in `finally`. With
   `O_NOFOLLOW` on each component, no symlink anywhere on the path is
   followed. A single `open` would protect only the last component.
3. `fstat` each fd:
   - every component must be a directory with `st_uid == 0` and
     `st_mode & 0o022 == 0`, the same predicate as `protected`;
   - the stage must also satisfy `st_mode & 0o077 == 0`.
4. Any failure returns no fd. The function itself never raises; the call
   site catches. The record never says why.

### 3. Writer (internal modes only)

The writer lives in `entry()`'s internal branch and runs before the
existing closed stderr line. It writes only if all of these hold:

- `sys.argv[1]` is a key of `CHANNELS`;
- a module flag `internal_trusted` is `True`. It defaults to `False` and is
  set only on the line right after `protected(Path(__file__), 'installed')`
  returns;
- `type(error) is Refusal`, the exact type;
- `error.record['site'] in INTERNAL_SITES[mode]`;
- `error.record['phase'] == 'initial'`.

Steps:

1. Build `{"schema":"unix-protection-f0-internal","version":1,"mode":m,
   "refusal":R}` in that fixed key order. `R` is `Refusal.record`, whose key
   order is fixed by `Refusal`.
2. Encode it with `json.dumps(..., separators=(',',':'), ensure_ascii=True)
   + '\n'`.
3. Validate it with the same internal validator the reader uses.
4. Require that it is at most 1024 bytes. An oversize record is refused,
   never truncated.
5. Open the stage with `open_stage(sys.argv[2])`.
6. `os.open(CHANNELS[m], O_WRONLY|O_CREAT|O_EXCL|O_NOFOLLOW|O_CLOEXEC,
   0o600, dir_fd=stage)`. `O_EXCL` gives first-writer-wins.
7. `fchmod(fd, 0o600)` makes the mode independent of umask.
8. `fstat` must show a regular file with uid 0, `nlink == 1` and size 0.
9. Call `os.write` exactly once and require the return value to equal the
   length. A short write is left as is, and the reader classifies it as
   `malformed`.
10. Close every fd in `finally`.

Any `Exception` is swallowed. The writer never changes the internal exit
code, the original error, the stderr line or any effect, and it prints
nothing.

The record contains no pid, uid, gid, path, argv, environment, timestamp or
payload. A later same-mode writer gets `EEXIST` and swallows it. Its
refusal is lost and not counted. Writers for different modes use different
files and never contend.

### 4. Cgroup progress marker (service only, best-effort)

The marker goes in `service()`, immediately before `control.mkdir(...)`:
`os.open('progress-cgroup', O_WRONLY|O_CREAT|O_EXCL|O_NOFOLLOW|O_CLOEXEC,
0o600, dir_fd=open_stage(run))`. Then `fchmod` to 0600, close everything and
write nothing.

Every exception is swallowed. If the marker failed, the bound simply comes
out lower than reality, which is safe for a lower bound. The marker never
adds a service failure path, and it does not change `service`'s effects or
the order of its checks.

### 5. Reader and quiescence (outer `run_body` `finally`)

The only structural change in `finally` wraps the existing stop call. The
stop arguments and every fence stay byte-identical:

```python
if unit_created:
    stopped = None
    try:
        stopped = subprocess.run([...unchanged stop argv...], ...unchanged kwargs...)
    finally:
        snapshot_internal(run_id, unit, stopped)   # total: never raises
    cg = Path('/sys/fs/cgroup/system.slice') / unit
    require(not cg.exists(), 'E_QUARANTINE_CGROUP')   # unchanged, independent
fixture_cleanup(stage)
...
```

Properties:

- **The stop is not assumed to have succeeded.** `snapshot_internal`
  computes `snapshot_available = stopped is not None and cgroup_absent`:
  - `stopped is None` means the stop raised, for example `TimeoutExpired`.
  - `cgroup_absent` holds only when `os.lstat(cg)` raises
    `FileNotFoundError`. Any other result, including another `OSError`,
    counts as not absent.
  - The return code is ignored (fact 5).
- **Why cgroup absence is the signal.** A cgroup v2 directory cannot be
  removed while it still has processes or child groups, and no process can
  enter a group that no longer exists. Absence therefore means no process
  remains in the unit subtree. Under the existing containment assumption,
  this permits a conditional, bounded post-stop snapshot of writers remaining inside that subtree.
  It does not prove that a root writer has not escaped the subtree, atomic
  consistency across files, immutability or complete capture. This is a
  source inference, **unverified on real hardware**. Bounded strict reads
  must remain safe even if privileged concurrent modification occurs.
- **Not snapshot_available.** All three statuses are `unknown`, and no internal file
  is opened. A writer may still be alive. Separate existence-only progress
  checks may open the trusted stage and stat marker metadata; they never
  open marker contents. “No open” tests refer to feedback channels only.
- **No fence is weakened.** The reader's own `lstat` grants nothing. The
  existing `require(not cg.exists())` still runs independently afterwards.
  It still raises `E_QUARANTINE_CGROUP`, and with that still skips
  `fixture_cleanup` and `rmtree`. If the stop raised, that exception still
  escapes `finally` unchanged after the snapshot.
- **The reader is total.** It catches `Exception` internally and assigns
  only module globals. `BaseException` keeps the SP-A3 no-record case.
- **Order.** The snapshot runs after the stop returns or raises. It runs
  before the quarantine check, `fixture_cleanup`, `protected(stage,
  'run_cleanup')` and `rmtree`. It closes all of its fds before returning.

When snapshot_available, the reader opens the stage with `open_stage(run_id)`. If
that fails, all three statuses are `unknown`. For each mode in `PRECEDENCE`:

1. `os.open(CHANNELS[m], O_RDONLY|O_NOFOLLOW|O_NONBLOCK|O_CLOEXEC,
   dir_fd=stage)`. `ENOENT` means `absent`. Any other error means
   `malformed`.
2. `fstat` must show a regular file (`O_NONBLOCK` keeps a FIFO from
   blocking, and the type check rejects it), `st_uid == 0`,
   `S_IMODE == 0o600`, `nlink == 1` and `0 < size <= 1024`.
3. Read at most 1025 bytes in a bounded loop. The byte count must equal
   `st_size` and be at most 1024.
4. The bytes must be ASCII, end with exactly one newline and contain no
   other newline.
5. Parse with the duplicate-key-rejecting hook. Require the exact key sets
   and exact types. `schema` and `version` must match. `mode` must equal
   `m`. Validate the refusal with the existing refusal rules: `decide`
   agrees, the ordinal is last, and there are at most `MAX_TRIPLES`
   triples. The site must be in `INTERNAL_SITES[m]` and the phase must be
   `initial`.
6. Rebuild the canonical line from the validated fields in fixed key order,
   and require it to equal the bytes read exactly. This rejects padding,
   reordering and alternative encodings.
7. If every step passes, the status is `captured`. Any failure gives
   `malformed`. The cause is never recorded. Bytes are never printed,
   logged, put into an exception or kept.

Statuses are all-or-nothing on `unknown`: either all three modes are
`unknown`, or none is. The reader never uses mtime, ctime, inode numbers or
read order as a time order.

### 6. Progress lower bound (existence only)

This step runs in the same snapshot, whether or not the conditional
completion signal is available. Monotone file creation is a source assumption
about these writers, not a guarantee against privileged concurrent changes.
The result describes bounded metadata observed at read time only; no atomic
consistency, complete capture or escaped-root coverage is claimed. If the
stage fd is valid, it checks two entries with
`os.stat(name, dir_fd=stage, follow_symlinks=False)`. An entry counts only
if it is a regular file with `st_uid == 0`. Contents are never read.

- `progress-cgroup` raises the bound to `cgroup`. It means the service
  reached the line before `control.mkdir`. It does not prove that the
  mkdir, delegation or cleanup succeeded.
- `fixture-identity` raises the bound to `account`. It means a capture saw
  an `arxexec<pid>` account with uid ≥ 1000. It does not prove that this
  fixture created the account, and it does not prove the account was
  removed.

The order is `none < staged < service < cgroup < account`. The field is
renamed `mutation_lower_bound`. The outer tokens `staged` and `service` are
assigned exactly as today. A failed or unverified check never raises the
bound, and `unknown` or `malformed` statuses never raise it either. A lower
bound can never prove that something did not happen. It also tells nothing
about how far the account or cgroup work got, or whether it finished.

### 7. Record version 2 (outer line, ≤ 4096 bytes)

Same envelope as v1, with these changes:

```text
"version": 2
"mutation_lower_bound": one of MUTATIONS = ('none','staged','service','cgroup','account')
"internal": null
          | {"service":S,"fixture-check":S,"harness":S,
             "carried": null|"service"|"fixture-check"|"harness",
             "refusal": null|<refusal record>}
S ∈ STATUSES = ('absent','malformed','captured','unknown')
```

The validator adds these rules to the v1 rules, which keep applying with
`mutation` renamed:

- **V1.** `internal is None` exactly when `mutation_lower_bound` is in
  `{none, staged}`. A unit was created exactly when the bound is at least
  `service`.
- **V2.** `internal` has the exact key set and exact `str` statuses from
  `STATUSES`.
- **V3.** Either all three statuses are `unknown`, or none is.
- **V4.** `carried` is the first mode in `PRECEDENCE` with status
  `captured`, or `null` if there is none. `refusal` is null exactly when
  `carried` is null. The refusal passes the internal refusal rules for
  `carried`: an allowlisted site, phase `initial` and `decide` agreeing.
- **V5.** Success means `final is None`. The v1 rule was `mutation ==
  'service'`. It becomes: the bound is in `{service, cgroup, account}`
  **and** all three statuses are `absent`. A success line with an
  `unknown`, `malformed` or `captured` status is rejected with `E_RECORD`.
  That run then exits 1 through the existing closed fallback, so an
  anomaly is never reported as acceptance. This is an intentional narrow
  deviation from the original blanket reader-exit invariance: a telemetry
  anomaly on otherwise successful execution produces only the closed
  E_RECORD fallback and exit 1. Do not invent first/final exceptions or
  cleanup failure to explain it. Existing body/stop/cleanup exceptions,
  first/final classification and cleanup tokens remain unchanged.
- **V6.** A status never raises the progress bound. The writer computes the
  bound only from the outer tokens and the existence checks, and the
  validator has no rule that ties the two together.

`PRECEDENCE` is a fixed display order for the single carried payload. It is
**not** a time order or a cause. A `captured` status does not claim that
that refusal caused the outer `E_SERVICE_OR_TEST`. Provenance stays the
outer envelope's association: matching reviewed source and workflow SHAs,
plus run and attempt. The internal file carries no provenance of its own.
Root wrote it inside this run's root-owned stage, and that is association,
not attestation.

The emitter produces v2 only. `replay` accepts exactly version 2. No real
v1 line exists, because F0 was never dispatched, so keeping v1 parsing
would only add attack surface (review question Q4). Overflow still raises
`E_RECORD_BOUND` and never truncates.

**Bounds.** Each internal line is at most 1024 bytes, which the writer and
the reader both enforce. The outer line is at most 4096 bytes, which
`encode_record` enforces.

The F0 review measured 1596 bytes for two maximal outer refusals. A
maximal internal refusal needs at most 32 triples, with every predecessor
passing (`["other",true,0]`). By a design-time estimate, that keeps a
carried refusal under about 800 bytes, and the whole v2 line near
2.5 KiB. The implementation must **measure** the true maximum, not assert
this estimate. The measurement takes the maximum over every carried mode
with its longest allowlisted site, 20-digit run and attempt values, maximal
first and final refusals, and the worst-case status strings. A test must
pin that maximum at or below 4096 bytes.

### 8. Asynchronous and concurrent writers

- The service, harness and fixture-check processes can all be alive at the
  same time. Each writes only its own file.
- Repeated or concurrent `fixture-check` calls: only the first `O_EXCL`
  creator writes. Later refusals are lost, and the record does not count
  them. "First" means the first to create the file. It is not the first
  refusal by any other measure.
- A writer that dies between creating the file and writing to it leaves an
  empty or short file. If the reader reads it while snapshot_available, the status
  is `malformed`. While not snapshot_available, the reader opens nothing, so the
  status is `unknown`.
- A writer that escaped the unit cgroup could create a file after the
  reader saw `absent`. Escaping requires root to write another
  `cgroup.procs`. Such a writer is already outside the existing cleanup
  fence. The snapshot reports only what was present at read time, and its
  strict validation still applies (see Known unknowns).

## Required tests (for the implementation PR; none run here)

Put them in a new `tools/unix-exec-validation/test_f0b_transport.py`. All
runtime effects must be mocked, and no test may run with privilege.

1. **Pure-mock layer.** These tests run on every platform. A fake `os`
   namespace supplies the flag constants and fake `open`, `fstat`, `read`,
   `write`, `fchmod`, `close`, `lstat` and `stat`. A guard in it rejects any
   path outside the fake tree. `driver.subprocess`, `Path` effects,
   `fixture_cleanup`, `rmtree`, `cg_preflight` and `copy_tree` are patched,
   as `test_f0_blockers.scenario()` already does. No test touches `/opt`,
   `/sys`, `/proc`, `/var/lib`, systemd or a real account.
2. **Owned temp, non-root (optional, POSIX only, skipped elsewhere and
   given no credit when skipped).** These run in a `tempfile` directory the
   test owns. They show real kernel behaviour only:
   - `O_NOFOLLOW` on a symlink gives `ELOOP`;
   - `O_EXCL` gives `EEXIST`;
   - `O_NONBLOCK` on a FIFO does not block;
   - a hard link gives `nlink == 2`.

   The uid-0 predicate is satisfied by a test-only wrapper that rewrites
   `st_uid` in patched `fstat` results. **The driver gets no uid parameter
   or other production seam** (lesson from ST-A3).
3. **Isolated counterfactuals** (lesson from ST-B2). Each case is a
   fixture that is fully valid except for one property. It must be refused
   for exactly that property:
   - **Writer:** `internal_trusted` false; a `Refusal` subclass; a site
     outside the allowlist; an `installed` site; phase `final`; an encoding
     over 1024 bytes; a stage that is a link at any component; a non-root
     component; a group- or other-writable ancestor; a stage mode with
     `0o070` bits; a create that sees an existing file; a post-create fstat
     with `nlink` 2; a short write. A writer exception must leave the exit
     code, the error and the stderr line identical.
   - **Reader:** a link; a FIFO; a directory; wrong owner; mode 0640; nlink
     2; size 0; size 1025; a byte count different from `st_size`;
     non-ASCII; no newline; two lines; a duplicate key; a non-canonical
     whitespace pad; reordered keys; a wrong schema or version; a
     mismatched `mode`; a site outside the allowlist; phase `final`;
     triples that disagree with `decide`.
   - **Hostile bytes:** the unique hostile bytes from every case must never
     appear in stdout, stderr, the record or any exception argument.
4. **Ordering through the real `run()`** using a call log. Cover these
   orderings:
   - stop returns rc 0 and the cgroup is absent, so files are read;
   - stop returns rc 5 and the cgroup is absent, so files are still read
     (the return code is ignored);
   - stop returns and the cgroup is present: statuses are `unknown`, no
     open happens, `E_QUARANTINE_CGROUP` is raised, and `rmtree` is called
     0 times;
   - stop raises `TimeoutExpired`: statuses are `unknown`, `final` is
     `E_COMMAND_TIMEOUT`, `fixture_cleanup` and `rmtree` are called 0
     times, and cleanup is `unconfirmed`;
   - the snapshot runs before the quarantine check, `fixture_cleanup`,
     `protected('run_cleanup')` and `rmtree`;
   - the fds opened equal the fds closed;
   - for an existing body, stop or cleanup failure, a reader exception
     changes neither its exit/classification nor any cleanup
     token.
5. **Record:**
   - real entry/run with all effects mocked covers otherwise successful
     execution with unknown, malformed and captured statuses, including
     reader-exception-to-unknown: fixed E_RECORD stderr and exit 1, without
     fabricated first/final refusal or cleanup-failure tokens;
   - each V1–V5 rule rejected by a valid-JSON line that violates only that
     rule;
   - `carried` precedence over all eight captured/non-captured
     combinations;
   - `unknown` and `malformed` on success rejected;
   - the measured maximal v2 line at or below 4096 bytes and the internal
     line at or below 1024 bytes, both pinned;
   - a v1 line rejected.
6. **Progress:**
   - a marker that is a link, a non-regular file or not owned by root never
     raises the bound;
   - `fixture-identity` contents are never read: an open of that name is an
     assertion failure;
   - progress is computed even when not snapshot_available;
   - unknown or malformed statuses never move the bound;
   - the marker's failure is swallowed, and `control.mkdir` is still
     reached in the same order.
7. **Compiled mutation kills.** These follow the same in-memory runner
   rules as `test_f0_blockers.mutations()`: anchors occur exactly once, a
   compile error or an errored run earns no credit, each kill needs a named
   assertion failure, the module is restored, and the source SHA-256 is
   checked. Each mutant must be killed by its isolated case:
   - `writer_ignores_trust`
   - `writer_ignores_installed_flag`
   - `writer_isinstance_refusal`
   - `writer_site_unchecked`
   - `writer_drops_excl`
   - `writer_drops_nofollow`
   - `writer_walk_skips_ancestor`
   - `writer_ignores_stage_mode`
   - `writer_truncates`
   - `writer_raises`
   - `reader_follows_link`
   - `reader_drops_nonblock_typecheck`
   - `reader_ignores_owner`
   - `reader_ignores_mode`
   - `reader_ignores_nlink`
   - `reader_unbounded`
   - `reader_ascii_dropped`
   - `reader_noncanonical_accepted`
   - `internal_site_unchecked`
   - `reader_accepts_final_phase`
   - `reader_mode_mismatch`
   - `reader_reads_unquiesced`
   - `reader_requires_rc_zero`
   - `reader_replaces_cg_fence`
   - `reader_after_cleanup`
   - `reader_raises`
   - `fd_close_dropped`
   - `carried_precedence_changed`
   - `success_accepts_unknown`
   - `progress_counts_link`
   - `progress_from_status`
   - `v1_accepted`
   - `outer_overflow_truncates`

   These 33 names are candidate mutations, not earned kills. Equivalent
   mutants receive no credit, including final-file NOFOLLOW removal when
   EXCL independently refuses the existing symlink. Do not make a fake OS
   follow that link to manufacture a kill. Ancestor NOFOLLOW remains a
   separate guard. Split combined nonblock/type tests if claiming each
   independently, asserting flags before a fake FIFO open to avoid hangs.
   Reassess ASCII/canonicality and duplicate trust-flag equivalence after
   implementation. Every negative case must satisfy all other schema,
   ownership, size and canonicality conditions.
8. **Unchanged surfaces:**
   - `git diff` is empty for both workflows, `disposable.rs` and all Rust
     source;
   - an AST test confirms the stop argv and kwargs, the `require(not
     cg.exists(), 'E_QUARANTINE_CGROUP')` line, `fixture_cleanup`,
     `protected(stage,'run_cleanup')` and `rmtree` are textually unchanged
     in `run_body`;
   - retain all existing F0 behavioral obligations while migrating record
     expectations to v2. Permit narrowly scoped mutation-anchor and
     counterfactual migrations in test_f0_blockers.py: V5 replaces the old
     xf_success anchor, and its invalid-success case must satisfy V1 so
     that only the success rule can reject it. Re-isolate every legacy
     mutant against the new cross-field rules, preserving named assertion
     attribution and zero errors/compile-failure credit. A count of 23
     is not a substitute for those properties; report equivalents honestly;
   - the SP-B2 sink AST test is extended to the new subprocess-free code,
     which adds no subprocess call.

## Delivery

- **One PR:** `feat(validation): bounded internal refusal transport (F0b)`.
  It changes only `driver.py`, the new test file, `test_f0_blockers.py`
  (v2 expectations plus narrowly scoped anchor/counterfactual migration), the tool README and the F0 evidence file. Before
  publication the coordinator updates PROGRESS separately.
- **Build and review roles.** Sol builds. Opus high does the independent
  source and security review. Sol high design review and the narrow correction recheck are complete;
  independent source review remains required.
  Three failed fixes trigger a provider handoff, as usual.
- **Base.** PR70 merged as `affaec9`, tree-identical to authoring `f0baed4`.
  Re-pin onto current main before implementation/publication. The separate
  CI discovery follow-up is PR71, head `e14ae4b`, pending in this record;
  do not duplicate its one-line change in F0b.
  It merges only through CI, `ci-time-budget` and a different-provider
  `agent-review`.
- **README limits.** The README must say four things:
  - the coverage table;
  - `absent` is not success;
  - `installed` is never captured;
  - progress is a lower bound.

## Known unknowns

- The exit status of `systemctl stop` for a collected `--collect` unit on
  the hosted image is unknown. The design does not depend on it.
- U9: whether systemd removes the delegated subtree when the unit stops. If
  it persists, the existing fence quarantines and all statuses become
  `unknown`. That loses information but stays safe.
- Whether a root process inside the unit could migrate out of the subtree
  before the stop. The existing cleanup fence carries the same assumption.
- Whether `disposable.rs` matches the post-0019/0020 Runner contract
  (SP-A4).
- Whether the hosted image meets the host floor.
- The measured maximum v2 line size. The implementation must pin it.
- A pre-existing observation, out of scope for F0b: `Path.exists()` follows
  symlinks. cgroupfs does not allow symlinks, so this is noted, not
  changed.

## Review needs (questions for the independent Sol high review)

- **Q1. Shape deviation.** The accepted contract had one `internal` slot
  plus `internal_count`. This design has three fixed per-mode statuses plus
  one carried payload, and drops `internal_count`: statuses can derive
  only the number of captured mode slots, never refusals or writers. Does this keep the contract's bound and intent?
- **Q2. Quiescence.** It is the stop call returning plus `lstat`
  `ENOENT` on the cgroup. The return code is deliberately ignored. Is that
  sufficient, and is `unknown` the right fail-closed result?
- **Q3. V5.** A success with any non-`absent` status becomes exit 1 with
  `E_RECORD`. Is this acceptable fail-closed behaviour, or a change to
  success semantics that needs its own disposition?
- **Q4.** Should v1 replay be dropped, since no real v1 line exists?
- **Q5.** Is wrapping the stop in `try/finally` equivalent for all fences
  and exception ordering?
- **Q6.** Is the root-down descriptor walk preferable to the contract's
  single stage open? Is the best-effort marker swallow acceptable?

## Separate small follow-up: CI coverage (do not bundle)

`ci.yml` (`c84515f4…`, line 37) runs:

```text
python -I -m unittest discover -s tools/unix-exec-validation -p test_driver.py
```

That pattern excludes `test_f0_blockers.py`. So the 32-test F0 regression
suite, including the replay validator, the orderings and the SP-B2 sink
test, is not a required check. Only the manual
`unix-execution-experiment.yml` runs `test_*.py`, and nothing dispatches
it.

The smallest fix is to change the pattern to `'test_*.py'`. That picks up
`test_driver.py` and `test_f0_blockers.py` today, and F0b's new file later.

Reviewers ran the F0 suite with Python 3.12.10 on Windows. Linux and macOS
runs under `-I` are unverified until CI runs them. The in-memory mutation
runner stays manual unless a separate budget decision adds it; the 15-minute
budget must hold.

This is a workflow change, so it needs its own one-concern PR and its own
review. Ideally it lands before F0b is published, so F0b's tests are
required from the start. It must not ride along in the F0b PR.

## Gates (unchanged)

- No real captured record exists, so the real-record replay gate is
  **OPEN**. There is no hypothesis, reproduction, diagnosis or fix.
- F2 is undispatched. Nothing here infers authority from a new SHA, and
  nothing authorizes a historical retry. D7 stands.
- M2 is incomplete, and real-Runner PA6 is **OPEN**.
- All native, installed and dedicated paths are **unverified on real
  hardware**.
- F0b implementation or execution is **not authorized** by this document.
  An independent Sol high review must come first.

## Coordinator acceptance clarifications

The independent reviews resolve Q1-Q6 under the conditional bounds above.
Use a neutral name such as snapshot_available for the completion signal.
In addition to the candidate mutation list, include ancestor-walk NOFOLLOW
removal and isolated malformed-success and captured-success counterfactuals
with named candidate mutants. Earn kills only through isolated assertions;
equivalents receive no credit. These additions strengthen the same contract
without changing runtime authority or the explicitly accepted limits.
PR71 is already merged: do not change CI discovery again. Earlier pending
progress entries are historical, superseded by the latest current checkpoint.
