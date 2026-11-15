# A7/A11 integration budget correction

2026-11-15, OpenAI / gpt-6.1-sol / high; parent `5e18eff` (PR125 merged
main `833fd55`). Documentation only, no implementation or native authority.
Retain Muse `ffa16f5` / retained `e99b1c2` except the two requested pins:

- [Proposal pin 2](../reviews/m2-a7-a11-integration-budget-proposal.md): actual
  `start --no-block`, monitor active/runtime/MainPID/exe/cgroup checks, then ACK,
  workers, both-case oracle and terminal exact success. Driver source lines
  1282-1349, 1455-1470, 1571-1572; no blocking-start claim.
- [Binding cleanup amendment](../reviews/m2-a7-a11-delivery-contract.md#binding-budget-amendment):
  new 10s/case acceptance allowance (5s/call), not an existing native bound.
  Runner mod.rs:200-226 calls unix.rs synchronously; unix.rs:422-449,481-504
  and cgroup.rs:124-163 establish 2s kill confirmation +2s reap, guard drops,
  and possible failed-removal retry. Full-call elapsed <=60s/15s, case <=90s,
  admission/checks <=15s; errors/residue/excess refuse. No separate cleanup
  timing claim, detached thread or hard syscall/join guarantee. Parent cancels
  and joins BOTH workers on failure/panic. Source-pinned actual nominal probe
  remains 9*5+5=50s; RunSpecs55/10, monitor120, Runtime140 and package startup30
  stay. Monitor includes startup: 15 startup/poll overhead +5 ACK +90 cases
  +10 join/completion=120; late terminal observation refuses. Shared ACK is
  currently120s; fixture must enforce5s. Admission's 5s polling loops end in
  synchronous wait (sudo_nnp.rs:542-613), not a native wall-clock guarantee.

Validation: static source assertions/callgraph inspection, relative Markdown
links and source line targets, strict UTF-8/no BOM/LF/final newline, three-file
scope and `git diff --check`. No application builds/tests needed or run for
this documentation correction. No host/root/sudo/helper/account/dispatch,
variable or network actions; no subagents, push, PR or merge. No PROGRESS,
ADR or source changes. Prior review content is untouched.

Amendment review/gates/merge precede implementation; D9b/source gates and
separate fresh-host native grant remain. D7a-h consumed; stale A1/A2 owner
reply closed. A7/A11, M2, full F/G and real-Runner PA6 remain
**unverified on real hardware**.
