# 0023: Independent M3 work alongside open M2 hardware gaps

Status: accepted within owner-delegated scope,2026-11-15; no acceptance waiver.

KICKOFF normally orders milestones1-6. The owner's2026-11-15 14:06 steering permits parallel work on other features with few dependencies; subsequent delegated babysitter steering explicitly directs remaining independent M2 work, then independent M3 while blocked hardware acceptance stays visible. This narrowly supersedes sequential implementation for independent M3 source only. PR142/143/144 therefore do not imply M2 completion.

M3's enrollment core and storage probes are independent of enabling the production dedicated executor. Their own independent different-provider reviews and all normal gates remain mandatory. CA keys, production endpoints, real enrollment, transport and cluster acceptance retain their own contracts. No M4 work or post-v1 scope follows.

The [live M2 closure plan](../PROGRESS.md#ordering-and-m2-closure-plan) lists every open item, evidence, blocker and active work. A7/A11 hosted is finalNO-GO, A12's sole authorized attempt is consumed with cleanup unconfirmed; no hosted correction, diagnostic or retry is authorized. Dedicated-host work requires an owner-provided qualifying environment and a separate explicit scoped authorization; agents perform no owner-admin setup and never reuse the uncertain-cleanup host.

M2 stays unchecked. Existing bounded accepted evidence is preserved, open hardware items remain **unverified on real hardware**, and complete F/G/PA6 acceptance is not inferred from nonnative tests or M3 success. Actively maintain the M2 source regression/acceptance checks on every relevant change; resume native closure only when its actual prerequisites exist. Do not schedule more hosted attempts or manufacture evidence tasks to simulate progress.
