# Review: M3 F2 founder/joiner real-identity mutual TLS (Opus)

2026-11-17. Reviewer **Opus 5.5 high**, one independent reviewer, no children,
isolated worktree. Verdict: **PASS, ready for exact-head publication review.**
No blocking findings.

## Pinned target

- Reviewed head: `2ccf985ff3bf1fd21a7c7641f92ed8579192321c` (evidence/docs).
- Source commit: `1f315bcb0b83f550ee3564e3c667f9477df11d18` (test + guard).
- Prepared base: `0bbd8e6b5a63c92a46306c91a99adf291b82f5fa`. It differs from main
  only in `docs/PROGRESS.md` and the brief.
- Main: `67cc62d538905f3d2ed89a7676d1a71b9f4617ff` (merge-base of head).
- `git diff 67cc62d..2ccf985` touches exactly 5 files:
  `consensus/tls_engine_tests.rs` (+250/-1), `tools/ci/test_windows_cluster_quarantine.py`,
  `docs/PROGRESS.md`, the brief, and `docs/evidence/m3-founder-joiner-tls.md` (new).
- Spec: `docs/briefs/m3-founder-joiner-tls-proof.md`, including the root N1
  clarification and the green-main prerequisite. Opus wrote that brief and root
  checked it. This review is independent of the Sol **implementation** only. It does
  not claim cross-provider independence for the design.

## Checks this reviewer actually ran

- Read the full source diff, the brief, the evidence, the PROGRESS delta, the pki
  helpers (`pump`, `complete`, `refusal`, `verifier`, `FixedTime`), the
  `AuthenticatedPeer` extractors and `ConsensusNode::peer_registry`.
- Frozen files: `issue.rs`, `verify.rs`, `Cargo.toml`, `Cargo.lock` and `.github/`
  have no diff versus main. The `issue.rs` and `verify.rs` blobs equal main's
  (`9e318aed`, `c8c198f4`). Their SHA-256 values (`3c3b5959...67fd`, `23078f4b...401c`)
  match the evidence table.
- Strict UTF-8 decode of all 5 changed files: OK, LF only. The test file and the guard
  are pure ASCII. The docs contain some non-ASCII characters (PROGRESS 82, brief 7,
  evidence 5), which is allowed.
- `python -m unittest -v tools/ci/test_windows_cluster_quarantine.py`: 4 tests OK,
  including `test_real_identity_proof_tokens`. A first attempt with `python -I`
  failed only at import because `-I` drops the working directory from `sys.path`.
  That was a runner artifact, not a guard failure.
- Re-hashed all 11 retained artifacts in `D:/Temp/arx-f2-sol-dad95efe2d`. Every hash
  matches the evidence table.
- Read `N1.patch`, `N2.patch` and the failure lines in `N1.log` and `N2.log`. Read the
  nextest summaries: 13/13 at 0.365 s and founder 13/13 at 5.500 s.
- Not run by this reviewer: cargo fmt, Clippy, nextest and the mutants. No concrete
  concern called for a rerun. Compile, test and mutant results below are **reused
  builder (Sol) logs, verified by hash and content**, not re-executed here.

## Standards

Sources: AGENTS.md (meaningful test per behavior, no false claims, unverified-hardware
labels, no production edits outside scope) plus the smell baseline.

- No documented-standard breach. The change adds a test and a guard only. It makes no
  production, Cargo, workflow, sampler, nextest, timeout or helper change.
  Hardware is labelled **unverified on real hardware** in the brief, the evidence
  and PROGRESS.
- S1 (non-blocking, judgement call: Mysterious Name / guard evasion).
  `use crate::cluster::pki::ca::ClusterCa as TestCa;` exists so the new body avoids
  the guard's forbidden token `Cluster`. That token is broader than its purpose,
  which is to exclude the three-node `Cluster` fixture. The brief's own step 1 names
  `ClusterCa::ephemeral`. The alias is honest (it really is the test CA), and the
  three-node `Cluster` is not used. Optional future tidy: narrow the forbidden
  pattern to `\bCluster\b` or `Cluster::` and drop the alias.
- S2 (non-blocking, judgement call: Duplicated Code). The body repeats the
  `refusal(pump(ClientConnection::new(.., ServerName::try_from(SERVER_NAME)), ServerConnection::new(..)).err().unwrap())`
  shape 4 times and the config-build shape 5 times. Writing it inline keeps the
  "no helper change" fence intact and lets the token guard see `pki::pump(` in the
  body, so leaving it inline is acceptable.
- Python guard: segment extraction reuses the existing `fn` split. The required and
  forbidden tuples are module constants, and both mutant families are exercised per
  token. No smell worth raising.

## Spec

Each brief step against the actual test:

- Steps 1-3: private temp dir asserted; real `ConsensusNode::start` with
  `founder_init([1; 16])` and the mem_net `Factory`; real `found()` == `FOUNDER`;
  `Mint` then real `enroll()` == node 2; `fkey` SPKI != `jkey` SPKI. Matches.
