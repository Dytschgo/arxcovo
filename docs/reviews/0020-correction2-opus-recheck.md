# ADR 0020 correction 2 independent Opus recheck

Reviewer: Anthropic / Opus 5.5 / high, 2026-11-12. Single review agent; no
subagents. Independence is claimed **only** for the coordinator's
OpenAI / gpt-6.1-sol correction-2 delta. The original ADR 0020 architecture
and correction 1 are Opus-authored; their independent credit stays with the
retained Sol reviews ([original](0020-sol-independent-design-review.md),
[correction 1](0020-correction1-sol-recheck.md)).

Disposition: **correction 2 closes P2, P6 and P7 at design level; no new
blocker; five non-blocking advisories.** The proposal stays proposed until
the coordinator records acceptance. This docs-only design review does not
authorize implementation, activation, merge, retry, experiment or any owner
operation.

## Exact scope and pins

- Target: `07475b8ac2abbd05af594e9f9c9b6f7d41cf52c0`; parent
  `2710141`, which has a tree identical to Sol correction-1 report
  `6bdfa141bc94c9cb84c64d8e9dd15292c7810b45` (`git diff --quiet` empty).
- Reviewed delta: `git diff 2710141 07475b8` over the
  [ADR](../adr/0020-linux-execution-hierarchy-and-janitor.md),
  [brief](../briefs/0020-linux-hierarchy-implementation.md),
  [PROGRESS](../PROGRESS.md) and the new
  [author disposition](0020-correction2-disposition.md) (4 files, +89/-12).
  The disposition is a claim checked here, not closure evidence.
- Source comparison: `fdea1c6` (Stage D PR55 on Stage C), as in the
  correction-1 recheck. Source was read, not inferred from prose.
- Working directory and `git rev-parse --show-toplevel` agreed on an
  isolated runtime worktree distinct from the canonical checkout; status was
  clean at HEAD = target. No checkout was created or switched.

## Standards

**No blocking finding.** The delta keeps the honest-evidence, inertness and
privacy rules of AGENTS and CONTRIBUTING: no future test is claimed as run,
production stays MechanismUnavailable, native/installed paths stay
unverified on real hardware, and no private data or owner path is added.
The PROGRESS entry names provider and outcome. All four files are UTF-8
without BOM and LF-only; `git diff --check` over the delta is clean; the
disposition's relative links resolve.

Nit (judgement, not a documented rule): two rewritten ADR lines in the D6
Bound paragraph (around ADR:386-388) exceed the file's ~78-column wrap.
Cosmetic only. No Fowler smell-baseline finding applies to this prose delta.

## Spec

**P2 closed at design level.** D6 now states the bound per immutable shared
admission domain, requires exactly one domain for the exclusive service
`runs/` subtree until restart, binds runner/port, slot accounting,
readiness and ticket allocator once, and forbids replace, reset, rebind and
second-runner APIs. Bounds and Consequences match; no process-wide or
global-orphan claim remains. R3 is narrowed to isolated test domains with
distinct subtrees, which removes the correction-1 counterexample. The
per-domain proof Sol accepted is unchanged and its assumptions are
satisfied by the source shape: `Execution` owns one `active`/`limit` pair
and one `backend` (`executor.rs:162-173`); `UnixBridge::new` takes a
`FnOnce` factory, so one bridge holds at most one runner, and a failed
construction stays failed (`unix_bridge.rs:40`, `:48-62`).

Encoding is feasible. In source the shareable handle is `Arc<Execution>`;
T20's "bridge handles" map to clones of it, and identity checks can use
`Arc::ptr_eq` in-crate without a new executor seam. The only current
`exec::Runner::new` calls are test code (`exec/tests.rs`,
`stage_b_tests.rs`, `cfg(all(test, linux))` `disposable.rs`), so a
non-test call-site pin is practical. The test-only
`Execution::unix_bridge_for_test` installs its backend through
`Arc::get_mut` before sharing, which binds it once and is not a rebind. No
hidden authority: the correction removes surface and adds no agent- or
owner-facing API, config field, MCP field or activation path. "Future
production wiring must preserve it before activation" is a precondition,
not a grant.

**P6 closed.** ADR D1 and brief E5 permit exactly the private flag path
`Runner::run_core -> unix::run -> Group::create`, Linux-gated, and list
`exec/mod.rs` and `exec/unix.rs` in the E5 manifest. Statics and
config-carried flags are excluded, and order, cancellation/fence checks and
legacy/phased results are explicitly preserved. Matches source:
`run_core` calls `unix::run(config, spec, cancel, shutdown,
caller_shutdown)` (`exec/mod.rs:190-205`), and `unix::run` is the sole
caller of `Group::create` (`unix.rs:219`), after the identity, account and
recheck steps that must stay ahead of it. See advisory A1 on the errno
clause.

