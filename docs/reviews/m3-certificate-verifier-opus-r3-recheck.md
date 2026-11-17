# M3 PR B TLS verifier: independent Opus R3 recheck

2026-11-16. Anthropic / Claude Opus 5.5 / high. Independent narrow recheck sub-agent for a Sol-built test-only correction. Single agent, no
children. This file is the only one committed. There are no source, brief, PROGRESS, evidence or ADR edits. No push, PR, status, comment, merge,
watch, hosted dispatch, CI rerun, install, native M2, VM/admin or old Lab Manager action was taken.

**Verdict: PASS for R3 (test-only). R3 is closed.** Each production side's resumption setting is now pinned by its own committed test. Reverting
either side alone fails exactly the matching test and leaves every other test green. The removal assertion fails independently of the
`handshake_kind` assertions, so neither masks the other. No source change is required. B1, R1 and R2 stay closed, as the earlier recheck found
(`0dea866`, retained as `56e6f07`). Production `4c16354` is unchanged. This verdict does not replace the PR gates listed under Limits.

## Pins

- Reviewed head: `c2eceeb076bd2176807e4f806a7e6ba6fbfd911c` on `work/m3-tls-verifier-boundary`. It was checked out in an isolated worktree on
  branch `t3/opus-r3-verifier-recheck` at the same SHA, and `work/m3-tls-verifier-boundary` contains it. The tree was clean before and after
  every run.
- `56e6f07..c2eceeb` touches only `crates/arxd/src/cluster/pki/tls_tests.rs` (+100), `docs/PROGRESS.md` and
  `docs/evidence/m3-certificate-verifier.md`. `2425f10..c2eceeb` adds only the earlier recheck document to that list. `verify.rs` is
  byte-identical to `2425f10`: `git diff --quiet` succeeds, the blob is `c8c198f4ee784c093c339930f83c91214b617f22` and the working-file SHA-256
  is `23078f4b…401c`, the same as the earlier recheck. Outside docs and `tls_tests.rs`, `277c3e9..c2eceeb` is empty, so nothing changed in
  the engine, helpers, Cargo, the workflows or the quarantine.
- The `tls_tests.rs` blob is `1626658eb4277f7c7f1ad4c930bdb08af533c01f`, matching the evidence pin. Its working-file SHA-256 is
  `46e916b3…33ba`.
- Main `e601caa594f591cc585007be8a455c07282cde8b` is an ancestor of the head and is the local `origin/main`. Root reports main CI
  `43862021319` as green (Windows 477 s). This recheck did not query it.
- Read: AGENTS.md; the review skill; the brief's "PR B every-connection verification correction"; evidence section "R3 independent per-side
  resumption regression"; the full review (`b344df7`); and the earlier recheck `docs/reviews/m3-certificate-verifier-opus-recheck.md`. Reviewer
  routing is Sol-built → Opus reviews. The grok-4.7 fallback was not needed.

## Actual runs (this recheck) versus reused evidence

Fresh target `D:/arxcovo-opus-tlsr3-c2eceeb/target`, TEMP/TMP `D:/arxcovo-opus-tlsr3-c2eceeb/tmp`, rustc/cargo 1.95.0, `--locked --offline`.
Every test command was `cargo test -p arxd --locked --offline --lib -- <filter>`. A script applied each edit with an exact-once replacement and
restored both TLS files in `finally`, then checked the restore by SHA-256. Logs are in `D:/arxcovo-opus-tlsr3-c2eceeb/logs` and are not
committed.

- **Unmodified control, run once** (filter `tls_tests tls_engine_tests`): 15 passed, 0 failed, 570 filtered out. Test time was 2.07 s, and
  wall time was 73 s including the fresh build (`control.log` `d479d4de…`). There was no natural engine red, so the stop rule did not apply.
