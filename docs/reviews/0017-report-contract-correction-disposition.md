# ADR 0017 correction disposition

Author: Anthropic / claude-opus-5-5 / high, architecture builder, 2026-11-11.
This file answers the [independent Sol review](0017-report-contract-sol-review.md)
of the proposed contract at `86b0a40b`. Correction base: `f8a02af7`. The
corrected [ADR 0017](../adr/0017-dedicated-report-validation.md) and
[brief](../briefs/0017-report-validation-implementation.md) stay **proposed**
until a fresh Sol recheck. Documentation only: no Rust, workflow, PROGRESS,
E1, N3 or diagnostic evidence edits, and nothing was run beyond text checks.

The review accepted typed evidence, the immutable pin and the five-probe
closure in principle. Those parts are unchanged, except where P1 or P2 below
touches them.

## P1 (medium): creation boundary — accepted, corrected

**Finding.** "No process object" and "forked Unix child that reached exec" are
different boundaries. That allowed a created-but-not-execed child to be
reported as `Failed { after_spawn: false }` (row F), with no cleanup claim and
no quarantine.

**Confirmed from source** (read, not run). In `crates/arxd/src/exec/unix.rs`,
ADR 0007's runner puts `setpgid`, the cgroup join, the ID drop, the cwd open
and the identity write in a `pre_exec` closure. It then calls
`Command::spawn()?`. A failure in that closure or in exec reaches the parent
through the same `Err` as a fork failure. The child may have run, as the
service identity, before its ID drop. The review's trace is therefore a real
ambiguity in the contract. No leak is claimed.

**Correction.**

- **One boundary:** the call to the creation primitive (Windows
  `CreateProcess*`, Unix fork/clone/vfork/posix_spawn), never target exec.
- `Failed { after_spawn: bool }` becomes `Failed { creation: Creation }`, with
  `NotInvoked | ProvenNone | Created | Unknown`. The dispatcher maps it with no
  wildcard: `NotInvoked` and `ProvenNone` give F; `Created` and `Unknown` give G
  (`cleanup_unconfirmed`, quarantine).
- **When an error proves no child.** Only when all three hold: the adapter
  called the primitive directly; the error result is distinguishable; and a
  cited normative specification says no child is created for it. No current
  path qualifies.
  - Unix `Command::spawn` `Err` is `Unknown`.
  - Windows `CreateProcess*` returning `FALSE` is `Unknown` until a native ADR
    cites a guarantee.
  - A successful `CreateProcess*` followed by a failed Job assignment or token
    read is `Created`, outside the boundary.
- **No downgrade.** A missing observation, PID, output, exit or cleanup fact
  never makes a report pre-spawn.
- **Cleanup without an observation.** A created child with no observation can
  reach row J (still quarantined) only if the required boundary is proven to
  contain it. A build-UID sweep cannot cover a child that may still run as the
  service identity.
- Cancellation and `Stopping` are honored only before the call.
- **Required synthetic test:** a `PhaseScript` created-not-execed case, with no
  identity bytes and no confirmed cleanup, must give G with quarantine and
  never F. The brief adds an `Unknown` sibling, the `NotInvoked` and
  cancel-before-call siblings, and mutations 20–21. No native experiment.

**ADR 0009 effect.** Labelled Amendment 1. ADR 0009's "spawn or executor error
before an identity observation" row is re-keyed to child creation. It follows
ADR 0009's own `cleanup_unconfirmed` precedence and its "any executor error
after spawn" quarantine.

## P2 (high): mismatched-principal disclosure — accepted, corrected

**Finding.** Row I would persist and return a non-build principal's SID or UID
(`observed_identity.id`, `metadata.observed_id`, then `list_activity`). It
justified that by lookup capability. The original ADR also called this "no
deviation".

**I do not challenge it.** No authority permits disclosing another principal's
identifier:

- AGENTS.md and KICKOFF forbid private owner data in MCP responses and logs.
- ADR 0009's privacy bound allows only "the observed build identity".
- Lookup ability is not disclosure authority. The "adds no capability"
  argument is withdrawn.

ADR 0009's mismatch row ("observed fields as observed") conflicts with its own
privacy bound. The correction resolves that conflict in favour of the fence and
labels the change.

**Confirmed from source.** `dispatch` builds `observed_identity` and the nested
`metadata` from one `Metadata` (`crates/arxd/src/service/executor.rs`). The
dedicated MCP path sends that value unchanged (`crates/arxd/src/mcp.rs`).
`Activity::list` returns every observed column of the agent's own rows
(`crates/arxd/src/store.rs`). `list_hosts.last_observed_identity` is a
constant null.

