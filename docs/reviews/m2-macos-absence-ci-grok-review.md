# M2 macOS absence CI review

Disposition: **no blocker in the CI delta.** Commit `041d7ce` matches the
accepted integration proposal and does not widen the collector guard.
**Publication of this HEAD is not cleared.** A pull request opened from
`041d7ce` against current main is wider than the collector, the retained
collector review, and the workflow. **The first bounded hosted query is
cleared only for a PR cut from current main whose patch is that set plus
this review.** The explicit authorization may cover the fixed read-only
query on the disposable hosted macOS runner through ordinary CI. It may not
cover an owner, admin, or service change, a nonempty profile, absence
proof, or fixture credit.

Reviewer: xAI / grok-4.7, one pass, 2026-11-14. The assignment forbids
subagents, so Standards, Spec, and security are this single reviewer.
Independence is from the Sol CI commit, not from a second reviewer. Label:
single-agent review.

M2 is **incomplete**. Real-Runner PA6 is **OPEN**. Linux Stage F evidence and
Stage G production wiring are **OPEN**. Installed, dedicated, and macOS
launchd paths are **unverified on real hardware**. Windows main71 run
`43545244266` attempt 1 has cause **unknown**; no retry is authorized. The
optional Windows replay harness stays parked. No native evidence or capture
authority is created here.

## Pins

Porcelain was empty on the reviewed tip and stayed empty through the checks
below. The review cwd is the linked worktree top. Its gitdir is that
worktree's own gitdir and differs from the common gitdir. The branch has no
upstream. Paths below are repository-relative. No owner path, account, or
machine path is recorded. Inspection was Git, the workflow, the unchanged
collector and its tests, the unchanged manager collector, ADR 0014, the
preparation evidence, and the retained collector review. No launchctl,
collector CLI, provenance file read, process, account, service, system, VM,
private-state, owner, install, network, GitHub, CI, or dispatch ran.

| Item | Identity |
| --- | --- |
| Review branch | `t3/macos-absence-ci-review` |
| Reviewed head | `041d7ce660637d6c6a8ac99729ca2254ba6b7bc3` |
| Head tree | `3d209d1405745a270655804f3cce9f3968b29173` |
| Head parent | `bfad0cd749c7194b8ffe60d6477ab3d5941066e3` |
| Parent tree | `fb37d684e2f14be2754c8c9190bb5afb233b2d10` |
| Actual main | `cbc908ca54fce7a5f2d9c1dd922c928ce5ba0fec` |
| Merge-base of head and actual main | `aa49ce1e99475d07d63c6b0d23fa94bdb30bab74` |
| Grok collector review | `d70da50c22980fe8546b502f960620d675dafefc`, parent `f072a170724c9a2f194f41b65ce6e8f10ecc8f5e`, tree `506ca63cc165066eacf957ee8c5e9da9d1e9ac44` |
| Retained review | `bfad0cd749c7194b8ffe60d6477ab3d5941066e3` |
| Sol collector | `1cc2cd186eda4827147373b381d389e7b2816f83` |
| Retained collector | `ec5731894405f1c5399e8a426e82871f6a22e6bc` |

`d70da50` is not an ancestor of `bfad0cd`. The collector-review blob is
identical at `d70da50`, `bfad0cd`, and this head:
`46c637c9c12803af3340c23e65c9f0d777d8952c`. `1cc2cd1` is not an ancestor.
`ec57318` is. `git diff` of the collector, its tests, and `tools/ci/README.md`
between `1cc2cd1` and `ec57318` is empty, and those blobs match this head.
`git diff --check` on `bfad0cd..041d7ce` was empty. The commit touches only
`.github/workflows/ci.yml` and `docs/PROGRESS.md`: 8 insertions, 1 deletion.

Ahead of `cbc908c`: 81 commits. Behind `cbc908c`: 4 commits (`cbc908c`,
`4ec11b9`, `2472ce9`, `b34110a`). The two-dot tree diff versus `cbc908c` is
12 files and does not change `crates/`. The three-dot diff from the
merge-base is 48 paths. Those counts are structure, not a review of the
other paths.

| File | Blob at reviewed head | LF SHA256 | Bytes | CR |
| --- | --- | --- | --- | --- |
| `.github/workflows/ci.yml` | `95409bfb63fc4f89dae950c5900cd473b3176868` | `70539ff3982db8ad55b4266373dc6fc99e3573bdf617080c9c85af33fe2e623c` | 3407 | 0 |
| `docs/PROGRESS.md` | `804c93d54b3c51cc53973bce9c3ced1a5fe5bc1c` | `6cbdce80b1fa4457409eab8ece62b195120258d5450d07a66e7c1fac66b05787` | 403056 | 0 |

