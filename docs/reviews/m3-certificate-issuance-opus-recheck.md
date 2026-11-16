# M3 PR A certificate issuance: independent Opus recheck of B1–B4

2026-11-16. Anthropic / Claude Opus 5.5 / high. I worked as the review sub-agent with no children. This is a fresh, narrow recheck of blockers B1–B4
from my full review (`beda9cb`, retained as `9d544f9`, file `m3-certificate-issuance-opus-review.md`). Sol's correction is `6e38a3d`.

- **Target:** head `ee2e806d869232235b5533cefc981df2ffe9259f`.
- **Delta reviewed:** `git diff 9d544f9 ee2e806`.
  - Source `6e38a3d` changes only `consensus/issue_tests.rs`, `pki/tests.rs` and `tools/ci/test_windows_cluster_quarantine.py`.
  - `ee2e806` changes only PROGRESS and evidence text.
  - `git diff --exit-code 89bcd2b ee2e806` is clean for `issue.rs`, `csr.rs`, `ca.rs`, `pki.rs`, `enrollment.rs` and `Cargo.lock`. Production code is
    identical to what I reviewed in full.
- **Read:** AGENTS.md, the review skill, my full review, brief E1–E7 (E3 and decision 6 in particular), the current evidence section "B1-B4 coverage
  correction", and every changed hunk. I also read `issue.rs`, `check_grant`/`apply` in `enrollment.rs`, and the `mint`/`write` helpers.
- **Edits:** this file only. I made no source, brief, PROGRESS or evidence edit.

**Verdict: Standards PASS, Spec PASS, Security PASS. B1–B4 are closed.** Each new or changed test passes unmutated, and each one fails on its own
targeted mutant at the intended assertion. With all four blockers closed, **the full PR A issuance source is approved at `ee2e806`**. Approval does
not authorize a merge. The five gates, green hosted CI on exact main and the 660 s Windows rule still apply. The non-blocking findings and retained
limits below stay recorded.

## Checks: actually run here versus reused

I used a new task-owned root, `D:/arxcovo-a-opus-recheck-ee2e806-243548709`, with `src`, `mutsrc`, `target`, `tmp` and `logs` under it.
- The source was `git archive ee2e806`: 749 files, recorded in `pristine.sha256`.
- Cargo ran with `CARGO_NET_OFFLINE=true` and `--locked --offline`. There was no install, fetch, re-resolution or cache cleanup, and no concurrent Cargo
  work.
- Mutants went into a separate `mutsrc` copy, so `src` stayed pristine.

| Check (run by me) | Result |
| --- | --- |
| Pristine build (`cargo test -p arxd --lib --no-run`) | 61 s, cold target. |
| **Unmutated control** on pristine `ee2e806`: `cluster::pki`, `cluster::consensus::issue`, `retained_grant_check_matrix` | **17/17 passed**, 5.50 s. This includes the new `retained_record_is_required…`, the 6-case I6 and `signature_algorithm_parameters_are_refused`. No natural red, so no E3 stop. |
| Instrumented build (`mutsrc`), no mutant selected | 31 s. 18/18 passed: the 17 above plus the retained P-384/SHA-256 counterfactual probe. |
| 23 env-selected mutants (same patch shape as the full review) | 21 killed, 2 expected survivors. See the table. Every red is a deliberate mutant red, separate from E3 natural-failure handling. |
| Python guard: whole `QuarantineTests` class, loaded from the temp copy | 2/2 PASS. |
| B1 bypass probes and 10 guard-function mutants against the guard's own test | See B1. |
| Restore | `sha256sum -c pristine.sha256` OK, 749 files. I deleted Python bytecode my probe created in temp `src/tools/ci/__pycache__`. Worktree `git diff --exit-code ee2e806` is clean before this file. |

