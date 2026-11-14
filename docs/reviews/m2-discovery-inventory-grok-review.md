# M2 discovery inventory review

Disposition: **no source blocker.** Retained `86917c4` is the same patch as
Sol `472c3420`. Retained `db99c80` is the same patch as Sol `8b2387f`.
Together they report a released discovery cache from `list_hosts` and cache a
development discovery only after a complete, audited, eligible table.
**This is a source review for the combined inventory feature. It grants no
merge, no installed acceptance, and no hardware credit.**

Reviewer: xAI / grok-4.7, one pass, 2026-11-14. The assignment forbids
subagents, so Standards, Spec, and security are this single reviewer.
Independence is from the Sol fixes, not from a second reviewer. Label:
single-agent review. Coordinator merge remains separate.

M2 is **incomplete**. Real-Runner PA6 is **OPEN**. Linux Stage F evidence and
Stage G production wiring are **OPEN**. Installed service and dedicated
execution remain **unverified on real hardware**. The optional Windows replay
harness and launchd-profile evidence stay parked. No daemon, native executor,
owner, account, service, network, VM, private-state, install, GitHub, or CI
operation was run.

## Pins

Porcelain was empty on entry and after the checks below, before this review
file. The review cwd is the linked worktree top. Its gitdir is that worktree's
own gitdir and differs from the common gitdir. The branch has no upstream.
Paths below are repository-relative. No owner path, account, or machine path
is recorded. The verdict covers only the two feature commits. The progress
note at the branch tip is outside that diff.

| Item | Identity |
| --- | --- |
| Review branch | `t3/review-m2-discovery-inventory` |
| Branch tip before this review | `4995f255029ada5769b3a765afe9e01a0440ca04` |
| Tip tree before this review | `2d1cf67fb43d4c9630b46bc74d7d30d76a1eba42` |
| Retained inventory fix | `86917c49d94e2224ff7c668d06e990dbf277d479` |
| Sol inventory fix | `472c3420b50b6ce399859891b896367c8f951e72` |
| Inventory patch-id, both commits | `70db5304e672e6bd1a6cec7e750b455f5098b781` |
| Retained cache fix | `db99c80c6d91e55a1153a8ae1a4f8a297acaae76` |
| Sol cache fix | `8b2387f067200ee91c323f3a003b35256dc12d30` |
| Cache patch-id, both commits | `c3a753b1bf729222c610587e591da2f6745e5202` |

Parents differ, so the commit trees differ. These feature blobs match between
each Sol commit and its retained commit, and the retained cache blobs match
the branch tip:

| File | Inventory blob | Cache / tip blob |
| --- | --- | --- |
| `crates/arxd/src/mcp.rs` | `723034d87b6a968ca0b5f89a55774a03156a4c81` | `ec3dc408b9b0b460567cffe581cf34eb412557a0` |
| `crates/arxd/src/mcp/discovery_tests.rs` | `529294dc4728db0aa1385952b463a361ecd1c715` | `ccf3e75733aba9ab07e25ac8032f152693be8f21` |
| `crates/arxd/src/development/mod.rs` | unchanged `91ed14c574cad89aba869e4d9f1a1f75fdf42b92` | `03be8eea75463f52e4cd4f3855921a69ed7a68c6` |
| `crates/arxd/src/development/tests.rs` | unchanged `7937a0dc23e37a0b1e993ffa4bf6e137e6e43480` | `f6962c26355fb817dc24bfd9e684f3b030f1ce92` |
| `docs/evidence/m2-discovery-inventory-roundtrip.md` | `d4fbd12cf994215f0fb4e5d9a13bcf8731392cc1` | same |
| `docs/evidence/m2-development-discovery-cache.md` | absent | `8e8f831fb4e7d2119d8ed92e457050668a28d77c` |

`86917c4` changes `mcp.rs` (+6/−1), new `mcp/discovery_tests.rs` (+164), and
the inventory evidence (+74). `db99c80` changes `development/mod.rs` (+40/−5),
`development/tests.rs` (+62), `mcp.rs` (+6/−1), `discovery_tests.rs` (+255),
and the cache evidence (+76). `git diff --check` on that commit was empty.
The no-cache slice of `mcp.rs`, taken the same way as `e7.rs`, matches
`e7-no-cache-golden.txt` (1548 characters). The Unix E7 test was not run.

## Call path

`list_hosts` still writes the existing execution facts and the no-cache
discovery block, then overlays `tool_discovery: cached`, `tools`,
`tools_observed_at`, and `last_observed_identity` when a released cache
exists. Current `execution` and `execution_available` stay as they were.

`discover_tools` reserves `discovery_observation` before work when either the
dedicated executor or the development executor is configured. `run_command`
does not. `Execution::dispatch` calls `finish_typed` and only then returns
the value. MCP calls `cache_discovery` after that future resolves `Ok`.
`cache_discovery` returns immediately unless `outcome` is `succeeded`, and it
replaces the cache only when the new ticket is greater. The development
command arm still serializes `ResultData` and still uses `outcome()` for the
audit. The discovery arm is the only place that adds `outcome` to the table.

