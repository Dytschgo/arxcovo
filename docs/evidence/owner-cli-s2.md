# Owner CLI S2 historical implementation evidence

OpenAI / gpt-6.1-sol / high, 2026-11-12. Implementation sub-agent; no
subagents, publication, GitHub operations or owner operations.

This records original source `6c452ec` / evidence `fe8e9a7`, not correction
attempt 1. The [correction evidence](owner-cli-s2-correction1.md) supersedes
current claims. S1 is now merged in #48 with its reviewed follow-up inherited.
Original run results below remain credited only to their original source.

## Isolation and dependency

Authoritative prepared workspace:
`C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-d3e15737-4e83222b75915cfa`.
Initial pwd/Git top-level matched this binding and differed from the forbidden
canonical checkout. Initial HEAD/base:
`813dd1e575e1f1cbfe50237c43d115a6134b39bd`, clean. Every shell command used
that explicit workdir. No checkout or worktree creation. At that historical
boundary S1 was a local dependency on b166306/main after PR47. S1 subsequently merged as #48 with
independent review and follow-up tests; correction attempt 1 inherits those bytes.
S1 sources were read only; shared valid_principal/windows_component_safe are
consumed without modification.

Source implementation commit/head: `6c452ecf30555764e86af966e34e91e80df1c5e5`
(`feat(client): create private client credentials without dispatch`).
The following evidence-only commit records verification without changing source;
its exact ID is supplied in the handback and repository log. Base-to-source
manifest is precisely the five Rust paths below; the final manifest also has
PROGRESS and this evidence.

## Scope and behavior

Manifest: new `crates/arxd/src/client_config.rs` and its `unix.rs`,
`windows.rs`, `tests.rs`; exactly one private module declaration in lib.rs;
this evidence and PROGRESS. No Cargo/dependency, CLI/parser/routing, main,
service test, owner/native/executor/config, packaging or workflow edits.

The initialized non-Clone/non-Debug secret owner precedes RNG, hashes the
borrowed 64-byte ASCII encoding before wiping, and owns raw/token/JSON until
volatile Drop wiping. serialize_owned is the single real serializer. A checked
six-times-escaped capacity bound precedes RNG. Scoped thread-local observers
report allocation pointer/capacity internally and wipe boolean only; assertions
never print pointers, token bytes, SIDs or fixture paths. Best effort excludes
aborts and compiler/allocator/OS copies.

The original source retains ancestor handles: Unix uses no-follow/CLOEXEC
relative descriptors; Windows uses fixed drives and omits delete sharing.
The original Windows metadata-only handles did NOT enforce rename refusal
(SP-1); correction evidence records the data-right fix and actual rename tests.
Only the immediate parent may be created. Existing objects are neither repaired nor overwritten. Creation is
exclusive with private creation modes/owner/DACL. The native adapters take
borrowed handles and no paths. The original injected operation errors ran
before native methods through
a trivial result_with wrapper; they did NOT prove native error propagation
(SP-3). Handle-fact acquisition meets at verify_with; original synthetic facts
do not prove real negative ACL acquisition (SP-4). The shared classifier
immediately returns Partial without hash,
rollback, deletion, rename or reopen. Prefix-write failures retain the prefix.
Windows facts reject every unsupported ACE (including an actual deny entry in
synthetic facts); production acquisition and fixture facts feed the same
checker through Verify. The genuine current-user adapter is never forged.

S2 emits no output and is not dispatched. The brief's actual dispatch assertions
belong to S3; structural handoff: exhaustive ClientError variants are Argument
(exit3), Elevated/Parent (exit4), Exists/Io/Partial (exit1). S3 must emit exactly
`arxd: client-config-partial\n`, empty stdout and no hash on Partial, with no
argument/OS details. S3 must FIRST change the service-install test to invalid
`install extra` before any routing; this slice did not touch that test.
Production create and ClientConfig::load were never called. JSON compatibility
is tested by deserializing the three-key payload without endpoint validation.

## Local checks and failures

Logs/manifests are local ignored `target/s2-*`; no credentials are logged.
All credentials are deterministic synthetic fixtures under owned temp roots.
No SYSTEM/owner/service/executor, fixed-pipe connect, account resolver, privilege
enable, service manager, private state, VM/Lab Manager, ignored fixture or
experiment was invoked. Normal workspace tests retain existing benign SDK
private-pipe/child-parent fixtures; no new such probe was introduced.