Retained artifacts (SHA256):
- `mutant-instrumentation.patch` `44431c89…6066`
- `mutant-results.json` `4978a60b…1d3a`
- `logs/02-pristine-control.log` `653c9285…214c`
- `logs/03-mutant-build.log` `5995baa2…870d`
- `logs/04-guard-recheck.log` `32aa0344…85b1`
- `instrument.py` `3539eac3…3026`
- `run_mutants.py` `abbf5b98…cb71`
- `guard_recheck.py` `304606be…8fc3`

The per-mutant logs are in `logs/mutants/`.

**Reused, not rerun by me (honest scope):**
- The coordinator's round on `6e38a3d`:
  - cluster nextest 136/136 (8.772 s, I4 4.685 s)
  - workspace all-target Clippy (3.72 s)
  - guard 2, sampler 58, Node 29, fmt and diff check
- My full-review source reading and its privacy positive controls (raw and hex PKCS#8 scanner reds). The scanner, I9 and `three_node.rs` are unchanged
  in this delta, so I did not rerun the scanner probes.
- I did not rerun the 136-test cluster set. My focused filter covers every changed test on the final source. The other cluster tests are untouched,
  and production is byte-identical.
- PR A's hosted Windows compile and test cost is still **unmeasured**. The 660 s rule applies, and nothing local stands in for it.

## Mutants on `ee2e806` (each run: 18 tests in-process, about 5.5 s)

| Mutant | Killed by (failing assertion) | Status |
| --- | --- | --- |
| `check_grant_off` (guard skips `check_grant`) | **only** `retained_record_is_required_even_while_member_remains_authorized`, at `issue_tests.rs:652`: left `Ok(())`, right `Err(Rejected(UnknownCode))` | **B2 closed** |
| `post_now_cached` (`post_now` sampled before guard 2's wait) | **only** `i6_actual_post_wait_clock`, at `:393`. Only case 5 can catch it: cases 0–4 are unchanged and survived this mutant on `89bcd2b`. | **B3 closed** |
| `sigparams_off` (signature-algorithm parameter refusal removed) | **only** `signature_algorithm_parameters_are_refused`, at `pki/tests.rs:234` | **B4 closed** |
| `gate_off` | `possession_algorithms…` and the new B4 test | killed |
| `gate_and_spki_off` | the above plus `zz_p384_sha256_probe` | counterfactual still met |
| `spki_off` | none | expected survivor, shadowed by (c) (retained limit) |
| `guard_key_off` | none | expected survivor, unreachable by construction (retained limit) |
| `leak_dn`, `leak_san`, `leak_ca`, `leak_ku`, `leak_eku` | `fresh_leaf_discards…` | killed |
| `serial_fixed` | I1 and `fresh_leaf…` | killed |
| `sign_key_off`, `wrongnode_off`, `leader_eq_off` | I7 | killed |
| `authorize_off` | I5 | killed |
| `permit_off` | `hook_store_identity_and_admission` | killed |
| `guard1_off`, `pre_now_cached`, `post_check_off`, `rollback_off` | I6 | killed |
| `guard2_off` | I5 and I6 (case 5 now also times out waiting for guard 2's read unit) | killed |

The 18 kills from the full review all reproduce on the new head, so the six-case I6 lost no kill. The three former survivors behind B2–B4 are now
killed, each by exactly one new or changed test.

## B1–B4 disposition

- **B1, E3 guard: closed.**
  - `assert_issuance_required` now matches `#\s*!?\s*\[`, so inner attributes are covered. It refuses any `ignore` token, and any `cfg`/`cfg_attr`
    other than whitespace-normalized `cfg(test)`.
  - `assert_issuance_modules` scans both parents (`issue.rs`, `pki.rs`) and requires exactly one exact binding in each.
  - **Bypass probes:**
    - All 13 forms from my `89bcd2b` probe are refused. That list holds all eight reported bypasses plus `cfg(windows)`, spaced `cfg ( windows )`,
      `cfg(all(test, not(windows)))`, `cfg(feature)` and `cfg_attr(target_family = "windows", ignore)`.
    - Also refused: `#[cfg_attr(windows, cfg(any()))]`, `#![cfg_attr(windows, path = …)]`, spaced `# ! [ cfg ( unix ) ]`, and `#` and `[` split across
      lines.
    - `cfg(test)` and `cfg ( test )` still pass, as intended.
    - A renamed `mod` declaration is refused for either parent.
  - **Guard-function mutants:** I ran 10 against `test_issuance_required_on_every_os`. These are killed: inner attributes unscanned; ignore check
    removed; exact `cfg(test)` replaced by the old platform-only regex; either parent unscanned; either binding unpinned; binding count ignored.
  - **Two survivors, non-blocking:**
    1. A guard narrowed to match `cfg` but not `cfg_attr` survives. Every `cfg_attr` case in the mutation list also contains `ignore`, so the ignore
       check hides the regression. The shipped guard does refuse `cfg_attr(windows, cfg(any()))`, but no test pins it.
    2. Dropping `#[cfg(test)]` from the pinned binding strings survives. That loosening only compiles the tests in more builds and cannot skip
       coverage.
- **B2, guard `check_grant`: closed.**
  - The test takes a real ticket from `committed(propose_response(redeem))` and requires guard `Ok`. Then a committed `Remove` at
    `NOW + CODE_TTL_MS + TOMBSTONE_MS` prunes the record. `retain` runs before the outcome match, so even this rejected Remove prunes it.
  - It then asserts `local_authorize == Ok`, followed by guard `Rejected(UnknownCode)`.
  - Next it re-mints the same code ID, asserts `local_authorize == Ok`, and expects guard `Rejected(Consumed)`, because the new record has
    `consumed: None`.
  - A single `check_grant_off` mutant fails it at the first post-prune assertion.
- **B3, guard-2 post-wait clock: closed.**
  - I6 case 5 pauses at AfterSign. It arms APPLY_PAUSE behind an in-flight write, then releases.
  - It advances the clock to `base + CODE_TTL_MS` only after `read_permits == READ_PERMITS − 1`, meaning guard 2's read is admitted and blocked behind
    apply. Then it releases apply.
  - It expects `GrantExpired` with `signs == 1`.
  - `post_now_cached` flips this to success and fails the test.
- **B4, signature-algorithm NULL parameters: closed.** The shipped test is my retained probe minus the `eprintln!`: a valid P-256 CSR whose outer
  `signatureAlgorithm` is re-encoded with `05 00` and has a corrected outer length. It requires `UnsupportedKey`. Only `sigparams_off` (and the
  broader `gate_off`) fails it, so the test reaches the parameter check and not a parse error.

## Standards

Sources: AGENTS.md (every behavior has a meaningful test; no secrets; one-concern PRs) and the smell baseline. I did not rerun fmt or Clippy, which
tooling enforces.

1. **AGENTS meaningful-test rule: now met** for the three live checks that breached it (`check_grant` in the guard, guard-2 `post_now`, signature
   parameters). Each check has an isolated test that its mutant fails.
2. Judgement call: the guard's mutation list has no `cfg_attr` case without `ignore` (B1 survivor 1). Adding `'#[cfg_attr(windows, cfg(any()))]'` at
   the next touch would pin it. Non-blocking.
3. Possible **Mysterious Name** in `pki/tests.rs:204-235`. `hl`, `n`, `pos` and the raw DER byte arrays make the hand-rolled re-encoding hard to read. A
   one-line comment naming the outer `signatureAlgorithm` and the `05 00` NULL would help. Test-only.
4. Possible **Duplicated Code**. The new B2 test repeats the `local_authorize(...) == Ok(())` assertion block twice, and the earlier I4–I6 scaffolding
   note from the full review still stands. Test-only.
5. Carried over from the full review and unchanged here: the I2 long lines, the I-test message chains, `MAX_CSR` bounding leaf DER, and the
   `committed` catch-all reason. All are judgement calls with no new instance.

No secret, private owner data or new dependency appears in the delta. PROGRESS and evidence describe the delta accurately and make no kill claim.

## Spec (brief E3, decision 3(c), decision 6, the B-list correction)

- E3 is now met as written ("no `ignore` or platform-dependent `cfg`/`cfg_attr` (including windows/unix shorthand, target_os and target_family)"),
  and stricter: only `cfg(test)` is allowed. The guard covers the four files and both module bindings, and "a mutation that inserts an `ignore` must
  fail the guard" holds in both test files and both parents. The guard enters no product path, so it adds no behavior beyond what was asked.
- Decision 3(c) "(no parameters)" and decision 6 step 6 ("No clock value sampled before a wait is used after it") are now each pinned by a test.
- No new quarantine, helper, deadline, timeout, retry, permit or selection change. `three_node.rs`, the `Cluster` helpers, the sampler inventory and
  the Windows quarantine of 8 are unchanged. I6 keeps its single `APPLY_PAUSE_OWNER` hold, so the extra case runs under the existing lock. The B2
  test arms no hook and needs none.

## Retained limits (non-blocking; recorded, not claimed)

- **The E3 guard is text-level, not a Rust parser.** I confirmed these forms still pass it:
  - a comment inside the attribute ahead of `cfg` (`#[/**/cfg(unix)]`, `#![/* x */ cfg(not(windows))]`, and a line comment before `cfg`)
  - a pinned binding wrapped in a block comment or an uninvoked `macro_rules!`
  - an in-body `if cfg!(windows) { return; }`

  All of these take deliberate obfuscation, which review would see. The realistic drift E3 guards against is plain `ignore` or platform `cfg`, and
  that is caught. Stripping comments before scanning, and refusing `cfg!` in the two test files, would harden it later.
- From the full review, unchanged:
  - (e) and the guard key equality are covered only by counterfactual (`spki_off`/`guard_key_off` survive; `gate_and_spki_off` is killed by the
    P-384 probe).
  - Leader binding is proven only by a synthetic ticket.
  - The scanner detects whole-DER leaks only, in raw and hex form.
  - The error-privacy tests are tautological.
  - The I3 "exactly one member" check is indirect.
  - Leaf `not_before` is not checked against the CA's `not_before`.
  - `parse_node_uri`'s `> 77` cut has no well-shaped overlong case.
- B2's `Consumed` leg is asserted, but `check_grant_off` fails the test at the earlier `UnknownCode` leg, so no single mutant isolates the `Consumed`
  assertion. `retained_grant_check_matrix` pins the pure `check_grant` variants.
- The runtime PKI is not future-proof: there is no production CA loading, TLS, transport or rotation. This slice does not claim them.

## Process disclosure

- My first guard-mutant table had a Python string-escape bug in **my harness script**, which raised an `AssertionError` in my own `count == 1`
  precondition before any guard test ran. I fixed the script and ran the table once. This was not a red in any code under test and not an E3 event.
- No `git fetch` and no `git status`. No push, PR, PR status, merge, watch, hosted dispatch, native M2, VM, admin step, install, cache cleanup or
  delegation.

## Summary

Standards: 5 findings, all non-blocking. The worst is the unpinned `cfg_attr`-without-`ignore` guard case. Spec: 0 blockers. B1–B4 are closed and
each is verified with an isolated mutant kill, and the worst residual is the text-level E3 guard limit. **Full PR A issuance source approved at
`ee2e806`.** Merge still waits on the five gates and green hosted CI on exact main, with Windows `verify` time measured against the 660 s rule. Hosted
crypto CI cost is unmeasured.

M2 stays open. M3 stays unchecked. The Windows quarantine of 8 is unchanged. There is no TLS, transport or production CA. Windows three-node stability
is unclaimed, and every hardware path is **unverified on real hardware**.
