# M2 macOS launchd query design review

Disposition: the Sol high proposal is **accepted as a fail-closed design
contract**, with the advisories and pre-declared implementation blockers
below. Production macOS queries stay `Unknown` until a later reviewed
profile exists. This review grants **no implementation, native capture,
publication, or merge authority**.

Reviewer: xAI / grok-4.7, one pass, 2026-11-14. The assignment forbids
subagents, so Standards, Spec, and security are this single reviewer.
Independence is from the Sol proposal, not from a second reviewer. Label:
single-agent review. Coordinator acceptance remains separate.

M2 is **incomplete**. Real-Runner PA6 is **OPEN**. Linux Stage F evidence
and Stage G production wiring are **OPEN**. Installed, dedicated, and
macOS launchd paths are **unverified on real hardware**. Windows main71
run `43545244266` attempt 1 has cause **unknown**; no retry is authorized.
The optional Windows replay harness stays parked. No claim here changes
those states.

## Pins

Porcelain was empty on entry. The review cwd is the linked worktree top.
Its gitdir is that worktree's own gitdir and differs from the common
gitdir. Paths below are repository-relative. No owner path, account, or
machine path is recorded. Inspection was source, Git, and documents, plus
three public Apple documentation reads. No build, test, launchctl, system,
process, account, service, VM, private-state, install, CI, workflow,
dispatch, or GitHub write ran.

| Item | Identity |
| --- | --- |
| Review branch | `t3/m2-macos-launchd-review` |
| Review head | `de1819783335f3895891fa756fc09c8ded974471` |
| Review tree | `c018ed72ab40b0965c62460a944329b42cceb9c9` |
| Actual main | `4ec11b914039c81f5401fdfbe678201372742964` |
| Actual-main tree | `e3ac4fc4ea6ecbeb307a94610dc0a07e0ca089e7` |
| Service tree at main and at `aa49ce1e99475d07d63c6b0d23fa94bdb30bab74` | `f3ad2c9ab0864d18634505fd8279d17033729ce7` (identical) |
| Supplied proposal | `657d06a63ca1ab93392ad086b80097c1b911eb2a`, parent `78032cffef451463537c59da23e6c433e65aaa5e` |
| Retained proposal | `f560c218986cbeb80c3033241e8c94316efd4ab0`, same parent |
| Proposal tree at both commits | `0b598219bcc0905fafff02918c9ced07681f6896` |
| Proposal blob | `docs/reviews/m2-macos-launchd-query-proposal.md` `93f3c6ccc8725e40f542818826551e4b1fd71829` |
| Proposal LF SHA256 | `0c3a818ef0e78e859d3391adda3dcc7b8814d5eb5d2e830ae102a2bc250cbc62` |

The proposal file is 21233 bytes, CR count 0. `git diff --check` from
`78032cf` to `f560c21` is empty. `657d06a` is not an ancestor of this head.
`f560c21` is. The blobs match at `657d06a`, `f560c21`, and `HEAD`.

This head is behind actual main by `b34110a`, `2472ce9`, and `4ec11b9`, and
ahead of it by documentation commits. The service sources and
`docs/service-install.md` used below match the actual-main blobs. The audit
`docs/reviews/m2-acceptance-path-recovery.md` blob
`184d2c77dcaf12e777587a04df298afad895a99d` is in this head and is absent from
`4ec11b9`. It has audit-context credit only.

| File at `4ec11b9` | Git blob |
| --- | --- |
| `crates/arxd/src/service/owner_query.rs` | `0fdf14996649262b7b077eb189fa365af086aa01` |
| `crates/arxd/src/service/owner_unix.rs` | `282f7d373e2f9c1508bc8410ddfb3027c227bccb` |
| `crates/arxd/src/service/owner_plan.rs` | `32a36454578553d7f8256be98e837bdb3f8885a1` |
| `packaging/macos/com.arxcovo.arxd.plist` | `cbb41a5785ca2a0261539a2cf8d48584310af84f` |
| `docs/service-install.md` | `7a7a7bba1861e9a4c57651cb03255f4ccfbd2ea7` |
| `docs/evidence/macos-manager-observations.json` | `da8326bfcb260fecc33de342be7fa179cac7f44a` |

