# Stage F A1/A2 source alignment evidence

2026-11-14; OpenAI / gpt-6.1-sol / high; implementation sub-agent, no delegates.
Source preparation only. Fresh independent Grok source review is required before
normal publication; no publication, merge, CI dispatch or native run occurred.

Source commit: `7fa9129e22df58218fcb7c083120f168ddfbfd24`.
Clean isolated starting top: `be5bed8` on `t3/stagef-a1a2-linux-source`; cwd matched
Git top-level. Read AGENTS, KICKOFF, engineering skill/profile, the full accepted
proposal and Grok review, ADRs 0019/0020 and the exercised source call graph.
Accepted basis: proposal `a3d02c3` (equivalent to `b0466f3`), review retained as
`7e0b01f`, coordinator single-capture correction `be5bed8`.

The driver prebuilds one protected libtest before runtime unit installation.
The fixed packaged `arxd.service` and standard drop-in are copied unchanged.
Only the fixture ExecStart reset/launcher, Restart=no and RuntimeMaxSec=140
are overridden. The launcher exclusively creates private NOFOLLOW output,
dup2s both descriptors and execs the exact A1/A2 vector with the five disposable/
notify environment entries. It hands feedback attribution to the existing
harness channel in the same process before exec; tokens/order/bounds stay fixed.
No service-side Python monitor or Cargo/timeout child remains.

The ignored A1/A2 test uses the production Linux port and setup algorithm.
Its generic orchestration lives beside the existing fake-port tests, so the
same setup-before-acceptance path can run on ordinary Windows CI without the
native adapter. Setup alone creates runs/enables controllers. The fixture
checks A2, sends READY, holds for the private identity ack, then returns so
libtest can emit its trailing summary. Outer compares MainPID/exe/supervisor
while held, enforces the 1 MiB cap and 120-second deadline, then requires the
exact selected test, one-pass/zero-failure summary and systemd success/status.

A fixed exclusive reservation plus nonblocking flock serializes invocations.
Loaded units, aliases, search-path files/drop-ins, runtime directory and fixed
cgroup collisions refuse before installation. Cleanup checks captured runtime
identities and the fixed fragment before stopping only the owned unit (40-second
outer bound), snapshots feedback under existing rules, requires cgroup absence,
removes exact owned files, reloads and safely removes the captured stage.
Partial installation or uncertain cleanup retains the reservation/evidence and
quarantines the environment. A1/A2 never calls legacy account cleanup. The old
ignored cross-account fixture and its guard assertions remain intact/unselected.

Checks run on the source commit (all local, offline where applicable):

- `python tools/unix-exec-validation/mutation_check.py`: 58 audited Python
  tests pass; 11 driver mutants and four Rust orchestration mutants killed by
  named assertions, zero error credit. Green baseline, exact-once anchors,
  successful mutant compilation and unchanged source fingerprints required.
  Rust stdin harness includes real pure parse/setup/fixture-helper slices and
  the actual Cancellation implementation; no Linux adapter or native test.
- `python tools/unix-exec-validation/test_f0_blockers.py --mutants`: 23 kills.
- `python tools/unix-exec-validation/test_f0b_transport.py --mutants`: 36 kills,
  four explicit equivalents, no error credit. Measured internal maximum 757
  bytes; outer maximum 2447 bytes; existing 1024/4096 caps unchanged.
- `cargo test -p arxd --lib --locked --offline exec::hierarchy::fake:: -- --skip h15_panic_payload_subprocess --test-threads=1`:
  26 pass, including the three new A1/A2 tests. H15 deliberately excluded.
- `cargo test -p arxd --lib --locked --offline exec::hierarchy::parse::`:
  11 pure parser tests pass.
- `cargo clippy -p arxd --tests --locked --offline -- -D warnings`: pass on
  Windows after correcting one collapsible-if warning in the new fake test.
- `cargo fmt --all -- --check` and `git diff --check`: pass.
- AST equality against the starting source confirms unchanged F0/F0b closed
  tokens, site inventory, modes, precedence and bounds. Driver SHA256 after
  final mutant checks: `a0a1a975520fe4d23e32aaf9f8b5e27d9af99b1808413c01b7a73aeaf5426752`.

The audited Python suite is `mutation_check.safe_suite()`: Guards excluding its
CLI subprocess case, A1A2, CgroupScan (walk/lstat fully mocked), F0Blockers and
F0b Transport/Records/EntryRun (host effects fully mocked). RuntimeGuards,
CopyGuards, actual driver CLI, protected/stat/proc/sys/native operations were
not executed. Superseded harness/zero-test RuntimeGuards cases moved into the
pure A1A2 coverage; legacy account cleanup tests were retained.

Failure/coverage limits: an attempted
`cargo check -p arxd --tests --locked --offline --target x86_64-unknown-linux-gnu`
stopped in libsqlite3-sys because `x86_64-linux-gnu-gcc` is unavailable. No
compiler was installed or alternate environment used. Therefore the complete
Linux ignored fixture/native adapter combination has not been compiled here.
Pure orchestration was compiled/executed on Windows; no native blocked-call,
service-stop, cgroup removal or hard-cancellation cleanup is proven. Early
mock adaptations and one fake timeout precondition needed correction; final
reported checks above passed. No check is inferred from CI or historical runs.

Byte-identical protected blobs at the source commit:

- packaged unit `0b337773c60e49fe84de76db018d2235ae076254`;
- standard drop-in `93a3abcce9782cc7fddf4c2c1dcc5fc74ec18a74`;
- manual workflow `e5f1a96dbfa522bda602db45bf24db000ebddd5c`;
- production setup `b33bed7cd5d7022f30c039e761c544567304d28c`;
- production Linux port `ae26fad095242b2d01e48e84e8695261e295cad3`;
- production parser `54dd52043e863d12189fda7216da43e095c96ffb`.

No PROGRESS, ADR, proposal, review, workflow, packaging, Cargo, policy or
activation changes. Manual owner/reviewed-SHA controls remain unchanged.
Historical run 43091339273 has unknown cleanup; its entire environment is
excluded from reuse. No diagnosis, historical retry or native authority follows
from this commit or a NEW_SHA. M2/F/G and real-Runner PA6 remain OPEN;
**unverified on real hardware**. Optional Windows/launchd tracks stay parked.

Publication correction: the driver hash above is the committed LF content hash
(blob c0f4f9ff487878293ab31f5c74ed16f1f842cda8), independently recomputed by
the reviewer and coordinator. The earlier bec6460b citation was incorrect.
No source bytes or test credits changed.
