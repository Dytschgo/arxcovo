# Owned anchor query local implementation evidence

Builder: Sol 6.1 high, no children, 2026-11-17. Local-only isolated worktree.
Authorized unpublished base: `2bee2043c0f53ac2f173d166f220ae59ec1b0f4f`.
Actual source main: `3022d601f09f24b0e5f092a33f2e9343b500471d`.
Normative contract: query brief, root R1/R2, independent recheck
`d56a4a2268fb9c9af6266e0b46d865a4f2f9a745` (retained 3b45bb07),
and ADR0005 owned-query amendment. No remote queries or public actions.

The query is a local advisory snapshot, with no bearer/signing/member/owner
authority or freshness, unspent-code, real-time expiry promise. Existing TLS
possession, code anchor, E-1 fence and signing guards remain authoritative.
No listener, discovery, custody or controller was added.

## Actual local work

Production: pure pending eligibility; internal fixed32 query; existing issue4
then read8 owned admission; same guarded store/membership image; static errors;
both permits and store Arc in blocking closure, ordered release before completion.
consensus.rs changes only the binding, issue Arc type and same4 initializer.
One mechanical lint correction removes the existing encode dead-code expectation,
which the new production caller makes unfulfilled. No behavior/API change.

New tests distinguish three pure table functions, four durable non-engine cases,
and one all-OS hosted-only engine case. The local durable fixture uses actual
StateStore.apply for Membership/Found/Mint/Redeem, actual shutdown (EngineEnd::Absent),
actual closure-entry counting, gated store lock, and actual owner AbortHandles.
Its synthetic TLS key is the committed founder voter. Any returned consumed-query
list takes the normal client anchor/proof release path before the zero-proof oracle.

Local evidence directory: `target/m3-owned-query-6959-evidence/`.
Task-only Cargo target: `target/m3-owned-query-6959/`. Logs/caches retained.

- `cargo fmt --all -- --check`: PASS (fmt.log); duration not separately captured.
- `cargo clippy --locked --offline --workspace --all-targets -- -D warnings`:
  initial exit101, 54.786s, stale encode dead-code expectation (clippy.log).
- Same Clippy after mechanical correction1: PASS, 5.763s
  (clippy-correction1.log). Correction budget used1/2.
- Initial tool-script parsing error happened before any shell/source operation;
  corrected tool invocation only, no project compile/parser repair.

Source commit: `f9d0a413d02737214af62601aa345c90634a4e26`.

## Completed bounded verification

Natural command (task Cargo target set as above):

```text
cargo nextest run --locked --offline -p arxd -E 'test(pending_query_) | test(owned_query_) | test(cluster::enrollment::anchor::tests::) | test(cluster::pki::preproof_tests::) | test(cluster::pki::preproof::) | test(cluster::pki::tls_tests::)'
```

PASS:26/26 (seven new pure/durable cases, nineteen existing compatibility cases),
653 skipped. Wall102.027s including compile; test phase6.667s (natural.log).
Ownership shutdown case6.664s, bounds/no-submit1.598s, consumed TLS0.112s.
No natural semantic/static-contract failure occurred.

- `python -m unittest discover -s tools/ci -p test_windows_cluster_quarantine.py`:
  PASS8, wall0.510s / runner0.183s (guard.log).
- `python -m unittest discover -s tools/ci -p test_windows_diag_sample.py`:
  PASS60, wall0.505s / runner0.213s (sampler.log).
- `cargo nextest list --locked --offline --workspace --all-targets --message-format json`:
  PASS, wall1.608s (inventory.json; includes Cargo stderr prefix).
  Actual inventory includes all eight additions, none ignored: three pure,
  four durable non-engine and QUERY_REQUIRED's sole engine case.
  Existing fixture28/sample8/founder13/TLS14/atomic1/anchored-proof1 paths
  remain unchanged; existing guards and sampler passed.
- `git diff --check`: PASS. No post-restoration passing-suite replay.

## Once-only compiled counterfactuals

Both used the same task Cargo target, locked/offline package arxd, and only
the targeted durable non-engine case. Neither started an engine. Each compiled
successfully, ran exactly once and failed only its designated assertion.
M1 awaits actual AbortHandle.is_finished; its saved failure propagates after
gate release/thread join, capacity restoration and store weak expiry.
M2 joins query work and completes actual clean shutdown before its oracle.


## Reused evidence and remaining gates

Read only root's existing `D:/Temp/arx-pr174-hosted/main-recovery-success.json`.
CI44126173935 on exact source main recovered functionally on attempt2 after ONLY
zero-step macOS runner cancellation retry. Ubuntu/Windows were not re-executed.
Root evidence reports108 tracked cases each OS, Unix fixture28, Windows553s.
This is reused historical source-main evidence, not a query implementation test.
Trusted first-attempt905s still blocks publishing/merges. No policy activation,
publishing exception, PR/push/network/admin/VM/old Lab Manager action occurred.

