# Ubuntu 26 narrow mirror omission: local implementation evidence

OpenAI / gpt-6.1-sol / high, implementation sub-agent, 2026-11-12.
No subagents. Local delivery only; fresh independent Opus high security/source
review is REQUIRED before coordinator push. This evidence is not that review.

## Identity and approved scope

- Initial HEAD: `1ac1aa167e03b1ba8d0a3284b512d6c71d15dcec`, clean.
- Runtime cwd and Git top level matched the supplied isolated T3 worktree and
  differed from canonical `D:/Code/Arxcovo`. Canonical checkout was not changed.
- Base helper, tests and contract were byte-identical to PR54 pin
  `344c406663ad08b5f0497ecee9eb67a4cbd7e896`; verified with Git object diff.
- Read AGENTS/KICKOFF, current proposal including owner disposition, ADR0021,
  retained Sol review, final Opus review including appended R2a/R2b corrections,
  and root R2b correction `6e7616f421cb8d06a311c333de4ea0da8f8b3cbd`.
- Owner-approved helper/test/contract scope and separate prerequisite commit
  in existing PR54 apply. Coordinator owns ADR/review/PROGRESS integration.
- Source commit: `e9d63df9a2670a9fe52b31415f4d94152cb76395`.
  Its exact manifest is the following three paths, with SHA-256:

| Path | SHA-256 |
| --- | --- |
| `tools/ci/ubuntu_mirror.py` | `888fd3e79c085db243a7004e469f9a330933a2faf968f7564dd167ce587f5f05` |
| `tools/ci/test_ubuntu_mirror.py` | `eb2f9e1b1fec686148b71b98b33d506b39b5cabc39adad70b3e3b40c8d9052fc` |
| `tools/ci/ubuntu-mirror.md` | `19c4238708f61a9a0ef0b53da06a16fe726ffe4d3525292dfd997445fef775a7` |

This evidence is a separate documentation commit; its full commit hash is in
the implementation handoff. Cherry-pick the source commit itself onto PR54,
without bringing unrelated product history. No push or GitHub action occurred.

## Behavior and coverage

Private pure `_rewrite_mirror_list_counted` returns `(bytes, omissions)`;
`rewrite_mirror_list` still returns bytes. `_apply` carries counts and `main`
prints fixed updated/omitted lines only after success. Direct mode returns zero
omissions. Refusal and argument errors have no success data or private payload.

Full raw-token matching precedes URL parsing. Both layers constrain lowercase
HTTP(S), LDH Ubuntu subdomains, label/hostname bounds and exact path; authority,
xn-- and query/fragment checks remain. ASCII line and token separators prevent
Unicode splitlines/split from silently removing C0 or non-ASCII prefixes.
HOSTS and every direct-source, trust, keyring, Valid-Until, file, recheck and
publication guard remain. No new contact target is introduced.

The existing 23 tests remain, changing only the expected `_apply` tuple.
Eight added test methods cover these behaviors (subcases included):

| Test suffix | Assertions |
| --- | --- |
| `representative_omissions_exact_bytes_counts_absence_order` | Three synthetic representatives: replace first HTTP, replace second HTTPS without slash, append untagged. Exact expected bytes/counts, dropped target absent, Kernel first and retained order. Canonical 144 -> identical 195 bytes, count zero. |
| `omission_shape_fences` | FTP, userinfo, ports, uppercase scheme/host, wrong paths, query/fragment including empty delimiters, bare/lookalike/trailing-dot hosts, underscore, 64-byte label, 254-byte hostname, foreign host, C0/DEL prefixes, percent/backslash, xn-- and explicit UTF-8 U+00E9 label. Additional C0 prefixes 1–31 except structural TAB/LF/CR and U+0085/U+00A0 prefixes refuse. |
| `omission_active_bounds_and_fallback_required` | One retained + 15 omitted succeeds exactly, +16 refuses; 16 retained +1 omitted refuses; all omitted refuses; multiple omissions; 4 KiB input and output bounds. |
| `omission_keeps_comments_ties_untagged_and_priority_shift` | Exact comments/blanks, existing ties, untagged spacing, priority +1; no new tie rejection. |
| `omission_publication_exact_plan_and_second_run_refusal` | Owned-temp publication for all three representatives, exact mirror/direct-source/vendor/indirect-source bytes; second run refuses with all targets byte-identical. |
| `omission_unknown_tail_and_kernel_anywhere_never_publish` | Retained + dropped + foreign/malformed/Kernel/duplicate-Kernel/nested-list tails, malformed and duplicate tags; Kernel first/middle/end. Complete target snapshots stay byte-identical, including direct source otherwise requiring rewrite; no staging/publication. |
| `omission_does_not_extend_direct_source_trust` | Unsupported Ubuntu direct legacy/deb822 sources refuse; direct owned-temp mode reports `(1, 0)` and leaves mirror list unchanged. |
| `main_success_and_refusals_have_only_fixed_count_tokens` | Success counts 0, 1, 15; mocked Refusal/OSError/private arguments emit exact fixed tokens, empty opposite stream, no private/error/success leakage. |

All fixtures represent the accepted predicate class, never the actual hidden
hostname or bytes. Exact historical hostname and unchecked tail remain UNKNOWN.
No historical root-cause resolution or native success is claimed.

## Commands, red history and final results

Environment: Windows, PowerShell, Python 3.12.10, Node v24.18.0.
All helper exercise used pure functions, owned temporary trees with fake native
facts, or the existing mocked main/context seams. No actual APT path, production
CLI/apply, sudo/root, network, package installation or Rust suite was invoked.

