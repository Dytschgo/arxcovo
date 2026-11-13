# E4 native harness: independent Opus pre-publication review

Reviewer: Anthropic / Opus 5.5 / high. This was a single-agent review, run in
a review sub-agent with no delegates. The builder was Sol (OpenAI), so this
review comes from a different provider. Date: 2026-11-13.

**Verdict: FAIL. Do not push yet. There is 1 blocker (Spec B1).** It is a
deterministic parser defect, and it fails closed: it causes no safety failure
and grants no false credit. However, the first hosted run would stop at the
initial S6 baseline. It would then execute zero mutants and yield no E4 kill
evidence. After B1 is fixed, the fix needs a narrow recheck before the first
push. That push may produce hosted native evidence only, not a merge. All
other fences, all 11 mutants and the S11 fixture correction pass this review.

## Pinned target and checkout

- Runtime cwd and Git top-level:
  `C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-5ef832ad-dc05d354c0602c50`.
  This is an isolated worktree on `t3/5ef832ad`, outside canonical
  `D:/Code/Arxcovo`. Status was clean at the start.
- Reviewed HEAD: `e9b00c4adb17ff03220608e27f2a42d60775202c`.
  - Base PR62 head: `93046dd45f40ac38beb0e2a8d1e5b0da26585155`.
  - Main: `af9289a5dbd648d94db338c017bbcd8f801c7ebf`.
  - `1efc35b` sits between the base and the harness commits. It is docs only.
- Retained commits have identical `git patch-id --stable` values:
  - `5e9198c` = `d24eaf4`
  - `c0c021f` = `9b29d38`
  - `7b371bc` = `0988f8d`

  Their trees differ only by `1efc35b` (PROGRESS plus the E4 evidence
  section).
- New commits:
  - Sol medium `f17a7ac` changes the S11 fixture.
  - `e9b00c4` changes docs.
- Delta `93046dd..HEAD`:
  - the new workflow
  - `tools/ci/e4-native/{driver.py,test_driver.py,mutations.jsonl}`
  - two evidence docs and PROGRESS
  - one Rust test file, with 2 lines changed
- `git diff --exit-code 93046dd HEAD` passed for these paths:
  - the three production rights/shim files
  - `tests/unix_shim.rs`
  - `exec/`
  - the E3 harness and workflow
  - `ci.yml`
  - `Cargo.lock`

  **Production bytes are unchanged.** No existing workflow, protection or
  policy file changed.
- Hashes recomputed at HEAD:
  - All five driver `PINS` match.
  - `MANIFEST_SHA` `15980053…` matches.
  - The evidence table rows for the workflow `48122e68…`, `driver.py`
    `7226a7df…`, `test_driver.py` `57f33fe4…` and `unix_shim_rights_linux_tests.rs`
    `102ec572…` all match.
  - All six harness inputs are tracked `100644` at stage 0.

## Checks actually run (this review)

| Check | Result |
| --- | --- |
| `python -I -B -m unittest discover -s tools/ci/e4-native -p test_driver.py`, Windows Python 3.12.10 | **21/21 pass**, 0.70 s. Tree still clean afterwards. |
| Scratchpad probe (`e4_adversarial.py`): driver parser and fragments, all in memory, no subprocess | See B1 and the fragment results below. |
| Read-only source reading in the local cargo registry. `cargo-nextest-0.9.151/Cargo.toml` pins `nextest-metadata =0.15.0` and `nextest-runner =0.129.0`. | Nextest output format confirmed (B1). |
| `gh api …/issues/comments/6863902435` (one read) | Prior PR62 review content confirmed. |

Not run: Cargo, rustc, nextest, the driver itself, any Linux/WSL/container
process, hosted CI, push, dispatch, watch, merge, retry, or settings changes.
There are **no native kill claims, no Linux runtime claims and no
fmt/clippy rerun.** Sol's report of fmt passing and 21 tests passing is
credited to Sol; I independently reproduced only the 21 tests.

## Spec

Sources: the E4 slice and mutation list in the brief
(`docs/briefs/0020-linux-hierarchy-implementation.md` §E4), ADR 0020 D8/R5,
the earlier E4 review, the compile/NNP recheck, and PR62 comment 6863902435.

### Blocking

