# M3 store destructor completion: advisory 1

2026-11-16; OpenAI / gpt-6.1-sol / high; isolated implementation sub-agent, no delegates.
Contract: `docs/briefs/m3-store-close-completion.md`, including root Sol check
`4b6a023`. Prepared base `71607f53caf69f1f6a4582d61fa8e993076b717a`.
Source commits: `474a8c4` (acknowledgements, gates and tests with OLD wait retained),
`fa49b72` (actual wait fix).

## Pins and source contract

Inspected before editing: `origin/main` = `6231da98b4aa902669e268be89de86b273d1bd16`;
`git ls-remote origin refs/heads/main` subsequently confirmed the same remote tip.
The local `main` ref was stale (`176af7a`), so it was not used as the source pin.
`git ls-tree 6231da9 crates/arxd/src/cluster/` and the initial source diff confirm:

- state store `90474881230e40061648efcedd0a51dd66eab54e`;
- log store `02db51fc2409201f7f84c731707329d71398727c`;
- consensus `c93c58f624e9e8a2503753bdb45e14d15c7808e7`;
- adapter `181e9dce6765b1db9339c254ec2dcead9b88a0b9`.

Toolchain is `1.95.0`; rusqlite is pinned `=0.37.0`.
Re-read the [Rust 1.95.0 Arc source](https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/alloc/src/sync.rs):
strong `fetch_sub(1, Release)` at line 2690 precedes `drop_slow` at 2731;
`drop_slow` invokes `drop_in_place` at 2034. Weak expiry can precede destruction.
The [Reference declaration-order rule](https://doc.rust-lang.org/reference/destructors.html#destructors.operation)
makes the last-field acknowledgement follow all resource fields.
Local rusqlite sources (`src/lib.rs:353` and `src/inner_connection.rs:140,402`)
confirm statement-cache flushing and ignored `close()` result, which calls SQLite close.

Rechecked the original lists: state `{db, image, blob, poisoned, _lock, test hook}`;
log `{db, meta, poisoned, _lock, test hook}`. Both use state_store's `open_db`,
which takes the file lock before opening SQLite. Each final `Ok(Self)` now creates
its own acknowledgement; failure paths before construction retain synchronous local drops.
`ConsensusNode::start` is the sole constructor and captures the two tokens before
wrapping the stores in Arcs. Source search finds start callers only in tests/three_node;
there is no production restart caller. Engine construction failure still carries no release claim.

`Released` clones an `Arc<AtomicBool>` and loads with Acquire; final-field
`ReleaseAck::drop` stores with Release. Both Weak handles remain for report counts,
test access and future peer_registry use. The only shutdown behavior edit replaces
the StoreRelease loop condition; the same five-second deadline and every other
ownership, abort and shutdown phase remain intact. No dependency, unsafe,
ManuallyDrop, explicit File unlock, retry, quarantine or production caller was added.
The flag means preceding field destructors finished, including unwinding; it does
not prove successful SQLite/OS close or immediate Windows unlock. Immediate real
reopen remains a separate checked result.

## Focused runtime proof

Windows local process tests only. Dedicated directories:
`D:/arxcovo-close-71607f5-sol/target` and `D:/arxcovo-close-71607f5-sol/tmp`.
All cargo checks used these environment variables:

```powershell
$env:CARGO_TARGET_DIR='D:/arxcovo-close-71607f5-sol/target'
$env:TEMP='D:/arxcovo-close-71607f5-sol/tmp'
$env:TMP=$env:TEMP
```

Actual commands and results:

1. `cargo fmt --all` passed while preparing the seam.
2. `cargo test --locked --offline -p arxd store_close_completion -- --test-threads=1`
   with OLD Weak wait: compiled successfully; 2 store tests passed, 2 shutdown
   tests FAILED at runtime, 0 ignored, 566 filtered, 0.09 s. Both failures:
   `shutdown returned while store destructor held its lock`.
   Log `old-wait.log`. The wrapper printed its tail and exited zero; the cargo
   log itself reports test failure. No compile-failure baseline credit.
   Temporary unused-token warnings in the old-wait production build disappear
   when shutdown uses the tokens.
3. Same command after the one-line wait fix: cargo exit 0; all 4 tests passed,
   0 ignored, 566 filtered, 5.65 s (`fixed-wait.log`). Other integration binaries
   had zero selected tests; no integration-suite pass is claimed.
4. `cargo test --locked --offline -p arxd --lib store_close_completion -- --skip shutdown --test-threads=1`
   with ack field moved immediately BEFORE the gate in BOTH stores: compiled,
   cargo exit 101, both tests failed at runtime on `ack preceded field destruction`.
   Explicit hash-recorded run: `misplaced-ack-explicit.log`, 0.07 s.
5. Same store-only command with gate moved immediately AFTER `_lock` in BOTH
   stores (ack still last): compiled, cargo exit 101; both tests failed at runtime on `gate must hold the real file lock`, 0.03 s (`gate-after-lock.log`).

6. After byte-for-byte restoration, `cargo fmt --all -- --check`: exit 0.
7. `cargo test --locked --offline -p arxd --lib store_close_completion -- --test-threads=1`:
   exit 0, 4 passed, 0 failed/ignored, 566 filtered, 5.67 s (`restored-green.log`).
8. `cargo clippy --locked --offline --workspace --all-targets -- -D warnings`:
   exit 0, 27.89 s (`clippy.log`).
9. `git diff --check`: exit 0.

All red credits above are compiled runtime assertion failures, never compile
failures. The earlier misplaced-ack run also failed both same runtime assertions
(`misplaced-ack.log`, 0.02 s); the explicit rerun additionally records mutant
hashes. No unexpected unmutated test failure was observed. No three-fix handoff
limit was reached. No test quarantine, CI filter or scheduling/deadline setting
was changed; local test-name selection follows the authorized focused scope.

The store tests hold the destructor before `_lock`, verify flag false AND actual
reopen `Io`, then release/join and require flag true AND immediate successful
reopen, with no retry. The single-node tests hold an extra store Arc, wait until
it is the sole strong owner, drop it on a thread, and prove Weak expired while
the destructor gate is held. The 500 ms negative window fails under the OLD wait.
The deadline variant requires `StoreRelease`, `Joined`, both true strong counts
zero and at least 5000 ms. Both tests release/join and immediately start/shut down
on the same directory. All new tests are ordinary all-OS tests outside three_node.

Mutation originals and restored SHA-256 hashes (byte equality checked after each run):

- state: `e1c9c53480f55d0818e1db3cdacbbc3031f7ad6ef3f81f27eaf8c2db4efd5402`;
- log: `265a10bb5ae6e3553f8aca7081d68afcecf4f206a4bac09f3bd80e50fe41cb60`.

Explicit misplaced-ack hashes: state `a1b2b5a6cbf84237240eae5048d363cd605345412020bdfd1758a4f4d0b3d56a`,
log `49fd23a6ee98959da9928a0e80d7507edcc3de51f5b46b68c350569144588d3b`.
Gate-after-lock hashes: state `0ae6c9d15865e9501ac35c663633b2d9f228d5c254b846014d385206e646ae8a`,
log `27ebc5d1042dec69e6996571b65a27907a74f9c986ccb73c7c2b51470d7221dd`.
Logs and original byte copies remain in the dedicated D: directory, not committed.

## Delivery boundary

Full cluster/three_node verification is **queued for root after the parallel TLS
round**. No full suite, D1/quarantined sanity, CI, native M2, VM/admin or hardware
work was run. No push, PR, review status, merge or watch operation was performed.
Fresh independent Opus source review, root-coordinated full cluster checks, all
five required gates and green main remain required before external merge.
The supplied CI pin (run 43854991316, Windows 595 s) is root-provided context,
not a CI run performed or independently verified here.

This is source-correctness advisory 1, not a Windows recovery attempt and not a
claim that any historical flake is fixed. Scenario 17's historical startup retry
is unchanged. No new retry was added. M2 remains open, M3 unchecked, all eight
Windows three-node quarantines unchanged; hardware paths remain
**unverified on real hardware**. Hosted Windows three-node remains unverified.
