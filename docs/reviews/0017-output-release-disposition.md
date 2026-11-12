# ADR 0017 output-release disposition

Author: Anthropic / claude-opus-5-5 / high, architecture builder, 2026-11-11.
This file answers the
[unverified-identity output probe](0017-unverified-output-probe.md). Base:
`2f3b4863dd431794a86114db2d4933d7a282c1ee`. Its source equals the completed
Sol validator at `c394ec5`; the probe's diagnostic source was restored. The
amended [ADR 0017](../adr/0017-dedicated-report-validation.md) and
[brief](../briefs/0017-report-validation-implementation.md) label this the
**output-release amendment, proposed**, until a fresh independent Sol review.
Documentation only: no Rust, PROGRESS, workflow, E1/E2 or diagnostic evidence
edits, and nothing was run beyond text checks.

Status update (Sol high implementation, 2026-11-11): the
[independent Sol review](0017-output-release-sol-review.md) accepted proposal
`0132c3e` for local implementation. The proposal-time status above and probe
remain historical evidence. See [implementation evidence](0017-output-release-evidence.md)
for actual checks; full independent Opus implementation review remains pending.

## Counterexample

The probe ran real `dispatch`, a synthetic backend and an owned temporary
store (run `b977ed9e-612e-4aff-b679-22f979ce4c16`, one diagnostic, two cases).
The pin was Windows standard SID A. Exit, cleanup and boundary facts were
normal. Both streams carried SID B text (`stdout.head` and `stderr.tail`).

| Case | Evidence | Row and metadata | Output returned |
| --- | --- | --- | --- |
| 1 | admissible observation with SID B | I: failed / `identity_mismatch`, `observed_id` NULL, quarantine | `stdout.head` = `stderr.tail` = SID B |
| 2 | no observation, no point | J: failed / `execution_failed`, observed NULL, quarantine | the same |

The identifier rule worked for every metadata route and failed on the output
route. **Source cause** (read at `2f3b4863`): in `complete()`, the
`Report::Completed` arm calls `assess`, then builds
`output_value(stdout, …)` and `output_value(stderr, …)` with no condition. The
existing privacy cases missed it because the test `completed()` helper and the
MCP privacy test supply empty streams. It is latent. `Native` still refuses
`mechanism_unavailable`, no private owner data was used, and no child or
native backend ran.

The problem is wider than identifiers. Output from a child that does not match
the pin, or whose identity was never observed, is not established as output of
the authorized build identity. It can carry anything that principal or token
could read, so scrubbing identifier-shaped strings would not close it.

## Decision

**Release predicate.** `stdout` and `stderr` are released if and only if the
`identity_matches_pin` value returned by `assess` is `Some(true)`. That value
is computed from the full, unredacted observation. It requires the platform's
kind and point, a valid ID, SID or UID and GID equality, and on Windows the
exact token class. Nothing else enters the predicate: not reason, outcome,
quarantine, stop, cleanup, `observed_id`, `observed_kind` or the bytes.

**Withheld form.** Both streams become exactly
`{"head":"","tail":"","dropped_bytes":null}`. Keys and the string types stay.
`dropped_bytes: null` means "not released; arxd makes no statement about its
size". It is never a measurement, and it is null only for a withheld stream.
The value is constant, so no content, length or count leaves the dispatcher.
The captured bytes are dropped unread: not converted, measured, formatted,
logged, persisted or cached.

**Unchanged.** Metadata classification, per-field identifier withholding,
reason and cleanup precedence, quarantine, creation phases, immutable start
mode, N3 drain, fixed output budgets, the lossy text mapping and the SDK
double-JSON bound. Rows A–G and `FakeForTest` have no streams today and keep
none. Discovery has no streams, and its `tools` are already returned only on
row N.

### Why this predicate and not another

| Alternative | Rejected because |
| --- | --- |
| String or regex scrubbing of SIDs, UIDs or markers | It does not address non-identifier private content. It corrupts legitimate output, and matching output may contain the same public synthetic string. The probe asked for a trust gate, not content filtering. |
| Gate on `observed_id.is_some()` | `observed_id` is set for a pinned SID with the wrong token class, and for Unix UID A with GID B. Both are identity failures. |
| Gate on `observed_kind.is_some()` | It is set for SID B (row I). |
| Gate on `reason.is_none()`, no quarantine, or success | It discards useful output for cleanup-only (H) and invalid-stop (K) failures with a valid identity. With success only, it also drops nonzero exit, timeout, cancel, output-limit and shutdown output (L, M). |
| Gate on `reason != identity_mismatch` | Rows J and H can hide an identity failure behind another reason. |
| Withhold for a mismatch but release for a "less privileged" token | The dispatcher validates equality with the pin, not a privilege order. `PresentOther` is unclassified. A conforming Windows adapter never resumes a mismatched child, so nothing useful is lost. |
| Withheld `dropped_bytes: 0`, the real count, or a sentinel | False measured claim, length disclosure, or invented evidence. |
| `stdout: null`, a missing key, or a new token | Needless type or key change; new tokens are forbidden. |

## Decision and test matrix