Parent blobs: workflow `fc065af9786e75c52cfb7668b726d0a89931cd5b`, PROGRESS
`a6d36ee018828760ffd83fd14630a04c7565cd66`. Unchanged at this head and at
`cbc908c` unless noted: `tools/ci/observe_launchd_absence.py`
`923b0374d28d3623c4cbb3a4d12c645721b8fe7d`,
`tools/ci/test_observe_launchd_absence.py`
`d78684c1e6f4be95b9787fe12610cd2372e2c2c3`, `tools/ci/README.md`
`2acc3dcf6aa55653378efcbd410cc9a65a1c1be0`,
`tools/ci/observe_manager.py`
`c7104fe1c827f992a90fa3d40668ada3455a0efa`, ADR 0014
`d3e03b9796d12e2a015d5dd343c2d622a6fb3731`, `owner_query.rs`
`20c51c17ba24e5d3b5cecfad2af5d2b662445888`, preparation evidence
`95edca46e848f2a387c3be7a6000482cb8cd4852` (same at `54e584f`).

Documents read: `AGENTS.md`, `CONTRIBUTING.md`, `docs/KICKOFF.md`, the review
skill, ADR 0014 including lines 109–142, 539–550, and 579–604, the
preparation evidence's CI proposal, and the retained collector review. Code
read: the workflow, the absence collector and its tests, and the manager
collector's macOS target list.

## Checks

Order was diff structure, call-graph reading, node workflow tests, then the
12 mocked collector tests. No mutant run was repeated. The node tests do not
encode the manager condition or the launchctl argv; they check the existing
shell install, concurrency, single 12-minute timeout, and single job env.

- `node --test scripts/ci-workflow.test.mjs`: 3 passed, 0 failed.
- `python -B -m unittest discover -s tools/ci -p test_observe_launchd_absence.py`:
  12 tests, OK, 0.038 s. The pattern loads only that module. Spawn, clock,
  read, plist, and metadata seams in those tests are mocks.

The collector call graph is unchanged. `main` rejects extra arguments and a
non-darwin platform, then calls `provenance`. A handled provenance failure
prints one fixed label and returns 2 before `capture`. `capture` has one
`Popen`. `summarize` always sets `query` to `Unknown`, `fixture_credit` to
false, and `diagnostics` to `withheld`. The module imports no hash function
and writes no file.

## Standards

**PASS. No hard breach. No baseline smell in this delta.**

`AGENTS.md` and `CONTRIBUTING.md` require an honest hardware label, no
unrun check presented as run, no secrets, no owner administration, and a
meaningful test for behavior. `docs/KICKOFF.md` keeps required checks as CI,
`ci-time-budget`, and `agent-review`, and keeps the 15-minute budget gate.
This commit is one conventional CI concern plus the PROGRESS entry the agent
rules require. It does not edit production service code, ADR 0014, the
collector, or another workflow. The new step's tests are the existing mocked
suite. No credential or owner path was added. The Fowler baseline matches
nothing in the two-file hunk that the repo's own PROGRESS style or the
accepted proposal does not already endorse.

## Spec

**PASS for the reviewed commit. No missing proposal line. No extra behavior
in the hunk.**

Checked against the preparation evidence, lines 169–192, which is the
originating CI proposal, and against ADR 0014's hosted-query option and
empty-profile addendum.

The hunk does only this:

- The existing manager step's condition changes from `runner.os != 'Windows'`
  to `runner.os == 'Linux'`. Its `run` block is byte-for-byte the same two
  commands. On the current matrix, Linux still runs them and macOS does not.
- The new step is immediately after that step, before `cargo fmt`, and is
  the proposal's YAML: name `Bounded fixed macOS absence observation`,
  `if: runner.os == 'macOS'`, then `python3 -B` of the 12-test discovery and
  `python3 -B tools/ci/observe_launchd_absence.py`.
- macOS therefore no longer runs `observe_manager.py`. That program's darwin
  path still prints both `system/com.arxcovo.arxd` and
  `system/com.apple.syslogd` (`observe_manager.py` lines 14–15 and 127–130).
  The workflow no longer reaches that path on macOS. The manager file itself
  is unchanged, so Linux still uses the systemctl vector inside it.

ADR fit of the coordinator's proposal: **yes, inside the accepted read-only
option, and no for every stronger claim.** ADR 0014 lines 541–547 allow one
read-only query of a deliberately absent label on a disposable hosted runner
only when the coordinator authorizes it, and they say the Absent constant is
what a root capture would pin. Lines 579–587 keep the production accept-set
empty and say exit 113 alone proves nothing. Lines 597–604 grant no owner
step and no production executor activation. A PR that publishes the reviewed
collector, the retained review, and this workflow, and whose text authorizes
only the fixed query on the ordinary hosted macOS `verify` job, fits that
option. Ordinary CI is not the root capture. The collector still publishes
`Unknown` and `fixture_credit` false. `owner_query.rs` is the same blob as
`cbc908c`. The proposal's refusal of owner, admin, and service changes, and
of a nonempty profile, is required for the fit, not optional.