- **Mutants and probes:** the 9 runs in the table below. Each used filter `pki::tls_tests` (the 10 pure tests only, no engine tests).
- **Final restored run:** 10/10 pass in 0.01 s (`final.log` `04d81148…`). `git status` was clean.
- **Hygiene:** `cargo fmt -p arxd --check` passed. `git diff --check 56e6f07 c2eceeb` passed. `cargo clippy -p arxd --all-targets --locked --offline
  -- -D warnings` passed in 29.4 s (`clippy.log` `847dcf5a…`). The quarantine guard `python -m unittest tools/ci/test_windows_cluster_quarantine.py`
  ran 2 tests, OK. That guard already scans `pki/tls_tests.rs` for `ignore` and non-`cfg(test)` attributes, and the new tests add none.
- **Reused, not rerun:** root's 152/152 cluster nextest and 152/152 in-process runs on `277c3e9`; workspace Clippy; guard 2, sampler 58 and
  Node 29; the earlier recheck's engine control and its 11 mutants; and root's R3 logs `D:/Temp/arx-tls-r3-{control,client-mutant,server-mutant,
  restored,clippy}.log`, which agree with the summary in the evidence. No broad, cluster-wide, three_node, D1 or quarantined test was run.
  The added code is three pure tests, so no new full-suite result is claimed.

### Mutants and probes (K = killed, S = survived)

| ID | Mutation | Result |
| --- | --- | --- |
| MC_client_revert | delete client `Resumption::disabled()` (`verify.rs:261`) | **K**, 9 passed and 1 failed. Only `production_client_refuses_resumption_from_ticket_issuing_server` fails, at `:561` with `left: Some(Resumed)`. |
| MS_server_revert | delete server `NoServerSessionStorage` and `send_tls13_tickets = 0` (`:274-275`) | **K**, 9 passed and 1 failed. Only `production_server_refuses_resumption_from_resuming_client` fails, at `:561` with `Some(Resumed)`. |
| MB_both_revert | delete all three lines | **K**, 6 passed and 4 failed: both R3 tests, the revocation test at `:453` (`Some(Resumed)`) and the expiry test at `:493` (`unwrap()` on `None`). |
| MC_no_full_asserts | MC, plus remove the loop's two `Full` asserts | **K**, 1 failed: `expect("protected side accepted a removed peer")` at `:568`. The removal check catches the client revert without the `handshake_kind` asserts. |
| MS_no_full_asserts | MS, plus remove the loop's two `Full` asserts | **K**, 1 failed: same `expect` at `:568`. The removal check catches the server revert without the `handshake_kind` asserts. |
| MS_storage_only | delete only `NoServerSessionStorage` | **S**, 10/10. This mutant is behaviorally equivalent: with `send_tls13_tickets = 0` and TLS 1.3 only, the server sends no tickets. |
| MS_tickets_only | delete only `send_tls13_tickets = 0` | **S**, 10/10. Also behaviorally equivalent: the server keeps no session storage and the default ticketer is `NeverProducesTickets`, so no ticket can be resumed. |
| P_swap_observer_prod | in the client case, make the adversarial server observe removal instead of the protected client | **S**, 10/10. `refusal()` does not record which endpoint refused (advisory A3). |
| P_swap_observer_MC | the same swap plus MC | **K**, at `:561` with `Some(Resumed)`. The per-side `Full` assertion catches the revert without relying on which side observes removal. |

## Security

- **R3 closed.** `resumption_adversary_pair` builds both endpoints with the production `client_config`/`server_config`. It re-enables
  resumption only on the unprotected peer: the client gets `in_memory_sessions(256)`; the server gets `ServerSessionMemoryCache::new(256)` and
  `send_tls13_tickets = 2`. The enum arms are wired correctly: `Client` leaves the client production config untouched, `Server` leaves the server
  untouched, and `Neither` re-enables both. Only the protected side's registry has the removal flip. Each protected test asserts `Full` on both
  ends on the first and the repeated connection, then flips membership and expects `NotMember`. The control asserts `Full` then `Resumed` on both
  ends, which proves the adversarial peer can actually exercise the bypass. Without that, a passing `Full` assertion would prove nothing.
- **Neither assertion masks the other.** MC and MS each fail on the `Full` check. With the `Full` asserts removed, each still fails on the
  removal `expect`. Each revert is caught only by its own side's test, so it is no longer hidden behind the safe opposite side.
