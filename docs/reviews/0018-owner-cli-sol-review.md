# ADR0018 owner CLI: independent Sol review

Reviewer: OpenAI / gpt-6.1-sol / high, 2026-11-12. One independent reviewer of
Opus-authored design; no subagents or implementation. **CHANGES REQUIRED.**
Standards: changes required (one test-plan blocker). Spec: changes required
(three contract inconsistencies). This does not accept or activate the design.

## Exact scope and evidence

- Prepared review branch: `t3/review-m2-cli-unix-bridge`, initially clean at
  `a9a38713ee16e905f894cff719813d68c84594a5`; this adds only review reports.
- Public-main reference: `79b7879e7330e2f8de408723f15ddfac399dd75f`.
- Target A base: `2f53332b4f4d3e1584f2efb80317856588576e3a`.
- Target A head: `ed208f27f01077a88993dc88feb8f01ce306b126` (the request's
  concatenated `head` + `ed208...`). Objects resolve locally. Reviewed the
  base-to-head diff and complete pinned ADR0018, owner-cli-wiring brief and
  PROGRESS delta, rather than substituting the prepared checkout.
- Range commits: `fcdaa35` ADR, `dd2e4f2` brief, `ed208f2` ledger.
- Origins: prepared AGENTS, CONTRIBUTING and KICKOFF M2/routing/privacy;
  ADR0009/0012/0014/0015; accepted ADR0016 and E1/E2 interfaces at
  `c4c010afac0fae085fa3f870f86a00b614c6a477` and
  `c73e5c7d2cc7df1f6084c25ebccb6f4434bca245`.
- Read source call sites: main and service binary tests, config/Resolver,
  owner_plan/configure_plan, Unix preflight/state checks, native state_guard,
  Windows guard/session/install signatures, and foreground privacy checks.
  Public PR36 production native execution is unavailable; unpublished ports
  are dependencies, not capabilities presumed present in main.

All proposal references below mean files at the Target A head. Source references
mean the stated source pin or prepared main where unchanged. Findings are source
counterexamples, not results of executed regression tests.

## Standards

**A-ST1 — blocking: the credential failure contract lacks meaningful regression
coverage.** AGENTS requires “Every behavior needs a meaningful test.” Brief §7
test 9 covers elevation, RNG, parent and target refusals, and success, but no
post-create failure. The proposed `create_with` injects only elevation and RNG;
neither exercises write/flush/handle-check failures. These are precisely the
branches that leave a credential file behind and choose the partial-result
diagnostic. Add an unprivileged fault seam or equivalent deterministic fixture
for every post-exclusive-create failure, asserting private retained bytes,
fixed `client-config-partial`, no hash/token output, and no rollback. Include
the Unix post-create `fchmod` failure from A-SP2. No real owner invocation is
needed. Kill mutations that misclassify partial failure as ordinary I/O or
success and that delete the retained file.

The conventional docs-only commits, proposed status, provider ledger and hardware
limits meet repository standards. Fowler smell judgement: the thin CLI façade is
justified by the authority boundaries; do not remove it as a Middle Man. Repeated
platform/error matches are deliberate exhaustive fences. No separate smell
finding warrants a redesign.

## Spec

**A-SP1 — blocking: hash before clearing the token, with one unambiguous order.**
Brief §3 S2 step 9 says “Overwrite the token and JSON buffers with zeros, then
return `hex(sha256(token_ascii))`.” ADR0009 Owner subcommands requires the
“token's SHA-256”; ADR0018 decision 7 promises the principal/hash configure
input. If `token_ascii` is among the cleared token buffers, the command prints
the hash of zeros instead of the credential it wrote. If it is a separate live
buffer, the clearing claim does not cover that copy. Compute and retain only
the non-secret digest before clearing all token representations and JSON, then
return that digest. Test with deterministic nonzero RNG bytes and compare the
returned hash with SHA-256 of the actual file's token text. A clear-before-hash
mutation must fail. This settles sequencing, not a new crypto mechanism.

**A-SP2 — blocking: the exclusive-create boundary and partial errors disagree.**
ADR0018 decision 7 says “A failure after the exclusive create reports
`client-config-partial`.” Brief §3 step 6 creates the file and then performs
Unix `fchmod(0600)`, but step 8 assigns `Partial` only to failures in steps 7–8.
A successful `openat` followed by failed `fchmod` already leaves a file and
falls outside that specified mapping. Define successful exclusive creation as
the boundary: every subsequent failure, including permission-setting, write,
flush and verification, returns `Partial` and leaves the private file. Keep
exclusive creation private from the first byte and never repair an existing
object. Extend the A-ST1 fault matrix across that boundary.

