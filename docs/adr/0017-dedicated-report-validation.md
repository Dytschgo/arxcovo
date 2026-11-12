# 0017: Dedicated execution report validation (N2)

Status: **accepted for local implementation** (Anthropic / claude-opus-5-5 / high, architecture),
2026-11-11. Revised the same day after the
[independent Sol review](../reviews/0017-report-contract-sol-review.md) of
`86b0a40b` (see the
[correction disposition](../reviews/0017-report-contract-correction-disposition.md)).
The [fresh Sol recheck](../reviews/0017-report-contract-sol-recheck.md) accepts the corrected contract for implementation; publication and normal gates remain pending. Runtime base: `4abf276a`,
byte-identical to the reviewed PR36 head `5c3c01e3`; later commits are docs only.
It refines the post-preflight part of the ADR 0009 execution contract and
amends two ADR 0009 rows (see [Relation to ADR 0009](#relation-to-adr-0009)).
It adds no schema, vocabulary, authority or approval flow, and it adds no native
backend. Production stays `MechanismUnavailable`. Native, installed and
dedicated M2 paths remain **unverified on real hardware**.

**Output-release amendment: accepted for local implementation** (Anthropic / claude-opus-5-5 / high,
architecture, 2026-11-11, base `2f3b4863`). It adds
[Output release](#output-release-privacy-normative-output-release-amendment-accepted)
and Amendment 3 under [Relation to ADR 0009](#relation-to-adr-0009), after the
[unverified-identity output probe](../reviews/0017-unverified-output-probe.md).
The [output-release disposition](../reviews/0017-output-release-disposition.md)
has the counterexample and test matrix. The [independent Sol review](../reviews/0017-output-release-sol-review.md)
accepts proposal `0132c3e` for local implementation. Every other section keeps its accepted
status unchanged.

Local integration checkpoint, 2026-11-12: the original design/runtime bases above
are historical. This port follows retained N3 `1976afb1a2f3e7d2b22cbf819909a0a22575be24`
on actual main #36 `0937cac`. The accepted typed-report, identity-privacy and
output-release contract is unchanged. The [full Opus implementation review](../reviews/0017-implementation-opus-review.md)
and [B1/B2 PASS recheck](../reviews/0017-blocker-corrections-opus-recheck.md)
are retained for their original heads. Fresh coordinator Opus high integration
review and hosted gates remain required before publication/merge. See the
[port evidence](../evidence/n2-integration-port.md). This establishes no native
activation or M2 acceptance.

## Context

The [report-validation probe](../reviews/service-report-validation-probe.md)
dispatched five synthetic dedicated `run_command` reports through the real
`Execution::dispatch`. All five were committed as `succeeded`, with no reason
and no quarantine:

| Probe | Changed fact (otherwise Windows Standard, exit 0, cleanup true, `windows_job`) |
| --- | --- |
| P0 | no identity observation and no matches flag |
| P1 | `cleanup_confirmed` absent |
| P2 | `rights = admin` while the configured rights are standard |
| P3 | `observation_point = child` |
| P4 | `tree_boundary = process_group` on Windows |

The cause is structural. Backends return a store `Metadata`, the row itself.
So a backend chooses `rights`, `identity_matches_pin`, `observation_point` and
even `reason`. `terminal()` then accepts `exited/0` with no identity, cleanup or
containment evidence. The [PR36 recheck](https://github.com/Dytschgo/arxcovo/pull/36#issuecomment-6806673472)
requires N2 to close before any native adapter lands.

## Decision summary

1. **Backends report evidence, never rows.** A spawned report carries a typed
   `Evidence` with only the facts the backend observed. It has no `reason`,
   `rights`, `identity_mode`, `program`, `argc` or matches flag. The dispatcher
   builds every row.
2. **The dispatcher holds an immutable `Pin`**, built once from the validated
   service config: platform, build identity, GID and rights. It computes
   `identity_matches_pin` itself, and it writes `rights` from the pin.
3. **Reports are split at one boundary: child creation.** That is the call to
   the platform's creation primitive (Windows `CreateProcess*`, Unix
   fork/clone/spawn), never target exec, resume or the identity read. A failed
   run states a creation outcome. Only "not invoked" or a cited normative
   "proven none" is pre-spawn. A failed run with a created child or an unknown creation outcome is
   post-spawn and quarantines. A missing observation never makes a report
   pre-spawn.
4. **`cleanup_unconfirmed` keeps top precedence.** Then come
   `identity_mismatch`, then `execution_failed`. A run succeeds only when every
   check passes and it exited with code 0.
5. **Nothing is manufactured.** Observed fields come only from an admissible
   observation. They are never filled from the pin. `cleanup_confirmed = 1` is
   stored only when the backend reported it inside the platform's required
   boundary. Otherwise the row says `0`, which is arxd's own statement that
   cleanup is not confirmed.
6. **N1 (dedicated only):** an identity mismatch after spawn quarantines. This
   applies ADR 0009's "any executor error after spawn". ADR 0010 development
   mode is unchanged.
7. **Only the build identity is disclosed.** An observed SID, UID or GID that
   differs from the pin is withheld (NULL) in the row, and therefore in every
   response, and it is retained nowhere. The mismatch stays truthful through
   the reason, `identity_matches_pin = 0`, the kind, the point, the token class
   and quarantine.
8. **Output is released only from a pin-matched child** (output-release
   amendment, accepted). Captured `stdout` and `stderr` reach a response only
   when `assess` computed `identity_matches_pin = Some(true)`. Otherwise both
   streams are returned in one fixed withheld form, without content or length.
   Reason, outcome, quarantine and the row do not change.

## Report API (normative shape)

```rust
pub enum Report {
    Unavailable(Unavailable), // creation primitive not invoked
    CancelledBeforeSpawn,     // final recheck, before the primitive, saw cancel
    Failed { creation: Creation },
    Discovered { evidence: Evidence, tools: Vec<ToolVersion> },
    Completed { evidence: Evidence, stdout: CapturedOutput, stderr: CapturedOutput },
    #[cfg(test)] FakeForTest { stop: StopReason, cleanup: bool }, // unchanged
}
pub enum Creation {        // child-creation outcome of a failed run
    NotInvoked,            // the creation primitive was never called
    ProvenNone,            // called; a cited normative guarantee: no child
    Created,               // a child handle, PID or child-side signal exists
    Unknown,               // called; outcome not distinguishable
}
#[derive(Default)]
pub struct Evidence {                       // only facts the backend observed
    pub observation: Option<Observation>,
    pub point: Option<ObservationPoint>,
    pub stop: Option<StopReason>,
    pub exit_code: Option<i32>,
    pub tree_boundary: Option<TreeBoundary>,
    pub cleanup_confirmed: Option<bool>,
}
pub enum Observation {                      // the `matches` field is removed
    Windows { sid: String, integrity_rid: u32, elevated: bool,
              elevation_type: ElevationType, administrators: GroupState },
    Unix { uid: u32, gid: u32 },            // real IDs, as in ADR 0007
}                                           // Debug never prints sid/uid/gid
pub enum ElevationType { Default, Full, Limited } // TOKEN_ELEVATION_TYPE 1, 2, 3
pub enum GroupState { Absent, DenyOnly, Enabled, PresentOther } // S-1-5-32-544
pub(crate) struct Pin { platform: Platform, identity: String,
                        gid: Option<u32>, rights: store::Rights } // no setters
```

- **Pin construction.** `Pin::new` returns `AdminUnsupportedOnPlatform` for
  macOS with admin rights. It returns `PreflightFailed` for shape errors: a
  Windows pin with a GID, or a Unix pin without a GID or with a non-decimal UID.
  `Execution` holds `Result<Pin, Unavailable>`. It is checked after
  `backend.preflight()` succeeds and before the busy check, so the production
  `Native` refusal (`mechanism_unavailable`) is unchanged. The production pin
  comes only from `Execution::configured`. Tests use a `#[cfg(test)]`
  constructor.

## Child-creation boundary (normative)

The boundary is the **call** to the platform's child-creation primitive. It is
not target exec, thread resume, Job assignment or the identity read.

- **Windows:** the `CreateProcess*` call that creates the suspended
  `exec-child` shim (ADR 0009 step 5). Logon, token checks, pipes, the
  attribute list, the Job object and the desktop come before it. Job
  assignment, the token read, resume and the shim's start of the target come
  after it.
- **Unix:** the fork, clone, vfork or posix_spawn that creates the child,
  including the one inside Rust std `Command::spawn` used by ADR 0007's runner.
  `setpgid`, the cgroup join, the ID drop, the cwd open, the identity report and
  exec all run in the child, after the boundary. A child that never reached
  exec was still created.

A failed run reports exactly one creation outcome:

| Creation | When a backend may claim it | Report | Row |
| --- | --- | --- | --- |
| `NotInvoked` | The failure, or the final cancel/shutdown recheck, happened strictly before the primitive was called | `Unavailable`, `CancelledBeforeSpawn` or `Failed { NotInvoked }` | D, E or F |
| `ProvenNone` | All three hold: the adapter called the primitive directly; it returned an error result the adapter can distinguish; and the primitive's normative specification says no child is created for that result. The native adapter's ADR must cite that text, and its tests must exercise the distinction. **No current path qualifies.** | `Failed { ProvenNone }` | F |
| `Created` | The primitive returned a child handle or PID, or any child-side signal arrived: identity bytes, a child-side error over the CLOEXEC channel, or an exit status | `Failed { Created }`, or an evidence report | G, or H–N |
| `Unknown` | Anything else after the call: an error that merges pre- and post-creation causes, a panic or unwind across the call, or a timeout | `Failed { Unknown }` | G |

Rules:

- **The phase comes only from the creation outcome.** A missing observation,
  PID, output, exit status or cleanup fact never makes a report pre-spawn.
  Without a qualifying `NotInvoked` or `ProvenNone`, the adapter must report
  `Created` or `Unknown`.
- **Current classifications** (from source and documentation reading, nothing
  run):
  - On Unix, `Command::spawn` returning `Err` is `Unknown`. ADR 0007's runner
    (`crates/arxd/src/exec/unix.rs`) installs `setpgid`, the cgroup join, the
    ID drop and the identity write as a `pre_exec` closure. An error from that
    closure or from exec reaches the parent through the same `Err` as a fork
    failure. So a child may have existed, and before its ID drop it ran as the
    service identity.
  - On Windows, a `CreateProcess*` call that returns `FALSE` is `Unknown` under
    this ADR. The ADR cites no documented guarantee that no process exists
    after a failed call. A native adapter ADR may reclassify it as
    `ProvenNone` only by citing one.
  - A successful `CreateProcess*` followed by a failed Job assignment, token
    read or resume is `Created`. A failed Job assignment does not prove the
    required boundary. A later token-read or resume failure may follow a
    successful assignment; only actual boundary and cleanup evidence can then
    justify the evidence-report alternative below.
- **Cleanup for a child without an observation.** `Failed { Created | Unknown }`
  is row G: `cleanup_unconfirmed` and quarantine. An adapter may instead return
  an evidence report with no observation (row J, which also quarantines) only
  when the required boundary is proven to contain the child, for example after
  a successful Job assignment or a proven cgroup join. A build-UID sweep cannot
  confirm cleanup of a child that may still run as the service identity, and
  neither can a cgroup the child may not have joined.
- **Cancellation and shutdown.** The final recheck happens immediately before
  the primitive is called. From that call until the bounded identity read
  ends, the backend must not honor cancellation. After that, it stops the
  child through the post-spawn path. `Unavailable(Stopping)` and
  `CancelledBeforeSpawn` are valid only for `NotInvoked`.
- **The dispatcher maps the outcome exhaustively**, with no wildcard. It cannot
  verify the claim (see
  [What the interface can and cannot establish](#what-the-interface-can-and-cannot-establish)).
  Each native adapter's tests must prove the classification at each of its
  failure sites.

## Admissibility and identity checks

An observation is **admissible** only when its kind and point are the
platform's required pair, and its ID passes the existing store syntax check
(shared with `Metadata::validate`, so a malformed value can never cause a
storage failure):

| Pin platform | Required kind / point | Required boundary | Match (computes `identity_matches_pin`) |
| --- | --- | --- | --- |
| Windows, standard | `Windows` / `post_switch` | `windows_job` | `sid` byte-equal to pin, `integrity_rid = 8192`, `elevation_type = Default`, `elevated = false`, `administrators = Absent` |
| Windows, admin | `Windows` / `post_switch` | `windows_job` | `sid` byte-equal to pin, `integrity_rid = 12288`, `elevation_type = Full`, `elevated = true`, `administrators = Enabled` |
| Linux, either | `Unix` / `pre_exec` | `linux_cgroup` | `uid` and `gid` equal to pin |
| macOS, standard | `Unix` / `pre_exec` | `mac_uid_sweep` | `uid` and `gid` equal to pin |
| macOS, admin | no pin exists, so it is refused before the backend | — | — |

- Elevation type, `TokenIsElevated` and the Administrators state are checked
  independently. A UAC-off admin token (`Default` and elevated) fails the
  standard check. So does a filtered admin token (`Limited` with
  `DenyOnly`). This matches ADR 0009 step 3: "no Administrators entry
  (including deny-only)". The type and group state are checked in memory and
  never persisted, as ADR 0009 step 7 requires.
- `process_group` is never a required boundary for dedicated runs. A
  `child` point, a `pre_exec` point on Windows, a `post_switch` point on Unix,
  and the wrong observation kind are all inadmissible.
- Unix rights cannot be established by the pre-exec report. ADR 0007 enforces
  real and effective UID/GID and the supplementary set inside the child before
  it writes the report. Rights stay a preflight fact (resolver
  `administrative`, plus ADR 0009's `sudo -n true` effect). The row's `rights`
  is the configured pin value on every OS. It is never reported by a backend.

## Identifier disclosure (privacy, normative)

**Protected identifiers** are an observed SID or UID that is not byte-equal to
the pin identity, and an observed GID that differs from the pin GID. They may
belong to the owner, another local account or a service account.

**Authority.** AGENTS.md and KICKOFF forbid private owner data in MCP
responses and logs. ADR 0009's privacy bound says responses never carry "SIDs,
UIDs other than the observed build identity". An observed value equal to the
pin *is* the observed build identity, so that bound explicitly allows it. No
authority allows any other principal's identifier. The fact that the service or
the build account could resolve a local SID or UID does not authorize
disclosing it. The earlier "adds no capability" argument is withdrawn.

Rules for `assess`. They apply to every row that writes observed fields: H and
I, and K–N, where the identifiers equal the pin by construction.

1. Compute `identity_matches_pin` from the full, unredacted observation first.
2. Write `observed_id` as the **observed** string only when it is byte-equal
   to `pin.identity`; otherwise NULL. Write `observed_gid` as the observed GID
   only when it equals `pin.gid`; otherwise NULL. Never write the pin in place
   of a withheld value.
3. Keep `observed_kind`, `observation_point`, `integrity_rid` and `elevation`
   as observed. They are token-class facts about the run's own child, not
   principal identifiers, and they carry the mismatch evidence (as in P2).
4. Admissibility requires a syntactically valid ID. So `observed_kind` set,
   `observed_id` NULL and `identity_matches_pin = 0` can only mean "a
   different principal was observed and withheld". No column, token or hash is
   added, and schema 3 already accepts that shape.
5. **Every current path reads that one value.** The `run_command` and
   `discover_tools` responses build `observed_identity` and the nested
   `metadata` copy from the `Metadata` that `assess` returns. The MCP layer
   sends that value unchanged (`crates/arxd/src/mcp.rs`). `list_activity`
   reads the row that the same value wrote. `list_hosts.last_observed_identity`
   is a constant null today. Any future source of it, or of a discovery
   identity, must take the redacted `Metadata`, never `Evidence`.
6. **Retention: none.** A protected identifier exists only inside the
   backend's `Evidence` until `assess` returns. It is not written to SQLite,
   the event log, the journal, stderr, the quarantine state or any cache. The
   `Debug` output of `Observation` redacts `sid`, `uid` and `gid`, so a panic or
   test message cannot print it.
   - Schema 3 has a single activity read scope: the calling agent principal,
     through `list_activity`. It has no owner-only column. ADR 0009 defers the
     owner view to the M6 dashboard. Retaining the identifier "for the owner"
     would therefore expose it to the agent, or it would need a new read scope
     and schema.
   - After an `identity_mismatch` quarantine, finding out *which* principal ran
     is an owner activity on the host, outside arxd. Owner-scoped forensic
     retention would need its own ADR with a read scope. It is not added here.
7. Development mode is unchanged; it records no observed identity at all.

The row stays truthful: `identity_mismatch`, `identity_matches_pin = 0`, the
kind, point and token class as observed, and quarantine. Nothing is
substituted or invented; a fact is only withheld.

## Output release (privacy, normative; output-release amendment, accepted)

**Gap.** Rule 5 above covers every metadata route. Captured output is a second
route. The `Completed` arm assessed and redacted the metadata, then always
returned both streams. So output from a child with a different SID, or with no
admissible observation at all, reached the response. The identifier was NULL in
the metadata but present in `stdout.head` and `stderr.tail`. See the
[probe](../reviews/0017-unverified-output-probe.md) (run
`b977ed9e-612e-4aff-b679-22f979ce4c16`, synthetic backend only). This is
latent because `Native` refuses, and an identifier is the least of the problem.
Output from an unvalidated child can carry anything that principal or token
could read.

**Trust basis.** ADR 0009 allows `stdout`/`stderr` as untrusted text from *the
run*. ADR 0007 says output may carry private data that the build account
generated. Neither ADR authorizes output produced under any other principal or
token class. The only fact that ties a child to the authorized build identity
and rights is a full pin match at the required observation point. That match
covers kind, point, ID syntax, SID or UID and GID, and the Windows token class.
Containment and cleanup facts describe the tree's lifetime, not who wrote the
bytes.

### Release predicate

```text
release(stdout, stderr)  ⇔  assess(...).metadata.identity_matches_pin == Some(true)
```

- The predicate reads only the `identity_matches_pin` value that `assess`
  returned. That value was computed from the full, unredacted observation before
  withholding. Use an exhaustive match over the `Option<bool>`, with no
  wildcard: `Some(true)` releases; `Some(false)` and `None` withhold.
- It never reads `observed_id`, `observed_kind`, `reason`, `outcome`, the
  quarantine flag, stop or cleanup facts, or the output bytes. In particular,
  `observed_id` is set for a same-SID token-class mismatch and for a Unix
  UID-matches/GID-differs mismatch, so it is not the gate.
- It is a trust gate on evidence. It is **not** content filtering. No regex,
  SID/UID pattern, marker list or other string scrubbing is applied to released
  output. Released output may legitimately contain any text, including a string
  that looks like another case's protected identifier.
- Released streams use the existing `output_value` unchanged: the same budget
  (`min(output_bytes, 7168) / 2` per head and tail), the lossy printable map, and
  `dropped_bytes` = backend-dropped bytes plus bytes trimmed by that budget.

### Withheld representation

A withheld stream keeps its key and object shape:

```json
{"head": "", "tail": "", "dropped_bytes": null}
```

- `head` and `tail` are empty strings. `dropped_bytes: null` is the only
  discriminator. It means "arxd did not release this stream and makes no
  statement about its size". It is not a measurement. It does not claim the
  child wrote nothing or that zero bytes were dropped. A released stream's
  `dropped_bytes` is always a number, so `null` occurs exactly when a stream is
  withheld.
- Both streams are always withheld together. The value is a constant. It does
  not depend on the captured bytes, their length, the backend's
  `dropped_bytes` or `output_bytes`. So no content, length or count of
  untrusted output leaves the dispatcher.
- Rejected alternatives: `dropped_bytes: 0` (a false measured claim, identical
  to an empty successful run); the real byte count (discloses untrusted
  length); a numeric sentinel (invented evidence); `stdout: null` or a missing
  key (a needless type and key change for the `Completed` shape); and a new
  token or key (forbidden).
- The withheld `CapturedOutput` is dropped inside the `Completed` arm without
  being converted, measured, formatted (including its derived `Debug`), logged,
  persisted or cached. No forensic output store exists; output stays
  non-persisted, as in ADR 0009.

### Decision by case

The amendment changes only the two output values. Every row keeps its outcome,
reason, metadata, quarantine and precedence.

| Case | Row | `identity_matches_pin` | Output |
| --- | --- | --- | --- |
| Admissible, SID or UID/GID different from the pin (Windows or Unix) | I | 0 | withheld |
| Pinned SID with the wrong Windows token class (P2, `Limited`, `DenyOnly`, `PresentOther`, `Default` + elevated) | I | 0 | withheld |
| Admin pin with a standard token (less privileged) | I | 0 | withheld |
| Unix UID A with GID B, or UID B with GID A | I | 0 | withheld |
| No observation, no point, wrong kind, wrong point, malformed or overlong ID | J | NULL | withheld |
| Cleanup or containment failure with an identity failure (for example SID B with cleanup NULL, or no observation with a wrong boundary) | H | 0 or NULL | withheld |
| Cleanup or containment failure (cleanup NULL or false, wrong boundary) with a **full match** | H | 1 | **released** |
| Full match; invalid stop or exit facts (stop NULL or `failed`, `exited` without a code, a code with another stop) | K | 1 | **released** |
| Full match; `timeout`, `cancelled`, `output_limit` or `shutdown` | L | 1 | **released** (partial output and its count kept) |
| Full match; exited non-zero | M | 1 | **released** |
| Full match; exited 0 | N | 1 | **released** |
| Rows A–G, and `FakeForTest` | — | — | unchanged: no `stdout`/`stderr` keys today |
| Discovery | H–N | — | unchanged: no streams; `tools` only on row N, which already requires a match |

Why each boundary falls where it does:

- **Identity failure withholds even a less privileged token.** The dispatcher
  checks equality with the pin. It has no validated order over token classes,
  and `PresentOther` is unclassified. On Windows, ADR 0009 step 7 kills the Job
  before resume on any mismatch, and the token is read before resume. So a
  conforming Windows adapter has no target output to lose in rows I and J. On
  Unix, output in such a report came from a child whose identity the parent did
  not validate. Either way, that output is exactly what the gate must not
  trust, and those rows already quarantine.
- **Cleanup-only failure releases.** With a full match, the bytes came through
  pipes that only the validated child and its descendants inherited (ADR 0009
  step 4's handle list; ADR 0007's CLOEXEC sweep). So their source is the
  authorized build identity. A failed kill, an unconfirmed empty
  tree or the wrong boundary leaves that unchanged. The same descendants could
  have written the same bytes under row N. Withholding would destroy the most
  useful evidence for a hung or runaway build, which is quarantined anyway.
  The reason stays `cleanup_unconfirmed`. Release does not soften it.
- **Invalid stop facts release.** Row K is an inconsistent backend report about
  how the run ended, not about who ran it. The output is still the build
  identity's output.
- **Identity versus cleanup-only, side by side.** An H row with SID B and an H
  row with SID A have the same reason, `cleanup_unconfirmed`, and both
  quarantine. Only the identity fact differs. So the first withholds and the
  second releases. Tests must pin this pair.

### Limits of the gate

The gate is a consistency check on typed claims, like the rest of this ADR (see
[What the interface can and cannot establish](#what-the-interface-can-and-cannot-establish)).
It cannot prove that the captured bytes were written after the observation or
only by the observed child. It attests nothing continuous after exec (ADR 0007:
set-ID programs remain subject to OS policy). Released output can still contain
private data that the build account itself can read. ADR 0007 already assigns
that to the owner's workspace and permission design. It is unchanged here.

## Outcome matrix

Precedence for spawned reports is top to bottom. The first row that matches
wins. "As reported" means the backend value is stored unchanged, including
NULL. Observed fields are written only for an admissible observation; otherwise
they are NULL along with `identity_matches_pin`. "Withheld" means the
[identifier rule](#identifier-disclosure-privacy-normative) applies.
`identity_mode` is always `dedicated_account`, and `program`/`argc` come from
the request, as today.

| # | Phase and condition | outcome / reason | stop, exit | observed, matches | rights | boundary, cleanup | quarantine |
| --- | --- | --- | --- | --- | --- | --- | --- |
| A | MCP invalid arguments (unchanged) | rejected / `invalid_arguments` | NULL | NULL | NULL | NULL | no |
| B | Dispatcher refusal: quarantined, stopping, backend preflight, pin error, busy | refused / mapped | NULL | NULL | NULL | NULL | no new |
| C | Dispatcher sees cancellation before `run` | failed / NULL | `cancelled`, NULL | NULL | NULL | NULL | no |
| D | `Unavailable(r)` | refused / `r.stored_reason()` | NULL | NULL | NULL | NULL | no |
| E | `CancelledBeforeSpawn` | failed / NULL | `cancelled`, NULL | NULL | NULL | NULL | no |
| F | `Failed { NotInvoked \| ProvenNone }` | failed / `execution_failed` | `failed`, NULL | NULL | NULL | NULL | no |
| G | `Failed { Created \| Unknown }` | failed / `cleanup_unconfirmed` | `failed`, NULL | NULL | pin | NULL, 0 | **yes** |
| H | Spawned; `cleanup_confirmed ≠ Some(true)` or boundary ≠ required | failed / `cleanup_unconfirmed` | as reported | if admissible, withheld | pin | as reported, **0** | **yes** |
| I | Spawned; admissible observation that does not match | failed / `identity_mismatch` | as reported | as observed but withheld, 0 | pin | `required`, 1 | **yes** |
| J | Spawned; no admissible observation | failed / `execution_failed` | as reported | NULL, NULL | pin | `required`, 1 | **yes** |
| K | Spawned; stop NULL or `failed`; `exited` without a code; a code with any other stop; or an invalid discovery table | failed / `execution_failed` | as reported | as observed, 1 | pin | `required`, 1 | **yes** |
| L | All checks pass; stop is `timeout`, `cancelled`, `output_limit` or `shutdown` | failed / NULL | as reported | as observed, 1 | pin | `required`, 1 | no |
| M | All checks pass; exited non-zero | failed / NULL | `exited`, code | as observed, 1 | pin | `required`, 1 | no |
| N | All checks pass; exited 0 | **succeeded** / NULL | `exited`, 0 | as observed, 1 | pin | `required`, 1 | no |

- **Quarantine** is one gate: a post-spawn report (G–N) whose reason is not
  NULL. `FakeForTest` keeps its current `cfg(test)` mapping and can never
  succeed.
- **Discovery** follows the same rows. Its `tools` list is returned only for
  row N; every other row returns no tools.
- **The response** keeps its current shape and keys. `observed_identity` is
  non-null only for an admissible observation, and it equals the row,
  including withheld NULL `id` and `gid`. No new response or row field exists,
  and no free text beyond the syntax-checked observed build ID can reach
  either.
- **Output** (amendment, accepted) is orthogonal to these rows. `Completed`
  releases `stdout`/`stderr` only when `identity_matches_pin = 1`. That is
  always true in rows K–N, true in row H only with a full match, and never true
  in rows I and J. See
  [Output release](#output-release-privacy-normative-output-release-amendment-accepted).

## Probe and correction disposition

| Case | Representation under this ADR | Required result |
| --- | --- | --- |
| P0 | `observation: None`, `point: None` | J: failed / `execution_failed`; observed and matches NULL; cleanup 1; quarantine |
| P1 | `cleanup_confirmed: None` | H: failed / `cleanup_unconfirmed`; cleanup 0; observed as observed, matches 1; quarantine |
| P2 | Backends can no longer set `rights`. The token-level form is the pinned SID with integrity 12288, `Full`, elevated and `Enabled` against a standard pin. | I: failed / `identity_mismatch`; `rights = standard`; the pinned SID (the observed build identity), integrity 12288 and elevation 1 stored; matches 0; quarantine |
| P3 | `point: Some(Child)` | J: observed and matches NULL; quarantine |
| P4 | `tree_boundary: Some(ProcessGroup)` on a Windows pin | H: `tree_boundary = process_group` stored, cleanup 0, `cleanup_unconfirmed`; quarantine |
| R1 (review P1) | Unix child created, pre-exec step fails, no identity bytes, no confirmed cleanup | `Failed { Created }` → G: failed / `cleanup_unconfirmed`; observed NULL; rights pin; cleanup 0; quarantine. Never F. |
| R2 (review P1) | Primitive called, outcome indistinguishable | `Failed { Unknown }` → G, as R1 |
| R3 (review P2) | Pin SID A; admissible observation with a different SID B | I: `observed_kind = windows_sid`, `observed_id` NULL, matches 0; B absent from the row, the response (both copies), `list_activity` and the database bytes |
| R4 (review P2) | Unix pin UID/GID A; observed UID and GID B | I: `observed_id` and `observed_gid` NULL; kind and `pre_exec` stored; B absent as in R3 |
| O1 (output probe, amendment accepted) | Windows standard pin A; `Completed` with SID B observed, or with no observation and no point; SID B text in `stdout.head` and `stderr.tail` | I or J as before. Both streams `{"head":"","tail":"","dropped_bytes":null}`; SID B absent from the whole response, the MCP text and the database bytes |

## What the interface can and cannot establish

The dispatcher checks that typed facts are consistent with immutable pins, and
it fails closed. It cannot establish that those facts are true. A reported
SID, token class, `windows_job` boundary, `cleanup_confirmed = true` or
creation outcome is the adapter's claim. Only native code backs it:

- On Windows: the token is read from the suspended child before
  `ResumeThread`; the process is assigned to the Job before resume, with no
  breakaway; and `TerminateJobObject` is followed by `ActiveProcesses == 0`.
- On Unix: ADR 0007's in-child checks run before the CLOEXEC report; then
  cgroup kill and empty leaf, or the sweep with zero real or effective build
  UIDs.
- On both: every failure site maps to the creation outcome that the
  [boundary rules](#child-creation-boundary-normative) allow.
- On both (output-release amendment, accepted): `CapturedOutput` holds only
  bytes read from the observed child's own stdout and stderr pipes. The adapter
  never merges its own diagnostics or another process's bytes into it, and on
  Windows it reads nothing before the pre-resume token read.

A Boolean cannot prove containment or token groups. So closing N2 is necessary,
not sufficient. Each native adapter still needs its own unprivileged tests
(ADR 0009 slice 8: synthetic `TokenGroups` buffers, a real Job tree-kill, and
the handle list), tests of its creation-outcome classification, and later
hardware evidence. Nothing here is acceptance.

## Relation to ADR 0009

**Amendment 1 (review P1): the pre-observation spawn-error row.** ADR 0009 maps
"spawn or executor error before an identity observation" to `failed` /
`execution_failed` with observed fields NULL. That trigger is keyed to the
observation, which is the downgrade the review found. Under this ADR the row
applies to failures before child creation (row F), and to a created child that
has no observation but whose cleanup is confirmed inside the required boundary
(row J). A created or unknown child without confirmed cleanup is
`cleanup_unconfirmed` and quarantines (row G). That follows ADR 0009's own
precedence ("`cleanup_unconfirmed` takes precedence over every other reason")
and its quarantine rule ("any executor error after spawn").

**Amendment 2 (review P2): identifiers on an identity mismatch.** ADR 0009's
mismatch row says "observed fields as observed". Its activity column says
"NULL or as observed", and its `run_command` result carries
`observed_identity {kind, id, …}`. Those conflict with ADR 0009's own privacy
bound and with AGENTS/KICKOFF whenever the observed principal is not the build
account. This ADR resolves the conflict in favour of the privacy fence:
`observed_id` and `observed_gid` that differ from the pin are NULL in rows and
responses. Every other observed field stays as observed. Columns, vocabulary,
schema and response keys are unchanged. The value domain changes in one way:
`observed_identity.id` and `gid` may be null while `kind` is set.

**Amendment 3 (output-release amendment, accepted): output release.** ADR
0009's `run_command` result returns `stdout`/`stderr` as `{head, tail,
dropped_bytes}` from the run, with no release condition. This ADR adds one:
a `Completed` report releases output only when `identity_matches_pin = 1`.
Otherwise each stream is `{"head":"","tail":"","dropped_bytes":null}`. Keys and
the `head`/`tail` string types are unchanged. The value domain changes in one
way: `dropped_bytes` may be null, and only for a withheld stream. Output stays
untrusted and is never persisted. ADR 0009's narrow pointer now names this accepted amendment.

**Clarifications within ADR 0009's tables** (no deviation):

- Who computes `identity_matches_pin`: the dispatcher.
- The mismatch row now quarantines (N1).
- Wrong containment counts as unconfirmed cleanup, per its attestation table.
- Inadmissible observations are dropped. ADR 0009 is silent on them.
- Pre-spawn cancellation is a typed report.
- Tools are returned only on success.

Its outcome and reason values, the observation points and the schema are
unchanged. ADR 0009 carries a narrow pointer that names all three amendments.
The local contract and amendments have passed independent Sol review. Full
implementation acceptance still requires the independent code review and hosted
gates; contract acceptance alone activates no native execution.

## Compatibility, rollback and acceptance

- **No migration.** Schema 3 and every vocabulary are unchanged. Rows written
  under this ADR, including a withheld `observed_id` with `observed_kind` set,
  satisfy the existing `CHECK`s. They are valid for the current and previous
  readers.
- **Rollback** is a revert of the implementation PR, with no data step.
- **Output-release amendment (accepted):** no migration and no persisted value.
  It changes only the immediate `run_command` response of a `Completed` report
  without a full match. Rollback is a revert of that change. A revert would
  reopen the probe's disclosure, so it is not a safe rollback while any
  non-refusing dedicated backend exists.
- **No production change.** `Native` still refuses with
  `mechanism_unavailable`, and `list_hosts` is unchanged. Only synthetic and
  test paths change. Existing tests that relied on backend-supplied `Metadata`
  or `Failed { after_spawn }` are rewritten, not weakened (see the brief).
- **Out of scope:** N3 and the concurrent-drain API, E1 provisioning and owner
  contracts, S4U or any credential mechanism, development mode, native
  adapters, and owner-scoped forensic retention.

## Sources and inference

Primary sources checked on 2026-11-11 for the original proposal (not re-fetched
for this correction):

- [TOKEN_ELEVATION_TYPE](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ne-winnt-token_elevation_type)
  defines Default = 1 (no linked token), Full and Limited.
- [TOKEN_ELEVATION](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-token_elevation):
  `TokenIsElevated` is a separate nonzero/zero fact.
- [Well-known SIDs](https://learn.microsoft.com/en-us/windows/win32/secauthz/well-known-sids)
  give the Medium RID 0x2000 (8192) and the High RID 0x3000 (12288).
- [SID attributes](https://learn.microsoft.com/en-us/windows/win32/secauthz/sid-attributes-in-an-access-token):
  `SE_GROUP_USE_FOR_DENY_ONLY` implies not enabled.
- [JOBOBJECT_BASIC_ACCOUNTING_INFORMATION](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_basic_accounting_information):
  `ActiveProcesses` drops only after process references are released.
- [MS-DTYP SID string syntax](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-dtyp/c92a27b1-c772-4fa7-a432-15df5f1b66a1):
  the authority is decimal below 2^32 and hexadecimal above it.

Repository sources read for this correction: `crates/arxd/src/exec/unix.rs`
(`pre_exec` sequence and `Command::spawn`), `crates/arxd/src/service/executor.rs`
(`dispatch` response assembly), `crates/arxd/src/store.rs` (`Metadata`,
`list`, the schema 3 `CHECK`s) and `crates/arxd/src/mcp.rs` (dedicated
routing and `list_hosts`). Nothing was run.

The output-release amendment uses no new external fact. It is an inference
from this contract, ADR 0009 (Windows steps 4, 7 and 8; the `run_command`
result) and ADR 0007 (in-child identity report before exec; build-account
output). Sources read at `2f3b4863`: `executor.rs` (`complete`, `assess`,
`output_value`), `executor/tests.rs` (`completed`, `assert_private`, the SDK
bound test), `application/tests.rs` (`dedicated_report_mcp_privacy_in_memory`),
the [probe](../reviews/0017-unverified-output-probe.md) and its patch. Nothing
was run for the amendment, and nothing native was validated.

Inference, not verified:

- That `ConvertSidToStringSidW` output byte-equals the configured canonical
  SID. Any difference fails closed as `identity_mismatch`, with the observed
  SID withheld.
- That an S4U child token has the shapes above. ADR 0009 already marks S4U
  admin linked-token behaviour unverified.
- Adapters must release process handles before trusting
  `ActiveProcesses == 0`.
- That Rust std reports `pre_exec` and exec failures through the `Err` of
  `Command::spawn`. This ADR relies on that only to classify the result as
  `Unknown`, which is the fail-closed reading. It claims nothing about whether
  std reaps the child.
- No creation primitive is classified `ProvenNone` here. A later adapter ADR
  must cite the normative text.