Documents read: `AGENTS.md`, `CONTRIBUTING.md`, `docs/KICKOFF.md`, ADRs
0009, 0014, 0015, 0018, and 0019, and the current service guide at the blob
above. The guide's baseline statement matches the code: macOS
`owner_query::launchd` returns `Query::Unknown`, and owner lifecycle refuses
`service-manager`.

## Standards

Repo rules that bind this proposal are `AGENTS.md` (honest hardware labels,
no claimed unrun checks, no secrets, no owner administration, milestone
scope), `CONTRIBUTING.md` (same limits), and `docs/KICKOFF.md` (v1 milestone
order, technical fences, no process theater). ADR 0014 is the lifecycle
contract. ADR 0015's ancestry and endpoint placement stay untouched. ADR
0018 keeps fixed owner errors and config-independent uninstall. ADR 0019
keeps native Unix execution inactive.

No hard standards breach. The proposal is documentation-only, names M2 as
incomplete, labels installed paths unverified, invents no sample transcript,
and refuses a diagnostic CLI, telemetry framework, replay harness, and
executor change. Two baseline smells are judgement calls:

- **Speculative Generality** in the proposal's nesting ceiling of 8 and line
  ceiling of 2048. Those numbers are safety caps for a future profile. They
  are not an observed launchd grammar. Coding them against a guessed
  delimiter now would freeze an imagined envelope. Keep them as review
  limits: a future fixture that exceeds them stays `Unknown` and comes back
  to review.
- **Mysterious Name** on "control structure" in the invalid-observation
  rule. The source PR should say which bytes are allowed. NUL is always
  invalid. TAB, LF, and CR are allowed only when the reviewed profile lists
  them as delimiters. Other C0 and C1 bytes stay invalid.

Smell baseline note: `LaunchdObservation` bundles fields that travel
together. That is the remedy for a data clump, not a new framework.
`bounded`'s Linux and mutating wrappers stay callers of a private helper.
Tooling does not enforce this document.

## Spec

The gap statement matches actual main.

`launchd` ignores both arguments and returns `Unknown`
(`owner_query.rs` lines 196–200). Tests lock that for empty, foreign, and
success-shaped bytes (lines 263–266). `Runner::query` runs
`/bin/launchctl` with `LAUNCHD_QUERY` (`print`, `system/com.arxcovo.arxd`)
and a 65536-byte cap (`owner_unix.rs` lines 770–784). `bounded` sets stderr
to null (line 820) and turns every unsuccessful wait into `Error::Manager`
before the caller can see an exit code (lines 857–859). `Err` becomes
`Unknown` (line 784). The parser cannot see an absence diagnostic.

`Manager::status` on macOS returns `not-installed` or `stopped` only for
`Query::Absent`, and `Error::Manager` for every other pair (lines 546–556).
`observe` rejects `Unknown` at `Gate::Always` before that match (lines
542–543). A parser that returned `Ours(Running)` would still fail status.
That part of the proposal is right.

`Query::active` is true for `Running`, `Reloading`, `Starting`, `Stopping`,
and `Loaded`, and false for `Stopped` and `Failed` (`owner_query.rs` lines
82–93). `Expect::Installable` and `Expect::Unloaded` use that predicate for
every platform (lines 105 and 112). ADR 0014 says macOS active is `Ours` in
any state, and macOS unloaded is `Absent` only. The Linux tests allow
`Stopped` and `Failed` through install (lines 308–316) and never pass
`Platform::Macos` for those states. `Gate::Loaded` already matches every
`Ours` (line 120), so bootout still runs for `Stopped`. The hole is install
and the unloaded postcondition, which would succeed and allow plist removal.
The proposal's platform-aware predicate is the right fix. Linux inactive and
failed behavior stays as it is.

Plan order matches the proposal's lifecycle rules: install checks
`Installable` before directories, provisioning, `disable`, and plist
publication (`owner_plan.rs` lines 263–292); macOS bootstrap is
`Gate::NotLoaded` (lines 298–301); macOS bootout is `Gate::Loaded` (lines
320–327); `Expect(Unloaded)` precedes plist removal (lines 329–333). The
embedded plist program is the fixed three-element vector (lines 1010–1016
and `packaging/macos/com.arxcovo.arxd.plist`). Identity in the proposal
matches those bytes. It does not invent a path.

