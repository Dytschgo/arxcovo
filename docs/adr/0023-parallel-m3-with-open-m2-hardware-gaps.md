# 0023: Independent M3 work alongside open M2 hardware gaps

Status: accepted within owner-delegated scope,2026-11-15; no acceptance waiver.

KICKOFF normally orders milestones1-6. The owner's2026-11-15 14:06 steering permits parallel work on other features with few dependencies; subsequent delegated babysitter steering explicitly directs remaining independent M2 work, then independent M3 while blocked hardware acceptance stays visible. This narrowly supersedes sequential implementation for independent M3 source only. PR142/143/144 therefore do not imply M2 completion.

M3's enrollment core and storage probes are independent of enabling the production dedicated executor. An independent reviewer separate from the builder under the current owner routing, and all normal gates, remain mandatory. CA keys, production endpoints, real enrollment, transport and cluster acceptance retain their own contracts. No M4 work or post-v1 scope follows.

The [live M2 closure plan](../PROGRESS.md#ordering-and-m2-closure-plan) lists every open item, evidence, blocker and active work. A7/A11 hosted is finalNO-GO, A12's sole authorized attempt is consumed with cleanup unconfirmed; no hosted correction, diagnostic or retry is authorized. Dedicated-host work requires an owner-provided qualifying environment and a separate explicit scoped authorization; agents perform no owner-admin setup and never reuse the uncertain-cleanup host.

M2 stays unchecked. Existing bounded accepted evidence is preserved, open hardware items remain **unverified on real hardware**, and complete F/G/PA6 acceptance is not inferred from nonnative tests or M3 success. Actively maintain the M2 source regression/acceptance checks on every relevant change; resume native closure only when its actual prerequisites exist. Do not schedule more hosted attempts or manufacture evidence tasks to simulate progress.

## Owner steering — dedicated PC and container acceptance (2026-11-18)

Relayed by the existing babysitter on Dylan's behalf. This is additional scoped
authorization, not a waiver of acceptance or exhausted-attempt rules.

Dylan authorizes this Windows PC, a future cluster member, as the M2 test host for
the applicable open A4 crash, A9/U9, A10, Stage F/G and PA6/Claude Code acceptance
paths. Native tests remain blocked until the owner creates the dedicated Standard
account, configures a disk cap and CPU/RAM reserves, and completes any required
admin installation. Agents perform no account creation, admin setup or silent
fallback to the owner's login. The coordinator sent the short M2 checklist from
[dev-host setup](../dev-host-setup.md) before any native execution. All data, VM
disks, build output and TMP/TEMP must be on D:, given low free space on C:.
D:\Code\LabManager, its installed state and Hyper-V VM agent-windows-test stay
untouched. Linux A7/A11 sudo/NNP and A12 remain dedicated-Linux-host pending;
Windows evidence cannot close those paths or imply complete cross-platform F/G.
This PC authorization satisfies the earlier request for a designated Windows
host, but does not establish that its required account or fences already exist.

For M3, use three containers on a GitHub-hosted Ubuntu runner where feasible,
with real inter-container network transport, one node killed, acknowledged writes
preserved, continued quorum writes, and expired/reused join codes refused.
VM/container acceptance is explicitly permitted by the kickoff. It earns only
VM/container acceptance; real multi-host hardware remains **unverified on real
hardware**. Do not substitute an in-process mem_net test or mock authentication.
Transport, custody and clock prerequisites need implementation and review before
the final acceptance can run. Local VMs are a fallback only if containers cannot
exercise the path; required hypervisor installation is an owner step listed first.
The Linux hosted experiment stop rules above remain unchanged; a separate M3
container harness is not a retry of those experiments. No container CI run has
been dispatched by this steering record. CI time budget, technical network/data
fences, independent review and normal merge gates remain mandatory.

Current routing follows AGENTS/KICKOFF's owner update: Sol 6.1 engineering and
independent reviews, Astra 6.2 when stuck as explicitly allowed, Claude UI only.
No earlier cross-provider rule overrides that owner update. Sol 3/3 exhausted and
historical Opus 0/2 remain unchanged; no founder diagnostic/fix authority follows.
The existing babysitter and its schedule remain untouched.
