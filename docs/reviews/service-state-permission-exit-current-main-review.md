# State permission exit port onto PR42: independent Opus integration review

Reviewer: Anthropic / claude-opus-5-5 / high. Review sub-agent, single agent,
no subagents, 2026-11-12. This is a cross-provider review of a Sol coordinator
port (OpenAI / gpt-6.1-sol / medium). It covers the port only. It makes no new
independent Opus architecture claim. Local only: no GitHub read or write, push,
PR, status, watch, approval, merge, retry or settings change.

Verdict: **PASS on both axes. No blockers and no new advisories. The two prior
advisories are carried forward unchanged.**

## Target and pins

- Base `4c2264d6c3caee9507e11870ad361daea6c1dfdf` (PR42 merge on main).
- Head `8c6f69c9c4e9f9daf9c9188efddc416a3250cd11`, one commit
  (`8c6f69c fix(service): preserve typed state permission exit codes`).
- Reviewed in the tool-prepared isolated worktree at exactly this head, with a
  clean status. `D:/Code/Arxcovo` was at the same head with a clean status and
  was not modified.
- Six files: `crates/arxd/src/service/application.rs` (+7/−1),
  `application/tests.rs` (+33), `docs/PROGRESS.md` (+9),
  `docs/evidence/state-permission-exit-current-main-port.md` (new),
  `docs/reviews/service-state-permission-exit-opus-review.md` (retained) and
  `docs/reviews/service-state-permission-exit-correction.md` (retained).
- Original reviewed source: `7781239b699ab026553ab2957286670d0025785d` on base
  `7fefe4d`. Prior Opus report from `48dd535`.

## Source identity (checked mechanically)

| Check | Result |
| --- | --- |
| `application.rs` blobs: base `4772032` → head `05dafc6`, and 7fefe4d `4772032` → 7781239 `05dafc6` | Identical before and after. The production delta is byte-for-byte the reviewed delta. |
| Production files changed in `4c2264d..8c6f69c` under `crates/` | Only `application.rs` and `application/tests.rs`. No unrelated production edit. |
| `application.rs` hunk | `Native::store` maps errors with `store_category(&error)`. The private `store_category(&anyhow::Error)` maps typed `Permissions`/`OwnerRequired` to `ExitCode::Permissions` (4). Every other error, typed or untyped, maps to `Store` (5). |
| Head `tests.rs`, first 32009 bytes compared with base `tests.rs` | `cmp` identical. Every existing N2 test byte is retained. |
| Head `tests.rs` tail after base length, compared with the 7781239 tail after the 7fefe4d length | `cmp` identical. This is the exact 33-line original classifier test, appended after the last N2 test (`dedicated_report_mcp_privacy_in_memory`). It is ASCII with LF line endings. |
| Retained Opus report blob at head and at `48dd535` | Both `f2f8761`. Unchanged. |
| Retained correction evidence blob at head and at `7781239` | Both `f9316e7`. Unchanged. |
| `git diff --check 4c2264d 8c6f69c` | Clean. |

I did not repeat the coordinator's git-apply preflight. The byte comparisons
above are consistent with what it reports: the EOF hunk failed with no change,
and the test was then appended by hand.

## Propagation on the N2 base (source)

`Native::store` → `Activity::open_service(layout)` (`anyhow::Result`) →
`native::state_guard(layout)?` (`Result<_, layout::Error>`). The `?` wraps the
typed error as the anyhow root, so `downcast_ref::<layout::Error>()` reaches
it. It would still reach it if a `.context(..)` were added later, and the
classifier test checks that case. At `7fefe4d` and at the base, `open_service`
is identical and the `state_guard` body hashes the same. Between those
commits, `native.rs` changed only by removing one test. The `store.rs` changes
are limited to `Metadata` identity validation, which is unrelated to the open
path. The platform `native_*` helpers and `layout.rs` are unchanged. ADR 0009
still assigns `4` to config/state ownership or permissions and `5` to store
unavailable. The retained report's propagation analysis is therefore still
valid on this base.

## Standards

No findings. The change is one focused conventional commit. The classifier is
private and does no string matching, and the test is pure. PROGRESS starts
with a single current section. That section has one queue-priority line and
records provider, effort, outcome and the actual checks. The earlier state
moves under a "Historical checkpoint" heading. Pending facts are labelled as
last observations: main after #42 is "pending at the last observation, not
claimed green". The SYSTEM-refusal test stays local at `0122bf1`, queued
behind PR43, and that commit exists locally. The **unverified on real
hardware** label and the incomplete-M2 status are kept. The new evidence file
and the relative links in PROGRESS resolve. No baseline smell applies to the
seven-line classifier.

## Spec

The settled behaviour is implemented without drift. Typed `Permissions` and
`OwnerRequired`, including under an anyhow context, give 4. The other eleven
`layout::Error` variants give 5, as do a generic anyhow error and a raw I/O
`PermissionDenied`. The test still lists every current `layout::Error`
variant, and the base enum is unchanged since 7fefe4d. No behaviour was added
beyond the reviewed correction.

## Evidence credit

- **Prior heads:** the builder's two mutations, its 254/2 nextest run
  (`c032f585`), my earlier full run (`876aba30`) and MB1–MB5 are credited only
  to `7781239`. They are not re-credited to this head.
- **Coordinator checks on this head, as reported and not rerun by me:** fmt,
  locked workspace all-target Clippy, and one normal full nextest run, 242
  passed and 2 skipped (`ec5774c0-e8d0-4e79-b87e-532bd1b42301`, 10.057 s). No
  instrumentation or new mutations. 242 equals the base's 241 plus this one
  test, which is consistent.
- **My check on this head:** the focused exact classifier test only,
  `cargo nextest run -p arxd --locked --lib -E
  'test(=service::application::tests::state_store_errors_keep_permissions_separate_from_storage_failures)'`.
  Result: **1 passed**, 205 skipped, run
  `c5b2a01e-1854-4520-835c-33cf6de024ab`, exit 0. It used synthetic errors
  only.

## Advisory disposition

- **A1, still open.** The `Native::store` call site is checked by source
  inspection only, and the MB5 survivor still stands. The evidence file
  discloses this.
- **A2, still open.** A typed `Layout` error from the state guard still exits
  with 5 rather than 7. That is not a regression, and the evidence file
  discloses it.
- The prior integration note about a test-file conflict is resolved: both test
  sets are kept.
- `ExecutionFailed` has not recurred but is not fixed. E2's red run `bdbf032b`
  stays held, and this port neither waives nor claims to fix it.

## Limits

No full suite was run by me. I ran no native helper, and no production
`Native`/`Application`/store constructor or `state_guard`. I performed no
owner, SCM, LSA, privilege, account, service, pipe, system-path, VM, Lab
Manager, private-state, experiment or ignored-fixture operation. No PR exists
yet. Publication waits for its place in the queue and for this review, and
merging needs all hosted gates. Native, installed and dedicated paths are
**unverified on real hardware**. M2 is incomplete.

Standards: 0 findings. Spec: 0 findings, with prior advisories A1 and A2
carried forward. Worst open item: A1, the call site has no test.
