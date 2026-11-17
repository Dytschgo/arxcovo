# Founder hosted observation: independent actual-source review

Reviewer: OpenAI / Sol 6.1 / high, 2026-11-18; isolated worktree, no children.
Independent of root builder. Scope: publication readiness for one diagnostic PR.
Base (failed main): 09134085dce93742d8812821dbe11834dcee7a9c.
Candidate: 144f43ed9f18ca8745ad4bfd4a04bc2ea49ee451; initially clean.
Base is the merge base. Reviewed all eight changed files and eight commits,
including retained disposition, plan, patch, safety review, result and PROGRESS.
Read AGENTS/KICKOFF, CONTRIBUTING, review/debug skills and active profiles,
ADR0005, ADR0023/24/25 and supplied raw hosted failed.log. No runtime diagnosis.
Task-specific static-only scope supersedes debug reproduction/fix loops.

## Standards

PASS for bounded publication preparation; no blocking or substantive smell finding.
Only found.rs and one F6 call change within crates. All other 128 of 130 tracked
crate files are byte-identical to failed main. No CI, tooling, dependency, lock,
profile, runtime, timeout, retry, quarantine or test-assertion change was found.
Current owner Sol routing overrides CONTRIBUTING's historical Opus requirement.
The conventional diagnostic commit is one concern; no architectural deviation.

Actually ran: git diff --check base..candidate (PASS); git merge-base identity;
in-memory application of every retained patch hunk with exact context checks;
lexical comparisons preserving literals, allowing whitespace and trailing-comma
formatting differences; seven production macro substitutions; F6 call reversal;
all tracked crate blob comparisons; frozen control comparisons; observer static
structure assertions; supplied hosted log size/hash verification. No Rust tool,
build, test, engine, installation, network or public write ran in this review.

Retained patch SHA256:
6fb49eba004936204ec45fb113f0f89b640fb2625686546222a6c67549b7d3cd
Reconstruction from failed-main blobs yields the previously compiled source pins:
found.rs SHA256 d6596f0b2371bd4a854d2d7213d87d20d4c03f63d994575217cbe22749f73013
found_tests.rs SHA256 6c601f548eed3e02c43adc48a0bdec18f958d28e70d6a334a39ff2a4874408a5
Candidate git blobs:
found.rs d564c89058e4e71e979d0a7161a7c872ff932aee
found_tests.rs 810ac3a254785853b3e8a9e4adfa8d35b37b17c7
This establishes formatting equivalence, not exact formatted-source execution.

Advisory: candidate compilation/lint/runtime checks remain pending normal CI.
The result document attributes one earlier compiled F6 invocation to Sol6.1/high
(result25aad48a; safety review b6a28f1a): exit0, 14 events, overflow=false,
all six phase exits and final exit Ok, final elapsed35.415ms. I did not rerun or
independently inspect its private runtime artifacts; reconstruction matches its
recorded applied-source hashes. This is reused evidence, not this review's run.

## Spec

PASS for the specified diagnostic seam; no blocking finding.
Removing cfg(test) observer/definitions and substituting the six found_phase
and one found_final production expansions recovers the full original found.rs
token sequence exactly. Non-test macros evaluate only their original work;
no observer arguments execute. Reversing the sole capture call restores the
entire found_tests.rs byte for byte, including all assertions and budgets.

Test macros retain the original Result and its existing outer ? propagation.
Mapped errors remain unchanged. No extra async phase helper or scheduling seam
was introduced. The FoundMembers inner ? still exits the original work block;
it can bypass proposal exit, so unmatched entry need not mean pending await.
All four SignerClock calls, both fresh guards, signing, late certificate discard,
original timeout_at match and final Instant::now() < deadline stay ordered.
The original 5s issue and 20s whole-test budgets and four-worker runtime remain.

Owners, read/issuance permits, work/cancellation ownership and resource drop
order are preserved: observer borrows are synchronous; original results move
back to their callers; no original resource is retained for emission. capture
awaits the issue helper inline. found's timeout work and issue permit have left
scope before formatting/output. Owners workers may still have their original
independent lifetimes; the trace does not report worker completion. No added
spawn, wait, yield, sleep, global lock, retry or cancellation fallback exists.

Only F6's initial issue installs task-local capture. No RefCell borrow crosses
await; task-local state follows scoped polls and is not inherited by spawned
Owners work. Other tests and later F6 operations do not collect events. Test
observer hooks still add synchronous overhead, including outside capture.

Advisory: monotonic samples and bounded buffer operations perturb timed work;
formatting/emission remains inside the existing whole-test budget. A final
observer sample is not the original final predicate sample. Pending identifies
a caller expression, not a causal storage/worker seam. Six Ok exits followed
by final Unavailable do not uniquely identify a cause. No fix is established.

## Security

PASS; no blocking finding. The array stores at most24 scalar/static events;
normal complete trace is14. Saturated length and overflow bit prevent growth;
overflow invalidates interpretation. RefCell state is scoped to the caller.
Emission uses only monotonic integer timings, fixed phase/edge/result/pending
labels, count and bool. Every FoundError is mapped to a fixed variant string;
BadCsr/Rejected payloads and underlying engine errors are never formatted.
No Debug of issuance, CSR/DER, keys, CA markers, proof/code, store contents,
paths, node IDs or pointers enter the buffer/output. Existing fail-closed
security guards and permissions are unchanged. No owner admin, VM, old Lab
Manager, secret or private owner-data operation was performed.

Advisory: panic/cancellation before capture returns may emit nothing. Admission
refusal can omit phase events. Passing nextest output can be hidden by existing
normal CI settings. Missing/overflowed output earns no localization or rerun;
no new output configuration is required for this bounded observation.

## Retained status and publication readiness

The supplied hosted log is48362 bytes, SHA256:
50706e4fd294eaf4552c0f8d4165b7a93895f1a9db5a2f52cecca2b2865319ff
It shows F6/F8 initial-issue Unavailable and F7's failed ClockRollback assertion;
F7's actual returned variant is not printed. Whole-test timings do not localize
an issuance phase. Earlier local greens have not reproduced this hosted red.

Retained preparation/grant/restoration wording is historical, resolved by the
plan's hosted-candidate addendum and PROGRESS's latest entry. Neither the consumed
local grant nor prior budget/quarantine exceptions authorize another execution,
fix or red-main merge. Main09134085 remains RED; PR176 held; M2 open; M3 unchecked;
Sol3/3 exhausted and historical Opus0/2 unchanged; **unverified on real hardware**.
PROGRESS was reviewed but not edited. Only this review document is committed.

Publication readiness: PASS for coordinator's one normal diagnostic PR push and
its first-attempt full normal CI jobs, retaining all gates. No custom dispatch,
rerun, fix, timeout/retry change, quarantine expansion or output-control change.
This report performs no publication and grants no merge onto red main. A green
observation is not hosted stability, cause, recovery, milestone or repair credit.
The diagnostic patch is reversible by removing the reviewed source diff;
retained evidence must survive. Coordinator alone handles publication and gates.
Standards: 0 blockers; Spec: 0 blockers; Security: 0 blockers. Advisories above.
