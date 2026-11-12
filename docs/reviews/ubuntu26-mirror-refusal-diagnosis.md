# Ubuntu mirror refusal: bounded diagnosis

OpenAI / gpt-6.1-sol / high, research sub-agent, 2026-11-12.

**Exact refusal predicate remains unknown. No compatibility fix is justified by
the available evidence.** The two jobs used different revisions of Ubuntu
26.04.1, not different major Ubuntu versions. Neither the failed runner's APT
file bytes nor its file metadata are exposed in the retrieved logs. The exact
new image's linked public source could not be retrieved. No failing-runner
fixture or reproduced red pure test is claimed.

## Pins and scope

- Runtime-prepared worktree: `t3-a3a606be-6e5d52c630ebd2a3`, branch
  `t3/diagnose-ubuntu26-mirror`. Initial pwd and Git top level matched this
  worktree, outside canonical `D:/Code/Arxcovo`; initial status was clean.
- Task base: `d5dbd1473ee2dac9d57c96e862bd36023686a901`.
- Published PR54 head: `344c406663ad08b5f0497ecee9eb67a4cbd7e896`.
- Actual main reference: `1604db1fbd686d3bf59e81f4d2702eaecc6b8bc0`.
- Helper SHA-256 at task base:
  `a945b3d812f1fc47fa43ac696e3ec52026b657867b3f6c0d4b274a5d409b80ff`.
  Git diff from published head to task base for `tools/ci` and CI workflow was
  empty. Investigation did not alter them.

Read AGENTS.md, complete KICKOFF.md, dytschgo-debug and its active profile,
helper/tests/docs, existing mirror reviews, scaffold CI-trust ADR, and
docs/evidence/ci-headless-shell.md. No subagents, implementation, PROGRESS edit,
checkout creation, canonical write, installation, production helper application,
APT command, root/VM/container, account/service/private-state operation or
GitHub mutation occurred. Existing tests use owned temporary trees and injected
facts; they do not establish native root or Unix permission behavior here.

## Primary failure evidence

Read-only Actions job-log API reads were made for jobs 131364954122 and
131287907596; the attempt-1 jobs API independently confirmed the failing head
and step conclusions. Only pertinent observations are retained here.

