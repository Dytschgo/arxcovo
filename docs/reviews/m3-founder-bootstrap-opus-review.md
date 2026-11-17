# M3 founder bootstrap F1: independent Opus source review

Reviewer: Opus 5.5 high (different provider from the Sol builder), 2026-11-16.
Single-agent review on two separate axes; no children were started.
Target: head `2c6c096d1c405ce1d03683c62413d81683d7f334` (source `ec3e5d2`, evidence-only
`2c6c096`) against base `ff4eece89ca0c76870cda6d971acec7e6cebf8ad`. The base has no
`crates/` difference from main `c612152`. Spec: `docs/briefs/m3-next-runtime-boundary.md`,
B1-B3 and the coordinator execution-order amendment, with the root contract recheck at
`455d8ea`. Standards: AGENTS.md, KICKOFF and the review smell baseline.

**Verdict: source PASS on Security and Spec. Two corrections are required before
publication: R1 is a docs encoding fix and R2 adds one test. Neither changes production
behavior.** This review grants no gate waiver. The amendment permits only isolated local
work. Publication and merge still need a fresh green exact main, the five normal gates and
hosted Windows timing.

## Identity checks (run by this reviewer)

- `git diff --stat ff4eece 2c6c096` shows 9 paths, +1147/-11, all on the brief allowlist.
  `ec3e5d2..2c6c096` changes only PROGRESS and the evidence file.
- Feature blobs match the evidence pins: found `b4de3da`, found_tests `4b46f4f`,
  issue `101f017`, registry `ccf1535`, ca `288be77`, pki tests `e531935` and guard `d1ba344`.
- `git diff --quiet c612152 2c6c096` passes for consensus.rs, adapter.rs, three_node.rs,
  mem_net.rs, pki.rs, pki/verify.rs, Cargo.toml (root and arxd), Cargo.lock, `.github/`
  and windows_diag_sample.py. They are byte-identical to main.
- The old `issue.rs` (9129 B) is an exact byte prefix of the new one (9640 B). Only the
  seal, `LeafSubject`, the `IssueTicket` impl and the `found` binding are appended.
  `enroll`, `committed`, `issue_guard` and `start` are unchanged.

## Commands run by this reviewer

Toolchain: rustc/cargo 1.95.0. Target directory is unique to this reviewer, set with
`CARGO_TARGET_DIR=<scratchpad>/target-review-2c6` and `CARGO_PROFILE_DEV_DEBUG=0`.

| Command | Result |
| --- | --- |
| `cargo test -p arxd --locked --offline --lib -- cluster::consensus::issue cluster::pki --test-threads=4` (unmodified, run once) | PASS 30/30 (F1-F10 13, I1-I9 and controls 11, PKI 6), 0 warnings, 6.47 s test time, 61 s including compile; log SHA-256 `206b124f...797bff` |
| M1: remove the `StaleIssuer` leader comparison in `founder_guard` | killed: rc 101, `f6_adapters` FAILED; log `66d4ad60...fcc986` |
| M2: `founder_guard` ignores the `authorize_peer` result (always `Ok`) | **survived**: rc 0, 13/13 pass; log `57bc4a30...189641` |
| M3: remove the second `founder_guard` after signing | killed: rc 101, `f7_clock_guard_order` and `f9_post_deadline` FAILED; log `cfffcc6d...942bed3` |

Mutants ran as `cargo test -p arxd --locked --offline --lib found_tests -- --test-threads=4`.
These are expected mutation results, not natural reds. After each mutant the file was
restored byte-for-byte and verified as blob `b4de3da`. `git status` is clean apart from this
review. There was no natural red, rerun or source fix.

The following builder claims were taken from the evidence log and not repeated, as
instructed: 155 nextest + 155 in-process tests, Clippy, fmt, guard 3, sampler 58,
workflow 4 and the 15 mutants. Static reading confirms that each claimed F1-F10 mutant
targets a real assertion. The broad, three-node, D1 and quarantine loops were not run.

## Security (actual source)

- **Founder binding.** The proposal uses `codec::cluster(state)` from the persisted store,
  not a caller value, with `FoundMembers::new([(FOUNDER, csr.digest())])`. Signing requires
  `resp.log_id.leader_id.node_id == self.id` and exactly `Founded { replay: false }`. It
  also requires guard 1 (same leader plus registry), then `pre_now >= t0`, `sign_leaf` with
  no await in between, guard 2 and `post_now >= pre_now`. `sign_leaf` rechecks ticket key
  == CSR digest. The returned leaf is the single committed `(1, digest)`.
- **Init once, refusals before Raft.** The order is permit, `NotFounder`, owned preflight
  (founded/applied/membership gives `AlreadyInitialized`), persisted `next_node_id != 2`
  (`FounderAllocation`), `BadCsr`, `CaExpiring`, then the only production `initialize`.
  `init_refusal` maps only `NotAllowed` to `AlreadyInitialized` and everything else to
  `Unavailable`. Nothing deletes, truncates or retries anything.
- **Sealed subjects.** `mod sealed` is private to `issue`, and the only impls are
  `IssueTicket` and `FounderTicket`. `FounderTicket` and its fields are private to `found.rs`,
  with no constructor outside `committed_founder` (tests are descendants).
  `ca.rs` names only `LeafSubject`.