Three requirements are partial:

1. **Structural rules cannot be coded before a profile.** The proposal fixes
   nesting, balance, and consume-all now, and also says the envelope bytes
   are unpinned and forbids an imagined header. A pre-fixture decoder has no
   honest delimiters. Accepted resolution: until a reviewed profile exists,
   `launchd` is an empty accept function and returns `Unknown` for every
   observation. Nesting, balance, and line ceilings bind that future
   profile. They are not a brace parser in the first patch.
2. **The promised ADR note is narrower than the deltas.** The proposal says
   the source PR notes the stderr and compound-absence change and does not
   amend ADR 0014 in this document. The same note has to cover the other
   deltas this proposal introduces: program and argv identity in addition to
   the plist path, macOS `Installable` and `Unloaded` treating every `Ours`
   as active, and status staying `service-manager` for `Ours(Loaded)` and
   for every pair other than the three rows below. Exit-only absence from
   ADR 0014 decision 3 is tightened to a full tuple, not dropped silently.
3. **The Linux wrapper environment is not fully listed.** Current `bounded`
   clears the environment and sets `PATH`, `LANG=C`, `LC_ALL=C`,
   `SYSTEMD_PAGER=cat`, and `SYSTEMD_COLORS=0` (lines 807–810). The proposal
   names the first three and requires old wrapper semantics. A shared helper
   that drops the two systemd variables changes the fixed Linux query
   environment. The source PR freezes all five on the Linux and mutating
   wrappers. The macOS query adds no further variables.

The status table is accepted as exhaustive: the only new success arm is
identical artifact plus `Ours(Running)`. Identical artifact plus
`Ours(Loaded)` is `Error::Manager`. Absent artifact plus any `Ours`,
including `Running`, is `Error::Manager`. A literal reading that special-cases
only `State::Loaded` with a missing plist is not the accepted reading.

No extra product surface appears. Plans, verbs, status tokens, and the
Windows harness stay as they are. `Action::Status` already returns
`Error::Manager` from the plan (`owner_plan.rs` line 261). Status remains
the read-only `Manager::status` path.

## Security

### Primary Apple evidence

Public reads on 2026-11-14 agree with the proposal's limit statements.

- The Apple Terminal guide
  (`https://support.apple.com/en-ca/guide/terminal/apdc6c1077b-5d5d-4d35-9c19-60f2397b2369/mac`)
  describes `launchctl` as the load and unload interface and lists
  `/Library/LaunchDaemons` for third-party system daemons. The page offers
  current OS versions, including macOS Tahoe 26. It still has no print
  schema, absence exit code, or diagnostic grammar.
- `SMAppService.Status`
  (`https://developer.apple.com/documentation/servicemanagement/smappservice/status-swift.enum`)
  documents registration and authorization: `notRegistered`, `enabled`
  (eligible to run), `requiresApproval`, and `notFound`. Eligible-to-run is
  not current running state. `notFound` is not a `launchctl` exit code.
  This API is not a substitute for the fixed system-domain print contract.
- The archived `launchctl(1)` URL cited by the proposal returns Apple's
  not-found page. This review did not read an installed manual and did not
  use a third-party transcript as grammar.

ADR 0014's statement that print output is not an API stays an ADR record.
This review does not re-verify that warning against a current Apple manual.
No stable machine-readable print API was found. A future fixture establishes
tested compatibility for one OS build. It does not establish a permanent
Apple grammar. If the installed manual and the capture cannot support a
safe profile, production stays `Unknown`.

### Absence, 113, and permission failure

The accepted absence proof is one complete observation: a reviewed exit code,
both streams at EOF, neither stream truncated, and the entire stdout and
stderr byte tuple equal to the reviewed absence pair. That pair names this
label in the system domain and contains no service record. Any other
nonzero result stays `Unknown`. That includes a bare code, empty output, a
permission or domain failure, a usage error, a signal, a deadline, and a
body that looks successful.

