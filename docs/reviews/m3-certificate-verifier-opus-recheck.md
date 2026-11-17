# M3 PR B TLS verifier: independent Opus recheck of the Sol corrections

2026-11-16. Anthropic / Claude Opus 5.5 / high. Independent recheck sub-agent for a Sol-built correction. Single agent, no children. This is the
only file committed. No source, brief, PROGRESS, evidence or ADR edit. No push, PR, status, comment, merge, watch, hosted dispatch, rerun, install,
native M2, VM/admin or old Lab Manager action.

**Verdict: CHANGES REQUIRED (test-only).** B1, R1 and R2 from review `4186d10` (retained `b344df7`) are closed. The production correction is
correct and complete on both sides. One new required test gap remains: R3. Each side's resumption setting works on its own against a
resumption-capable peer, but no committed test pins either one, so reverting a single side leaves every committed test green. No source change is
required.

## Pins

- Reviewed head `2425f104a39e7d706b53b597510c9bcabe84b6eb` (`work/m3-tls-verifier-boundary`), checked out in an isolated worktree on branch
  `t3/review-tls-verifier-recheck`. Clean tree before and after.
- Commits since the full review: `4c16354` (source correction plus test/brief changes), `277c3e9` (merge of main `e601caa594f591cc585007be8a455c07282cde8b`,
  PR158 store-close), and `2425f10` (test refinement plus evidence/PROGRESS docs). `e601caa` is an ancestor of the head. The merge base is `6231da9`.
- Merge integrity: `git diff 4c16354 277c3e9 -- crates tools Cargo.lock` touches only the six store-close files (`consensus.rs`,
  `consensus/tests.rs`, `log_store.rs` and its tests, `state_store.rs` and its tests). `git diff e601caa 277c3e9` touches only the TLS-slice files
  and docs. So the source merge is a clean union with no TLS-scope expansion. `277c3e9..2425f10` changes only `tls_tests.rs` (+17/-9), PROGRESS and
  evidence.
- Changed TLS blobs match the evidence pins: `verify.rs` `c8c198f4ee784c093c339930f83c91214b617f22` and `tls_tests.rs`
  `dd5ef2b3ade262ddc8fa35eca660c73122b35d49`. Working-file SHA-256: `verify.rs 23078f4b…401c`, `tls_tests.rs 9f23128e…639a`, and
  `tls_engine_tests.rs d3e4f878…f05b`, which is unchanged since the full review.
- Read: AGENTS.md; KICKOFF routing (Sol-built → Opus reviews; grok-4.7 high fallback not needed); the review skill; the brief's "PR B
  every-connection verification correction"; evidence addenda; the full review; and rustls 0.23.46 builder defaults in the local registry
  (`server/builder.rs:113-123`: memory cache 256, `NeverProducesTickets` ticketer, `max_early_data_size: 0`, `send_tls13_tickets: 2`;
  `client/builder.rs:172,180`: default resumption, early data off).
- The full source review of unchanged code (ordering, registry weak reference, DER/SAN strictness, target binding, mem_net, quarantine guard,
  Cargo.lock +2) is reused, not repeated. Only `verify.rs:259-276` and the `tls_tests.rs` changes are new production and test bytes.

## Actual runs (this recheck) versus reused evidence

Fresh target `D:/arxcovo-opus-tlsrecheck-2425f10/target`, TEMP/TMP `D:/arxcovo-opus-tlsrecheck-2425f10/tmp`, rustc/cargo 1.95.0, `--locked --offline`.
Every command was `cargo test -p arxd --locked --offline --lib -- <filter>`. Logs are in `D:/arxcovo-opus-tlsrecheck-2425f10/logs` and are not committed.

- **Unmodified control, run once** (filter `tls_tests tls_engine_tests`): 12 passed / 0 failed / 570 filtered. Test time 2.08 s; 66 s wall
  including the fresh build (`control.log` `3c1c0d986055c…`). There was no natural red, so no stop rule applied.
- **11 mutants or probes.** Each was applied by a scripted exact-once replacement or append. Each restore was hash-verified for `verify.rs` and
  `tls_tests.rs`, and `git status` was clean afterwards. The final `sha256sum -c` passed on all three TLS files.
