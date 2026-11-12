# Ubuntu mirror read-only diagnostic: independent review

Anthropic / Opus 5.5 / high. Independent Standards/Spec/security review, read only,
2026-11-12. Different provider from the source author (OpenAI gpt-6.1-sol high) and
the docs-redaction author (Sol medium). Single-agent review, no subagents.

**Verdict: CHANGES REQUESTED before any push or hosted execution.**
On safety the candidate passes: it adds no writer, makes no outbound request, keeps
its output closed and numeric, and keeps the failing job red. On usefulness it falls
short. Every shared-parser refusal comes out as one opaque `parser-unknown` event.
A content change between image revisions is the likeliest class of cause, and in that
case this run could not name the predicate, so a second CI trial would be needed.
Point 2 of the proposal explicitly asked for format/trust/URI classification. The
change needed to close the gap is small and privacy-safe (finding S1).

## Pins

- Worktree: runtime-prepared T3 worktree. `pwd` and `git rev-parse --show-toplevel`
  matched, the initial status was clean, and the path is not canonical `D:/Code/Arxcovo`.
- Target head: `88079771978a7e7a92812c352c801973222d5250`.
  Source/test: `c7524f6bc3ed4d61a00abbe0d1f0b1179a8aa4de`.
  Evidence: `9f73d87267f388d6965d614cd83d7a5c4a725351`.
  Base: `caf8e3abc8e31e7edfe5ff5c398cfbdc11c89ebf`.
  Actual main `1604db1fbd686d3bf59e81f4d2702eaecc6b8bc0` is an ancestor of the target.
- `caf8e3a..8807977` changes exactly five files: the workflow (+4), the collector
  (271 lines), the collector tests (340 lines), PROGRESS and the evidence file.
  `c7524f6..8807977` has no changes under `tools/` or `.github/`. Root `8807977`
  only redacts the account path and repairs two dashes from CP1252 to UTF-8.
- `ubuntu_mirror.py` and `test_ubuntu_mirror.py` are unchanged from the base.
  All five SHA-256 values in the evidence manifest match `git show c7524f6:<file>`.
  The helper hash is `a945b3d8…f1fc47fa43…80ff`, matching the diagnosis.

Read in full: AGENTS.md, KICKOFF.md, the review skill, the entire diagnosis including
all five proposal points, the evidence file, the collector, the collector tests, the
helper and the workflow.

Commands I ran: `pwd`/`rev-parse`/`status`/`log`/`diff --stat`/`merge-base --is-ancestor`/`diff --check`;
the manifest hashes; and the focused suite
`python -B -m unittest discover -s . -p 'test*ubuntu_mirror.py'` from `tools/ci`
(42 tests, OK, 1 skipped, the Unix-only one). I also ran one scratch probe outside
the repository. It loaded the pinned helper with `-I` and passed synthetic strings to
the pure `rewrite`/`rewrite_mirror_list` only.

I did not touch /etc, sudo, root, the network, GitHub, the babysitter, VMs, Lab
Manager, private state or ignored fixtures. Native Unix descriptor, timer and
permission semantics are **unverified locally** (Windows host).

## Standards

| # | Sev | Location | Finding |
| --- | --- | --- | --- |
| T1 | Medium | `.github/workflows/ci.yml:68-70`; evidence `:131-136`; PROGRESS current-state bullet 4 | **The collector's tests never run in CI.** Every other observer in this workflow runs its own `test_*.py` (lines 39, 44, 66); this one does not. The Unix-only `dir_fd` symlink test is therefore described as "pending hosted Linux", but no step will ever run it. The hosted run would also produce numeric output with no native check that `scandir(fd)`, `O_NONBLOCK` on a FIFO, `O_NOFOLLOW` or `fstat` behave the way the facade assumes. AGENTS says to "never claim a check that was not run", and that wording promises a check that is not wired. |
| T2 | Medium | `observe_ubuntu_mirror.py:240-267`, `:117-119` | AGENTS says "every behavior needs a meaningful test". `main()` has none: the always-return-1 invariant (central to keeping the job red), argument count → 2, `BaseException` → `internal/unknown`, and the close error. `DescriptorTree.entries` mapping category 1→`fds[2]` (apt) and 2→`fds[3]` (sources.list.d) has no test either; a swap would show up only on the hosted run. Both can be tested safely: patch `diag.context`/`diag.observe`/`diag.DescriptorTree`, force a non-Linux platform so no timer is installed, and use a facade `scandir`. |
| T3 | Low | `observe_ubuntu_mirror.py:259-261` | Alarm cleanup race. If SIGALRM fires after `finally` starts but before `setitimer(0)`, `Deadline` escapes `main`. Close and print are skipped, and Python writes a traceback to stderr (code lines and the workspace path; no runner data). The step stays red, so this is cosmetic. Fix: block SIGALRM with `signal.pthread_sigmask(SIG_BLOCK, {SIGALRM})` at the top of `finally`, then cancel. The handler is never restored, which is harmless at process exit. |
| T4 | Info | `observe_ubuntu_mirror.py:23`; evidence `:84` | `MAX_READS = 2_200_000` is 2.2 MB, not the 2.2 MiB the proposal suggested. That is tighter and documented. A worst case of 32×64 KiB plus rechecks would report `total-read` rather than a helper predicate; real runner files are tiny. |
| T5 | Info | `observe_ubuntu_mirror.py:200, 235` | `deb822` emits `False` as a format value. A later report-bound `Stop` would keep that row as if it were a refusal. With about 520 events at most against a 16 KiB budget this is unreachable in practice. |

