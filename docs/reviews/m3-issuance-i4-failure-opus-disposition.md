# M3 issuance PR A: I4 red, Opus read-only disposition

Reviewer: Opus 5.5, high, 2026-11-16, as the other-provider handoff after Sol stopped on the first engine red. Head `8c3436e` (base `8961772`).
Scope: read-only diagnosis plus this document. No source change, build, test, rerun, dependency resolution, push, PR, CI, VM or admin step. Crypto review
is deferred because the slice is incomplete: final I7 is uncompiled, and the 4/4 pure PKI pass predates the added OID check.
Inputs read: `docs/evidence/m3-certificate-issuance.md`; the brief's I4 and E3 (`docs/briefs/m3-certificate-issuance-first-slice.md`); the resumption brief's red
handling (`docs/briefs/m3-bounded-read-resumption.md`); recovery plan §5–§6 (`docs/reviews/m3-windows-three-node-opus-recovery-plan.md`); the D1 S3 control
and its cleanup record (`docs/evidence/m3-windows-three-node-diagnostics.md:51,54,80`); and the source cited below.

## Classification: fixture protocol ordering in the new I4 test (cleanup after a deliberate deposition)

This is **not** a product read defect. The evidence is sufficient to classify the failure, but **not** to credit a fix, because no corrected head has run.
What the source and the bounded evidence actually show:
1. I4 deliberately deposes the issuer: it isolates `l` (`issue_tests.rs:170`), waits for a replacement Leader, gets the retry issued there, then calls
   `isolated.clear()` and `converge()` with nothing in between (`issue_tests.rs:212-213`).
2. `converge` takes `leader()` (`three_node.rs:219-222`). `leader()` returns the **first node in index order** whose watched metrics say `Leader`
   (`three_node.rs:123-146`). Node 1 (index 0) is checked first, and node 1 is commonly the founding leader because `initialize` runs on `node(0)` (`:69`).
3. The DIAG line has `polls=0`, and `before`, `after` and `at` all sit at t=3539. At the moment of failure node 1 published term 1 `L`, while node 3 was `L`
   at term 2 and node 2 was `F` at term 2. These are positive samples, not inferences from missing ones. `leader()` returned the deposed node on its first
   scan, before node 1 had received any term-2 message after the heal.
4. That node's read returned `NotLeader`, which the adapter maps only from OpenRaft `ForwardToLeader` (`consensus.rs:435-436`). That is the designed safe
   refusal for a deposed leader, the same outcome R2, T2 and the S3 control require. No stale state was returned. `read_class` gave `Churn`, from the
   term-2 history on nodes 2 and 3, not `SameTermCandidate`.
5. Signature match with the recorded precedent: the first D1 S3 control run failed **in its own cleanup** with the identical shape. `leader()` returned the
   deposed node, which was still publishing term-1 Leader metrics while the replacement led at term 2, and the single `converge()` read gave `NotLeader`.
   The correction changed control code only: wait until the deposed node observes the new term, then converge (`diagnostics.rs:1061,1089-1100`). No helper
   changed. D1 fact 4 records the same cross-node overlap.

The same lifecycle applies: deliberate leader isolation, a higher-term replacement, a heal, then helper use while the deposed node may still publish
Leader metrics. A product read defect would need an `Ok` read from the deposed node or a steady same-term refusal (`SameTermCandidate`). Neither occurred.
Not established, and not claimed: which internal OpenRaft message produced the `ForwardToLeader`; the host platform's contribution; and whether anything
later in `converge` (wait, applied/snapshot equality) or in `finish` would pass. Those steps never executed.

## Proposed protocol barrier (docs-level, for root and Sol review BEFORE any implementation)

This is a test-body change to I4 only. It changes no helper, deadline, retry, assertion, Params, quarantine, filter or diagnostic. It mirrors the
reviewed S3 control lines.
```diff
         let l = c.leader().await;
+        let term = c.diag.sample(c.node(l).raft.as_ref().unwrap()).term;
         let code = code(11);
@@
         assert_eq!(leaf.node.get(), 4);
         c.router.isolated.lock().unwrap().clear();
+        // I4 deposed the issuer on purpose. It may still publish Leader metrics, so
+        // converge only after it observes the replacement term (D1 S3 precedent).
+        c.node(l).raft.as_ref().unwrap().wait(Some(Duration::from_secs(5)))
+            .metrics(|m| m.current_term > term, "deposed issuer observes new term").await.unwrap();
         c.converge().await;
```

Why this is a legitimate barrier and not a mask:
- The barrier is bounded. Nothing is retried: if the deposed node never sees the new term, I4 fails **at the barrier**, with its own label. It is a
  wait on a named protocol event, not a longer deadline on an existing step. The expected wait is one heartbeat (100 ms, `adapter.rs:390`), well inside
  the 45 s fixture bound and the ≤25 s added-wall-time goal.