- **Reused, not rerun:** root's results on `277c3e9`: 152/152 cluster nextest (9.591 s), 152/152 in-process tests with 13 ignored (9.17 s),
  workspace Clippy (7.14 s), and guard 2, sampler 58, Node 29 and fmt. Also reused: root's final self-signed refinement focused test and arxd
  Clippy (3.57 s). Root's red logs (`D:/Temp/arx-tls-review-red.log`: `Some(Resumed)` at `tls_tests.rs:452`; `arx-tls-expiry-red.log`:
  `Option::unwrap()` on `None` at `:492`), the fixed log, the server-chain mutant log and the final logs are present. The earlier 30-day expiry test
  passed only because its ticket had expired; root replaced it honestly. This recheck independently reproduces both reds on the final test
  bytes (M_old_prod below). No broad, cluster-wide, three_node, D1 or quarantined test was run. No historical-stability or re-enable credit is given.
- Root last read main CI `43862021319` as pending. This recheck did not query it.

### Mutants and probes (K = killed, S = survived)

| ID | Mutation | Result |
| --- | --- | --- |
| M_old_prod | remove both `4c16354` production blocks (client `Resumption::disabled`, server `NoServerSessionStorage` and `send_tls13_tickets = 0`) | **K** 0/2: revocation test `left: Some(Resumed)` at `:453`; expiry test `Option::unwrap()` on `None` at `:493` (the resumed connection succeeded after expiry) |
| M_old_prod_no_kind_asserts | M_old_prod, plus removing the `handshake_kind` and call-count asserts | **K**: `unwrap()` on `None` at `:457`. The removal assertion fails on its own under old behavior. |
| M_client_only_revert (+ probes) | remove only client `Resumption::disabled` | **S** on all 7 committed tests; killed only by temp probe `client_side_alone_vs_ticketing_server` (`client kind: Some(Resumed)`) |
| M_server_only_revert (+ probes) | remove only server storage/tickets lines | **S** on all 7 committed tests; killed only by temp probe `server_side_alone_vs_resuming_client` (`Some(Resumed)`) |
| P_production_probes | temp probes on production bytes | 3/3 pass: each production side forces Full and refuses `NotMember` against a resumption-capable peer; the both-hostile control does resume |
| M_drop_server_chain | drop server `inner.verify_client_cert` | **K**: direct test, foreign/self-signed assert at `:362` |
| M_server_fixed_time | server chain check at fixed in-date time 1001 | **K**: direct test, `unwrap_err()` on `Ok` at `:370` (939 accepted) |
| M_server_flatten_to_Expired | server maps every webpki error to `Expired` | **K** at `:362` |
| M_server_drop_time_context | server maps `*Context` to plain `NotValidYet`/`Expired` | **K**: `unexpected … InvalidCertificate(NotValidYet)` |
| M_client_flatten_to_Expired | client maps every webpki error to `Expired` | **K**: `bounds_root_key_time…` (foreign assert at `:176`) and `reused_configs_recheck_expiry` |
| M_client_drop_time_context | client maps `*Context` to plain variants | **K**: both time tests, `unexpected … Expired` / `NotValidYet` |

## Security

- **B1 closed.** `client_config` sets `Resumption::disabled()` (`verify.rs:260-261`). `server_config` sets `NoServerSessionStorage` and zero
  TLS 1.3 tickets (`:274-275`). The default ticketer is already `NeverProducesTickets` and early data is off by default, so no stateless-ticket or
  0-RTT path is left. There are no other `ClientConfig`/`ServerConfig` builders in `crates/` apart from one test-only `dangerous()` client in
  `tls_tests.rs:207`. The new reused-config test runs two handshakes, asserting `Full` on both sides and registry calls rising 2 → 4 (one
  verifier per side per handshake), then gets `NotMember` after the flip. M_old_prod and M_old_prod_no_kind_asserts show that the kind, removal and
  expiry assertions each fail under the old config behavior. The expiry test warms up 30 min before `not_after` and advances to 1 h after it. Ticket
  age (1.5 h) stays well inside the stateful ticket lifetime, so the test really exercises resumption past expiry rather than an expired ticket.
- **Both per-connection verifier sides are enforced in production** (P_production_probes). A production server facing a client with resumption
  re-enabled still does full handshakes and refuses `NotMember`. The same holds for a production client facing a server that has storage and
  tickets re-enabled. This matters because a removed or expired peer controls its own config: the server-side setting protects against a hostile
  client, and the client-side setting protects against a hostile server.