Exit 113 is not pinned. `docs/evidence/macos-manager-observations.json`
records source `533ec14e81e3d55596ffe48704abcd7d11c07c65`, run `43055469150`,
job `130643624743`, image `macos-26-arm64` / `20261102.0473.1`, one summary
exit value 113, summary booleans, and a second summary with exit 0 and state
`duplicate`. It disclaims parser changes, mutations, and installed
acceptance. It has no stream bytes, stream identity, completeness, locale,
argv, deadline, or manual extract. It is historical summary context. It is
not fixture credit. A duplicate summary state is not a state token.

`Manager::new` already requires root and the fixed binary (`owner_unix.rs`
lines 478–494). A permission failure on the query is still `Unknown`.
Absence is not inferred from it. Disabled, missing PID, empty stdout, and a
successful bootout are not absence. Bootout failure is not reparsed as
absence.

### Identity, nesting, and duplicates

Ours requires one complete record for domain `system`, label
`com.arxcovo.arxd`, plist path
`/Library/LaunchDaemons/com.arxcovo.arxd.plist`, and the fixed program and
argv from the plist. A path token alone is not program identity. Nested
state, path, or PID fields are not substitutes. Duplicate required fields
are `Unknown`. Last-key-wins is rejected. Optional blocks are an explicit
list in the profile. An unknown addition is `Unknown`. A wildcard that
skips the rest of the document is rejected.

A trustworthy record with a different target, domain, path, program, or argv
is `Foreign`. A malformed record is `Unknown`. Both halt before mutation.
`Unknown` uses `service-manager`. `Foreign` uses `service-foreign-registration`.
Neither result removes a plist.

The query does not open a path taken from print output. Disk identity stays
`Fs::artifact` on the fixed plist: no-follow ancestry, exact bytes, and the
existing leaf checks. Each gate observes again. The query does not read
owner config or resolve the build account. Root remains inside the existing
trust boundary. Concurrent root mutation is still possible between the two
observations. This proposal does not claim an atomic pair.

PID, when a running profile requires one, is a positive decimal that fits in
`i32`. It is discarded after the parse. It is not a process lookup. Last exit
code is not liveness. The first running mapping is `Ours(Running)` only.
Loaded non-running, once a fixture proves it, is `Ours(Loaded)` and status
stays `service-manager` until a later reviewed mapping. `spawn scheduled`,
`not running`, `failed`, and the historical `duplicate` summary are not
mapped now.

### Capture, refusal, and cleanup

Accepted observation: `Exited(i32)`, `Signalled`, or `Incomplete`, plus both
buffers, both completeness bits, and both truncation bits. `Incomplete`
covers spawn, pipe setup, read, wait, and deadline failure. No errno text
crosses the boundary. Complete means EOF on that stream. Process exit
without EOF is `Incomplete`. The exit value is the wait status code.
`128 + signal` is not an exit code. A signal is `Signalled` and parses to
`Unknown`.

Caps: stdout 65536, stderr 4096, exact cap plus EOF accepted, the next byte
a failure. The sum 69632 is the aggregate ceiling. A read that crosses a cap
is not parsed, including the bytes of that read past the cap. Truncation and
incomplete streams win over bytes that look like absence or a foreign
record.

The macOS query drains both nonblocking pipes. Each iteration must touch
both streams, with at most 65 reads of at most 1024 bytes per stream, and
must check the existing 10 second deadline every iteration. A `WouldBlock`
on one stream does not skip the other. Overflow stops capture. The inherited
10 millisecond sleep stays inside the deadline. There is no unbounded
`wait_with_output`.

The current early `!success => Manager` return is not the macOS query path.
After a nonzero exit, both pipes still drain to EOF or to the deadline. Only
a complete pair is parsed. An incomplete prefix that matches an absence
fixture is `Unknown`. Kill and the at-most-1-second reap run when capture
itself fails. A finished nonzero exit with both EOFs is an observation, not
a discarded failure. If the reap does not finish, the result is still
`Unknown`, with no retry. Cleanup outcome is not absence and not success.