- **B1. The driver cannot parse real nextest output for the S6 integration
  binary, so no S6 baseline or S6 kill can ever be recorded.**
  - `statuses()` expects the binary label to be exactly `unix_shim`. The
    manifest uses `"binary":"unix_shim"`, and `baseline()` and `classify`
    pass that value through.
  - In nextest 0.9.151, `RustBinaryId::from_parts` (nextest-metadata 0.15.0)
    gives an integration-test binary the ID `package::target`. The format is
    documented there as stable API. Here that ID is **`arxd::unix_shim`**.
  - `DisplayTestInstance` (nextest-runner 0.129.0) prints the full binary ID
    followed by a space. Truncation applies only to the progress bar.
  - The real line is therefore `PASS [   5.065s] (1/1) arxd::unix_shim s6_probe_…`.
    For that line, the probe shows:
    - `statuses` returns `[]`
    - `baseline_result` raises `baseline_missing_test`
    - `classify(100, FAIL…)` returns `invalid`
  - The library binary ID stays the bare `arxd`, which the E3 hosted run
    already validated.
  - Effect: `baseline_before` fails closed at S6, after the library and L8
    steps. All 11 mutants stay unrun, and the job exits 1. If the S6
    baseline ever passed, the S6 mutant could still never count as `killed`.
  - The portable test helper `status(kind, name, binary)` invents the same
    unverified `unix_shim` label. That is why 21/21 passes while the defect
    remains. This is exactly the "actual runner stream" class of bug that
    affected E3.
  - **Required fix.** Match the binary ID `arxd::unix_shim` for the
    integration target, while still selecting it with `--test unix_shim`. If
    the manifest changes, re-pin it.
  - **Required tests.** Add tests that use a line in the real format:
    - `PASS`/`FAIL … (1/1) arxd::unix_shim <S6>` is accepted.
    - The bare `unix_shim <S6>` form and other prefixes such as
      `x::unix_shim` are rejected for S6.
    - The library form `arxd <name>` is unchanged.

  Keep every other part of the classifier as it is.

### Verified (source analysis; nothing native ran)

**Workflow**
- Separate and non-required. It uses a plain `pull_request` trigger with a
  paths filter that covers the E4 shim files.
- It has `contents: read` only and no secrets. Checkout uses
  `persist-credentials: false`.
- It uses the same three immutable action SHAs as the E3 workflow; the only
  diffs are names and paths.
- Toolchain 1.95.0, nextest `0.9.151`, `timeout-minutes: 10`,
  `CARGO_INCREMENTAL=1`, `CARGO_PROFILE_DEV_DEBUG=0`.
- A shell step fails on numeric UID 0.
- There is no sudo, `pull_request_target` or dispatch.
- Required checks, protection and `ci-time-budget` are untouched.

**Driver fences**
- Before any read, it refuses non-Linux, extra argv, UID or EUID 0,
  non-hosted runs (`GITHUB_ACTIONS`/`RUNNER_OS`) and a cwd mismatch. It sets
  umask `077`.
- Before any subprocess, it checks all source pins and the manifest pin, and
  dry-applies every fragment. A test asserts that nothing is spawned before
  the pins pass.
- Each target must pass `regular()`:
  - It must be in the fixed list.
  - No component, including the root's parents, may be a symlink or have
    the reparse attribute `0x400`.
  - It must be a regular file with exactly one link (`nlink == 1`).
- Git must list it as tracked `100644`, stage 0, exactly once.
- Only `TARGETS[:2]` can be mutated: the two production files. The tests and
  `tests/unix_shim.rs`, which holds the NNP fence, can never be mutated.
- Each fragment must occur exactly once in the whole file and exactly once
  before the last `\n#[cfg(test)]\n`. The probe shows count 1 and 1 for all
  11 fragments. No fragment contains `/usr/bin/sudo` or `NO_NEW_PRIVS`.
- Restoration happens in the mutation `finally`, then a whole-set
  restore-and-hash happens in the outer `finally`, even after a baseline
  failure. The five-file fingerprint is checked before and after every
  baseline.

**Bounds and timing**
- Reads are capped at 256 KiB per source, 32 KiB for the manifest and 8 KiB
  for Git output. Captured output is capped at 2 MiB.
- No shell is used. Linux runs use a new session and `killpg` with SIGKILL,
  plus a `wait`/`join` timeout.
- Global deadline is 480 s.
- First-baseline caps are 180, 15 and 60 s.
- Each mutant gets at most 35 s and must leave an 85 s reserve. That reserve
  equals the final caps of 50, 15 and 20 s.
- A budget of zero or less raises `deadline`, so it fails closed. The final
  baseline therefore always runs, or fails closed, within the job's 10-minute
  limit.
- The worst case, about 480 s plus reap time plus setup, fits in 600 s.

**Truth rules**
- A kill requires all of the following:
  - exit code 100
  - exactly one `FAIL` line for the exact full name, with or without the
    padded counter
  - exactly the error line `error: test run failed`
  - no `SKIP` anywhere in the output
- A compile error (101 or `error[`), no tests (exit 4), a survivor, a
  malformed line or a bound failure stops the run and exits 1.
