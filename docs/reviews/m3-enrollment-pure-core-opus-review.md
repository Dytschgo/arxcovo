# M3 pure enrollment core: independent Opus source/security review

Reviewer: Anthropic / claude-opus-5-5 / high, review sub-agent, single agent,
no subagents, 2026-11-15. Local only: no push, PR, comment, status, merge,
watch, CI, host, native, VM, Lab Manager, network endpoint, signing, storage,
Raft or owner/admin action. This file is the only edit.

Verdict: **PASS for the named I/O-free pure core and its tests. Publication
eligible; no blocker.** One test gap (A1) and four low advisories. Hosted
required checks and the normal merge gates still apply. Nothing here accepts
anything beyond the pure core.

## Target, identity and independence

- Sol implementation `730a18aaf72627685955e495b4fbeb19826792ee` (parent
  `9867540`), kept as `493106a86b5179209e6aabe7cffc8205475153cb` on parent
  merge `de19a92`. `git patch-id --stable` matches for both commits
  (`2505e72c...`). The two trees differ only in `docs/PROGRESS.md`, which
  comes from the merge.
- The merge `de19a92` changed only `docs/PROGRESS.md` relative to `8e80207`
  (main, PR141). Coordinator `f57b5a7` is one PROGRESS line. So relative to
  `8e80207`, the source delta is exactly the five Rust paths, the evidence
  file, the brief and the Sol review.
- Paths: `crates/arxd/src/lib.rs` (+`pub mod cluster;` only),
  `crates/arxd/src/cluster.rs` (`pub mod enrollment;` only),
  `cluster/enrollment.rs`, `enrollment/code.rs`, `enrollment/tests.rs`, plus
  `docs/evidence/m3-enrollment-pure-core.md`. Nothing changed in `Cargo.toml`,
  `Cargo.lock`, `crates/arxd/Cargo.toml`, `.github` or deny/gate config. The
  code uses only the existing `sha2 =0.10.9`, `subtle =2.6.1` and `hex =0.4.3`
  pins. `getrandom` is referenced only in a doc comment.
- There are no production callers. No module outside `src/cluster` names
  `cluster::`.
- **Cross-provider (independent):** this review covers the Sol
  implementation, tests and evidence.
- **Not independent:** the brief (`1fbad65` -> `c6fe014`) is Opus work.
  Sol's delta review closed T1/T2 on it. I review conformance to the brief,
  not its design.

## Read

AGENTS.md, KICKOFF (review rules), the corrected brief,
`docs/reviews/m3-enrollment-first-slice-sol-review.md`, the evidence file,
all three new source files in full, and the `ConstantTimeEq for [T]` impl in
the `subtle` 2.6.1 registry source.

## Standards axis: PASS

- Uses the exact files from the brief. The layout is small and flat: one
  state type, one code module, one test module. It adds no framework,
  feature flag or optional churn.
- The source contains no I/O, `std::time`, `std::fs`, `std::net`, tokio,
  `unsafe`, logging, serde or `getrandom` call (checked with grep). The only
  randomness comes through the injected `fill`.
- The commit is a single conventional commit. The evidence states the 33-byte
  marker fixture fix honestly, and that fix changed no production logic.
- Nit A4 (low): `KeyDigest::of_spki` reports size errors as
  `CodeError::Malformed`, whose `Display` reads "malformed join code". The
  brief does not specify this error type. It is harmless, but the message is
  misleading if surfaced later. Rename it when the issuance slice adds real
  SPKI parsing; do not rename it now.

## Spec axis: PASS (brief conformance)