- **Error attribution follows the handshake order.** In the client case, the server accepts first and the client fails when verifying the server
  certificate. In the server case, the server fails when verifying the client certificate. In both, the error comes from the protected endpoint's
  `process_new_packets`, in the `complete` order.
- Production is unchanged since the earlier recheck, so no new trust, registry, ordering or error-surface change exists. The earlier advisory
  still holds: the registry callback runs before proof of possession.

## Spec

- Brief: "Disable client session resumption, server session storage and TLS1.3 ticket issuance … reusable configs must perform full handshakes
  and check local registry/time each time." This is met. Each side is now enforced against a non-conforming peer, and a committed test pins
  each side.
- The earlier recheck required "two committed tests, one per side … `Full` on a repeat connection and `NotMember` from the production side
  after removal". Both tests are present. The optional `Resumed` control is present too. The probe shapes match the earlier recheck's temporary
  probes, which were not committed.
- The change adds no ignore, platform `cfg`, filter, retry, timeout, helper, transport or production CA. The `complete`/`pump`/`reused_connections`
  helpers are reused without modification.
- Advisories carried from the full review: A1 (custom SAN/trailing checks unreachable behind webpki) and A2 (`!is_handshaking()` untested). Both
  remain open and non-blocking.
- **A3 (new, advisory):** `refusal()` downcasts the error but does not record which endpoint produced it. The probe that swaps the observer
  survives on production bytes. The protected wiring plus the `Full` assertions still give per-side kills, so this is not blocking. Returning the
  error with its side from `complete`, or asserting through the protected connection, would pin the attribution.
- **A4 (evidence precision, advisory):** root's per-side mutant logs ran only the matching test, with 584 tests filtered out. On their own,
  they do not show that "only the matching focused test" fails. This recheck ran all 10 pure tests per mutant (9 passed, 1 failed each), which
  supports the claim.

## Standards (AGENTS.md / CONTRIBUTING.md plus the smell baseline)

- Hard: "Every behavior needs a meaningful test". This is now met for both per-side resumption settings. The two single-line server mutants
  survive, but they are behaviorally equivalent, so no test can distinguish them. The server test pins the behavior rather than those lines.
  It would also catch a future change that adds a producing ticketer and drops `send_tls13_tickets = 0` (reasoning only; not run).
- Evidence honesty is good. The redundant `Arc` compile error is disclosed and was fixed before any runtime run. The three pure tests are not
  credited with a new broad rerun. No hosted, Windows-timing, stability or hardware claim is made.
- Possible Duplicated Code: `resumption_adversary_pair` repeats the `client_config`/`server_config` construction block a fourth time. It also
  repeats the paired `handshake_kind` assertions in the loop and in the control. A `reused_pair(...)` helper and an `assert_kind(c, s, kind)`
  helper would cover all of them. This is a judgement call only.
- The earlier recheck's possible Primitive Obsession note on `assert_time_error(…, bool)` is unchanged and carried as a judgement call.

## Limits and remaining gates

- There is still no TLS PR. Before a merge it needs a PR with all five required gates green on its exact head, a fresh different-provider
  exact-head review of that PR, and the Windows hosted `verify` cost measured against the 660 s budget. This recheck credits none of those.
- Local timings are not hosted Windows timing. Main `e601caa` CI green confirms main only. It gives this TLS slice no hosted cost credit and no
  Windows stability credit.
- No production transport, listener, CA, renewal or membership work exists, and none is credited. This recheck makes no stability, transport,
  production CA or quarantine re-enable claim.
- M2 open, M3 unchecked, eight Windows quarantines unchanged. All hardware paths are **unverified on real hardware**.

Findings: Security: R3 closed, 0 new blocking. Spec: 0 blocking; advisories A1/A2 carried, plus new A3 (refusal not attributed to a side) and
A4 (evidence precision). Standards: 0 hard; 1 judgement smell (Duplicated Code) plus the carried Primitive Obsession note.