- Initial combined source read failed due to PowerShell brace syntax; repeated
  with explicit filenames. An optional .config/nextest.toml read found no file.
- First Clippy compile failed: DRIVE_FIXED is unavailable under existing Windows
  API features. One source correction used the existing repository convention
  (3, DRIVE_FIXED). Subsequent locked all-target workspace Clippy passed.
- Focused01: `cargo nextest run --locked -p arxd -E 'test(client_config)'`,
  run 6e32aa9f-81b3-4f3b-896b-2b68f60d7daf, 9 passed (includes one existing
  integration test matching that substring), 271 filtered out.
- Focused02 after real owned-temp Windows nonprivate-parent/junction fixtures:
  `cargo nextest run --locked -p arxd -E 'test(client_config::)'`,
  abd69048-ca7a-4157-b890-efe5cdb29407, 9 passed /272 filtered out.
- Focused03 after held-handle inheritance/existing-private-parent fixture:
  same command, c8a21667-4cc8-48ef-a51a-7c484225efa7, 10 passed /272 filtered.
- Linux: `cargo check --locked -p arxd --lib --tests --target
  x86_64-unknown-linux-gnu` failed before arxd compile: missing
  x86_64-linux-gnu-gcc for bundled SQLite. No Unix compile/test credit.
  Three real Unix invalid-Verify fixtures (hard link, mode0640, JSON+x), Unix
  privacy/symlink cases and Mode/SyncParent mutations await hosted Unix.

The Windows fault matrix covers Write/prewrite/prefix, Flush, Verify operation
errors and acquisition errors, plus independent foreign owner, wrong type,
reparse, two links, wrong length, absent DACL, actual deny ACE, unsupported ACE,
foreign allowed recipient and non-disk facts with a valid control. Each negative
requires Partial/no hash, exact operation order/no later method, retained
private complete JSON or exact prefix/empty bytes as applicable, unchanged
parent/no extra entry and true wipe observation. Existing-target refusal checks
bytes, length, modification time and privacy; parent refusal checks no mutation.
No production output path exists; the output assertions remain an S3 gate.

## Mutations and final verification

Final focused command `cargo nextest run --locked -p arxd -E
'test(client_config::)'`: b2bc2e1a-1c80-4f34-8ba0-08c2c25f1a9a,
11 passed /272 filtered out. Added exact Windows owner/DACL/fact comparisons
and existing-target preflight refusal checks. `cargo fmt --all -- --check`
and `cargo clippy --locked --workspace --all-targets -- -D warnings` passed
on the final source (logs s2-fmt-final and s2-clippy-final).

22 mutation runs compiled and failed runtime assertions (exit100). One earlier
wipe-raw attempt failed compilation (E0499, duplicate mutable token borrow),
earning NO credit; the corrected mutation removes raw from the wipe iterator
without aliasing and fails the boolean wipe assertion. Its failed build log
is retained as s2-mutation-wipe-raw-compilefailed.log. No production fix was
made in response to a mutation. Every attempt restored captured bytes in a
finally block; every recorded restoration is true. The final ACL-repair
mutation touched only the owned synthetic parent fixture.

Each command: `cargo nextest run --locked -p arxd -E 'test(FILTER)'`.
Local logs s2-mutation-NAME.log and manifest s2-mutations.jsonl retain exact
commands/results. Test assertions use fixed labels; no secret buffer, SID,
address or fixture path is formatted.

