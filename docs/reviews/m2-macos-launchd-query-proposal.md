# M2 macOS launchd query proposal

Author: OpenAI / gpt-6.1-sol / high, design sub-agent, 2026-11-14.
Status: proposed bounded contract; **independent Grok/Muse design review required before implementation**. No independent review is claimed here. Explicit task scope overrides the general kickoff delegation and PROGRESS requirements for this documentation-only assignment.

## Source pins and validation performed

- Isolated cwd equals Git top level in the supplied `Arxcovo/t3-ff5bc7ea-2d11b299e7379efc` worktree (owner home prefix omitted); initial HEAD `78032cffef451463537c59da23e6c433e65aaa5e`; initial status empty. No checkout/ref refresh occurred.
- Implementation baseline is the supplied actual-main Git object **`4ec11b914039c81f5401fdfbe678201372742964`**, not a locally named main or optional Windows branch. Its service tree is `f3ad2c9ab0864d18634505fd8279d17033729ce7`, identical to supplied earlier main `aa49ce1e99475d07d63c6b0d23fa94bdb30bab74`. `git diff` between those pins for service source was empty; service source at this checkout also matches `4ec11b9`.
- Read AGENTS/KICKOFF, Coordinate and Design skills, ADRs 0009/0014/0015/0018/0019, `service/{owner_query,owner_unix,owner_plan}.rs` including tests, the fixed macOS plist, and current-main `docs/service-install.md` (blob `7a7a7bba1861e9a4c57651cb03255f4ccfbd2ea7`). ADR 0014 supplies the query/lifecycle contract; ADR 0015 supplies the unchanged ancestry/runtime fences; ADR 0018 supplies fixed owner errors/status and config-independent uninstall; ADR 0019 keeps native execution inactive.
- Read the audit `docs/reviews/m2-acceptance-path-recovery.md` from this checkout, blob `184d2c77dcaf12e777587a04df298afad895a99d`, introduced by `ae6a6ad045c061b8357d48f32accef2adce14131`. It is **not in `4ec11b9`** and receives audit context credit only. No unpublished diagnostic implementation is used.
- Source/Git/document inspection and public Apple documentation reads only. No tests/builds, actual launchctl, native/system/process/account/service/owner/private-state/VM inspection, installation, dispatch or GitHub operation. No old Lab Manager or protected VM access. Final diff/whitespace/status inspection is documentation validation, not runtime validation.

## Actual gap and primary-source limits

At the baseline, `owner_query.rs::launchd(bool, &[u8])` always returns `Unknown`. `owner_unix.rs::Runner::query` invokes fixed `/bin/launchctl print system/com.arxcovo.arxd`, but `bounded` discards stderr and converts every nonzero exit to `Error::Manager`; the parser cannot observe an absence diagnostic. The existing macOS status branch recognizes only absent registration. Returning a running query alone would still leave status failing.

There is also a latent predicate hazard: shared `Query::active` excludes `Stopped` and `Failed`. ADR 0014 requires **every loaded macOS job** to block install and fail the unloaded postcondition. A parser must not expose those states to the current predicate unchanged.

Primary material inspected on 2026-11-14:

- [Apple Terminal guide](https://support.apple.com/en-ca/guide/terminal/apdc6c1077b-5d5d-4d35-9c19-60f2397b2369/mac) describes launchctl as the interface for loading/unloading jobs and identifies `/Library/LaunchDaemons` as third-party system daemons. It supplies no output schema, absence exit-code guarantee or diagnostic grammar.
- [Apple SMAppService.Status](https://developer.apple.com/documentation/servicemanagement/smappservice/status-swift.enum.md) describes registration/authorization states; enabled means eligible to run. It does not establish current running state, the fixed system target's loaded configuration, or launchctl error semantics. It is not a drop-in solution to this contract; migrating registration would be separate architecture work.
- The attempted archived Apple `launchctl(1)` URL (`https://developer.apple.com/library/archive/documentation/Darwin/Reference/ManPages/man1/launchctl.1.html`) returned 404. No installed macOS manual was inspected. ADR 0014 explicitly records that print output is not an API. Treat its grammar as undocumented/unstable; this session did **not** independently verify that manual warning against a current primary manual. Future evidence must include the target OS's installed Apple manual for print/domain semantics and its warning. No third-party example or external repository is a grammar authority here.

Decision: use the existing fixed print query with an evidence-pinned, fail-closed compatibility grammar. There is **no claimed stable Apple machine-readable print API**. No alternative documented mechanism establishing all required facts was verified. Native evidence, even when obtained, will establish tested compatibility, not a permanent Apple guarantee. No numeric absence constant or sample launchctl transcript is invented in this proposal.

The existing `docs/evidence/macos-manager-observations.json` (blob `da8326bfcb260fecc33de342be7fa179cac7f44a`) records source `533ec14`, hosted run `43055469150`, job `130643624743`, image `macos-26-arm64` / `20261102.0473.1`: missing target exit 113 plus summary booleans, and an Apple syslog control with duplicate state. It explicitly disclaims parser changes, mutations and installed acceptance. These are historical summaries, **not raw grammar fixtures**, not proof of stream placement/completeness, not proof that 113 uniquely means absent, and not evidence of an Arxcovo loaded/running service.

## One input contract

Replace only the macOS parser's bool/stdout interface with a borrowed bounded observation. Conceptually:

```text
LaunchdObservation {
  termination: Exited(i32) | Signalled | Incomplete,
  stdout: bytes, stderr: bytes,
  stdout_complete: bool, stderr_complete: bool,
  stdout_truncated: bool, stderr_truncated: bool
}
launchd(observation) -> Query
```

`Incomplete` covers spawn, pipe setup/read, wait and deadline failure; no errno or error text crosses this boundary. Complete means EOF observed, not merely process exit or a read returning WouldBlock. Termination code is the real exit code, never `128 + signal`. The parser also checks lengths itself. Any incomplete stream, truncation, signal, timeout, invalid encoding/NUL/control structure, or out-of-bound length gives `Unknown`, even if retained bytes look conclusive. Transport failure must take precedence over apparent absence/foreign output. No bool-success conversion before parsing.

Capture contract: stdout at most **65,536 bytes**, stderr at most **4,096 bytes**, aggregate at most **69,632 bytes**; exact limit plus EOF is valid, first excess byte is failure. Two nonblocking pipes drained with bounded work per loop (at most 65 reads of at most 1,024 bytes per stream per iteration); check the existing 10 s absolute deadline every iteration. Overflow terminates capture rather than draining indefinitely. Preserve at most the caps and mark failure; never treat an overflow prefix as complete. On failure use the existing kill/at-most-1 s reap attempt and return Unknown regardless of cleanup outcome. No unbounded `wait_with_output`, blocking drain, inherited pipe EOF wait, log, Debug derive or stream forwarding. Tests must not accidentally print observations in assertion failures.

Retain fixed binary integrity/root authority checks, argv, cwd `/`, null stdin, cleared environment, fixed PATH and `LANG=C`/`LC_ALL=C`. No shell, caller-selected target, command fallback or retry. Capture both streams only for this query. Keep Linux's 4 KiB query behavior and all 35 s mutating command behavior unchanged (stderr discarded; any nonzero/signalled/timeout failure halts). A small private capture core may be shared with existing `bounded`, with its old wrapper semantics pinned by regression tests; this is not a general subprocess framework.

## Parsing and identity decisions

A production compatibility profile must be a small constant grammar table derived from reviewed native fixtures. It specifies exact outer envelope/target header, indentation or structural delimiters, required field locations/cardinalities, supported line endings, allowed optional fields/blocks, state vocabulary, and absence stream/code/text tuple. **Those bytes are currently unpinned.** Do not implement an imagined envelope such as a guessed header or absence phrase. Until fixtures supply a profile, its matching set is empty and production remains Unknown.

Structural rules are fixed now: accept exactly one complete service record for **domain `system`, label `com.arxcovo.arxd`**. Scope fields to that record's immediate service level; nested state/path/PID fields are never substitutes. Bound nesting to 8 and line count to 2,048; excess gives Unknown. Require balanced structure and consume the entire observation; no regex/substrings over arbitrary output, last-key-wins, loose whitespace normalization or matching arbitrary stderr suffixes. Required duplicate/missing fields and unknown structural forms give Unknown. Optional ignored metadata/blocks must themselves be fixture-listed and structurally consumed; unknown additions conservatively give Unknown. Production code contains no synthetic profile that accepts invented success bytes.

Identity comprises the exact target/domain plus fixed plist path `/Library/LaunchDaemons/com.arxcovo.arxd.plist` and fixed effective program/argv:

```text
/Library/Application Support/Arxcovo/libexec/arxd
service
run
```

Require their observed native representations to agree, with no extra arguments or alternative executable/identity override. A path token alone does not prove loaded executable identity. If print does not expose enough to establish these facts, loaded recognition stays Unknown pending a separately reviewed solution; do not weaken to a path substring. The fixed service dispatch reads its fixed config layout; do not inspect owner config values or resolve build accounts in the query. “Ours” identifies a managed registration, not byte-for-byte equality of every cached launchd option or proof of a healthy daemon. Never claim the print output proves every original plist key.

Independently retain `Fs::artifact`: protected no-follow ancestry, identical fixed plist bytes, correct leaf ownership/mode/link count/ACL/local filesystem rules. No path normalization, symlink following, repair, chown or reading any path returned by launchctl. Reobserve for each existing gate/expectation; never cache a query across operations. Root administrators are within the existing trust boundary; the separate disk/query observations do not create an atomic defense against concurrent root mutation.

| Complete observation | Result |
| --- | --- |
| Exit 0, empty stderr, exact accepted structure and identity, supported state | `Ours(state)` |
| Exit 0, accepted service envelope with unambiguous different target/domain/path/program/argv | `Foreign`; no mutation |
| Exit 0 but malformed, duplicate, missing identity or unsupported structure/state | `Unknown` |
| Nonzero with exact evidence-pinned absence code **and entire stdout/stderr pair**, naming this label in the system domain and containing no service record | `Absent` |
| Any other nonzero, including 113 alone, empty failure, permission/domain/usage error, or contradictory success record | `Unknown` |

Foreign can only follow structurally trustworthy output; malformed contradictory records are Unknown. Either refusal is safe. An absent plist, absent PID, disabled bit, no process, old exit code, empty stdout or successful bootout is **never** sufficient for `Absent`.

Absence profile must distinguish missing service from unavailable/inaccessible domain, permission denial and malformed invocation. No wildcard prose prefix/suffix, local hostname/UID acceptance, generic errno mapping or “nonzero means missing.” If native evidence cannot distinguish these, leave **all nonzero results Unknown**. ADR 0014's exit-only candidate must be tightened to this compound proof in the later source PR's explicit contract note; this proposal does not silently amend an ADR.

## Loaded, running and lifecycle safety

Keep existing Query/State/Status types. Fixture-proven current running state with a positive decimal PID may map to `Ours(Running)`; PID is parsed/bounded locally (positive i32) and immediately discarded. It is corroboration, not a process lookup or identity proof. Contradictory/missing PID in a profile that requires it gives Unknown. Do not use last exit code as current liveness.

For a fixture-proven loaded nonrunning state, map to `Ours(Loaded)` in the smallest first implementation. All unpinned state vocabulary remains Unknown. Do not map speculative `spawn scheduled`, `not running`, `failed`, or historical summary `duplicate` to a status. Existing ADR 0014 candidate strings are candidates only. A later mapping of Starting/Stopped/Failed requires native fixtures and must preserve the loaded predicate below. No new product status token is needed.

Make `Installable` and `Unloaded` platform-aware: on macOS **every `Ours(_)` is active/loaded**, including Stopped/Failed if ever introduced. Only `Absent` satisfies Unloaded or Installable (with absent/identical artifact). Linux inactive/failed behavior remains unchanged. `Gate::Loaded` accepts every Ours; `Gate::NotLoaded` accepts only Absent; Unknown/Foreign always halt. Do not rely only on choosing Loaded today: pin the invariant over every State variant.

- Install: Unknown/Foreign halts before directories/provisioning/disable/plist publication. Every loaded macOS state returns Active. Only proven absence can proceed; existing final Unloaded check remains.
- Start: require identical plist/preflight and known nonforeign manager state. Enable then bootstrap only on fresh Absent. Loaded skips bootstrap. Start success remains command-plan success, not a health claim; subsequent status must independently establish running.
- Stop/uninstall: disable, fresh gate, bootout only on Ours. Fresh Absent skips bootout. Any command failure halts; never reinterpret bootout failure as absence. Require a new Absent query after bootout before removing plist. Loaded without PID still requires bootout. Existing exact-byte/no-follow removal and config/state/account retention stay intact.
- Status: `(absent artifact, Absent)` -> not-installed; `(identical, Absent)` -> stopped; `(identical, Ours(Running))` -> running. `(identical, Ours(Loaded))` stays fixed Manager until a current-state mapping is fixture-proven; never call it stopped as stop proof. Loaded with absent artifact, unsafe/foreign artifact, unsupported states and Unknown refuse. Add a pure macOS status helper rather than leaving running unreachable in `Manager::status`.

## Smallest code PR and meaningful tests

After independent design review and the relevant evidence gate, one Sol source PR covers only `service/owner_query.rs`, the narrow `owner_unix.rs` capture/adapter/status wiring, and adjacent pure tests/fixture assets. Existing owner plans/vectors stay intact; change expectations in owner_query rather than adding operations. Include a short reviewed contract clarification for the stderr/compound-absence change; no diagnostic CLI, CI/workflow, telemetry, manifest service, replay harness or executor work.

Pure tests must reach these behaviors without actual commands/services/accounts. Test the byte parser separately from scripted lifecycle tests: the latter currently inject Query and cannot validate capture/parsing. Add a deterministic injected pipe/clock/exit seam only as needed inside the private capture helper; no runtime selector. All future assertions use fixed labels/booleans so failures do not print raw bytes.

1. Each accepted native fixture plus minimally changed negatives: wrong label/domain/path/program/argument; nested shadow fields; duplicates; missing header/field/closing structure; concatenated records; unsupported state/metadata; invalid UTF-8/NUL; exact cap and cap+1 for both streams; incomplete EOF and truncation despite plausible prefix; success stderr warning and nonzero success-shaped body. Synthetic cases receive pure parser credit only.
2. Absence: exact accepted tuple; same code with empty/wrong stream, unrelated/missing target or domain, extra line, denied access or domain failure; arbitrary other nonzero and signals/timeouts. Until a native tuple exists, assert that every proposed numeric-only/synthetic diagnostic is Unknown. No native acceptance credit from those tests.
3. Capture seam: fair dual-stream drain, stderr flood, stdout flood, exit before EOF, EOF before exit, inherited pipe never closes, read/wait/setup/spawn failure, deadline cannot starve, failure cleanup bounded. Linux/mutating wrapper regression assertions preserve previous error/output behavior.
4. All State variants on macOS block install and fail Unloaded; Linux Stopped/Failed still allowed. Every Unknown/Foreign gate halts. Script install shows no mutating call on uncertain/loaded query; start bootstraps once only on absence; stop/uninstall skip only on absence; post-bootout Unknown/loaded preserves plist and later suffix. Test pure status matrix, including running and loaded-with-no-artifact refusal.

Required mutants killed by these cases: exit!=0 -> Absent; accept 113 alone; ignore stderr or stream completeness; parse truncated prefix; accept nested state/path; duplicate last-wins; remove target/domain/path/argv identity check; treat no PID as unloaded; map all exit-zero records to running; make macOS Stopped/Failed installable; remove post-bootout query; remove Unknown gate refusal; bypass parser in native adapter. Native-wrapper wiring still needs source review and later actual capture evidence; a synthetic seam cannot prove real pipe behavior. Future implementation must run targeted meaningful tests and normal required checks; this proposal ran none.

## Evidence prerequisite and next gates

This is a plan, **not a command execution or native authorization**. A later separately scoped evidence task may use an existing authorized, nonprivate disposable macOS environment; do not provision accounts, install/register/start/stop jobs or dispatch anything for this proposal.

1. Preserve target macOS version/build/architecture, public runner image if hosted, source pin, fixed command identity, locale, wall-clock deadline and separate stream completeness/overflow facts. Read the installed Apple launchctl manual relevant sections, not private service state. Retain its exact print warning and target-domain semantics within permitted quotation limits.
2. Only proposed service observation argv is **`/bin/launchctl print system/com.arxcovo.arxd`**, with the capture environment/bounds above. No broad `print system`, process list, arbitrary label, Apple-job scan, logs, config dump, private paths or environment collection. Do not pipe through grep: stream placement, structure and failure status must remain observable locally.
3. In a known absent disposable environment, capture the complete bounded pair locally and derive a minimal sanitized grammar fixture. Publish only reviewed fixed-label/path/diagnostic structure, real exit code, completeness facts and provenance. Never upload raw full print output: it can contain environment, account, executable and other private values. Unknown fields are omitted only in a labelled **derived** fixture with an explicit structural redaction map; they do not prove an unredacted complete grammar. If sanitation cannot preserve trustworthy syntax/absence distinctions, stop; no fixture credit. Do not hash private output as a substitute.
4. A loaded/running fixture requires a separately authorized disposable Arxcovo service already in that state and nonprivate fixed configuration. The read-only query does not create that prerequisite. Require label/domain/path/program/argv/state/PID evidence and nested-key controls. Loaded nonrunning coverage also needs real evidence before recognition. Positive and absence fixtures can be accepted separately; one must not imply the other. Without absence evidence, fresh install and uninstall final proof stay blocked. Without running evidence, status running stays blocked.
5. Independent Grok/Muse review must challenge grammar provenance, ambiguity of absence diagnostics, config identity and lifecycle predicates before source implementation. If current Apple manual/evidence cannot justify a safe profile, retain Unknown and return that specific gap to architecture; do not add a sprawling diagnostic framework. A source PR needs different-provider exact-head review, green required checks and coordinator merge. A later per-OS installed lifecycle/boot/restart acceptance remains separate from parser fixtures and hosted tests.

No fixture was created, captured or executed here. All installed/dedicated macOS paths remain **unverified on real hardware**. Parser work alone cannot close M2: macOS still lacks its dedicated UID precheck/sweep/janitor executor; Windows dedicated execution is separate; Linux F/G and real-Runner PA6 remain OPEN. The optional Windows harness is parked; main71 is a known intermittent failure of unknown cause with no retry authorized. No change here widens those scopes or authorizes owner steps.