## Security

**PASS. No blocker. The workflow does not loosen the reviewed guard.**

On macOS the new step is the only manager-query step. The script it names
has one child argv, `["/bin/launchctl", "print", "system/com.arxcovo.arxd"]`,
and one replacement environment: `PATH` `/usr/bin:/bin`, `LANG` `C`,
`LC_ALL` `C`. The workflow sets no step `env` and does not prefix the
command. The job-wide `CARGO_PROFILE_DEV_DEBUG` stays in the parent
environment; the child argv is absolute, so PATH does not select the binary.
Caps stay 65536, 4096, and 69632. Query budget stays 2.0 s and cleanup budget
stays 1.0 s. The workflow contains no regex, so a provenance failure cannot
widen one.

The step has no pipe, redirect, `tee`, hash, `sudo`, install, or
`upload-artifact`. It has no `continue-on-error` and no custom shell. The
script returns 2 before `capture` when provenance fails, and returns 0 after
printing the closed JSON. Exit 0 is a sanitized report, not a candidate
match and not absence. Exit 2 fails the step. Neither result may be repaired
by printing the environment or widening a regex in the workflow. Stop and
return to source review.

Unchanged around the hunk: `permissions: contents: read`, one
`timeout-minutes: 12`, the matrix, `workflow_dispatch` already present at
line 6, the stock-directory step, the Linux mirror `sudo` step, and the
other workflow files. The stock-directory test still opens `/bin/launchctl`
as a file (`native.rs` lines 561–565) and does not execute it. No required
check name is edited. `ci-time-budget` and `agent-review` are outside this
diff. No job-duration measurement was made. The added step does not change
the budget gate or the 12-minute timeout.

The retained collector advisories stay accepted and bounded. This CI review
does not close them and the 12 mocked tests are not fixture credit:

1. `AttributeError`, `ExpatError`, and `RecursionError` from plist parsing
   can still escape `main` before `capture`.
2. The README's "at most one second" cleanup sentence is stronger than the
   non-preemptible spawn, kill, and close latency.
3. The kill-fault test still plants a read error; the success path does not
   lock the whole dict; the 16385-byte sentinel, a non-regular file, and a
   rejected environment value stay implemented and untested.

The public field name `tuple` remains the retained judgement-call smell.
`synthetic-candidate-1` remains a pointer to the source literal.

### Native proof

Native proof from this review is **empty**. Diff structure, three node
workflow tests, and 12 mocked tests do not establish a live `launchctl`
status, pipe EOF, signal delivery, truncation, provenance values, or the
candidate bytes. They do not establish that a green `verify` job means the
service is absent. A later exact match may still set `tuple` to
`synthetic-candidate-1` while `query` stays `Unknown` and `fixture_credit`
stays false. That is not a compatibility fixture and not a nonempty parser
profile.

## Publication

Do not open the pull request from `041d7ce`. Against `cbc908c`, the tree
diff also contains five paths this review does not clear:

- `docs/evidence/windows-main71-native-gate2.md`
- `docs/reviews/m2-acceptance-path-recovery.md`
- `docs/reviews/windows-main71-bootstrap-contract-grok-recheck.md`
- `docs/reviews/windows-main71-telemetry-handoff-grok-design-review.md`
- `docs/reviews/windows-main71-telemetry-handoff-proposal.md`

`AGENTS.md` asks for one-concern pull requests. Those five documents are a
second concern. The three-dot PR view from `aa49ce1` would be wider still.

Cleared patch, cut from `cbc908c`:

- `tools/ci/observe_launchd_absence.py`
- `tools/ci/test_observe_launchd_absence.py`
- `tools/ci/README.md`
- `docs/evidence/macos-launchd-absence-preparation.md`
- `docs/reviews/m2-macos-absence-collector-grok-source-review.md`
- `docs/reviews/m2-macos-absence-ci-grok-review.md`
- `.github/workflows/ci.yml`
- `docs/PROGRESS.md`

The PR text may authorize the ordinary `verify` job on the disposable
hosted macOS runner to run that fixed script. It must say the run is
read-only, that exit 0 is `Unknown`, and that exit 2 stops the workflow
without a regex change. It must not authorize owner, admin, or service
changes, a nonempty profile, root pinning of Absent, PR29 clearance, or
hardware credit. This review does not push, dispatch, or run the collector.

## Close

Standards: 0 hard breaches, 0 baseline smells. Spec: 0 blockers in the
commit; the ADR fit is the hosted read-only query only. Security: 0
blockers; worst residual hazard is treating a green verify job, exit 113,
or `synthetic-candidate-1` as absence or a nonempty profile. Publication of
`041d7ce` as the PR head: blocked. Publication of the narrowed patch for the
first bounded hosted query: cleared.