**A-SP3 — blocking: full replacement claims conflict with old-config validation.**
ADR0018 decision 4 says “The previous bytes serve only as the compare-and-replace
expectation and for the build-pin check.” Brief §4 instead runs old
`ServiceConfig::parse` on both platforms, then `configure_plan`.
At the Target A base, `ServiceConfig::parse` invokes execution/account and
`workspace_safe` validation, and `configure_plan` calls `old.revalidate` again.
Counterexample: same pinned account, old workspace removed, new valid workspace
provided in a full replacement. Parsing the old config refuses before the new
config can replace it. This is inherited library behaviour, not a newly found
authority bypass. Smallest correction: narrow ADR0018's claim to explicitly
retain old-config revalidation, document this refusal and its manual owner
remedy, and test it with a counting resolver. If replacement despite stale old
execution facts is intended, that requires an explicit accepted API/ADR change;
do not silently weaken `configure_plan` during CLI wiring.

## Satisfied constraints and nonblocking notes

- All six Windows service verbs use OwnerGuard. The stronger start/stop/status
  rule is explicitly labelled as an ADR0012 deviation. Configure uses the
  accepted E1 session rather than constructing store/right authority; install
  uses E2 registration-last provisioning; uninstall consumes SCM proof before
  session/recovery/right access. Unix uninstall uses the config-free owner plan.
- Routing precedes Tokio, malformed owner-prefixed argv fails usage before
  native resolution, and foreground routes remain unchanged. Counts, UTF-8,
  flags, semantic-versus-syntactic error separation and exhaustive fixed exit
  mapping are sufficiently specified. Code-4 Permissions/OwnerRequired is
  preserved independently of whether correction `7781239` is merged.
- Start uses inventory parsing and state checks, without privileged workspace
  inspection or build-account lookup. Drop preflight/image handles before SCM
  start. Running Windows state sharing and admin access to SYSTEM-private
  objects are explicitly unknown and must fail closed; no success is inferred.
- Full replacement uses serde and fixed limits 1 / 8 GiB / 512 pids. No new
  product approval, public capability, MCP owner entry, account creation,
  password fallback or S4U retry appears. Manual build-pin/config deletion is
  an owner operation, never agent authority.
- Client creation has useful fences: unelevated current user, normalized
  absolute path, no-follow ancestor walk, pinned parent, immediate-parent-only
  creation, explicit private creation, exclusive target, handle verification,
  OS RNG, fixed endpoint/server identity, no secret output or argv. Implement
  Unix mutations relative to retained descriptors and Windows path operations
  with all ancestor pins held; checking first and reopening an unpinned full
  path would not satisfy this contract. No filesystem behaviour was exercised.
- Advisory A-A1: ADR0018's “Because the hash was never printed, that token is
  unusable” overstates the partial-file property. An owner who can read a
  complete retained file can compute its hash. Say that the command supplied no
  registration hash and does not claim a usable completed credential; retain
  the private-file/no-rollback remedy. This is not an authorization fence.
- Advisory A-A2: brief §3 says the `security::private` rule allows “no other ACE
  type,” whereas the current Windows helper also permits deny ACEs. Explicitly
  choose the stricter allow-only rule or match that helper's deny handling;
  never use its path-based implementation instead of handle validation.

## Implementation handoff, gates and limits

Before wiring any owner dispatch, replace the existing binary-test
`["service", "install"]` with `["service", "install", "extra"]` and the new
usage expectation, as the brief requires. Make this prerequisite effective
before any intermediate test run: an elevated hosted Windows test must never
execute a valid owner verb. Retain unmanaged `service run` regression coverage.
Unit tests must prove every spawned usage-test vector is parser-rejected; no
test may construct NativeOps. Portable fake seams cover composition, exact
outputs, error variants, changed pins, no-config uninstall and start ordering.

Implementation remains gated on accepted design, hosted-green prerequisite
libraries, queue drain and independent review. Proposed tests and mutations
above were **not run**. This review ran only Git/text reads and wrote the two
reports; no cargo/build/test, native probe, resolver, owner, service, SCM/LSA,
privilege, account, pipe, VM, Lab Manager or private-state action occurred. No
GitHub operation occurred. Reports are reversible documentation; future client
file creation and lifecycle mutations are not undone by reverting code and must
retain their explicit partial-state semantics. Installed/native/dedicated paths
are **unverified on real hardware**. No M2 acceptance or merge is granted.

Totals: Standards 1 blocker (partial-failure test plan); Spec 3 blockers (hash
order, partial boundary, old-config contract) and 2 advisories.