- It preserves everything I4 currently proves: isolation, a replacement leader at a higher term, the old issuer's `Unavailable|StaleIssuer` with no leaf
  inside 5.5 s, issue permits free, read units returning in 2 s, and the new-leader retry. After the barrier, `converge` runs **unchanged**: one fresh
  read on the helper-selected leader, every acknowledged index covered, and every node (the healed old issuer included) applied with an equal snapshot.
- Once `current_term > term`, node 1 cannot publish term-1 Leader. It can publish Leader again only by winning a later election, in which case it is the
  real leader. The predicate reads the same metrics watch that `leader()` reads.
- Counterfactual: the recorded `8c3436e` red is exactly the no-barrier case. A docs-level check for the reviewer is that removing the barrier restores
  only the precedent's signature (deposed-term Leader plus `Churn`). A red **after** the barrier that is `SameTermCandidate`, a convergence `Wait`, or
  arrives with no attribution is **not** covered by this disposition, and it routes under the existing rules.

Assertion gap found, separate from the red (pre-readiness strengthening, not part of this barrier): I4 asserts only `leaf.node == 4`. Node 4 is also
`first_free`, so that check cannot tell a replay from a fresh consumption, and the leaf's SPKI is never compared to the CSR. The brief's "same node ID
(`replay`)" and the requested "replay SPKI/node" evidence are therefore **not yet asserted** at `8c3436e`. Before readiness, add a replay discriminator
(for example: no second member or node 5 after converge, or the replay outcome itself) and a check that the leaf SPKI equals the CSR SPKI. The barrier
neither adds nor removes those checks.

## Shared-helper fix versus new-fixture ordering

A change to `leader()` (for example, prefer the highest term), to `converge()` (retry on `NotLeader`), or to `attributed_read` would be a **shared helper
fix**. It would change the quarantined eight, R1–R5 and the D1 controls, and it would mask the S3a/S3b seam that the recovery plan exists to
discriminate. Recovery plan §5 forbids new helper retries, the resumption brief forbids any change to fixture helpers, and a `converge` change requires
an evidenced S1 cause. None of that applies here. I4, unlike the quarantined tests, **creates** the deposed leader itself. Restoring a single
current-term leader before handing the cluster back to a helper is that test's own obligation, exactly as it was for the S3 control. Hence the
correction is new-fixture protocol ordering.

## Attempt budget mapping (honest, no new authority)

- **Sol:** the three-attempt cluster stop stays. Sol correctly stopped on the first red and made no rerun or fix. This disposition is not a fourth Sol
  cluster fix and authorizes none.
- **Opus recovery §5:** two fix attempts, both unused. They are scoped to the quarantined Windows eight and need per-seam DIAG citations. This diagnosis
  consumes neither. I advise against charging an I4 test-body barrier to §5: it fixes no quarantined test and no helper.
- **Not automatic:** applying the barrier needs an explicit root decision, recorded as a brief amendment (for example "E7: I4 heal barrier"). That
  decision should classify the barrier as a new-test ordering correction, assign the builder, and require the other provider to review the exact diff.
  The conservative assignment is for Opus to apply it with Sol reviewing, which avoids any reading of it as a fourth Sol fix. No gate, quarantine or
  acceptance waiver is implied.

## Stop-rule hard gap

E3 sends any red to the resumption brief's handling. Read literally, a red with a helper attribution goes to the recovery plan, and the PR is **blocked
until §6 re-enable evidence exists or the owner decides**. §6 concerns the eight quarantined tests and hosted Windows evidence. It may never be met
(after a §5 stop, the route is the owner's option B). So, literally, one ordering defect in a new test would block all of PR A, and therefore M3
issuance, on unrelated work. The rule has no branch that tells the new test's own deposition-cleanup ordering (a precedent signature: deposed-term
Leader alongside a higher-term replacement, with `Churn`) apart from evidence about the quarantined seam. Its text also says "red on the PR head",
while this red was a local, pre-PR first execution; Sol's conservative reading was correct. Closing the gap is a root decision and should not be
inferred. A minimal amendment: allow one reviewed test-body barrier for a precedent-matching deposition-cleanup red, with every other helper-attributed
red keeping the existing route.

## Next authorized work (root decides; nothing here starts it)

1. Root accepts or rejects this classification and, if it accepts, issues the E7 amendment with a builder and a reviewer.
2. If accepted: one new head containing the barrier and the I4 replay/SPKI assertions. Then a single execution of the full required local list in the
   evidence doc (final I7 compile/run, pure PKI including the OID check, engine I1–I9, Clippy, fmt, guard, sampler, controls), with no rerun of any head.
3. Then a fresh Opus source/crypto review of the complete slice, and the five gates. If rejected: PR A stays blocked under the literal E3 route.

Status unchanged: M2 open (A7 FINAL NO-GO, A12 consumed with no retry); M3 unchecked; Windows quarantine of 8 unchanged; Windows three-node
**unverified on hosted Windows**; all hardware paths **unverified on real hardware**.
