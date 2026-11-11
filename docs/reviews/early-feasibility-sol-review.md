# Sol review of retained early feasibility research

Reviewer: OpenAI / gpt-6.1-sol / medium, coordinator, single reviewer.
Scope: the five research/setup documents at `9193e935e5e40ca8c507711b9da440a8ebb4bb26`
(ADR0003–0006 and dev-host-setup), read against KICKOFF and current ADR0009/0010.
This records the other-provider review of the retained Opus research. It does
not independently review Sol's own status/integration edits; their review
belongs to the subsequent Opus PR recheck.

## Standards

No blocking finding for retaining these as proposed/historical research.
The five documents were read in full. UTF-8, balanced fences and relative links
were checked. Runtime/dependency/workflow files are unchanged by the PR's own
scope; no build or hardware result is implied by those structural checks.

## Spec

No blocking finding for this documentation scope. It preserves QEMU/WHPX,
the dedicated standard account, official owner-supplied media, TEST-only Windows
hosts, protected static resources and owner-only administration. It does not
adopt a new network design: isolated user networking is explicitly insufficient
for required guest-to-guest reachability. CPU/RAM/disk values are initial owner
prerequisites, not measured enforcement. No VM may run unattended before caps
and reserves exist. Current service/development ADRs supersede historical ADR0006;
no automatic credential fallback, third S4U probe or owner-PC action is authorized.

Raft notes retain committed membership, durable deduplication and enrollment,
CA identity/key binding, quorum and real-time expiry limits. Learner-until-three
and voter caps remain unadopted suggestions. CA distribution, storage, expiry
clock assumptions and revocation still require M3 decisions and tests.

## Source checks and limits

The pinned [QEMU WHPX source](https://raw.githubusercontent.com/qemu/qemu/v11.1.3/target/i386/whpx/whpx-all.c)
contains the migration blocker; its [snapshot source](https://raw.githubusercontent.com/qemu/qemu/v11.1.3/migration/savevm.c)
checks migration blockers before saving. [QEMU's WHPX prerequisites](https://www.qemu.org/docs/master/system/whpx.html)
require the Windows feature. These corroborate source-level limits, not the
owner's selected binary or standard-account behavior. Docs.rs OpenRaft pages
could not be fetched during this check; researched version/API candidates must
be reverified when M3 begins. No current dependency recommendation is made.
No admin/account/service/VM operation, private-state read or native probe ran.
All hardware and installed paths remain **unverified on real hardware**.

Disposition: retain research and owner prerequisites; do not count this as
implementation, architecture adoption, milestone acceptance or approval of
Sol-authored integration metadata. Fresh independent PR review and all gates
remain required before merge.
