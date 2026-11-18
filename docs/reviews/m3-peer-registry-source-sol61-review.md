# Independent try-lock registry source review

Sol6.1/high independent reviewer,2026-11-18. PASS.
Base8b36405 through3b5cdccc0f146508f38620d0438da3e8f2f90822.
Reviewedblobs consensus8f08c3334d8ac6d3aad804bbfa2566765a92aac7,
peer_registry1d4a4082ed0d9e253625dd34d0884f4347d1e204.

Standards0: exactly2non-doc paths, removingmodulebinding restoresbaseconsensus
byte-for-byte. All4tests synchronous/unignored/allplatform. Spec0: Weakcapture,
try_lock, actualauthorize/staticmaps, guardbeforetempowner, noqueue/retry/snapshot
or existingcallback changes. TestsactualFound/Remove/immutability, heldmutexrefusal
beforerelease+laterexplicitacceptance, poison/cloneownership/droppedstore.
Heldlock test savesobservations/releasesguard/joinshelper BEFOREassertions,
so blockinglockcontrol canfinishcleanup insteaddeadlocking.

Security0: Mutexpoison != SQLwritepoison; tempowner mayrun destructors; Released
notOS/SQLitecloseproof. No fullTLSsafety/freshowner/bootstrap/perpetualauth claims.
Reviewed12retainedartifacthashes, fmt/Clippy and4/4PASS. Poisonhelper intentional
joinedfixture. Reviewer ran no Rust/build/engine/network. Verdictapplicable after
2plannedcontrols onlyif bothreviewedblobs+remainingnon-doc tree exactlyrestored,
with accuratereview/evidence/PROGRESS retention. Futurecontrols attributedseparately.
No retry/merge/timeout-change grant. Counters/hardwarelimitations unchanged.
