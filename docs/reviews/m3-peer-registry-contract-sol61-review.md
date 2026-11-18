# Independent try-lock registry contract review

Sol6.1/high separate reviewer,2026-11-18. PASS for narrow no-mutex-wait callback
prerequisite after precise limitations recorded. ActualRegistry/SmAdapter APIs fit;
two-path sourceallowlist preserves existingengine/store/TLS/owner behavior.

Standards0. Spec precision correction: no explicitquery/persistence I/O and no mutex
waiting, but a temporaryWeak upgrade may become laststrongstore owner during
shutdown; dropping it can run SQLite/file destructors synchronously. Live TLSowner
must account for race before runtime-worker safety claim. Root recorded correction.
Mutexpoison distinct from privateSQLwritepoison; no storehealthproof. Released
observes precedingdestructors, not verifiedSQLite/OSclose success.

Security0 in narrowed scope: actualauthorize_peer/staticrefusals, no snapshot or
owner/control/bootstrap authority, localappliedstaleness/perRPCreauth explicit.
Heldmutex test must saveobservations, releaseguard and joinhelper before EACH
potentialpanic/assert/unwrap includingtimeoutpaths. Retentiontests keep callback
clonesalive. ADR0027 may beaccepted after corrections withoutanothercontractreview;
actualsource review/normalgates stillrequired. No source edits/tests/build/network/
engine run. Existingcounters/hardwarelimitations unchanged.