A development probe is eligible when it exits 0 with cleanup confirmed and a
current-user match, or when it returns the existing `Reason::Unavailable`.
Timeout, cancellation, shutdown, output limit, failed cleanup, and a user
mismatch set `found: null` and fail the table. A nonzero exit keeps the
existing found/version fact and fails the table. Any other probe error
returns that error. Cancellation after the last probe returns the existing
null/failed result. The test probe is `cfg(test)` and returns before
`windows::run`.

## Checks

Read all 12 tests named by the evidence run, both evidence files, the
discovery loop, both MCP cache sites, `cache_discovery`, and the dedicated
release outcome. The three MCP tests use an initialized rmcp client over an
in-memory duplex and a temporary activity database. The dedicated test drives
the existing scripted dispatcher. The development tests drive `Execution::work`
through the test probe. The success outcome is produced by that loop and
released by `dispatch`. One equal-ticket call passes an already released
response into `cache_discovery` after the live race; it is an ordering check,
not the producer.

- `cargo test --locked --offline -p arxd --lib development::tests -- --test-threads=1`:
  9 passed, 0 failed, 0 ignored, 371 filtered out, 0.01 s.
- `cargo test --locked --offline -p arxd --lib mcp::discovery_tests -- --test-threads=1`:
  3 passed, 0 failed, 0 ignored, 377 filtered out, 0.54 s.
- `cargo fmt --all -- --check`: passed.
- `cargo clippy --locked --offline -p arxd --lib --tests -- -D warnings`:
  passed, 30.65 s. This lints test targets. It does not execute them.
- No-cache golden comparison: exact match. `git diff --check` on `db99c80`: passed.

Linux and macOS were not run. Revert of the two retained commits is a two-way
door.

## Standards

**PASS. No hard breach. No baseline smell that should change this diff.**

`AGENTS.md` requires a meaningful test per behavior, no secrets, no unrun
check presented as run, and a hardware label. `CONTRIBUTING.md` requires the
same focused test and forbids credentials. Each commit is one conventional
`fix(mcp)` concern. The tests drive the real inventory handler and the real
discovery loop. The evidence files label installed and dedicated paths
**unverified on real hardware** and do not claim the Unix E7 run.

The three-line cache call sits in the dedicated branch and the development
branch. Those branches already return separately. Extracting a helper would
not change the contract. The `cfg(test)` probe is the mock seam for this
loop, and it is absent from a non-test build. The Fowler baseline does not
otherwise match this diff.

## Spec

**PASS. The rendered cache and the development completion gate match the slice.**

`docs/evidence/m2-discovery-inventory-roundtrip.md` says a released
observation is `tool_discovery: cached` in `tools`, `tools_observed_at`, and
`last_observed_identity`, while execution facts keep their existing values.
The no-cache block stays in place and matches the golden. The same file leaves
development caching unresolved because `dispatch` `Ok` is not completion.

`docs/evidence/m2-development-discovery-cache.md` requires completion from the
whole fixed table: exit 0 with cleanup and current-user match, or
`Reason::Unavailable`. Other probe results fail or return their error.
`outcome` is withheld when audit completion fails. `Ok` alone is not
eligibility. Older and equal tickets cannot replace a newer cache. Failed,
partial, cancelled, refused, and storage-failed calls keep the previous
observation. Development mode and claim stay, and cached identity stays null.

The dedicated roundtrip checks the cached rendering, unchanged execution,
refusal after shutdown, and rejection of an older ticket, an equal ticket, a
legacy table without `outcome`, and failed, refused, and partial releases.
The producer roundtrip checks a mixed found/missing release, timeout,
probe error, cancellation, nonzero exit, a real SQLite `UPDATE` failure, and
shutdown. The ordering test holds the older producer until the newer
discovery completes, then checks that the older release and an equal ticket
leave the newer tools in place. The producer unit test checks timeout,
cancellation, shutdown, output limit, cleanup, user mismatch, all-known
absence, and cancellation on the final absence. The eight existing
development tests still pass, including the command response bound and the
ban on observed identity or rights in development metadata.

## Security

**PASS. Cache publication follows the audited eligible discovery and adds no authority.**

The cache stores `tools`, the response's `observed_identity`, the start
ticket, and the start time. A development table has no observed identity, so
the cached identity is null. The tests require that null after release and
after shutdown. No dedicated identity is written by the development path, and
configuring the development executor does not activate the dedicated
executor or fall back to it.

`finish_typed` commits before `dispatch` returns the value. The storage
trigger makes that `UPDATE` fail, the client receives an error, and the prior
ticket remains. Cancellation, a failed outcome, and a non-succeeded body do
not replace the cache. Command responses are not given an observation ticket.
Version text remains the existing 256-character stdout fact, and only an
eligible table is cached. Uncertain probes publish `version: null`.

## Advisories

1. A clean nonzero exit is cache-ineligible and audited failed. Its live
   table still uses `found: true` with the stdout version. The MCP stage
   covers the failed outcome and the unchanged ticket. It does not pin
   `found`. The uncertain cases do pin `found: null`. This does not block.

## Blockers

None.

Standards: 0 findings. Spec: 0 findings. Security: 0 findings.