Smell baseline: possible Mysterious Name on `facts` (the tuple of context booleans
versus the test helper `facts()` that builds stat facts). Nothing else is material.
`Report` and `DescriptorTree` are cohesive.

## Spec (bounded diagnosis, five-point proposal)

| # | Sev | Spec line | Finding |
| --- | --- | --- | --- |
| S1 | **High (blocks execution)** | Point 2: "format/trust/URI classification"; point 4 | **Parser refusals are not classified** (`observe_ubuntu_mirror.py:226-227`; evidence `:60-63`). My probe ran plausible synthetic inputs through the pinned pure functions. Each one gives the identical collector event `parser-unknown`, but the raise sites differ: a `.ubuntu.com` host outside the allowlist (EC2 regional or ports) → `replacement:35`; a keyring path other than the pinned one **and** a missing `Components` → `rewrite_sources:97`; an inline/continuation `Signed-By` → `rewrite_sources:93`; a foreign mirror-list host **and** an already-rewritten list that contains the Kernel entry → `rewrite_mirror_list:169`; 17 entries → `:177`. Metadata refusals are well served: each names its predicate with a category and sorted index. That is an adequate first narrowing for the policy branch, but only that branch. A parser-branch result would tell us "source file *i*, *n* bytes, deb822 yes/no, refused" and nothing actionable. |
| S2 | Low | Point 1: "observation after failed helper"; point 4 | Post-failure ambiguity. `nonempty=false`, or a mirror-parser refusal at line 169, can mean the original input was like that. It can also mean a partial publication already happened. The evidence acknowledges the general limit (`:65-68`). The S1 booleans for "Kernel mirror present" resolve it at no extra privacy cost. |
| S3 | Low | Point 5: "proof that no filesystem mutation or outbound request occurs" | Partial, and honestly stated. The owned-tree snapshot plus the `apply`/`_apply` spies cover the exercised seam. The AST name fence is supplemental. There is no outbound proof beyond the absence of an implementation. That is acceptable given that the import graph is stdlib plus the pinned pure helper. |
| S4 | Info | Point 4: structural fixture | The encoder is declared unavailable. The proposal allows this ("record that a faithful pure fixture is still unavailable"). With S1 in place, the remaining evidence gap is the exact public hostname or keyring path. Those must come from the official image-revision source, not from runner output. |

Satisfied: point 1 (no apply/`_apply`/staging/chmod/fsync/replace/APT/network; it calls
only `rewrite`/`rewrite_mirror_list`, and grep finds no `apply`/`_apply`/`read_source`
references). Point 2, apart from the format/trust/URI classification in S1 (intent
comparisons, policy verdicts, counts, category/index, byte length, plan count, recheck,
allowlisted errno, closed vocabulary, no text, traceback, names or values). Point 3
(all caps; descriptor-relative `O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC` parents including `/`
and `/etc`, which is stricter than the helper; children opened `O_RDONLY|O_NOFOLLOW|O_CLOEXEC|O_NONBLOCK`
then `fstat` regular/root/single-link/private-write; bounded actual reads with a
one-byte over-read; fresh byte, dev and inode recheck; stop on the first unsafe input
with no reopen or fallback). Point 5: owned/injected tests for every refusal class.

Security checks: the context guard precedes all filesystem access. There is no CLI
root or environment selector (any argument → 2). The trusted-sudo invocation and its
preserved environment are identical to the helper's. The workflow condition is
`failure() && runner.os == 'Linux' && steps.ubuntu_mirror.outcome == 'failure'`, and
it adds no `continue-on-error`. Package and smoke steps stay skipped, and the
collector always returns 1. I found no red-to-success path.