| Contract item | Implementation | Test that pins it |
| --- | --- | --- |
| Constants, `NodeId = NonZeroU64`, ID widths | exact match | `skew_boundaries...` (MAX_TIME), cap tests |
| Common order: WrongCluster → TimeOutOfRange → ClockBehind (`proposed+SKEW < committed`) | `apply` L220–231; WrongCluster before the time checks | race test (wrong cluster at `u64::MAX` is WrongCluster, byte-identical) |
| All sums checked before mutation; the unreachable `None` maps to TimeOutOfRange | `expires`, `usable_time` and every `expires+TOMBSTONE` checked before L247 | `MAX_TIME_MS` mint+redeem round trip; `+1`/`u64::MAX` byte-identical |
| `eff = max`, ratchet, prune `expires+TOMBSTONE <= eff` is the only removal | L232, L247–252 | `retained_cap...` (`prune-1` full, `prune` frees exactly the oldest) |
| Mint order: replay/RequestConflict → CodeIdCollision → TooManyPending → StateFull → insert | L261–303 | `mint_order...`, `retained_cap...`, `pending_precedes...` |
| Replay returns the stored expiry, never refreshes it | returns `record.expires_ms` | replay at t=1 and t=66 keeps the original expiry |
| Usable = unconsumed ∧ `eff+2·SKEW < expires` | L281, L328 | exact `-1`/`0` boundary for redeem; freed pending slot at exactly `TTL-2·SKEW` |
| Redeem order: Unknown → BadProof (ct) → consumed{Consumed/KeyConflict/replay} → Expired → NodeIdsExhausted → consume | L312–343 | mostly pinned; see A1 for BadProof-before-Expired |
| `u64::MAX` never issued; exhaustion leaves the code and counter unchanged | `checked_add(1).and_then(NodeId::new)` before mutation | `node_exhaustion...` (`only_time_changed`, code still unconsumed, replay still answers) |
| Rejects after step 4 change only `committed_ms`/pruning | early returns precede every record or counter write | `only_time_changed` on Consumed, KeyConflict, BadProof, TooManyPending, StateFull, NodeIdsExhausted; pruning on UnknownCode |
| Grant: private fields, getters only, `applied_ms = eff`, original `code_expires_ms` | fields private to `enrollment` (stricter than "in-crate") | getters checked; replay keeps expiry |
| `check_issuable` (1) SignerClockBehind, (2) GrantExpired on None or `>=`, (3) Ok | L140–154 | `delayed_sole...`: 3 600 000, `-1`/`0`, `u64::MAX`, `applied-SKEW-1`/`applied-SKEW` |
| `joiner_reply` and static Display strings | L168–176 | every `Reject` variant plus `Minted` |
| Parse: cap before trim, ASCII-only trim, exact prefix, 200 hex, checksum, one `Malformed` | `code.rs` L113–141 | padded-valid text beyond the cap, NBSP, `é`, `g`, short, wrong prefix, flipped check |

Mixed-case hex is accepted. Uppercase input parses to the same code, which is
allowed by "ASCII hex" and tested. The checksum compare is not constant time.
That is acceptable: it is a local paste check on the joiner side, not an
oracle.

## Security axis: PASS (pure-core scope)

- **Proof check.** `record.verifier.bytes().ct_eq(actual.bytes())`: the
  `[u8; 32]` arrays coerce to `subtle`'s slice impl, which short-circuits only
  on length (fixed at 32) and is otherwise constant time. The stored value is
  the verifier, a hash of the proof, so a state snapshot cannot redeem a code.
  The BadProof check runs before any consumed, replay or expiry handling, so
  without the proof a caller learns only UnknownCode or BadProof. `code_id` is
  not a credential, and `joiner_reply` collapses both outcomes anyway. Timing
  cannot be unit-tested, so this is review evidence only.
- **Binding.** The proof is `H(tag‖cluster‖code_id‖secret)`. I recomputed the
  fixed vectors (proof `80280cf0…`, verifier `c4819857…`, check `569a558e`,
  206 chars, SPKI `abc` digest) independently with Python `hashlib` (`-I`),
  and they match. Cluster and code-ID sensitivity are asserted. The command
  `cluster` is checked against the state cluster before anything else.
- **Redaction.** `JoinSecret`, `RedeemProof` and `JoinCode` print
  `<redacted>` in Debug and have no Display or Serialize. State, records,
  `Consumption`, `Command` (nested proof), `Outcome` and grant store or print
  only the verifier, the key digest, IDs and times. The marker test checks
  raw, hex and `{:?}`-array forms of the secret and proof, and the exposed
  text, across state, mint, redeem, retry, grant, code, proof, secret and
  command Debug. It would catch a stored proof printed as a decimal array.
  `RedeemProof` has no byte accessor outside `cfg(test)`. Zeroization is not
  claimed, which matches the brief.
- **One-use, races and dedup.** The first applier consumes the code. A
  same-request retry with the same key replays without allocating, the same
  request with a new key gets KeyConflict, and any new request gets Consumed.
  Mint dedup on `(actor, request)` comes before the caps. BTreeMap iteration
  keeps replicas deterministic.
- **Finite state.** Pending is capped at 8 usable codes and total records at
  64. No record is dropped early. Node IDs only increase and survive pruning
  (`next_node_id` 74 after 64 grants; node 12 after the prune).
- **Time.** Out-of-range or stale proposals are byte-identical no-ops. A
  forward jump expires codes and stalls correct clocks with ClockBehind, as
  documented. An expired consumed code still replays its grant, but the guard
  refuses it (no refresh).
- **Authority claims.** The module and grant doc comments say plainly that
  `apply` is not proof of consensus commit and that physical expiry is not
  modeled. The delayed sole proposal test grants on proposal time and then
  refuses at 3 600 000 through the injected guard. Neither the evidence nor
  the code claims consensus, physical expiry or network authentication.