**Correction.**

- **Protected identifiers:** an observed SID or UID not byte-equal to the pin,
  and an observed GID not equal to the pin GID.
- `assess` computes `identity_matches_pin` from the full observation first.
  It then writes `observed_id` and `observed_gid` only when they equal the pin;
  otherwise they are NULL. It never writes the pin in their place.
- Kind, point, integrity RID and elevation stay as observed, so P2-style
  token-class evidence stays truthful.
- Kind set with a NULL ID and matches 0 unambiguously means "different
  principal, withheld". There is no new column, token, hash, schema migration,
  native lookup or approval flow, and schema 3 `CHECK`s already accept the
  shape.
- **All paths are covered by one value.** Withholding in `assess` covers the
  row, both response copies and `list_activity`. Any future source of
  `last_observed_identity` must use the redacted `Metadata`.
- **Retention: none.** Not SQLite, logs, quarantine state or caches.
  `Observation`'s `Debug` redacts identifiers. Reasons:
  - schema 3 has one activity read scope, the agent principal, and no
    owner-only column;
  - ADR 0009 defers the owner view to M6;
  - AGENTS forbids private data in logs.
  Owner-scoped forensic retention would need its own ADR. It is not added.
- **Tests** use distinct pin A and protected B values, for the Windows SID and
  for the Unix UID and GID. They prove B is absent from all of these paths:
  - the immediate response, by a recursive walk that includes `metadata`;
  - the row;
  - `Activity::list`;
  - the quarantine refusal;
  - the database bytes and the journal sidecar;
  - `Debug`;
  - one MCP end-to-end test over `run_command`, `list_activity` and
    `list_hosts`.
  Mutations 22–28 cover the rule.

**ADR 0009 effect.** Labelled Amendment 2. Its mismatch row, activity column
text and `observed_identity.id`/`gid` value domain change (null allowed with
`kind` set). Keys and schema are unchanged. ADR 0009's pointer paragraph now
names both amendments; its tables are not edited while 0017 is proposed.

## Requirement differences from `86b0a40b`

| Area | Before | After |
| --- | --- | --- |
| Phase boundary | "process object", narrowed to a Unix child "that reached exec" | The creation-primitive call, never exec; typed `Creation` outcome |
| `Report::Failed` | `{ after_spawn: bool }` | `{ creation: Creation }`; F for `NotInvoked`/`ProvenNone`, G for `Created`/`Unknown` |
| Unknown outcome | Unspecified | Post-spawn: G, quarantine |
| Mismatch identifiers | Stored and returned as observed | Withheld (NULL) unless equal to the pin; matches computed first |
| Retention | Row kept the observed ID | None anywhere; redacted `Debug` |
| ADR 0009 relation | "No concrete deviation" | Amendments 1 and 2, labelled; pointer narrowed |
| Brief scope | executor, its tests, the store helper | Adds one test in `application/tests.rs` and makes the test fixtures `pub(crate)`; production scope unchanged |
| Tests | Probes, rows, privacy of invalid IDs | Plus the created-not-execed/`Unknown` phase tests, the B-identifier privacy matrix, MCP end to end, and mutations 20–28 |

Unchanged: typed `Evidence`, `Pin`, admissibility table, precedence, rows A–E
and H–N semantics (except withheld identifiers), the bounds, the kind, stop,
cleanup and identity-mismatch semantics, schema 3 and the probe results for
P0–P4.

## Sources and limits

- Read: the full Sol review; AGENTS.md; the KICKOFF routing and privacy
  sections; ADR 0009's preflight and post-preflight tables, privacy bound,
  quarantine, MCP surface, activity schema and `list_activity`; the
  report-validation probe; and at `f8a02af7`, `executor.rs`, `store.rs`,
  `mcp.rs`, `exec/unix.rs` and the relevant executor and application tests.
- Not done: no Rust build or test, no native or identity API call, no
  documentation re-fetch, no subagent, no push, PR, GitHub write, merge or
  dispatch, and no owner, admin, SCM, LSA, account, VM or Lab Manager action.
- The Rust std `pre_exec` error propagation is used only to choose the
  fail-closed `Unknown` classification. The claim that `CreateProcess*`
  documentation gives no no-process guarantee comes from reading, not a
  fresh fetch, and it also only selects the fail-closed path.
- Installed, native and dedicated M2 paths remain **unverified on real
  hardware**. No M2 acceptance is claimed.

Next step: a fresh, independent Sol recheck of the corrected ADR and brief.
Sol high implementation follows only if that recheck accepts them.