Capture of both streams is for this query only. Linux keeps a 4096-byte
stdout cap, null stderr, and `Unknown` or `Manager` on nonzero, signal,
timeout, and oversize output. Mutating commands keep a 35 second limit, null
stdout and stderr, and halt on any nonzero, signal, or timeout. No shell,
no caller-selected target, no fallback, and no retry.

### Redaction and provenance

A nonempty accept-set requires a reviewed profile whose every field is
pinned public structure or an explicitly listed ignored block. A derived
fixture that omits fields does not prove consume-all, and it does not fill
the accept-set. Hashing raw output is not a substitute. If sanitation cannot
keep the absence and identity distinctions, there is no fixture.

A future evidence record, when separately authorized, keeps OS version,
build, architecture, public image identity if hosted, source pin, the exact
argv, locale, deadline, and per-stream length, EOF, and overflow facts. It
quotes only the installed manual's print warning and target-domain wording,
inside ordinary quotation limits. It does not store a raw print. Print
output can carry environment, account, and executable values even when the
plist path itself is already public. This review stores none.

### What stays closed

The proposal does not enable ADR 0019, the macOS UID sweep, or dedicated
execution. A later `running` token would still leave M2 open. The service
guide's stop before install remains in force. No owner step is requested.

## Pre-declared implementation blockers

These reject a future patch even if this contract stays accepted:

- A decoder for a guessed print envelope, or any synthetic bytes in the
  production accept-set.
- Exit 113, any bare nonzero code, or a permission, domain, or usage failure
  mapped to `Absent`.
- Historical `macos-manager-observations.json`, a redacted derivation, or a
  third-party transcript used as grammar credit.
- Linux or mutating `bounded` behavior changed: caps, null stderr, halt
  rules, or the five environment variables.
- A runtime selector, `Debug` or `Display` that dumps observation bytes, a
  log of either stream, or a path from print output opened or normalized.
- A new status token, telemetry channel, diagnostic CLI, workflow, or
  Windows harness change.
- Treating this review as authorization to run `launchctl` or to publish.

## Oracles and mutants

Pure tests use fixed `Query`, `Status`, booleans, and error variants.
Assertion failures must not print raw buffers. The scripted plan runner
keeps injecting `Query`. It does not prove capture or parsing. The native
mapping is a pure function from observation to `Query` that the macOS query
arm calls. A test of that function kills a bypass. A scripted lifecycle test
does not.

Parser, accept-set empty:

- Every observation is `Unknown`, including exit 0 with empty stderr, exit 0
  with a success-shaped body, exit 113 with any body, empty failure, and
  synthetic absence text.
- Mutants that map nonzero to `Absent`, accept 113 alone, or accept exit 0
  without a profile must fail these tests.

Parser, after a reviewed profile only:

- One accepted fixture maps to the pinned `Query`. Negatives: wrong label,
  domain, path, program, or argument; nested shadow fields; duplicate
  required fields; missing header, field, or closer; a second record;
  unsupported state or metadata; invalid UTF-8; NUL; exact cap and cap plus
  one on each stream; incomplete EOF; truncation of a plausible prefix;
  nonempty stderr on an otherwise accepted success; nonzero exit with a
  success-shaped body.
- Absence: the exact tuple is `Absent`. The same code with a different
  stream, extra line, wrong target, or permission text is `Unknown`.
  Signals and deadlines are `Unknown`.
- Mutants: ignore stderr or completeness; parse a truncated prefix; accept a
  nested path or state; last-key-wins; drop a target, domain, path, or argv
  check; treat a missing PID as unloaded; map every exit 0 record to
  `Running`.

Capture seam, injected pipes and clock, no `launchctl`:

- Both streams drain when one is noisy. Exit before EOF is `Incomplete`.
  EOF before exit waits for the wait status. A pipe that never closes hits
  the deadline and is `Incomplete`. Spawn, read, wait, and setup failures
  are `Incomplete`. Overflow does not parse the excess. Deadline is checked
  even when reads return data. Failed capture still attempts one bounded
  reap, and the query result stays `Unknown` if that reap does not finish.