New engine case is UNRUN locally; no local Windows engine test is authorized.
Independent full-source/exact-head review and all five normal gates, Windows<=660s
and fresh main remain required. No universal startup/panic cleanup promise.
M2 open; M3 unchecked; Sol3/3 and historical Opus0/2 unchanged.
Hardware paths are **unverified on real hardware**.
PROGRESS is root-owned and unchanged. Proposed entry: local owned-query work
prepared; integrate only after independent review and required evidence/gates.

### M1

Command: `cargo nextest run --locked --offline -p arxd -E 'test(owned_query_timeout_shutdown_retains_submitted_resources)'`.

Exit100; wall38.937s; retained M1.log/M1.json/M1.patch.
M1: actual post-abort read capacity8 versus expected7; issue remained3.
Baseline and restored SHA256 (identical): `350b634354c1d12de617dadbce3c828bf215efb77f7ed9ddaed99ba6ecef234a`.


```diff
--- crates\arxd\src\cluster\consensus\anchor_query.rs
+++ crates\arxd\src\cluster\consensus\anchor_query.rs
@@ -70,7 +70,7 @@
                     drop(issue_unit);
                     Err(AnchorQueryError::Unavailable)
                 } else {
-                    blocking(move || {
+                    let result = blocking(move || {
                         #[cfg(test)]
                         entered.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                         let result = (|| {
@@ -90,12 +90,13 @@
                         })();
                         // Guard first, then Arc, read, issue, then completion.
                         drop(sm);
-                        drop(read_unit);
                         drop(issue_unit);
                         Ok(result)
                     })
                     .await
-                    .unwrap_or(Err(AnchorQueryError::Unavailable))
+                    .unwrap_or(Err(AnchorQueryError::Unavailable));
+                    drop(read_unit);
+                    result
                 };
                 if !tx.is_closed() && Instant::now() < deadline {
                     let _ = tx.send(result);
```

### M2

Command: `cargo nextest run --locked --offline -p arxd -E 'test(owned_query_consumed_tls_zero_proof)'`.

Exit100; wall37.609s; retained M2.log/M2.json/M2.patch.
M2: same actual server plaintext assertion received32 versus expected0.
Baseline and restored SHA256 (identical): `0f0f185d2f4b604f879738dcb84b046a0f282af6ad81ccfd7bf0d1ca3a65195b`.


```diff
--- crates\arxd\src\cluster\enrollment\anchor.rs
+++ crates\arxd\src\cluster\enrollment\anchor.rs
@@ -32,7 +32,7 @@
             .checked_add(2 * SKEW_MS)
             .ok_or(AnchorQueryRefused)?;
         let record = self.records.get(&code).ok_or(AnchorQueryRefused)?;
-        if record.consumed.is_some() || usable >= record.expires_ms {
+        if usable >= record.expires_ms {
             return Err(AnchorQueryRefused);
         }
         BoundVoters::observe(self, membership.ok_or(AnchorQueryRefused)?)
```

## Retained local log hashes

- fmt.log: `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`
- clippy.log: `d6fc78018cc57d423b1437c352ea1a29a93a55d13c5aa0e06e0093677d423cac`
- clippy-correction1.log: `f657601f61b9c46fc3edabd72eb557f427bb0fbcad122c16062f3e8ca9a94cf0`
- natural.log: `4c47feee6a816cc76757b68a7252a759c1c96146df81068b6ab72aaf73c0d375`
- guard.log: `be81a2f0548b4d72a32c6b872ac6ebc857ca42f2c6d2291f736c983251155fb5`
- sampler.log: `23f260d49eab8df08222d578d2f84e275549907bf89bf65ca5cf5608d060cbd5`
- inventory.json: `e752a24cf5577be7d99ce6530871dc8b27a5f5364fd30ae92f20a2e1fcd2344c`
- M1.log: `698f125466a80e349c9d9be0dc839d64df909ca7daf68dd21e6f5e185ad879ed`
- M2.log: `7c5e535b5aa517fcca7ae05118b48d1548add85aac4298e51aadeae9d082ee1d`


Outcome: authorized bounded local implementation/verification complete. Source
restored exactly after each mutant, then both hashes rechecked before source commit.
All owned command sessions completed; passing natural cases and both intentional
failure cases cleaned their gated work. No new cleanup guarantee is inferred.
The retained build cache last compiled M2; source, not a post-restoration binary
execution, is what the restoration hashes establish.

Suggested root PROGRESS entry: Sol6.1/high local owned-query implementation
f9d0a41; fmt/Clippy(after1 mechanical lint correction)/26 selected tests/8 guard/
60 sampler/real inventory PASS; M1 and M2 designated compiled failures once each,
exact source restored. Hosted ONE-start query pipeline UNRUN; independent exact-head
review and five gates pending; source-main905s publishing hold remains.