Commands actually executed:

- `python --version`; `node --version`.
- `python -I -B -m unittest discover -s tools/ci -p test_ubuntu_mirror.py`:
  baseline 23/23; final 31/31 (last run 0.216 seconds).
- `python -I -B -` inline baseline fixture process: loaded baseline Git object
  into an in-memory module; all three representative omission fixtures refused
  as expected, canonical 144 -> 195 assertion passed.
- `node --test scripts/ci-workflow.test.mjs scripts/policy.test.mjs scripts/evaluate-budget.test.mjs scripts/enable-auto-merge.test.mjs`:
  19/19, zero failures/skips (last duration 110.4069 ms).
- `python -I -B -` inline mutation harness: compile each edited source string,
  exec in a fresh `ubuntu_mirror` module, load test source into a fresh module,
  run `unittest.defaultTestLoader.loadTestsFromModule` through TextTestRunner.
  Each mutant runs all 31 tests. Assert nonempty assertion failures and zero
  execution errors. No production file is mutated. Compare production bytes
  after runs, then run restored source and focused disk suite.
- `python -I -B -` UTF-8/LF/manifest/hash assertions: all three source files
  decode as UTF-8, contain no CR, end in LF; exact three-path manifest and
  PR54/base identity passed. `git diff --check` passed.

Local development red history: first expanded suite had one fixture error
(legacy syntax mistakenly put in a `.sources` fixture). Correcting that fixture
to deb822 made 31/31 pass. First mutation harness stopped on the omission-removal
mutant because expected-positive calls raised parser exceptions rather than
assertion failures. Positive-test wrappers now convert unexpected Refusal into
explicit assertion failures. That incomplete harness received no kill credit.
All 21 mutants subsequently compiled and ran, first before and then again after
the ASCII-separator hardening. Final results below supersede the earlier run.
These are local test/harness changes, not failed hosted correction trials.

## Final mutation results

Each row ran 31 tests, compiled successfully, failed assertions as counted,
and had zero execution errors. Changes were in memory only, never committed.
Paired edits loosen only the named predicate in both raw and parsed guards.
The hostname regex is shared by raw and parsed layers, so changing it edits
both derived guards. Production retains both layers.

| Mutant | Temporary edit | Assertion failures |
| --- | --- | ---: |
| Remove omission | Disable branch | 6 |
| Any host | Shared hostname grammar -> `[a-z0-9.-]+` | 6 |
| Raw uppercase | Raw scheme permits `HTTP` | 1 |
| Raw C0/DEL prefix | Raw pattern permits leading C0/DEL | 29 |
| Raw prefix match | Raw `fullmatch` -> `match`, empty ?/# pass | 2 |
| Paired port | Raw optional numeric port; relax parsed netloc equality | 1 |
| Paired userinfo | Raw optional user@; relax parsed netloc equality | 1 |
| Paired path | Raw and parsed path allow `/other` | 1 |
| Paired label length | Shared `{0,61}` -> `{0,62}` | 1 |
| Paired hostname length | Raw 253 -> 254 and parsed len <=254 | 1 |
| Paired xn-- | Remove raw/shared negative lookahead and parsed label check | 1 |
| Paired UTF-8 | Both shared LDH character classes allow U+00E9 | 1 |
| Write omitted | Append dropped line to output | 6 |
| Zero fallback | Allow zero retained | 3 |
| Cap excludes drops | Cap only retained count | 2 |
| Kernel omittable | Shared raw/parsed host union includes Kernel; relax explicit exclusion | 6 |
| Direct omission | Unsupported Ubuntu source returns None instead of refusal | 3 |
| Host output leak | Append synthetic hostname to omitted output | 1 |
| Early success | Return partial bytes/count immediately on omission | 13 |
| Unvalidated tail | Break parser loop on omission | 13 |
| Omitted as retained | Increment retained counter on omission | 1 |

Symmetric equivalence checks: raw-only port widening and parsed-only netloc
relaxation each ran 31/31 green because the other layer still rejected ports.
Both receive NO kill credit. The paired port mutant is killed. This demonstrates
the required exemption without deleting production guards for kill credit.

Restored final production source ran 31/31 green in memory and on disk. Final
helper hash is the manifest hash above; bytes remained unchanged during mutation
execution. No broad Rust run was appropriate for this helper-only change.

## Ledgers and delivery boundary

- PR54 performance: 2 of 3 failed, attempt 3 UNUSED. No reset.
- Hosted fetch-path invocations: 2 failed, unchanged. No new hosted invocation.
- Failed hosted fixes before this local work: 0; this work adds no failed
  hosted fix. Local mutation failures are intentional sensitivity evidence.
- No optional precheck, dispatch, CI/rerun, label, PR, push or merge.
- No PROGRESS, ADR, other review, policy, workflow, dependency, packaging or
  product-source change. No account, service, private state, VM, Lab Manager,
  owner/admin or experiment operation. Babysitter unchanged.
- Native, installed and dedicated paths remain **unverified on real hardware**.
  M2 remains incomplete; E_PROTECTION and ExecutionFailed remain separate,
  unresolved and unretried. Local E1/E5 remain held.
- Coordinator must obtain fresh Opus high independent security/source review
  before pushing the cherry-picked source; final candidate still needs its own
  unchanged required checks and measured acceptance. This source is not merged.