All cases use real `dispatch`, a durable row, nonempty `head` and `tail` in
both streams (SID B text, `ARX0017-OUTPUT-MARKER` and `62002`), and backend
`dropped_bytes` 5 and 7. "Row unchanged" means outcome, reason, metadata and
quarantine equal the accepted ADR 0017 expectation.

| Test case | Evidence (pin Windows standard A unless stated) | Row | matches | Output | Required assertions beyond "row unchanged" |
| --- | --- | --- | --- | --- | --- |
| `out_sid_b` | SID B | I | 0 | withheld | exact withheld value; `assert_private` on the whole value; no marker; MCP |
| `out_same_sid_full` | SID A, 12288, `Full`, elevated, `Enabled` | I | 0 | withheld | `observed_id` = SID A, yet withheld; kills the `observed_id` gate |
| `out_same_sid_limited_denyonly` | SID A, `Limited`, `DenyOnly` | I | 0 | withheld | as above |
| `out_admin_pin_standard_token` | admin pin, standard token | I | 0 | withheld | the less privileged token is still withheld |
| `out_unix_uid_b` | Linux; UID 62002, GID 61001 | I | 0 | withheld | `observed_gid` = 61001 kept |
| `out_unix_gid_b` | Linux; UID 61001, GID 63003 | I | 0 | withheld | `observed_id` = "61001", yet withheld |
| `out_no_observation` | no observation or point (P0) | J | NULL | withheld | MCP |
| `out_wrong_kind` | Unix observation on a Windows pin | J | NULL | withheld | — |
| `out_wrong_point` | `pre_exec` on Windows | J | NULL | withheld | — |
| `out_malformed` | SID `S-1-x` | J | NULL | withheld | no `StorageFailed` |
| `out_sid_b_cleanup_null` | SID B, cleanup `None` | H | 0 | withheld | paired with `out_sid_a_cleanup_null` |
| `out_none_wrong_boundary` | no observation, `process_group` | H | NULL | withheld | — |
| `out_sid_a_cleanup_null` | cleanup `None` | H | 1 | **released** | quarantined and `cleanup_unconfirmed`, yet released; MCP |
| `out_sid_a_wrong_boundary` | `process_group` (P4) | H | 1 | **released** | — |
| `out_stop_failed` | stop `failed`, no code | K | 1 | **released** | quarantined, yet released |
| `out_timeout`, `out_cancelled`, `out_output_limit`, `out_shutdown` | that stop, no code | L | 1 | **released** | partial output and counts 5 and 7 kept |
| `out_exit_nonzero` | exited 2 | M | 1 | **released** | — |
| `out_ok` | none | N | 1 | **released** | SID B text and `62002` returned verbatim; `assert_private` on non-stream keys only; MCP |
| `out_linux_ok`, `out_macos_exit_nonzero` | Linux N; macOS M | N, M | 1 | **released** | — |
| withheld constant | empty capture versus 8192 × `0xFF` with `u64::MAX` dropped | — | — | withheld | byte-identical JSON |
| SDK worst case | existing `output_fits_sdk_double_json_worst_cases` | N-shaped | 1 | released | unchanged bound below 65,536 bytes |

Released values are exact: `stdout` =
`{"head":"S-1-5-21-700-800-900-4242","tail":"ARX0017-OUTPUT-MARKER","dropped_bytes":5}`
and `stderr` = `{"head":"ARX0017-OUTPUT-MARKER","tail":"62002","dropped_bytes":7}`,
with the default 4096-byte request.

**Identity failure versus cleanup-only failure.** `out_sid_b_cleanup_null` and
`out_sid_a_cleanup_null` are both failed / `cleanup_unconfirmed`, and both
quarantine. They differ only in the identity fact, and only the first
withholds. The brief requires one named assertion over that pair.

**Durable and envelope coverage.** In every case, the database bytes (and any
remaining protected journal) and the `Activity::list` page contain neither SID
B nor the marker. Output is never persisted, whether it is released or not.
The in-memory MCP test runs `out_sid_b`, `out_no_observation`, `out_ok` and
`out_sid_a_cleanup_null` through `run_command`, then `list_activity` and
`list_hosts`. The existing 50-case durable matrix also gets the nonempty
payload and asserts release exactly when its expected match is 1. Mutations
29–40 in the brief map each wrong gate to a named case.

## Limits

- This is a contract and source inference. No new external fact was needed,
  and no primary source was fetched. Nothing native was validated.
- The gate checks typed claims for consistency. It cannot prove that the bytes
  were written after the observation or only by the observed child. The ADR
  adds that to each native adapter's obligations. It attests nothing
  continuous after exec. Released output can still contain data that the
  build account itself can read (ADR 0007).
- No actual process, pipe, socket, production constructor, SCM, LSA, account,
  privilege, owner, admin, service-manager, VM, Lab Manager or S4U operation.
  No push, PR, GitHub write, dispatch or merge. The probe's diagnostic and its
  run evidence are unchanged.
- Installed, native and dedicated M2 paths remain **unverified on real
  hardware**. M2 is incomplete. Sol contract review now passes; a fresh independent Opus
  implementation review and hosted gates are still required.