- Lowercase `skipped` in the summary line does not trip the `SKIP` check.
  The probe confirmed this.
- Baselines require exit 0, a `PASS` line for every named test and no error
  lines. They allow only the exact markers `E4_SKIP_INHERITED_NNP` and the
  L8 hardening skip, and they record each separately. `E4_SKIP_ROOT` and
  unknown skips fail closed.

**S11 correction (`f17a7ac`)**
- The parent reads `prctl(PR_GET_NO_NEW_PRIVS)` directly. It asserts the
  result is `0` or `1`; any other result fails the baseline. It skips the
  false case only when the result is `1`, with an explicit marker.
- The masked path that hard-coded `true` and skipped itself is gone.
- If the hosted runner ever had NNP inherited, the marker would make the
  S11 mutant `invalid` (fail closed), never `killed`.
- Production bytes are unchanged.

**Baselines match the accepted brief**
- Nine fake-ELF Linux unit tests: S8–S13 plus the final owner check.
- S6 in its own invocation, still NNP-fenced. Under NNP, the real system
  probe runs unprivileged, and S6 asserts `no_new_privs:true` and
  `sudo != exit_zero`.
- E3 L8, which reads only the process's own status.

**No privileged execution in any baseline or mutant**
- No library test calls the production `probe()` or `observe()`.
- Every `run`/`call` uses an owned temporary anchor, owner EUID ≠ 0.

### Mutants (each against the brief's mutation list; source analysis only)

| # | Mutant → selected test | Kill mechanism | Safety |
| --- | --- | --- | --- |
| 1 | S9c path reopen → S9c | After the hook swap, re-resolves C1 by path and runs B. Reading `A` fails on the missing file and panics, in both the file-swap and directory-swap cases. | `cfg(test)` only. Before any reopen, asserts owner ≠ 0, anchor ≠ `/`, owner = EUID and the fixed `e4_helper` argv. Reopens only `settings.anchor()`. `candidate()` still requires the file owner to be EUID, so a root-owned `/usr/bin/sudo` cannot be chosen even indirectly. It also panics, rather than executing anything, if reached with default settings. |
| 2 | Ancestor trust → S9 | Drops the owner/mode check, keeping only the type check. Case 3 (`usr` mode 0702) now gives `exit_zero` instead of `untrusted`. | Final-file owner check kept; owned fake only. |
| 3 | Final NOFOLLOW → S9 | Case 2's `../usr/bin/sudo` link is followed, giving `exit_zero`. | The link stays inside the anchor; owner check kept. |
| 4 | Alias suffix → S9 | Case 6 (`trusted/usr/bin`) is accepted as an alias, so C1 runs. | Alias skip only. |
| 5 | Execute despite untrusted → S9 | Case 0 runs the trusted A, giving `exit_zero`. | `chosen` is always trust-checked. |
| 6 | Final CLOEXEC → S8 | The script case runs through `/dev/fd/N`, giving `exit_zero` instead of `unknown`. | Owned script writing under the anchor. |
| 7 | Inherited env → S8 | The helper's `vars_os().count()==1` fails, giving `exit_nonzero`. The brief names S12; S8 is the behavioral discriminator, and the evidence doc records that deviation. | Environment goes to the owned fake only. |
| 8 | PATH lookup → S10 | The helper runs `root/sudo`, so the result is no longer `absent`, the child fails and the parent assertion fails. | Requires PATH to equal the owned anchor, anchor ≠ `/` and owner ≠ 0, and the anchor fd already passed the EUID-owner trust check. libtest rejects `-n` before running any test, so the probe is never re-entered. |
| 9 | Timeout → `exit_nonzero` → S8 | The S sleeper is expected to give `unknown`. | `Child` drop still kills and reaps. |
| 10 | Rights before tools → S6 | `rows.last()` is a tool row, so `rights` is missing. | Touches only the `unix_shim.rs` order. The `pre_exec` NNP fence in the immutable test is unchanged and still fails closed. No fail-safe `125` path is removed. |
| 11 | NNP hard-coded `true` → S11 | The false child sees `Some(true)`, so its assertion fails, the parent fails, and the run counts as killed. | `nnp()` is not a fence. The NNP fence lives in the test's `pre_exec`, which no mutant can touch. |

No mutant weakens the NNP fence, removes a fail-closed return or reaches
real `/usr/bin/sudo` outside the NNP-fenced S6. All 11 brief classes are
present, each runnable with `blocker:null`; "eleven" is not satisfied by ten.
Mutant 10's kill becomes creditable only after B1 is fixed.

### Advisories (non-blocking)