- The existing `/bin/echo` cap of 4 and 5, `/bin/false`, and short
  `/bin/sleep` cases in `ordinary_process_timeout_and_output_bounds` remain
  Linux-wrapper regressions. They are not launchd fixtures. New tests must
  not grow that case into a real `launchctl` run.

Predicate and status, injected `Query`:

- On macOS, every `State` variant fails `Installable` with `Error::Active`
  (`service-active`) and fails `Unloaded` with `Error::Manager`
  (`service-manager`). Only `Absent` passes those two expectations.
- On Linux, `Stopped` and `Failed` still pass `Installable` and `Unloaded`.
  `Running`, `Reloading`, `Starting`, `Stopping`, and `Loaded` still fail.
- `Unknown` and `Foreign` fail every expectation and every gate.
  `Gate::NotLoaded` is true only for `Absent`. `Gate::Loaded` is true for
  every `Ours`.
- macOS status: `(Absent, Absent)` is `not-installed`; `(Identical, Absent)`
  is `stopped`; `(Identical, Ours(Running))` is `running`. Every other macOS
  pair is `Error::Manager`, including `(Identical, Ours(Loaded))` and
  `(Absent, Ours(Running))`. Linux `linux_status` is unchanged, including
  `Loaded` as `Error::Manager`.
- Script install on `Unknown`, `Foreign`, or any macOS `Ours` records no
  directory, provision, disable, or plist write. Start records bootstrap
  once, and only after `Absent`. Stop and uninstall skip bootout only for
  `Absent`. A post-bootout `Unknown` or `Ours` leaves the plist in place.
- Mutants: macOS `Stopped` or `Failed` installable; unloaded without a fresh
  `Absent` query; `Unknown` allowed through a gate; status reports `stopped`
  for `Ours(Loaded)`; status reports `running` without an identical plist.

A synthetic pipe seam does not prove the production file descriptors. Native
wrapper wiring still needs source review and, later, a separately authorized
capture. This proposal ran no such capture, and this review runs none.

## Evidence gate

Not authorized here. ADR 0014 still requires coordinator authorization before
a read-only absent-target observation. When that separate task exists, its
only proposed argv is `/bin/launchctl print system/com.arxcovo.arxd` under
the capture bounds above. No `print system`, process list, extra label, log,
config dump, or private path. No pipe through grep. Positive and absence
fixtures are separate. Absence evidence is required before fresh install and
uninstall can prove `Absent`. Running evidence is required before status can
return `running`. One fixture does not imply the other. A loaded fixture
requires a service that is already in that state under a nonprivate fixed
configuration. The read-only query does not create that state. No account,
install, bootstrap, bootout, or dispatch is part of this design review.

## Smallest next step

No owner or admin action. No native run.

The useful safe slice, after the coordinator decides to implement, is one
source change limited to `owner_query.rs`, the macOS query and status wiring
in `owner_unix.rs`, and pure tests:

- Add the observation type and make `launchd` return `Unknown` for every
  observation. No grammar table and no fixture bytes.
- Make `Installable` and `Unloaded` platform-aware, as specified above.
- Add the pure macOS status helper and call it from `Manager::status`.
- Add the private capture helper and point the macOS query arm at
  `launchd`. Linux and mutating wrappers keep today's results.
- Leave `owner_plan.rs` operations unchanged. Adjust expectations in
  `owner_query` tests where the macOS predicate changes scripted results.

That slice does not make macOS install, start, stop, or status succeed on a
real job. `Unknown` still halts at `service-manager`. It closes the latent
`Stopped` and `Failed` hole and gives a later fixture a parser boundary that
cannot accept invented bytes.

The evidence capture is not this next step. It stays blocked on a separate
authorization. A nonempty profile is a later source change with its own
exact-head review.

## Close

Standards: 2 judgement-call smells, 0 hard breaches; worst is speculative
nesting and line ceilings treated as a grammar. Spec: 3 partial findings;
worst is coding structural rules as a decoder before a profile exists.
Security: 0 proposal blockers; worst residual hazard is promoting the
historical summary, a bare exit code, or a redacted print into a nonempty
accept-set. This review does not authorize implementation, capture, or
publication.