**P7 closed.** W3e now holds the worker before its final shutdown/admission
check, expects refused `stopping` and zero port calls, and in a separate
case lets already-admitted work call the port, then cancel and drain with
an honest report, without a zero-call assertion. This matches source. The
worker's last executor shutdown check is in-slot, after `fetch_add` and
before `backend.run` (`executor.rs:340-351`), and the dispatch-time check
under the workers mutex already precedes spawn. The bridge gate checks
readiness, not shutdown. The zero-call case is therefore deterministic for
any hold point that precedes that in-slot check. The admitted case can hold
inside the fake port, as T19 does. See advisory A4.

### Advisories (non-blocking; fold into a wording pass or slice review)

- **A1 errno clause is inaccurate for one branch.** ADR:128 and brief:375
  say to keep "original errno capture before (create-path) cleanup". At
  `fdea1c6` that holds only for the setup-failure branch (`e` is captured
  before `unlinkat`). The `openat`-failure branch calls `unlinkat` and
  **then** reads `last_os_error()` (`cgroup.rs:84-88`). If that `unlinkat`
  fails, which is exactly the residue case, the legacy
  `ExecError::OperatingSystem` code is the unlink errno. Recommended: say
  the existing per-branch read order is preserved bit-for-bit, because
  "no `ExecError` change" and R4 already require that, and require that
  `release_leaf` adds no errno-affecting call before the caller's read.
  Alternatively declare a deliberate result change. Do not leave both
  readings open.
- **A2 T20 straddles E6 and E7.** T20 sits in E6, but its startup-discovery,
  ticket-allocator and service-wiring-pin parts depend on E7 artifacts. Keep
  the shared-`Execution` latch, slot, readiness and runner checks in E6.
  Move the startup-discovery, allocator and single-wiring-site pin checks
  to E7, so that each PR keeps one concern and its tests stay compilable.
- **A3 Name `exec::Runner::new` in the pin inventory.** `exec` is `pub mod`
  and `Runner::new` is `pub`, so the domain rule is enforced by source pins,
  not by types. Say explicitly that the pin rejects any non-`cfg(test)`
  `Runner::new` or `UnixBridge` construction outside the one wiring factory.
- **A4 W3e reuses the single E6 executor seam.** E6 allows "one `cfg(test)`
  barrier seam only". T18's seam, after preflight and before `fetch_add`,
  already precedes the in-slot shutdown check, so it satisfies W3e. State
  that reuse, so that "immediately before" is not read as permission for a
  second executor seam.
- **A5 Scope wording drift.** D4 still says the janitor runs "once per
  process", and D7 calls the ticket "process-local". Under the new rule,
  both are per domain: R3/T20 build several test domains in one process,
  and a once-per-process static would contradict "no process-global
  static". Also keep the allocator with the cache it orders (`Activity`,
  E7), so tickets from one cache never come from two allocators.

## Checks actually performed

Read-only git identity/status/log, ref and tree resolution, retained-tree
equality, delta statistics and full delta inspection; `git diff --check`;
strict UTF-8/BOM/LF byte checks; source reads at `fdea1c6` of `exec/mod.rs`,
`exec/unix.rs`, `exec/cgroup.rs`, `service/executor.rs`,
`service/unix_bridge.rs` and `service/application.rs`, plus a construction
call-site inventory. Read AGENTS, the relevant KICKOFF/CONTRIBUTING rules,
the review skill and profile, the correction-1 recheck and the correction-2
disposition.

Before commit, this report was checked for strict UTF-8 without BOM,
LF-only bytes, existing relative links, `git diff --cached --check`, and
staging limited to this file. No build, behavior test, mutation test, native
run or CI check was run. Every named future test (R1-R4, T18-T20, W3e) is a
requirement, not evidence.

No implementation, proposal or PROGRESS edit, and no private-state read. No
native, root, owner, account, service, cgroup, VM, Lab Manager or experiment
action. No CI tuning, retry, label, push or GitHub write. Production remains
MechanismUnavailable on every OS. Stage C PR53 and Stage D PR55 remain
unmerged per the supplied context; GitHub state was not refreshed.
Implementation waits for their merge and for contract acceptance. CI
investigations remain parked. Native, installed and dedicated paths remain
**unverified on real hardware**. M2 is incomplete. ExecutionFailed /
E_PROTECTION remain unresolved, with no waiver or retry.

Standards: 0 blocking (1 cosmetic nit). Spec: 0 blocking; P2/P6/P7 closed;
5 advisories. The most significant is A1, the errno-order clause that is
inaccurate for the `openat` branch.