## Findings

- **A1. Test gap; low severity; non-blocking; fold into the next enrollment
  change.** The test name
  `first_applier_wins_and_proof_precedes_consumed_or_expired_status`
  overclaims: no shipped test sends a wrong proof to an *expired unconsumed*
  code. Counterfactual: I moved the Expired check above the proof check in an
  exported copy, and all 13 shipped tests still passed. Only my scratch test
  `cf_wrong_proof_on_expired_unconsumed_code` failed. The current code is
  correct, and `joiner_reply` hides the difference, so this is not a security
  defect. But brief step order (2)-before-(4) is unpinned. Fix: one assertion,
  a wrong proof at `CODE_TTL_MS` → `BadProof`, or rename the test.
- **A2. Future-adapter obligation; low.** Apply a byte-identical `Mint` (same
  `code_id` and verifier) after the original record is pruned, and it
  re-inserts a fresh, usable code. I observed this directly: mint at 0,
  consume, then mint again at `TTL+TOMBSTONE` gives `Minted{replay:false}`,
  and a new joiner using the old text gets node 11. This follows the contract,
  because CodeIdCollision covers only retained records. With 128-bit random
  IDs, an accidental collision is negligible. The obligation for the gated
  adapter: build `Mint` only from a fresh `JoinCode::generate` on the
  proposing host. Never re-propose stored Mint commands, and never accept a
  client-supplied verifier. Record it with the brief's other future-adapter
  rules. Do not change the pure core now.
- **A3. Spec gap for the gated issuance slice; low.** One `KeyDigest` can
  enroll through two codes and receive two node IDs (observed:
  `Enrolled{10}`, `Enrolled{11}`). The brief neither forbids nor requires key
  uniqueness. Decide it when certificate subject/node binding is designed.
  Not a defect here.
- **A4. Nit:** the SPKI error message (Standards section above).
- **A5. Advisory:** `JoinSecret`, `RedeemProof`, `JoinCode` and `Command`
  derive ordinary `PartialEq`, which is not constant time. No production code
  uses it, and the authenticating comparison uses `ct_eq`. No change needed.
  Future adapters must not compare credentials with `==`.

My extra counterfactual
`cf_mint_wrong_cluster_and_clock_behind_byte_identical` passed. Mint shares
the common prefix, so the untested Mint-side WrongCluster and ClockBehind
paths behave correctly. Mutations that skip the proof check or disable
SignerClockBehind are each caught by a shipped test.

## Checks: actual versus reused

I ran these myself on Windows, with `CARGO_TARGET_DIR`, `TEMP` and `TMP` set to
the existing D: target that the builder already inspected. Nothing was
installed or deleted.

- `cargo fmt --all --check`: passed.
- `cargo nextest run -p arxd --locked --offline cluster::enrollment` on the
  unchanged worktree at `f57b5a7`: 13 passed, 454 skipped (run
  `e4bfca10-0e01-4fb2-9f84-a0799d1f4d7a`).
- `cargo clippy -p arxd --all-targets --locked --offline -- -D warnings`
  (the brief's gate form, offline): passed.
- `git patch-id`, the tree diff, a diff of the dependency, workflow and gate
  paths (empty), and a grep for forbidden APIs.
- Python `hashlib` recomputation of every fixed vector.
- In-memory counterfactuals in a `git archive f57b5a7` copy at
  `D:/Code/arxcovo-opus-review-scratch/m3-enrollment` (left in place, not
  committed): four characterization tests and three one-line mutations (M1
  Expired-before-proof: survived the shipped tests; M2 proof skip: killed;
  M3 SignerClockBehind disabled: killed). The worktree source was never
  modified.

Reused, not rerun: the builder's workspace-wide Clippy and `git diff --check`.
Not run: hosted checks, the full test suite, and cargo-mutants or any
mutation tool (I make no survival or kill claim beyond the three listed).

## Publication eligibility and limits

The pure core is eligible for PR publication once the coordinator reconciles
PROGRESS. Merge still requires green hosted required checks. A1 should land
with the next enrollment change, or in this PR if it is revised anyway. These
remain open, unclaimed and **unverified on real hardware**: consensus commit
authority, the pre/post-sign issuance guard in a real adapter, physical
expiry, CA placement and signing, storage and crash behavior, the proof wire
format, SPKI/CSR proof of possession, mTLS, LAN/Tailscale, three-node
availability and zeroization. M2 remains open. User steering permits
independent M3 work. A7 FINAL NO-GO stays closed. A12 cleanup is still
unconfirmed, and nothing was retried.
