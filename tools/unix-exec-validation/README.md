# Disposable Linux experiment tooling

This is a guarded manual CI experiment, not a local provisioning tool. Run only
the pure tests on a development machine:

```powershell
python -m unittest discover -s tools/unix-exec-validation -p 'test_*.py' -v
```

`driver.py run` refuses wrong platform/context before any mutation or privileged
probe. Internal modes also require root and the exact protected installed path.
Do not invoke `run`, `service` or `harness` locally, even with spoofed CI variables.
Environment checks are accidental-use guards, not host attestation.

The public CLI accepts only `guard` or `run`, with no parameters. Run identity is
two bounded positive decimal GitHub run numbers. The driver reserves an exclusive
`/opt/arx-ci-<run>-<attempt>` directory and same-named transient systemd unit.
Source, registry cache and Rust 1.95.0 toolchain are copied to root-private paths;
links and special files refuse copying. Copying pins directory descriptors,
uses O_NOFOLLOW for every open, verifies opened inode/type identity, and writes
only into exclusive private staging. This prevents link-swap dereferencing; it
does not attest a concurrently edited checkout by the trusted runner user. No fixture task account can replace the
source, driver, Cargo, rustc, compiled executable or runner. Cargo is offline,
locked, and receives only the explicit environment in `cargo_env`; no token,
credential file, runner environment dump, or Git config is copied. Checkout's
credential persistence is disabled. Compilation happens before fixture creation.

Read-only preflight requires systemd, cgroup2, available memory+pids, protected
ancestry and a bounded (20,000-entry) hierarchy scan without links or group/other
write grants. Unrelated delegated users are allowed. The unit ancestors and
whole subtree must remain root-owned. After account creation and before the
first Runner call, a synchronous internal fixture-check repeats the scan and
refuses any entry owned by the generated UID or GID anywhere. Migration requires
write access to both destination and common ancestor cgroup.procs; root-owned
protected shared ancestors prevent migration into unrelated delegation.
There is no claim that GitHub's default kernel/image satisfies these requirements.

Only the generated service gets `Delegate=yes`. The driver moves itself into a
new `control` child before enabling memory+pids in the service root, then makes
an empty protected `runs` child with those controllers enabled. No global root
controller settings are written. The library's per-run children live below
`runs`; the fixture UID gets no cgroup ownership or write grants.

A root-owned Cargo runner verifies the executable and exact libtest arguments,
checks fixture/account/group absence and records its PID before exec. The harness
itself creates `arxexec<PID>`, its private group and `/var/lib/arxexec<PID>`.
The service captures fixture inode and numeric UID/GID while the test is alive.
If residual resources lack capture, cleanup quarantines rather than guessing.
The harness currently does not emit the observed UID; success means its internal
identity assertions passed, not that an independent UID trace was collected.

Finally, stopping only the captured transient unit must remove its cgroup tree.
Residual account cleanup requires captured UID/GID, expected shell/group, no live
UID processes, and matching protected fixture path/inode. Removal uses symlink-safe
`rmtree`; no glob account deletion or recursive cgroup deletion exists. An orphan
group or changed resource fails cleanup. Failure retains root-private staging for
quarantine and fails the job; the fresh hosted VM is discarded. A hard workflow
cancellation/VM termination cannot guarantee Python finally runs; discarding the
host is the last boundary, not proof of successful cleanup. Do not retry probes.

The ten-minute job bounds a 360-second build, a 120-second exact Cargo invocation,
and a transient service with 140-second runtime plus ten-second stop timeout.
The service output is drained through a bounded 1 MiB collector; overflow fails
the test. No inherited RLIMIT_FSIZE restricts Cargo/rustc artifact writes. Public evidence is restricted to fixed
a validated F0 line or fixed fallback error codes. Raw build/test output stays
root-private and is not uploaded; no panic text or environment is printed.

Primary references checked during preparation:

- [Checkout at the existing pinned SHA](https://github.com/actions/checkout/blob/11d5960a326750d5838078e36cf38b85af677262/README.md), including `persist-credentials`.
- [Rust toolchain action at the existing pinned SHA](https://github.com/dtolnay/rust-toolchain/blob/683b588c487a69d01f6c6ddf40f036ff682cc2c8/action.yml).
- [Kernel cgroup v2 documentation](https://docs.kernel.org/admin-guide/cgroup-v2.html), including no internal tasks and controller delegation.
- [systemd resource-control primary source](https://raw.githubusercontent.com/systemd/systemd/main/man/systemd.resource-control.xml) and [systemd-run primary source](https://raw.githubusercontent.com/systemd/systemd/main/man/systemd-run.xml), covering delegation and transient services. The rendered freedesktop page timed out; upstream manual sources were fetched successfully. Installed-runner behavior still needs independent review and CI evidence.

Static tests are guard/planning evidence only. Runtime setup, systemd trimming,
Cargo runner arguments, account cleanup and Linux containment are **unverified
on real hardware**. The first reviewed hosted dispatch at `5b4f6b2` failed
with `E_PROTECTION` before any harness-pass or cleanup-confirmed evidence. Its
mutation phase is not established. The reviewed-SHA variable was cleared; no
retry occurred. See `docs/evidence/unix-execution-first-dispatch.json`.

Git verification uses command-scoped safe.directory for the exact checkout path;
it does not depend on the hosted image's global wildcard safe.directory setting.
It still compares HEAD to the exact selected SHA. The trusted checkout/build
steps remain the source provenance boundary (this is not content attestation).

Safe mutation verification: `python -I tools/unix-exec-validation/mutation_check.py`.
It executes the real mutated driver functions through the same mocked regressions,
never provisioning accounts, services or real cgroups. Ordinary CI also runs the
pure tests; the manual workflow guards, pins and default-off variable are unchanged.


### F0 local artifact/replay mechanism (no dispatch authorization)

`protected(path, site)` uses 13 fixed call-site tokens. The shared pure
`decide(triples)` preserves the historical link / owner-zero / write-mask
predicate and conjunct order. Regular files remain permitted. Triples are
ordered; a record must end at its first refusal. At most 32 observations per
refusal are retained; overflow refuses rather than producing truncated evidence.

`replay(line)` accepts only an ASCII JSON line including its newline within
4096 bytes, with exact schema/version and fields. Duplicate fields, invalid
types, unknown tokens, inconsistent predecessor/refusal order and oversized
input/output are refused. Provenance carries matching reviewed source/workflow
SHAs and bounded decimal run/attempt association inside that same line. This
association is supplied by the existing gates, not cryptographic attestation.

The outer `run` entry emits one validated line with separate first/final
exceptions, conservative mutation boundary and cleanup disposition. Closed fixed RuntimeError codes and command timeout/failure codes are retained;
OS errors use E_OS. Other arbitrary
exceptions become `E_RUNTIME` without formatting their payloads. Cleanup still
fails closed; an unconfirmed environment cannot be reused. Existing workflow
fences remain unchanged; all outer subprocess stderr is explicitly suppressed.

F0b emits/replays **version 2 only**, with `mutation_lower_bound` and
`internal`. Synthetic v1 input is rejected; no retained real v1 line exists.
Three fixed stage files carry at most 1024 bytes each; the complete outer
line remains bounded to 4096 bytes including its newline. Files use a root-down
descriptor-relative NOFOLLOW walk, a private root-owned stage, EXCL creation,
CLOEXEC, root-owned regular 0600 files and nlink 1. The first creator wins;
later same-mode refusals are lost. A short write becomes malformed.

| Internal site | Conditional capture |
| --- | --- |
| installed | Never captured: the channel ancestry is not yet trusted |
| service_stage, harness_stage | Only if the subsequent independent stage walk passes; stable trust failures cannot write |
| service_cg, service_runs | Escaping exact initial Refusal and successful writer |
| capture_base | Escaping exact initial Refusal; service-finally masking and first-creator loss remain gaps |
| harness_binary | Escaping exact initial Refusal before exec |

The snapshot follows stop (including a raised stop), before the unchanged
quarantine and cleanup fences. Only stop-returned plus cgroup lstat
FileNotFoundError permits channel reads; rc 0 and rc 5 are treated alike.
This is conditional evidence within the existing containment assumption,
**unverified on real hardware**. It proves neither escaped-root coverage nor
atomic consistency, immutability or complete capture. Other outcomes give all
three statuses `unknown`, with no feedback-channel opens. Bounded reads reject
special files, invalid metadata, duplicate keys and noncanonical payloads.
`absent` means no validated file was found; **absent is not success** and does
not mean no refusal occurred. Non-Refusal errors, termination and masked
exceptions can remain uncaptured. The single carried payload uses service,
fixture-check, harness display precedence, never chronology or causation.

Progress is an existence-only lower bound: none < staged < service < cgroup <
account. Root-owned regular progress-cgroup metadata means the service reached
the line before control.mkdir; fixture-identity metadata means capture saw an
account. Contents are never opened by this snapshot. Neither proves creation,
completion or cleanup. Missing/unsafe markers cannot prove no mutation.
Unknown/malformed/captured statuses never themselves raise progress. On apparent
success, any such status gives only fixed E_RECORD stderr and exit 1, preserving
actual first/final and cleanup state. Existing failures retain their semantics.

Safe synthetic mechanism tests (no filesystem observations, native commands,
provisioning or system driver execution):

```
python -I -B -c "import sys,unittest; sys.path.insert(0,'tools/unix-exec-validation'); import test_driver; r=unittest.TextTestRunner(verbosity=2).run(unittest.defaultTestLoader.loadTestsFromTestCase(test_driver.F0Mechanism)); sys.exit(not r.wasSuccessful())"
```

No retained real refusal line exists. The real-record replay gate remains
OPEN; synthetic tests are mechanism evidence only. No hypothesis, historical
reproduction, fix, retry or dispatch is claimed. A new source SHA does not
authorize an experiment. Fresh Opus high security review, parent merge/repin
and normal gates are required before publication. M2 remains incomplete,
real-Runner PA6 OPEN, and hardware paths **unverified on real hardware**.

F0 blocker regression checks (all runtime effects mocked in memory):

```
python -I -B -c "import sys,unittest; sys.path.insert(0,'tools/unix-exec-validation'); import test_f0_blockers as t; unittest.main(t)"
python -I -B -c "import sys; sys.path.insert(0,'tools/unix-exec-validation'); import test_f0_blockers as t; t.mutations()"
```

The legacy bounded runner compiles 23 independent mutations in memory, requires a
named assertion failure for each (compile errors earn no credit), restores the
module binding, and verifies the raw source SHA-256. It never writes mutations.
Success replaces the old harness/cleanup markers: first=null, final=null,
mutation_lower_bound at least service, three absent statuses and cleanup=confirmed in one validated line. first is the
original body exception (including pre-try failures), or null; final is the
escaping exception. Cleanup-only failure keeps first=null.
If association gates fail, the sole output is the closed fallback stderr line
`unix experiment refused/failed: <code>`. Record failures use E_RECORD.
BaseException, process termination and workflow cancellation may emit no record
and cannot prove cleanup. Internal coverage remains conditional as listed above; installed is never captured.


F0b mocked regressions and in-memory candidate ledger:

```
python -I -B -c "import sys,unittest; sys.path.insert(0,'tools/unix-exec-validation'); import test_f0b_transport as t; unittest.main(t)"
python -I -B -c "import sys; sys.path.insert(0,'tools/unix-exec-validation'); import test_f0b_transport as t; t.mutations()"
```

Every credited mutation requires a green baseline, exact-once anchor, successful
compile, zero test errors and named assertion failures; source bytes stay
unchanged. Equivalents receive no kill credit. See the F0b evidence ledger for
executed commands, maxima, migration and limitations. Fresh Opus high source/
security review is still required before publication. F2 remains undispatched.