- Step 4: for each leaf, the CA signature is verified, raw SPKI == the CSR key's SPKI,
  the SAN is exactly one URI `node_uri([1;16], n)`, and the serials differ. Calls on
  `peer_registry()` itself return `(1,fd)`/`(2,jd)` Ok, `(1,jd)`/`(2,fd)` `KeyMismatch`
  and `(3,jd)` `NotMember`. Matches.
- Step 5: four configs are built once with the unchanged production
  `server_config`/`client_config`, `pki::verifier(&ca, reg)` and one shared
  `FixedTime(NOW/1000)`. Each config pairs a leaf with its own key and targets the
  correct peer. Matches.
- Step 6: joiner client -> founder server gives `(1, 2)`; founder client -> joiner
  server gives `(2, 1)`. `pump` returns `(from_client, from_server)`; `complete`
  asserts TLS 1.3 on both sides, and the extractors refuse incomplete handshakes.
  Matches.
- Step 7: a joiner client targeting `node(2)` against the founder server gives
  `TargetMismatch`. Founder authorization is valid there, so only the target check
  can produce that refusal. Matches.
- Step 8: committed `Remove{node 2}` gives `Removed{replay:false}`. The same
  retained Arcs then give `NotMember` in both directions, and `(1,fd)` stays Ok. In
  direction jc->fs the client first verifies founder (still Ok), so the refusal
  comes from **server-side client verification**. In direction fc->js it comes from
  client-side server verification. Both verifier roles therefore observe revocation
  from the same configs. Matches.
- Step 9: `shutdown().await.unwrap()` runs while the originals of all four configs
  are alive. `retained` held clones, and `reg` and `ca` were consumed earlier. Then
  jc->fs gives `Unavailable`, and `fc`/`js` are dropped explicitly. Matches.
- Exclusions: no `assume`, `test_ticket`, `pki::leaf/registry/connections`,
  `handshake(`, `[250`, three-node `Cluster` or second `ConsensusNode`, and no joiner
  replica. The word `ignore` and `cfg`/`cfg_attr` do not appear in the test. It runs
  as `tokio::test(multi_thread, 4)` with a single 20 s outer `timeout`, and all sync
  verification runs in awaited `spawn_blocking`. Matches.
- Guard: `TLS_REQUIRED` gains the name. The existing required-gate mutant (a gate
  inserted before `async fn founder_joiner_...`) now fails through the fixture
  inventory check. The new `TLS_REQUIRED[:-1]` inventory-drift probe fails through
  the actual-tests set and the `tokio::test` count of 3 + 2. There are 5 required
  removals and 8 forbidden injections. EXPECTED/WINDOWS_UNSAMPLED/ISSUANCE/TLS_FIXTURE
  are unchanged, so quarantine stays 27 (8 + 5 + 11 + 3). The constructor
  classification and the single `three_node` import assertion are unchanged. The
  sampler, the workflows and the 13 founder tests are untouched. Matches.
- Mutants (reused, verified): `N1.patch` keeps the Weak upgrade, the lock, the
  poison->Unavailable mapping and the store drop, and evaluates `authorize_peer` but
  returns `Ok(())`, exactly as the root clarification requires. It fails at test line
  448, `(1, jd)` expecting `KeyMismatch`. `N2.patch` keeps the `peer_identity` parse
  and returns constant node 1. It fails at line 507, `(1,1)` vs `(1,2)`. Restore
  JSONs equal the pre-mutation hashes. The evidence honestly states that step 8 was
  not reached under N1, and that no exclusivity claim is made for either mutant.
- Verification honesty: the evidence discloses the initial nextest selection mistake
  (`issue::` omitted, 13 run, not 26). The missing founder 13 were then run alone and
  no passing set was replayed. Both summaries confirm 13 + 13 = 26 unique cases.
  Hosted CI 44064366449 is labelled as reused root observation. No false claim found.
- Unrequested behavior: none.
- P1 (non-blocking, **outside Sol's diff**): PROGRESS line 13 (the "Historical
  PR159 failures" bullet, from root's prepared base `0bbd8e6`) still ends "first-main
  CI remains pending". Line 5 and the log already record CI44064366449 green. Root
  can drop that clause at publication time.

## Readiness

Ready for the coordinator's exact-head publication review and the five normal
gates (hosted Windows `verify` <= 660 s). Root should refresh exact-main state before
publishing. Still unproven: the hosted run of the new required test on all three OSes,
any joiner replica, multi-host or socket path, and hardware, which remains
**unverified on real hardware**. M2 open, M3 unchecked, recovery Sol 3/3 / Opus 0/2
unchanged. Windows fixture coverage remains quarantined and unverified on hosted
Windows.

Standards: 0 blocking, 2 judgement-call smells (worst: the S1 `TestCa` alias around
an over-broad guard token). Spec: 0 blocking, 1 non-blocking doc nit outside the
diff (worst: P1, the stale "first-main CI remains pending" clause).

This reviewer did no push, PR, status, comment, watch, merge, admin, VM, install or
cache deletion, and started no children.
