# Ubuntu 26 mirror compatibility correction: design proposal

Anthropic / claude-opus-5-5 / high, design sub-agent, 2026-11-12. Design only.
**Fresh independent OpenAI Sol high review is required before any
implementation.** This document is not acceptance. It contains no source,
workflow or policy change, push, CI run, rerun, label or GitHub write.

## Pins and scope

- Base `8c7ad0ebc6892b1d108d920e91cd5a2f2ca12df4`. The isolated runtime worktree
  was clean at start and is not the canonical checkout. No subagents were used.
- Helper `tools/ci/ubuntu_mirror.py` SHA-256
  `a945b3d812f1fc47fa43ac696e3ec52026b657867b3f6c0d4b274a5d409b80ff`. This
  matches `bd707ae` and the observer pin. `git diff 1604db1 8c7ad0e -- tools/ci
  .github` is empty, so main's helper and workflow are the observed ones.
- Read: AGENTS.md; the diagnostic result and its sanitized JSON; the historical
  diagnosis (`bb777de`), observer source and decoder vocabulary (`b1c7a4d`),
  and the correction evidence (`bd707ae`); `tools/ci/ubuntu-mirror.md`; helper
  tests; `scripts/ci-budget.mjs`, `scripts/evaluate-budget.mjs`,
  `scripts/policy.mjs`; and the headless-shell candidate record at `344c406`.

## What the observation proves, and what it does not

Decoding the retained JSON with the pinned version-2 vocabulary (sorted stage
and predicate names) gives this sequence. All context and parent checks pass.
Three source files are enumerated and read: a 141-byte `.list`, a 3004-byte
`.sources` and the 270-byte legacy `sources.list`. Each passes the native file
guards and parses with `indirect=True`. The mirror list then passes its native
guards at **152 bytes**. `rewrite_mirror_list` raises at line 169 with
host-allowed=false, host-ubuntu=true, host-kernel=false, scheme-allowed=true,
path-allowed=true, netloc-host=true, query=false and fragment=false.

Facts that follow directly:

1. The first refused mirror-list entry has a lowercase hostname. It has no port
   or userinfo, because `netloc == hostname`.
2. That hostname ends in `.ubuntu.com` and is not one of the three `HOSTS`.
   It is not the Kernel mirror, so the refusal is not a second application of
   our own helper.
3. Its scheme is `http` or `https`. Its path is exactly `/ubuntu` or `/ubuntu/`.
   This rules out `ports.ubuntu.com/ubuntu-ports`, snapshot paths and ESM paths.
4. Every entry before it passed all existing checks. **Entries after it were
   never evaluated.** No recheck or publication stage was reached.
5. No source file has an active off-allowlist `*.ubuntu.com` URI, because
   `replacement()` line 35 would have refused during source parsing first.
6. The size changed: 152 bytes against the 144-byte list in the public
   provisioning script and in the earlier passing run. The earlier run fetched
   a 195-byte list after the helper ran, which is 144 + 51 for the Kernel line.

What it does not prove: the exact hostname, its entry index, the other entries,
the bytes of #54's attempt-2 input, the runner's cloud for this job, or that a
correction would pass recheck and publication. This was one invocation on image
`20261111.82.6`. It is not a statement about future images.

## Public provisioning evidence

Only official `actions/runner-images` objects and Canonical's `cloud-init`
repository were read. The requests were anonymous, read-only fetches of public
objects, with a generic client string and no repository or private values.

- `actions/runner-images` main `62b20e715386d6a54c8b9cad40e10a36039db4f2`,
  `images/ubuntu/scripts/build/configure-apt-sources.sh` (SHA-256
  `5ef119f8fec3dfac66f0a8e2c7896b3b1a0fcd1a02c4ab70acec51993eb78f45`),
  lines 11–13, appends exactly `http://azure.archive.ubuntu.com/ubuntu/`
  priority 1, `https://archive.ubuntu.com/ubuntu/` priority 2 and
  `https://security.ubuntu.com/ubuntu/` priority 3. That is 144 bytes.
- Tag `ubuntu26/20261111.196` (`93a4e4e9…`), built the same day as the failing
  revision, has a byte-identical script (same SHA-256). The earlier passing tag
  `ubuntu26/20261101.186` (`0d1e1dc…`) wrote the same three entries, per the
  retained diagnosis.