| Observation | Earlier successful Ubuntu job | Failing Ubuntu job |
| --- | --- | --- |
| Job | [131287907596](https://github.com/Dytschgo/arxcovo/actions/runs/43248592153/job/131287907596) | [131364954122](https://github.com/Dytschgo/arxcovo/actions/runs/43270363385/job/131364954122) |
| OS | Ubuntu 26.04.1 LTS | Ubuntu 26.04.1 LTS |
| Image | ubuntu-26.04 / 20261101.186.1 | ubuntu-26.04 / 20261111.82.6 |
| Header cloud | Azure | AWS |
| Mirror tests | 23 pass at 15:32:39 UTC | 23 pass at 17:33:17 UTC |
| Helper | updated:1 at 15:32:39.7781537 UTC | refused at 17:33:17.9149265 UTC; exit 1 |
| DNS observation | All six resolved | All six resolved |

The failing [attempt-1 API response](https://api.github.com/repos/Dytschgo/arxcovo/actions/runs/43270363385/attempts/1/jobs?per_page=10)
identifies the mirror step as **number 19**; 23 is the helper test count. The
subsequent Playwright installation, UI smoke and Linux Electron smoke were
skipped. The earlier job fetched a 195-byte mirror list after the helper ran;
that size alone does not establish the original file's bytes.

The helper does not perform DNS resolution or package acquisition. Its generic
error combines Refusal and OSError. The observed message therefore cannot
distinguish context, parsing, trust, filesystem-policy, stale-input or staging
failure. Successful unit tests do not identify the failed native predicate.
Cloud/image differences are observations, not established causes.

## Official provisioning comparison and retrieval limits

The earlier [official release](https://github.com/actions/runner-images/releases/tag/ubuntu26%2F20261101.186)
confirms Ubuntu 26.04.1 and image 20261101.186.1. The matching
[build template](https://api.github.com/repos/actions/runner-images/contents/images/ubuntu/templates/build.ubuntu-26_04.pkr.hcl?ref=ubuntu26%2F20261101.186)
uses the Azure source and schedules configure-apt-sources.sh before
configure-apt.sh. Its [source template](https://api.github.com/repos/actions/runner-images/contents/images/ubuntu/templates/source.ubuntu.pkr.hcl?ref=ubuntu26%2F20261101.186)
uses Azure marketplace source parameters.

The exact earlier [APT source provisioning script](https://api.github.com/repos/actions/runner-images/contents/images/ubuntu/scripts/build/configure-apt-sources.sh?ref=ubuntu26%2F20261101.186)
appends the three existing test-fixture mirror entries (Azure HTTP, archive
HTTPS, security HTTPS, priorities 1/2/3), substitutes the exact Azure URI in
ubuntu.sources with the fixed mirror+file URI, and copies the source file to
the cloud-init template. It depends on existing base-image source bytes; it
does not specify the complete runtime source file or ownership facts.
Appending also does not prove the file was initially empty.

The matching [APT configuration script](https://api.github.com/repos/actions/runner-images/contents/images/ubuntu/scripts/build/configure-apt.sh?ref=ubuntu26%2F20261101.186)
configures acquisition retries/timeouts, package-index behavior and other APT
settings. Those settings are not read by this helper and cannot by themselves
select one of its explicit refusal predicates. No provisioning script was run.

The failing header links [release ubuntu26/20261111.82](https://github.com/actions/runner-images/releases/tag/ubuntu26%2F20261111.82).
Web retrieval returned 404. Contents API requests at that exact ref for
[configure-apt-sources.sh](https://api.github.com/repos/actions/runner-images/contents/images/ubuntu/scripts/build/configure-apt-sources.sh?ref=ubuntu26%2F20261111.82),
[configure-apt.sh](https://api.github.com/repos/actions/runner-images/contents/images/ubuntu/scripts/build/configure-apt.sh?ref=ubuntu26%2F20261111.82),
[the build template](https://api.github.com/repos/actions/runner-images/contents/images/ubuntu/templates/build.ubuntu-26_04.pkr.hcl?ref=ubuntu26%2F20261111.82)
and [the included-software README](https://api.github.com/repos/actions/runner-images/contents/images/ubuntu/Ubuntu2604-Readme.md?ref=ubuntu26%2F20261111.82)
returned 404 with no commit found for the ref. Encoded release/tag endpoints
also encountered connector URL validation errors; those errors alone were
not treated as evidence of missing upstream source.

The public [release collection](https://api.github.com/repos/actions/runner-images/releases?per_page=10)
instead listed ubuntu26/20261111.196 at target commit
93a4e4e98715026b60fdf86893cf193b4be3bd47. It is a different tag from the
failed job's header and cannot substitute for it. Current main's
configure-apt-sources.sh also broadens the substitution to base archive and
security URIs; this is not proof of the failed revision's provisioning.
Consequently an exact old/new provisioning diff cannot be established here.

## Feedback loop and refusal inventory

Executed on this Windows task worktree:

```text
python -B -m unittest discover -s tools/ci -p 'test_ubuntu_mirror.py'
Ran 23 tests in 0.146s
OK
exit 0
```

This includes pure rewriting and owned-temp injected publication/refusal tests.
It confirms the pinned suite passes; it is **not** a red-capable reproduction
of this job. No actual failing input can be replayed from the public logs or
retrieved source. Debug Phase 1 therefore remains incomplete. Causal ranking,
instrumentation execution and fix phases are not entered. The requested
source audit below inventories existing branches without selecting a theory.

| Helper seam (task-base line) | Predicate or failure class needing evidence |
| --- | --- |
| runner_context / apply (140, 230) | Linux, effective root and four exact intent-field comparisons |
| _apply directory scan (184) | Both fixed parents are root-owned directories without group/other write; 1–32 eligible source files |
| read_source (127) | Open without following final symlink; regular/root-owned/single-link/private-write file; size and actual read at most 64 KiB; native OSError |
| rewrite / replacement (108, 23) | UTF-8, no NUL, size, recognized suffix; exact indirect URI; allowed host/scheme/netloc/path; no credentials, port, query or fragment |
| rewrite_list / trust (55, 43) | Valid active source line, suite/components, preserved safe trust options and exact explicit keyring |
| rewrite_sources (74) | One URI field; no mixed target/foreign URIs; supported single-line unique fields, required fields, keyring and trust checks |
| rewrite_mirror_list (148) | At most 4 KiB, valid UTF-8/no NUL; 1–16 permitted Ubuntu entries; only optional bounded priority tag; bounded output |
| _apply planning/recheck/publication (198–225) | Nonempty plan; same bytes/device/inode on fresh read; staging/write/mode/fsync/replace/cleanup OSError |

The runtime guard is intent only. Neither spoofing it locally nor relaxing
ownership or syntax checks would provide evidence about this hosted failure.
The error could occur before publication or during publication; no unchanged
runner-file or rollback guarantee can be inferred from the single token.

## Minimal next proposal: reviewed hosted read-only diagnostic

No compatibility patch is proposed until the actual predicate is captured.
Prepare the following as a separate reviewable diagnostic change; **review
before any CI run**, with no run authorized by this document:

1. On a disposable hosted Linux runner only, perform a read-only equivalent of
   the helper's context checks, fixed-root enumeration, guarded reads, pure
   rewriting and plan/recheck validation. Never call apply/_apply: _apply writes.
   Exclude mkstemp, chmod, fsync publication and replace entirely. No APT,
   network probe, resolver edit or repeated application. Retain the original
   failing helper and required CI outcome; diagnostics must not manufacture a
   successful mitigation or turn a refusal into a no-op success.
2. Emit fixed stage/predicate identifiers and bounded numeric/boolean facts:
   individual intent comparisons, directory-policy verdicts, file count,
   fixed path category/index, byte length, regular/root/single-link/private-write
   verdicts, format/trust/URI classification, plan count, recheck verdict and
   allowlisted errno. Do not emit raw exception text, traceback, environment,
   arbitrary filenames, source lines, URLs, keyring values or unrelated files.
   Unknown content receives a fixed unknown identifier and stays refused.
3. Preserve current 32-source/64-KiB/4-KiB/16-entry bounds, and additionally
   bound directory enumeration (including unrelated entries), total bytes,
   elapsed time and report size (suggested caps: 128 directory entries,
   2.2 MiB reads, 10 seconds, 16 KiB output). Use directory descriptors and
   no-follow opens for fixed parents/children, reject reparse/symlink paths,
   validate fstat facts and bounded reads, and perform fresh identity/byte
   comparisons without writing. Stop on unsafe input; never inspect another
   path to work around a failed guard.
4. For supported public Ubuntu syntax only, produce a canonical nonsecret
   structural fixture description from validated tokens, never a raw dump.
   Preserve any failing syntax shape only through an explicitly reviewed,
   value-redacting encoder. Unknown values stay redacted; if that loses the
   failure, record that a faithful pure fixture is still unavailable. Obtain
   the actual image revision's official public provisioning commit/templates
   when published. Metadata verdicts must accompany syntax evidence.
5. Test the diagnostic with owned-temp/injected reads: every refusal class,
   read bounds, symlink/unsafe-parent rejection, stale inputs, errno handling,
   redaction, and proof that no filesystem mutation or outbound request occurs.
   A read-only diagnostic cannot exercise a publication-only failure; if its
   entire dry plan passes, keep staging/publication as unresolved rather than
   invoking a write to identify it.

Only after that evidence identifies a compatibility issue may a narrowly
scoped proposal name the precise added accepted grammar/layout. A regression
fixture must fail through the pinned pure parser/owned-temp seam first, then
pass the proposed change. Preserve signature verification, Ubuntu keyring
selection, exact allowhosts/URIs, private root-owned regular-file policy,
no-follow/bounded reads, fresh re-read before write, Ubuntu fallback and refusal
of unknown inputs. Do not widen any of these merely to make CI pass.

Any source fix needs independent Opus review and prospective hosted acceptance
before publication/measurement. Freeze exact-head first-attempt criteria:
diagnosed predicate resolved, signature/package fetching and both smokes pass,
all platform coverage and required checks retained, existing job/budget limits,
plus the already agreed performance criteria if part of the performance PR.
A diagnostic-only run is not a successful performance measurement.

## Commands, boundaries and handback

Personally executed: Get-Location; git rev-parse --show-toplevel; git status
--short/--porcelain; git rev-parse HEAD and the published/main pins; git branch
--show-current; git log -1; git remote get-url origin; Get-Content/rg reads of
the named repository and skill files; git diff 344c406 HEAD for tools/ci and
the workflow; Get-FileHash for the helper; the focused unittest command above.
Read-only connector calls fetched the two job logs, attempt-1 jobs, and the
official repository resources linked above; web opened the two release URLs.
Initial guessed template filename was 404; directory listing resolved the
actual build.ubuntu-26_04.pkr.hcl filename before the matching reads.
Final document-only diff and whitespace checks are recorded in the handback.
No raw log archive or invented failing fixture was committed.

Failure remains performance attempt 2 of 3, nonqualifying on A1/A4 irrespective
of Windows. Diagnosis neither consumes attempt 3 nor resets attempts. Main's
906-second failed baseline remains authoritative; PR53 stays unlabelled and
blocked. Existing independent Opus measured review was not duplicated.
ExecutionFailed/E_PROTECTION remain unknown with no fix, waiver or retry.
M2 remains incomplete; native/installed/dedicated paths are **unverified on real
hardware**. No push, PR, label, merge, watch, rerun, dispatch, settings or
babysitter action was performed. Research completion returns this evidence to
the parent; the next needed data is the bounded hosted predicate report and
matching official source, under a separately reviewed diagnostic proposal.
