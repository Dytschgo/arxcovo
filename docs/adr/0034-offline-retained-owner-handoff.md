# ADR0034: Retained offline startup owner handoff

Status: design accepted; implementation/source review pending. Live integration unavailable.
Date:2026-11-19.

PR187's offline Prepared retains its real outer OS lock and checked roots/files,
but exposes no owner-bound transfer to a future startup job. A pathname getter
cannot establish which objects SQLite/Engine opened. The reviewed next slice
therefore validates exact startup intent and transfers retained ownership only.

Prepared records the verified recovery cluster/node, supplied fresh cluster and
private Ready/Begun/Failed creation state. Handoff validates mode, both canonical
addresses and binding. For fresh founder storage, Begun is irreversible before
the first create-capable opener. First refusal survives partial effects and
ordinary caught unwind; failed ownership is retained and cannot be retried into
success. No automatic repair/delete/recapture or persistent rollback claim.

Existing real store openers initialize untouched cluster/first_free2 genesis,
without Found/membership. While both stores are owned, stage only the fixed
database/lock/optional-zero-journal delta. Preserve original guards/outer lock and
install atomically. Captured journal absence or zero identity must remain exact
through drop and immutable inspection; additions/removal/replacement/growth or
recovery sidecars refuse. A real positive must survive or scope stops/narrows.

The two startup files and inline real filesystem/store tests are the sole source
scope. Checked inputs borrow a path; no Engine/start/founder/store-core/transport/
config/format/dependency or live operation changes. The deferred twelve-phase
framework, fake job/publisher and sealing facades remain excluded.

Actual opened-object binding before Engine effects is an unconditional separate
prerequisite. Current open_db drops its checked File before opening SQLite by
path, and start exposes no post-open/pre-Engine or pre-return cleanup owner.
Qualified CA/member-key custody/constructor, actual Found-origin publication,
fresh attachment eligibility and a real retained process job remain open.

The offline source change is reversible. It creates only owned fixture/genesis
storage in the explicit future operation; it migrates or repairs no existing
store. Meaningful fixtures, independent exact-source review and normal gates
remain required. [Bounded brief](../briefs/m3-retained-owner-handoff.md).
M2OPEN/M3UNCHECKED; **unverified on real hardware**. Exhausted counters unchanged.
