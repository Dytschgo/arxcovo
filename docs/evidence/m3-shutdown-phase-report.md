# Shutdown phase report: implementation evidence

2026-11-16. Sol high. Implemented on input
`3ed100b4b33aa918549c64ca7972fba43a1b568b`, with source base
main `6e9edd5a51d1df41cdb412af6031f959de2d9d60`.
The prior `643416e` evidence-only ownership handback remains correct historically;
the final Caller/shutdown borrow correction in
`docs/reviews/m3-u6-shutdown-diagnostic-sol-review.md` settles its blocker.

## Implemented contract

`Orphaned(ShutdownReport)` carries only Copy enums and bounded counts/timings.
Counts saturate at 64; elapsed milliseconds saturate at u16::MAX. Main has no
read-owner units, so read_held is logically zero. The first failure retains
Lock/Engine/Owners/StoreRelease attribution. Completed owners are reaped before
the timed owner wait, and reaped errors still mark failure. Successful shutdown
still returns Ok. The shared five-second deadline, abort-and-join, weak-reference
polling and Cluster::stop unwrap remain in place.

Snapshot timing: owners_done_at_start counts the initial nonblocking reap;
owners_waited is JoinSet length after that reap; owners_left and propose_held
are captured at owner timeout **before abort/join** releases owners and permits.
last_owner_join_ms covers progress before abort, excluding cancellation joins.
Store strong counts and total_ms are captured at return after weak polling.

The minimal cfg(test) APPLY_PAUSE runs in the actual apply blocking closure,
before locking its exact state store. The proposal test waits for that gate,
observes one held proposal permit, aborts AND joins its caller, then proves the
permit remains held and its owner is pending. The caller Arc is dropped by the
joined cancellation before removing/unwrapping the node. Shutdown must report
Owners/Joined, owners_waited=1, owners_left=1, propose_held=1, read_held=0 within
5.5 seconds. Only after shutdown returns is apply released; actual weak store
references and permits must then clean up within two seconds. No cancelled
caller result is asserted. No new proposal API or unsafe aliasing is used.

A separate held-state-store Arc produces StoreRelease/Joined with zero owners
waiting/left, zero held permits and a retained state store. Dropping that Arc
must release actual stores within two seconds. The normal leg exercises the
unchanged Ok shutdown. An already completed owner panic is reaped and still
reports Owners failure with owners_panicked=1; the induced panic is expected.
EngineEnd::JoinFailed and EngineEnd::Deadline remain uncovered.

## Actual local validation

One implementation attempt consumed (1 of 2); no failed implementation attempt.
Two deliberate temporary mutants are evidence probes, not attempted fixes:

- Replace every owner-phase failure label with StoreRelease. This compiles,
  runs the cancelled-proposal test, then fails its new phase assertion:
  left StoreRelease, right Owners; exit 101.
- Replace the store-release failure label with Owners. This compiles,
  runs the store-only test, then fails its new phase assertion:
  left Owners, right StoreRelease; exit 101.

The original source was restored in a finally block after the probes. The full
cluster run below rebuilds and validates the restored implementation, including
the strengthened post-cancellation pending-owner assertion. These are runtime
red-before attribution checks, not missing-type compilation failures and not
a demonstration of a preexisting behavior bug.

All commands used a new unique D: target and TEMP/TMP directory. Dependencies
were already available; Cargo commands used --offline; no cache install ran.
Commands and observed results:

| Check | Result |
| --- | --- |
| cargo test -p arxd --offline r6_shutdown_report -- --nocapture | 3 passed; initial green |
| cargo test -p arxd --lib --offline cluster:: -- --nocapture | 112 passed, 2 ignored, 0 failed; 8.67 s |
| Exact built U6 executable, --exact --nocapture, eight sequential processes, each capped at 60 s | 8/8 passed; unchanged U6 body |
| cargo fmt --all -- --check | exit 0 |
| cargo clippy --workspace --all-targets --offline -- -D warnings | exit 0; 26.55 s |
| git diff --check | exit 0 |

Final cluster proposal report: first=Owners, engine=Joined, owners_waited=1,
owners_left=1, propose_held=1, read_held=0, log_strong=1, sm_strong=2,
total_ms=5003, measured shutdown=5.0032432 s. Store-only report:
first=StoreRelease, engine=Joined, owners_waited=0, owners_left=0,
propose_held=0, read_held=0, log_strong=0, sm_strong=1, total_ms=5007.

Source diff against main is restricted to the three allowed files; U6 and
Cluster::stop are unchanged. No workspace-wide test pass is claimed. No
PROGRESS, Cargo, workflow, ADR, PKI or A0 read-path change. No child agent,
push, PR, merge, watch, VM, admin or owner-data SQL operation occurred.
Fresh independent Opus source review is required before publication.

The natural hosted U6 cause remains unknown. Induced state-machine latency is
a compatible sufficient mechanism, not the established hosted cause. This is
diagnostic attribution, not a fix. Hardware paths remain
**unverified on real hardware**.

## Retained actual evidence blobs

These Git objects contain actual captured command output, with only absolute
workspace and isolated-run path prefixes replaced to avoid retaining private
local path data. Read with `git show <blob>`. Empty fmt/diff output shares the
empty-blob identity. Objects are local evidence and do not travel with a commit
unless explicitly retained by the coordinator.

| Log | Git blob |
| --- | --- |
| `r6` | `e4d62520661a542457c8935aa5e4f44eebcaf723` |
| `owners-phase` | `249ddcd871c30d9aa4e06119e9b16e4436ec29df` |
| `store-phase` | `5206d34126b1011c9bdda26e08e16ea802c00d49` |
| `cluster` | `9ea2f1d9247a97475de9ce8a0d5ef6bd83cfe02c` |
| `fmt` | `e69de29bb2d1d6434b8b29ae775ad8c2e48c5391` |
| `clippy` | `371a28f1d08a2cc657826ce7cea74f2e301cd349` |
| `diff` | `e69de29bb2d1d6434b8b29ae775ad8c2e48c5391` |
| `u6-1` | `8e8d7f93c40baa9991ca5a484e270f7dd1b9ca8e` |
| `u6-2` | `8e8d7f93c40baa9991ca5a484e270f7dd1b9ca8e` |
| `u6-3` | `a55fa642bf59782d87c961222b72b731d4b784cf` |
| `u6-4` | `6f90dd86aeed561471ec277e28e1334f3126329b` |
| `u6-5` | `a55fa642bf59782d87c961222b72b731d4b784cf` |
| `u6-6` | `b7db5bd15a865e3181bc55297ae8a7b1e48f39c1` |
| `u6-7` | `6f90dd86aeed561471ec277e28e1334f3126329b` |
| `u6-8` | `615d981afe01034fe0299b189b74886e7bc0f447` |

## Final source blob identities

| Source | Git blob |
| --- | --- |
| consensus.rs | `62332494fa343b462dde0f7917e8ff5e18ccdb15` |
| consensus/adapter.rs | `aca96cd5d5f2a50ee2e941c2994a055a9563c72b` |
| consensus/three_node.rs | `4633123c1be95376150dbcd8b6a4c3dc6d565245` |

## B1 test-strength correction

Opus516356b0 (retained0da693b) confirms source report timing is correct but the store-only >=1 assertion also admits counting the node's own handle. Sole code delta ec67681 makes that assertion exactly1, isolating the external holder. No production code changed. Fresh focused verification and independent recheck are pending; earlier source/cluster checks remain historical evidence, not a claimed final-head rerun. This is the second seam implementation attempt, not a behavior fix.