| Mutation | Filter | Run ID | Result |
| --- | --- | --- | --- |
| exclusive | existing_target_and_parent | 995a47ae-9897-442f-adbd-f95b47f3592d | assertion failed; restored |
| elevation | lexical_matrix | a6b8e109-5a32-4ae2-a1bb-74c159eaf003 | assertion failed; restored |
| clear-before-hash | endpoint_allocation | dab23ae0-01e5-40e1-b72c-0a5580d92e9c | assertion failed; restored |
| raw-hash | endpoint_allocation | f08d57c2-b263-4bb9-8f5b-1ef58565b021 | assertion failed; restored |
| partial-io | post_create_native_result | 32a8cbff-6391-478b-ba44-51afff4a9a50 | assertion failed; restored |
| failure-success | post_create_native_result | 01591630-b0a4-4925-95c0-6884671434bb | assertion failed; restored |
| ignore-verify | windows_every_fact | 4cfa5659-4721-4709-82ae-73afd511aef3 | assertion failed; restored |
| delete-partial | post_create_native_result | de6ac427-a006-408e-9d56-95e912d862e2 | assertion failed; restored |
| delete-on-close | endpoint_allocation | 5f2da7e4-11b5-45f5-a11d-e40f1753558a | assertion failed; restored |
| deny-ace | windows_every_fact | 53de77ad-7dea-455e-8e82-0e95e8c2c5a6 | assertion failed; restored |
| owner-after-rng | partial_rng | ae4d0b91-5b7c-4df0-b3a0-7aeb96d035e9 | assertion failed; restored |
| rng-forget | partial_rng | 31f00c77-6fbc-4c37-b69c-671002e042f0 | assertion failed; restored |
| wipe-raw | endpoint_allocation | 833684ac-cbf8-4564-a361-5fb12303f0df | assertion failed; restored |
| wipe-token | endpoint_allocation | 2ea8df66-530f-4eea-886b-6878b2a17120 | assertion failed; restored |
| wipe-json | endpoint_allocation | 0a34682e-3dc4-4b50-a303-7e0866e54004 | assertion failed; restored |
| native-result-ignore | post_create_native_result | 8e1c958b-0a59-4057-89c1-d4a22bba8274 | assertion failed; restored |
| acquisition-ignore | post_create_native_result | 7dde33cd-e9fd-4174-8908-bc74a295cd0c | assertion failed; restored |
| checker-bypass | windows_every_fact | c59e5343-d6dd-4871-b697-8b2e84854c8a | assertion failed; restored |
| checker-ignore | windows_every_fact | 6ac6c871-8274-41da-9fac-14d23467eb0b | assertion failed; restored |
| allocation-growth | endpoint_allocation | 79ce3e3e-35fd-4844-b720-7b048dbf54af | assertion failed; restored |
| parent-privacy | windows_nonprivate | a05b5c95-2880-40d9-8dcd-0d7cae111246 | assertion failed; restored |
| reacl-existing | windows_nonprivate | 29e01b1c-afd5-40ae-a6fa-a326543b713f | assertion failed; restored |

Unix fchmod-before-create, existing-parent chmod, mode/length omission and
Unix-specific native-result mutations cannot be credited without Unix build
and execution. Windows existing-parent ACL repair is covered explicitly.
Parser/service mutations belong to S1/S3; token echo/output capture is an S3
handoff because this private S2 library has no output/dispatch routine. No
artificial overflow/serialization fault fixture was introduced.

Exactly one final normal run, at source HEAD `6c452ecf30555764e86af966e34e91e80df1c5e5`:
`cargo nextest run --locked --workspace`, default profile/concurrency, no
instrumentation, custom environment, retry or timeout change. Run ID
`62ad402c-9a88-413c-81d3-9c3e4bba7a9d`: **282 passed, 2 skipped**, 11.451s execution time.
Log `target/s2-full-final.log`. No natural ExecutionFailed occurred; no failure
rate, root-cause fix or baseline waiver is claimed. No full-suite rerun.

Source blobs at this tested head (Git object IDs):

| Path | Blob |
| --- | --- |
| crates/arxd/src/client_config.rs | 0b96530b0fd6737657d62a3749b9a771c1e6f09d |
| crates/arxd/src/client_config/tests.rs | 671f432e9cb4d76288f1e26a81123a9d92322b31 |
| crates/arxd/src/client_config/unix.rs | 9da582031e7fe18c9ee87868295edb63c4135cc9 |
| crates/arxd/src/client_config/windows.rs | accb36ed95af1b7434e9d6262592a3dde9a1dd9a |
| crates/arxd/src/lib.rs | b43adc8d8415577f18c84a52b1f59d3316c5a65e |

## Remaining gates and rollback

For correction attempt 1, fresh independent Opus exact-head review and all
hosted required gates remain mandatory; S1 has merged with its follow-up.
Unix compile/test/mutations require Linux/macOS.
The single final normal full workspace run is complete. A future natural
ExecutionFailed must stay red, with no retry-to-green or weakened assertion/timeout. The existing baseline ExecutionFailed remains unresolved.
Native/installed/dedicated paths are **unverified on real hardware**. M2 is
incomplete; local passing checks are not implementation acceptance.

Rollback is a reviewed revert of the S2 commits; no installed state was created.
Product Partial files are deliberately retained. TempDir cleanup concerns only
owned synthetic fixtures. No product approval flow or new authority was added.