- **A1. Nextest isolates each test in its own process group.**
  `runner/unix.rs:33` calls `process_group(0)`. When the driver uses
  `killpg` with SIGKILL on cargo's group after a deadline or overflow, test
  processes and their fakes are orphaned rather than killed. The comment
  "Also closes descendants" overstates this. The run already fails closed,
  the hosted VM is disposable, and fixtures are bounded (sleeper at most
  10 s), so the effect is limited. Reword the comment, or note the
  limitation in the evidence doc.
- **A2. S6 compile budget is unmeasured.** The `--test unix_shim` path
  rebuilds the non-test lib, the bin and the integration binary. The first
  baseline gets 60 s for that. After restore, the final S6 cap is 20 s,
  which must cover recompilation plus S6's own runtime of at least 5 s. That
  can hit the deadline after the S6 and S11 mutants. Doing so fails closed,
  but it would waste a run. Consider moving reserve from the library final
  baseline (50 s; E3 measured 2.1 s) to the S6 final cap, without raising
  the total.
- **A3. Evidence attribution.** If the `try` block raises and the
  `finally` baseline also raises, the second reason replaces the first in
  the emitted record. SIGINT is not mapped to `GuardError`, so it would
  print a traceback. Cancellation of the disposable VM limits the impact.
- **A4. Stale evidence text.** These parts of
  `linux-rights-probe-e4-native.md` still read as current:
  - the "ten executable candidates and one blocked fixture gap" table
  - "20 passed"
  - "No E4 Rust source or test bytes changed"
  - the "Blocking fixture gap: S11" section

  Only the appended correction section supersedes them. Mark those sections
  as historical.
- **A5. Silent root return in S6 (carried, not from this delta).**
  `tests/unix_shim.rs` `ordinary()` returns with no marker, while the brief
  requires an explicit one. The driver's and workflow's UID/EUID refusal
  mitigates this, and it remains pinned test bytes.
- Prior S-A2, S-A3, A2–A8 and the PR62 non-blocking items are not reopened.

## Standards

Sources: AGENTS.md (meaningful tests, honest claims, bounded reads, safe
paths, no secrets, conventional small commits) and the Fowler smell
baseline. Smell findings are judgement calls.

### Blocking

None independent of Spec B1. Treat the root cause, S-A1, as part of the B1
fix.

### Advisories

- **S-A1. A test fixture invents an unverified runner format.**
  `test_driver.py` `status()` writes `{binary} {name}` with
  `binary="unix_shim"`, so the parser test and the format it expects can
  never disagree with each other. That conflicts with AGENTS' "every
  behavior needs a meaningful test". Use verbatim nextest line shapes, as
  B1 requires.
- **S-A2. Speculative Generality / dead path.** The `if row["blocker"]`
  branch in `execute()` cannot run, because `load_mutations` rejects any
  non-null blocker. Two portable tests exercise it only by patching
  `load_mutations`. Either delete it with those tests, or keep it labelled
  as a historical guard.
- **S-A3. Readability nit.** New doc text again drops spaces before
  numbers, for example "manifest now11", "has11 runnable" and
  "memory.21 portable". This repeats the PR62 nit.
- **S-A4. Repeated Switches (possible).** The `binary` →
  `--lib`/`--test` mapping in `command()` and the label in `statuses()` are
  two decisions keyed on the same primitive. One small map from manifest
  binary to (cargo selector, nextest binary ID) would have made B1 hard to
  write.

## Gates, limits and carried state

**Push and merge**
- This review does **not** allow a push. After B1 is fixed, a narrow
  re-review of the parser and fixture diff and the re-pinned hashes is
  required. A PASS then allows a push for the first hosted native evidence
  only, not a merge.
- Merge still needs all of the following:
  - real ordinary-UID hosted baselines and 11 actual kills
  - an exact-head different-provider integration review
  - the five required checks green

**Platform limits**
- This was a Windows-only review. No Linux path, compile, nextest run or kill
  was exercised here.
- Hardware and native paths remain **unverified on real hardware**.
- M2 is incomplete. Production remains inactive/MechanismUnavailable.

**Unresolved; not retried or diagnosed here**
- the privacy flake
- `E_PROTECTION`
- `ExecutionFailed`

**Not touched**
- canonical
- the old Lab Manager installation and its state
- the `agent-windows-test` VM
- owner infrastructure
- private state
- ignored fixtures
- toolchains

**Count:** Spec has 1 blocker (B1: the S6 integration binary ID
`arxd::unix_shim` is unparsed, so S6 baselines and kills are impossible) and
5 advisories. Standards has 0 independent blockers and 4 advisories; the
worst is S-A1, the invented runner format behind B1.