FIFOs and devices cannot block: `O_NONBLOCK` children are refused at `fstat`, and
`O_DIRECTORY` parents fail with `ENOTDIR`. Descriptors close on every path (each
parent fd is appended before its policy check, and `read` closes in `finally`).
Serialization uses sorted-index IDs and bounded scalars and rejects bad schema
values. Errors map to an allowlist index and are never stringified. Enumeration and
sort order match `_apply`, so the sorted index refers to the same file.

## Required before execution (minimal, privacy-safe)

1. **S1: emit the raise site.** On `mirror.Refusal`, walk to the innermost
   traceback frame. Emit `raise-function` (an index into a fixed allowlist:
   `replacement`, `trust`, `rewrite_list`, `rewrite_sources`, `rewrite`,
   `rewrite_mirror_list`) and `raise-line` (an integer of at most 251). Emit them only
   if `f_code.co_filename` equals the loaded helper's spec origin; otherwise emit 0/0.
   Never read or emit arguments or values. Line numbers decode against the pinned
   helper SHA already in the manifest.
2. **S1/S2: add closed booleans for the compound sites only.** Produce booleans
   only, from fixed keys or fixed comparisons; never values.
   - At `rewrite_sources:97`: has `types`, `suites` and `components`; `Signed-By` present; `Signed-By` equals the pinned keyring.
   - At `:93`: line is a continuation, or field is a duplicate.
   - At `replacement:35`/`:39` and `rewrite_mirror_list:166/169`: host in the allowlist; host ends with `.ubuntu.com`; host equals the Kernel mirror host; scheme is http/https; path is `/ubuntu(/)`; netloc equals the hostname; query or fragment present.

   Either re-tokenize the refusing input with the same fixed regexes or read only
   the named frame locals. Unknown → fixed 0, and the input stays refused.
3. **T1: run the collector's unit tests natively.** Add a step after the observation,
   with the same `if:` condition, that runs
   `python3 -B -m unittest discover -s tools/ci -p 'test_observe_ubuntu_mirror.py'`
   as the runner user. That way a test failure cannot suppress the observation, and
   the step adds no `|| true`. Extend the Unix-only tests with owned-temp native
   cases: a real `DescriptorTree.entries`/`read` on a directory fd (regular read
   succeeds), a FIFO child (returns immediately; `regular=false`), a hard link
   (`single-link=false`) and a directory child (`regular=false`). Run as non-root,
   `owner=false` is expected and is asserted.
4. **T2: add tests for `main()` and `entries`** as described above.
5. Add tests mapping each S1 probe case to its expected raise site and booleans.
   Assert that redaction still holds; for example, the encoded report never
   contains a host, path or keyring substring.

T3 is recommended but not required. These changes leave the helper, the guards,
the bounds and the CI policy untouched and add no compatibility fix. Sol implements;
Opus reviews only the delta against these pins.

## Hosted run recommendation (after the required changes and delta review)

Use only the separate diagnostic branch, not PR54, not attempt 3, and with no
`ci-speed` label. The coordinator triggers one first-attempt CI run of the reviewed
exact head. The observation runs only if the unchanged helper fails on the hosted
Ubuntu runner.

Record the following:
- the runner image header revision;
- the single JSON line;
- the native collector test result;
- the helper's token.

Do not rerun to "catch" a particular image. The `ubuntu-latest` revision is not
controllable. A result from an image other than 20261111.82.6 is evidence about that
image only.

If the helper passes, record "not reproduced on revision X" and stop. If the
complete plan passes, record `publication-unresolved` and authorize no write. The
job stays red, and no gate, budget, timeout, OS matrix or signed-mirror policy
changes.

## Limits carried forward

Native/installed/dedicated paths are **unverified on real hardware**. Unix
descriptor and alarm semantics are unverified locally. M2 is incomplete. Attempt 2
remains nonqualifying (A1/A4). PR53/54 remain unlabelled and budget-blocked; main's
906 s failure is the authoritative baseline. Attempt 3 is untouched.
ExecutionFailed/E_PROTECTION remain unknown, with no fix, waiver or retry. Nothing is
authorized for execution or publication until the coordinator accepts a review.

**Count:** Standards 5 findings (worst: T1, collector tests never run natively).
Spec 4 findings (worst: S1, opaque parser refusal; blocks execution).