- `9b75b60` (2026-11-11, upstream #15295) widened only the `ubuntu.sources` sed
  substitution and added a `cloud-init status --wait`. It did not change the
  mirror-list entries.
- Tag `ubuntu26/20261111.82` is still absent from the public tag list (checked
  with `git ls-remote --tags` on 2026-11-12). The raw script at that ref
  returned 404. **The exact image's provisioning source remains unavailable.**
- `images/ubuntu/scripts/tests/Apt.Tests.ps1` asserts only that the sources use
  `mirror+file:` and that priorities are unique and more than one. It does not
  pin hosts, so the image's own tests would accept a changed host.
- `canonical/cloud-init` main `30b3ea21ec86269a2e31ae0028fbb4a7cc5b41a1`,
  `config/cloud.cfg.tmpl` (SHA-256 `68a4925a…48c2`), lines 348–358. Its Ubuntu
  amd64 search templates are `http://%(ec2_region)s.ec2.archive.ubuntu.com/ubuntu/`,
  `http://%(availability_zone)s.clouds.archive.ubuntu.com/ubuntu/` and
  `http://%(region)s.clouds.archive.ubuntu.com/ubuntu/`. This documents that
  region-qualified `*.ubuntu.com` hosts with an `/ubuntu/` path are a normal
  first-boot product. It does not show that any such name is in our list.

Conclusion: no public primary source explains a 152-byte runtime list with an
off-allowlist host. Whatever wrote it is runtime or unpublished provisioning (inference only, not an exhaustive causal finding).
Third-party issue comments in upstream runner-images (#15296 and #15297, about
the 2026-11-11 Canonical DNS outage) are not used as evidence.

## Falsifiable possibilities (ranked; no cause claimed)

| # | Possibility | Support | Falsifier (privacy-preserving) |
| --- | --- | --- | --- |
| H1 | First-boot or provisioner logic on some capacity replaces an entry with a cloud-region Ubuntu mirror, in a cloud-init template family | #54's failing job header said AWS and the passing job said Azure (retained diagnosis). The observed shape equals the template shape. The +8 bytes is *consistent with* one 9-character EC2 region replacing `azure.` (not evidence of it) | Booleans for membership in each cloud-init template regex. Also read the diagnostic job's own header cloud from already-public log metadata, which needs no hosted run |
| H2 | An official country mirror, `<cc>.archive.ubuntu.com` | Path `/ubuntu` fits | Boolean `^[a-z]{2}\.archive\.ubuntu\.com$` |
| H3 | The unpublished `20261111.82` build differs from the public same-day script | Tag absent; `.196` is identical to main | Inspect the tag if it is published. No run is needed |
| H4 | An earlier step in our workflow edits the list | Weak: the preceding steps are read-only observers, Rust/Node and lint. host-kernel=false rules out our helper | Source inspection: no step before line 62 of `ci.yml` writes `/etc/apt` |
| H5 | Another Canonical host with an `/ubuntu` path (for example `old-releases`) | Weak | Exact boolean for that fixed public name |

The correction below does **not** depend on which of these holds. It depends
only on facts 1–5.

## Pure feedback loop (representative, not actual bytes)

Fixtures use the synthetic label `fixture-unlisted.ubuntu.com`. It is
deliberately not a guess at the hidden host, and nothing resolves or contacts
it. Each fixture is the public 3-entry list with one entry replaced or added:

- `pos1-http`: the synthetic host replaces the priority-1 Azure entry.
- `pos2-https-noslash`: `https://…/ubuntu` replaces the archive entry.
- `untagged-last`: an untagged entry is appended after the three public entries.

I ran these locally against the pinned helper with `python -I -B`. Only
`rewrite_mirror_list` was called: no CLI, context, `_apply`, filesystem write
or network access. Script:
`C:\Users\DYLANF~1\AppData\Local\Temp\claude\d7482bff6c6c81a4\7c6d6c50-461b-4550-ad61-37a117750151\scratchpad\loop\red_fixture.py`.

```text
helper sha256 a945b3d812f1fc47fa43ac696e3ec52026b657867b3f6c0d4b274a5d409b80ff
pos1-http rewrite_mirror_list 169 (False, True, False, True, True, True, False, False)
pos2-https-noslash rewrite_mirror_list 169 (False, True, False, True, True, True, False, False)
untagged-last rewrite_mirror_list 169 (False, True, False, True, True, True, False, False)
canonical 144: 195
python -I -B -m unittest discover -s tools/ci -p test_ubuntu_mirror.py -> Ran 23 tests, OK
```

All three fixtures reproduce the observed site and the exact 8-fact tuple, so
the pure loop is red-capable for the observed predicate. It is
representative: it cannot reproduce entries that were never evaluated, the
real hostname or native metadata.

## Proposed correction: omit, never trust, the observed entry shape

The change is confined to `rewrite_mirror_list` and a fixed count on stdout.
`HOSTS`, `MIRROR`, `KEYRING`, `trust()`, `replacement()`, `rewrite_list`,
`rewrite_sources`, `read_source`, `_apply` planning, recheck and publication,
`runner_context` and argv handling are unchanged.

**Rule.** An active mirror-list entry is *omitted* (its line is not written)
only if **all** of these hold. This is the observed parsed predicate class,
further restricted by the proposed raw-token grammar; that extra grammar
was not established for the actual uncollected input:

1. Before `urlsplit`, use `re.fullmatch` on the entire raw token (never
   search, prefix matching or a normalized copy). It must match lowercase
   ASCII `http://` or
   `https://`, followed by the hostname grammar below and exactly `/ubuntu`
   or `/ubuntu/`. Reject C0/DEL controls, all whitespace, non-ASCII bytes,
   any literal `?` or `#` (including empty delimiters), userinfo, ports,
   percent escapes and backslashes. Uppercase scheme is refused, not
   normalized. Then `urlsplit(token)` must succeed; ValueError refuses.
2. `scheme in ("http", "https")`.
3. `hostname` is not None, `netloc == hostname`, and `len(hostname) <= 253`.
4. The hostname fully matches
   `(?:[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.)+ubuntu\.com`. That is ASCII LDH
   labels, at least one subdomain label, and no trailing dot. Bare
   `ubuntu.com`, IDNA or non-ASCII names, IP literals and look-alike suffixes
   do not match. Explicitly reject every hostname label starting `xn--`;
   an ASCII A-label must not evade the stated IDNA exclusion.
5. The hostname is not in `HOSTS` and is not `mirrors.edge.kernel.org`.
6. `path in ("/ubuntu", "/ubuntu/")`, with no query and no fragment.
7. The token count and optional `priority:N` tag pass the existing line-166
   grammar, which is checked first and unchanged.

Every other entry follows today's code. HOSTS entries are kept with priority+1.
Anything else refuses, including foreign hosts, the Kernel host,
`mirror+file:` and other non-HTTP(S) schemes, and wrong path, port, userinfo,
query or fragment.

**Bounds.** Retained HOSTS entries must number 1–16. Omitted entries are
counted against the same cap (retained + omitted ≤ 16). Zero retained entries
refuses. That keeps an Ubuntu fallback behind the Kernel entry and so preserves
the protection against the Kernel-only failure seen in job 130963746682. The
4 KiB input and output bounds are unchanged.

**Evidence token.** On success, `main` also prints exactly
`ubuntu-mirror-omitted:<n>`, with n from 0 to 15. Nothing else is printed:
no host, line or index. This lets a later hosted run show whether the
predicate recurred and was handled (n ≥ 1) or did not occur (n = 0, path not
exercised). Smallest interface: add a pure `_rewrite_mirror_list_counted(data)
-> (bytes, int)`. Keep `rewrite_mirror_list(data)` returning bytes so the
existing tests stay valid, and have `_apply` carry the count. Updating the one
`_apply` return-value assertion is expected.

### Why this preserves trust

- **The contact set only shrinks.** In indirect mode APT reaches Ubuntu only
  through list entries. After the change the list holds the Kernel mirror plus
  validated `HOSTS` entries. The off-allowlist host is removed, not added,
  not preserved and not queried. No allowlist, wildcard or broad `*.ubuntu.com`
  trust is introduced.
- **Integrity checks are unchanged.** There is no new root path trust, TLS
  change, keyring, `Signed-By`, `trusted`/`allow-insecure` option or signature
  waiver. APT still verifies InRelease signatures against the Ubuntu archive
  keyring, and `Check-Valid-Until` behavior is unchanged.
- **Allowlist origin is precise.** Retained hosts are the three entries in
  public runner-images `configure-apt-sources.sh` lines 11–13, pinned above.
  The Kernel mirror's origin is unchanged (`tools/ci/ubuntu-mirror.md`).
- **The widened input is restricted beyond the observed parsed tuple.** The only inputs that
  newly pass are entries matching all eight observed facts plus strict label
  grammar. Those inputs are discarded and do not influence the output.
- **Tradeoff, stated plainly.** This replaces "refuse an unrecognized list" with
  "drop exactly this unrecognized shape". If the dropped host was the only one
  reachable on some capacity, APT fails visibly in the install step. That is an
  availability risk, never a silent trust loss.

### Plan, fallback and idempotency behavior

| Input | Output (representative) | Documented APT order |
| --- | --- | --- |
| Public 144-byte list | Byte-identical to today's 195-byte `FALLBACK_MIRRORS`; omitted 0 | Kernel, Azure, archive, security |
| `pos1-http` | Kernel p1, archive p3, security p4; omitted 1 | Kernel, archive, security |
| `pos2-https-noslash` | Kernel p1, Azure p2, security p4; omitted 1 | Kernel, Azure, security |
| `untagged-last` | Kernel p1, Azure p2, archive p3, security p4; omitted 1 | Kernel, Azure, archive, security |
| Only synthetic entries | Refusal (no Ubuntu fallback) | none |
| Corrected output fed back | Refusal at the Kernel host (unchanged double-run guard) | none |

Omission cannot create a priority tie that was not already in the input,
because the +1 shift is preserved. The whole plan is still built before any
publication. Source files are unaffected: the observed sources passed, and an
off-allowlist host in a direct source still refuses at line 35.

## Required tests (owned temp and pure only)

Add these to `tools/ci/test_ubuntu_mirror.py`. Leave the existing 23 tests
unchanged except the single `_apply` return assertion.

1. **Red then green.** The three representative fixtures refuse on the pinned
   helper (already shown above) and pass after the change. Expected bytes are
   exact, the synthetic host is absent from the output, and the omitted count
   is 1.
2. **Unchanged canonical output.** The 144-byte list still produces exactly
   `FALLBACK_MIRRORS`, with omitted count 0.
3. **Shape fence.** Each single-predicate deviation still refuses: `ftp`,
   userinfo, port, uppercase host (netloc differs from hostname), `/other`,
   `/ubuntu-ports`, query, fragment, bare `ubuntu.com`, `x.ubuntu.com.`,
   `ubuntu.com.evil.invalid`, an underscore label, a 64-character label, a
   254-character host, and `foreign.invalid`. Also refuse leading C0/DEL,
   uppercase scheme, empty `?`/`#` delimiters, percent escapes, backslashes,
   ASCII `xn--` labels, and a non-ASCII (UTF-8) label under `.ubuntu.com`.
4. **Bounds.** All entries omitted refuses. 16 retained entries plus 1 omitted
   refuses. One retained + 15 omitted succeeds; one retained + 16 omitted
   refuses. Multiple omissions count independently. A comment-padded list
   over 4 KiB still refuses.
5. **Idempotency.** Rewriting the corrected output again refuses, and the
   second-run refusal leaves the bytes unchanged under `_apply`.
6. **Order.** `apt_mirror_order` gives Kernel first, then the retained entries
   with unique priorities for those fixtures only. Existing ties and untagged
   entries remain accepted, byte-preserved except the existing +1 shift; no
   universal uniqueness claim or new tie rejection is introduced.
7. **Full plan.** `_apply` on an owned temp tree with an indirect source and the
   `pos1-http` list publishes the list only. The source is unchanged, two list
   reads happen (plan and recheck), no stage files remain, and the synthetic
   host is absent.
8. **Sources unchanged.** A direct `.list` or `.sources` with the synthetic
   host still refuses at `replacement` line 35.
9. **Unknown tail and full-plan refusal.** After an allowed entry and an
   omittable entry, append each foreign, malformed or Kernel entry. Every
   case refuses the entire plan, with byte-identical targets and no staged
   publication, including a direct source that would otherwise change.
   Kernel anywhere and duplicate Kernel refuse. A malformed priority tag
   on an otherwise omittable entry refuses. Assert retained order and exact
   bytes; omitted targets never appear. Add early-success, unvalidated-tail
   and omitted-counted-as-retained mutants.
10. **Output.** `main` prints only `ubuntu-mirror-updated:N` and
   `ubuntu-mirror-omitted:M`. Injected errors still print only
   `ubuntu-mirror-refused`.

**Mutations the implementer must run and restore** (each must fail at least one
test):

- Remove the omission branch.
- Drop the `.ubuntu.com` grammar (omit any host).
- Three raw-layer-only mutants must fail their named shape-fence cases:
  accept an uppercase scheme; allow a C0/DEL prefix; replace fullmatch with
  prefix matching so a trailing empty query or fragment is accepted.
- For port, userinfo, another path, label/host length, `xn--` labels and
  non-ASCII labels, use one paired mutant per behavior: loosen that raw
  clause together with every matching later guard for that same predicate.
  The named shape-fence case must catch the resulting behavior change.
  These temporary paired edits do not authorize removing production backup
  checks. If any independent remaining guard still makes a single-layer
  mutant equivalent, record it as equivalent with no kill credit. This
  exemption is symmetric: neither a raw-only nor a parsed-only edit must
  fail when the other layer still rejects the input. Keep both layers in
  production; never delete a guard to manufacture a mutation kill.
- Write the omitted line back.
- Allow zero retained entries.
- Exclude omitted entries from the cap of 16.
- Treat the Kernel host as omittable.
- Apply omission in `replacement()` for sources.
- Print the host in the token.

## Counter ledger

| Counter | Value | This proposal |
| --- | --- | --- |
| Performance attempts (#54) | 1 failed (`43248592153`, Windows install threshold); 2 failed (`43270363385`, Ubuntu refusal); **3 unused** | consumes none |
| Hosted fetch-path helper invocations, failed | 2: #54 job `131364954122`; diagnostic job `131461334918` (diagnostic only) | adds none |
| Failed fixes for this predicate (AGENTS three-strikes) | **0 of 3**. Neither invocation was a correction | none |
| Thresholds and baseline | Numerical/performance A1-A4/A6 unchanged; A5 scope amendment requires owner decision; main 906 s | no reset |
| Local pure runs (this task) | 1 fixture script (3 cases, all red as expected); 1 helper suite run 23/23 OK | n/a |

## Sequencing and owner-decision boundary

Policy (`scripts/policy.mjs`, `scripts/ci-budget.mjs`) measures the first
attempt of the latest main **push** run, currently 906 s. A PR can pass
`ci-time-budget` only with `ci-speed` and its own successful first attempt on a
`pull_request` run faster than that. A standalone correction PR is not a speed
change, so it cannot honestly carry `ci-speed`. It also cannot merge while the
budget is red.

Delivery is **blocked pending an explicit prospective owner decision**.
The existing frozen A5 permits only `.github/workflows/ci.yml`,
`scripts/ci-workflow.test.mjs`, `crates/arx-guest/tests/cli.rs` and named docs
against `48d4916`. A separate commit cannot make extra paths satisfy A5.

The concrete proposed amendment adds exactly these paths to the existing A5
scope (already permitted A5 paths remain permitted):

- `tools/ci/ubuntu_mirror.py`
- `tools/ci/test_ubuntu_mirror.py`
- `tools/ci/ubuntu-mirror.md` (update the helper's behavioral contract and
  omitted-count output; do not knowingly leave it describing old behavior)
- `docs/adr/0021-ubuntu-mirror-list-omission.md`
- `docs/evidence/ubuntu26-mirror-compatibility-fix.md`
- `docs/reviews/ubuntu26-mirror-compatibility-proposal.md`
- `docs/reviews/ubuntu26-mirror-compatibility-sol-review.md`
- `docs/reviews/ubuntu26-mirror-proposal-correction-opus-recheck.md`
- `docs/reviews/ubuntu26-mirror-proposal-final-opus-recheck.md`
- `docs/reviews/ubuntu26-mirror-compatibility-implementation-opus-review.md`
- `docs/PROGRESS.md`

The owner must
also explicitly permit carrying this compatibility prerequisite as a
separate commit in existing PR54 despite the one-concern restriction.
No additional source path, guard waiver or performance-threshold change is
proposed. A1-A4/A6, attempt counts, <900s total, Windows compile <108s and
compile/Clippy <1.12, platform/test coverage and first-attempt measurement
all remain. A5 alone would prospectively gain those three non-docs paths and named docs;
it must not be called unchanged. The ADR is proposed pending the same owner
decision; no architecture deviation is silently accepted.

If either permission is declined, stop this route and record the budget
owner-decision blocker; no new stack, false speed label, status override,
empty main push, rerun or manual-dispatch baseline substitution is available.
Original speed gains must still be independently measured on the final head.

Conserve the optional manual precheck: it is not required and cannot prove
future image/tail behavior. No cache-isolation or zero timing-effect claim
is made. Before any hosted trial, resolve scope authorization and obtain
independent source review with pure complete-plan tests/mutations. The
remaining attempt 3 is the final candidate's first pull_request run, never
a manual precheck. If an owner later separately approves a precheck, it is
fetch-path hosted invocation 3 and correction trial 1, never a performance
attempt. A fetch failure during performance attempt 3 counts in both the
performance and fetch ledgers. No hosted execution is authorized by this
proposal.

## Missing evidence and the minimal observation (not requested now)

The correction above needs no exact hostname. A *precise allowlist addition*
(the alternative rejected here) would need public-pattern membership evidence,
which does not exist yet. If it is ever wanted, the smallest privacy-preserving
observation is:

- Fixed booleans per active entry, evaluated past the first refusal: in
  `HOSTS`; matches each cloud-init template regex (EC2, clouds-AZ,
  clouds-region); matches `^[a-z]{2}\.archive\.ubuntu\.com$`; matches the
  correction's omission rule.
- The entry index and active-entry count.
- The job header cloud: AWS or Azure.

None of these is a hostname, byte, region or path value. The header cloud of
diagnostic job `131461334918` is already in public log metadata. Reading it is
a zero-cost, read-only falsifier for H1 that needs no hosted run. **No
additional hosted run is proposed now.**

## Retained boundaries

Native, installed and dedicated paths remain **unverified on real hardware**.
M2 is incomplete. ExecutionFailed and E_PROTECTION remain separate and unknown,
with no retry. Local E1 and E5 stay held. No PROGRESS, source, workflow or
policy edit; no push, CI, rerun, label or GitHub write; no private state,
owner/admin, account, service, cgroup, VM or experiment. The babysitter was not
touched.

## Sol correction checkpoint

Coordinator OpenAI / gpt-6.1-sol / medium, 2026-11-12. This correction addresses
S1/SP1-SP3 from [the independent review](ubuntu26-mirror-compatibility-sol-review.md).
Exact host and the unchecked tail remain UNKNOWN. Synthetic fixtures are only
representative. Public objects inspected do not explain the input; they do
not exhaust possible causes. The owner scope decision is pending, not granted.
No implementation, build, behavior test or hosted trial ran in this correction.

## Final bounded text correction

Coordinator OpenAI / gpt-6.1-sol / medium, 2026-11-12: R1-R4 from the
[Opus correction recheck](ubuntu26-mirror-proposal-correction-opus-recheck.md)
are addressed prospectively. The earlier async owner question omitted the
helper contract doc and is superseded; it grants no permission. The full
path list above and proposed ADR define the corrected decision. Raw-layer
mutants replace impossible redundant parsed-check kills. The precheck ledger
rule is restored. No build, behavior test, implementation or hosted run.

## Owner disposition - 2026-11-12

The owner explicitly approved both corrected limited exceptions: the exact
A5 path expansion in the proposal and carrying the compatibility prerequisite
as a separate commit in existing PR54. This supersedes the historical pending
scope/bundling status above. Coordinator accepts the corrected technical
contract, including the explicit non-ASCII shape-fence case and paired-mutant
requirements. Independent source review is still required before push. All
speed thresholds, platform/test coverage and failed-attempt counts remain
unchanged. Performance attempt 3 is unused; no optional manual precheck is
planned. No additional native, owner, experiment or activation authority.