- **R3 (required, test-only): neither side's setting is pinned.** The committed regressions pair two production configs. Either side alone forces
  Full, so reverting one side survives every committed test (M_client_only_revert, M_server_only_revert). A future one-line regression on the server
  side would let an attacker-controlled removed client resume past removal or expiry, and the suite would stay green. Required: add two committed
  tests, one per side. Each pairs one production config with a peer whose resumption is re-enabled (client `Resumption::in_memory_sessions(256)`;
  server `ServerSessionMemoryCache::new(256)` and `send_tls13_tickets = 2`). Each asserts `Full` on a repeat connection and `NotMember` from the
  production side after removal. The temp probe shape in this review compiled and behaved exactly like that (pass on production, killed by the
  single-side revert). A both-hostile control asserting `Resumed` is optional, but it proves the probe can detect resumption.
- No new trust, registry, ordering or error-surface change. The existing advisory limit is unchanged: the registry callback runs before proof of
  possession.

## Spec

- **R1 closed.** `client_certificate_chain_time_and_registry_refusals` calls `verify_client_cert` directly. The self-signed fixture is in date
  (940..10000, checked at 1001), has the correct `node_uri([1;16], node 4)` URI SAN and ClientAuth EKU, and is refused only as
  `UnknownIssuer | BadSignature`. The test also covers a foreign same-cluster-id CA, `NotValidYetContext` at 939, `ExpiredContext` at
  `941 + 30 d`, `KeyMismatch`, `NotMember` and `WrongCluster` (brief: "Direct client-certificate verification must independently reject
  foreign/self-signed roots, time and registry failures"). M_drop_server_chain, M_server_fixed_time and the two server flatten mutants are killed
  by this test alone.
- **R2 closed.** `assert_time_error` requires `NotValidYetContext` / `ExpiredContext` exactly. It is used on client direct, client handshake,
  server direct and reused-config expiry checks (brief: "time checks assert native NotValidYetContext and ExpiredContext variants"). The
  context-dropping mutants on both sides are killed. Plain `Expired`/`NotValidYet` are no longer accepted.
- Brief: "Regressions reuse configs before and after local removal and across leaf expiry with a still-valid ticket age". This is met. The
  every-connection contract is met for the paired Arxcovo configs. R3 above is the remaining gap against non-conforming peers.
- No transport, production CA, renewal or membership work was added. The documented local-apply staleness of revocation is unchanged. The
  `pump` → `complete` + `pump` split keeps the 32-round cap and the TLS 1.3 assertions, and existing callers are unchanged.
- Advisories A1/A2 from the full review (custom SAN/trailing checks unreachable behind webpki; `!is_handshaking()` untested) remain open and
  non-blocking.

## Standards (AGENTS.md / CONTRIBUTING.md plus the smell baseline)

- Hard: "Every behavior needs a meaningful test". R3 applies to the per-side resumption settings. Otherwise the new behaviors have killing tests.
  Evidence honesty is good: the inadequate 30-day expiry test is disclosed and replaced; the last test-only refinement is not credited with a broad
  rerun; no hosted, Windows-timing or hardware claim is made.
- Possible Duplicated Code: the `client_config`/`server_config` construction block is repeated across the two reused-config tests and the existing
  `connections` helper. A small `reused_pair(ca, key, reg, time)` helper would serve them and the R3 tests.
- Possible Primitive Obsession: `assert_time_error(error, early: bool)` with `seconds == 939` call sites. Passing the expected variant is clearer.
  This is a judgement call only.

## Limits

No production transport, listener, CA, renewal or membership work exists, and none is credited. Probes are in memory only and were not committed.
Local timings are not hosted Windows `verify` timing. The 660 s Windows hosted cost is still unmeasured. There is no Windows stability, quarantine
recovery or historical credit. All hardware paths are **unverified on real hardware**. M2 open, M3 unchecked, eight Windows quarantines unchanged.

Findings: Security: B1 closed, plus 1 required test gap (R3: per-side resumption settings unpinned). Spec: R1 and R2 closed, 0 new, 2 advisories
carried. Standards: 1 hard (same R3 gap) plus 2 judgement smells.