- **No CA custody or exposure.** `ephemeral*` and `private_marker` stay `#[cfg(test)]`, so
  production code cannot reach `found`. `FoundError` is `Copy` with static Display, and no CA
  or key bytes enter `Found`, state or errors (F8 checks the sqlite and WAL files at the top
  level of the directory). No zeroize, custody or transport is claimed.
- **Budget and clock.** The deadline is taken first. `timeout_at(..)` with the guard
  `Instant::now() < deadline` discards late DER (F9 and builder mutant). ClockRollback has
  two checks (F7 with exact call and read counts).
- **Owned preflight (B3).** Permit acquire happens before the lock. If the store is
  `closed`, poisoned or `None`, the call returns before spawning and the permit drops. It
  reaps, then spawns into `owners.set` under the lock with no await. The closure order is
  guard, `sm`, permit, then `tx.send`. The caller holds only `rx`. There is no detached task
  and no `read_blocking` increment. F10 a/b/c and the builder's two ownership mutants
  confirm this. After shutdown, `found` returns `Unavailable` through `closed`.
- **Join unchanged.** `enroll` and `issue_guard` are byte-identical. `sign_leaf`'s generic
  bound accepts `&IssueTicket` unchanged. I1-I9 pass.
- **Validity helper.** `sign_leaf` runs `leaf_validity(now)?` first, so a `Sign` overflow is
  still returned first, and then `leaf_fits` returns `CaExpiring`. Both use one formula. The
  duplicate computation is a judgement call, not a defect, and is accepted.

## Spec axis

- **R2 (required before publication, test-only).** Brief item 4e says the guard maps
  `authorize_peer`: "`WrongCluster` gives `Rejected`, and the rest give `Revoked`." No test
  covers this refusal, and mutant M2 survives. F6 pins only `StaleIssuer`. That refusal is
  the guard's registry binding, which is the only re-check that the committed registry still
  holds `(1, key)` at signing time. AGENTS.md requires a meaningful test for every behavior.
  Fix: in F6, call `founder_guard` on a crafted `FounderTicket` with the current leader (from
  metrics). Assert that another key gives `Revoked` and cluster `[2; 16]` gives
  `Rejected(WrongCluster)`. This is a gap in the contract's test list, not a builder
  deviation, and it needs no source change.
- **S1 (accepted).** `issue.rs` declares `pub(in crate::cluster) mod found;`, but the brief
  says `mod found;`. The brief's own pure test in `pki/tests.rs` needs that path.
  `FounderTicket` and the seal stay private. Record it in the evidence; no ADR is needed.
- **S2 (accepted).** F10c waits for `before - 2`, but the brief says "drops by exactly 1".
  The extra decrement is the test Gate's own `Arc` clone, so the intent is the same. F1/F3
  start through the generic `start` with first_free 2 instead of `founder_init`;
  `founder_init` is pinned in the pure test.
- No unrequested behavior: no endpoint, listener, membership change, renewal, retry,
  quarantine, helper or sampler change.

## Standards axis

- **R1 (required before publication, docs).** `docs/evidence/m3-founder-bootstrap.md`
  (three 0x96 bytes and one 0x97) and `docs/PROGRESS.md` (one of each) at `2c6c096` are not
  valid UTF-8. Python `decode('utf-8')` fails, while both were valid at `ff4eece`. These are
  Windows-1252 dashes in the title, "F1-F10", "I1-I9" and the new PROGRESS heading. Re-encode
  them as UTF-8 or use ASCII hyphens.
- **Low advisory (judgement call, no demand).** The seal-guard regex
  `impl\s+(?:\w+::)*Sealed\s+for` misses a generic `impl<T> sealed::Sealed for T`. Only
  issue.rs, found.rs and found_tests.rs could write one. The brief's required mutants are all
  covered, and Rust privacy remains the guarantee.
- **Smells (judgement calls, dropped).** `committed_founder`/`founder_guard` mirror
  `committed`/`issue_guard`. The brief prescribes that parallel trusted adapter, so this
  Duplicated Code is dropped. The one-line `select!` in `drop_admitted` and the dense Python
  comprehensions hurt readability but are not defects.

## Limits and priorities

1. R1 docs UTF-8 and R2 guard-refusal test, as one small correction commit. Whether it
   counts as implementation correction 1/2 is for the coordinator to decide.
2. Root Sol re-check of that correction. Then fresh green exact main (c612152 is still red
   on R5 and the 1475 s budget, as supplied by the coordinator; no remote query was made
   here), the five gates and a measured hosted Windows `verify` against the 660 s rule. F9
   and F10 each add about 5 s of wall time.
3. PR159 stays held; PR162 is not integrated. Counters are unchanged: Sol Windows 3/3, Opus
   recovery 0/2. Feature mutants are not fix attempts. M2 open, M3 unchecked. Single-process
   temp-directory evidence only. Multi-host, transport and custody are
   **unverified on real hardware**.

No push, PR, status, merge, watch, dispatch, rerun, install, native/admin/VM work, cache
deletion or child agent was used. The only repository change is this file.
