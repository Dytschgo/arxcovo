# E4 native evidence harness: local preparation

Builder: OpenAI / gpt-6.1-sol / high, implementation sub-agent, 2026-11-13.
Local work only, no delegation. Fresh independent Opus 5.5 high review is
required before **any push or hosted execution** of this harness. No review,
hosted result, Linux compile or native mutation kill is claimed here.

## Checkout and scope

Runtime cwd and Git top-level matched the supplied isolated worktree on
`t3/e4-native-evidence-harness`, outside canonical. Initial status was clean
at prepared head `93046dd45f40ac38beb0e2a8d1e5b0da26585155` (supplied PR62
head, supplied main `af9289a`). Canonical was not used for work.

Read AGENTS, KICKOFF, ADR 0020 D8/R5, the E4 brief's S6/S8–S13 and mutations,
the existing production/tests, the independent E4 review and compile/NNP
recheck, and E3's corrected native harness/parser/tests/workflow. The retained
recheck closes B1/B2 by source analysis and scratch-stub checks; it does not
establish real native compilation or runtime acceptance.

Source/workflow/manifest commit: `5e9198c`. Portable tests commit: `c0c021f`.
Only the new E4 workflow, `tools/ci/e4-native/` and this note are delivered.
No E4 Rust source or test bytes changed. No existing CI, Cargo, policy,
protection, E3 harness/workflow or PROGRESS changes. PROGRESS is root-owned.

## Prepared execution

The separate **non-required**, plain `pull_request` workflow has read-only
contents permission, credentials persistence off, the same three immutable
action pins as E3, nextest `0.9.151`, a ten-minute job cap, incremental `1`
and dev debug `0`. It does not alter required checks. It must receive review
before publication can trigger it.

The driver refuses Windows/non-Linux and UID **or EUID** zero before reading
source or starting commands. Its entry point also requires the hosted Linux
environment and matching cwd. No sudo, account creation, owner, service,
VM, system configuration or privilege transition command is added.

Unmodified baselines before and after mutations run separately:

1. Nine exact Linux E4 library tests: S8, S9 all-candidates, S9b alias, S9c
   swap, S10 PATH, S11 parser, S11 child observation, S12 constants and final
   object owner rejection. Existing tests copy their ELF into owned temporary
   directories and use fixed helper reentry; the driver never selects ignored
   tests directly.
2. The existing E3 L8 test, with its exact inherited-Standard-hardening skip
   separately recorded as `skipped`, rather than `passed`.
3. Exact Unix S6 integration in its own binary invocation. Its production
   system-sudo observation remains protected by the unchanged Linux
   `PR_SET_NO_NEW_PRIVS` pre-exec fence. Spawn aborts if setting NNP fails.
   No mutant modifies this fence or any fail-safe path.

E4's inherited-NNP marker is reported separately from library PASS. Unknown
skip markers, root skips, missing tests and baseline failures fail closed.
NNP false observation cannot be credited when inherited NNP skips it.

The manifest contains fixed fragments and exact named tests. Five pinned
source/test files and the manifest must be regular, tracked, single-link
files without symlinks or Windows reparse components. Only the two production
source files named by the mutations can be altered. Every fragment must occur
exactly once before the test-module boundary. The manifest itself has a
pinned SHA-256. All source pins are in the driver and checked by portable tests.

Reads, Git inventory, captured output, subprocess wall times and total runtime
are bounded (262144 bytes per source, 32768 manifest, 8192 Git output, 2 MiB
per native output, 480-second driver budget). Subprocesses use no shell and
owned process groups on Linux. Mutation budgets reserve 85 seconds for final
checks. Exhaustion fails closed; no timeout threshold is relaxed.

Original bytes are restored in mutation `finally`, and all pinned targets are
restored and hashed in the outer `finally`, including on baseline failure.
Baselines check fingerprints before/after execution. JSON records contain
fixed labels/categories, numeric UID/EUID, durations, exit codes and hashes;
raw compiler/test output and temporary paths are not emitted or uploaded.

## Exact eight versus eleven scope

E3 retains **eight** native mutations unchanged. E4's brief requests
**eleven** distinct mutation classes; the alias alternatives constitute one
class. This manifest has eleven rows: **ten executable candidates and one
blocked fixture gap**. It never reports ten as completion of eleven.

| E4 mutation | Selected discriminator | Preparation status |
| --- | --- | --- |
| Reopen path after validation | S9c file/directory swap | Candidate |
| Drop ancestor trust | S9 all-candidates, writable ancestors | Candidate |
| Drop final NOFOLLOW | S9 all-candidates, final link | Candidate |
| Alias accepts suffix | S9 all-candidates, `trusted/usr/bin` | Candidate |
| Execute despite untrusted | S9 all-candidates, markers | Candidate |
| Drop final CLOEXEC | S8 script case | Candidate |
| Inherit environment (brief S12) | S8 helper environment assertion | Candidate; behavioral discriminator is S8 |
| PATH lookup | S10 isolated PATH child | Candidate |
| Timeout becomes exit_nonzero | S8 sleeping helper | Candidate |
| Rights line before TOOLS | Separate NNP-fenced Unix S6 | Candidate |
| Hard-code observed NNP true | S11 child observation | **Blocked: self-skip** |

Candidate means an exact production-fragment replacement and selected fixture
have been inspected and exercised **in memory** by Python; no Rust mutation
was written, compiled or executed locally. Real Linux compilation and actual
assertion kills remain required. A compiler error never counts as a kill.

The path-reopen mutant is limited to the selected S9c test. Its cfg(test)
branch asserts a non-root current owner, a non-system anchor and fixed ELF
helper argv, reopens only `settings.anchor()`, then resolves C1 relative to
that anchor. It never embeds or reopens actual `/usr/bin/sudo`. The PATH
mutant likewise requires PATH equal to the owned fixture anchor and opens
only its `sudo` entry. S10 discriminates the changed lookup outcome; its
default helper argv need not produce the marker/exit-zero behavior of S8.

For an actual kill, nextest must return 100 and show exactly one FAIL for the
selected full test name, no SKIP anywhere, no compiler error, and exactly one
normal `error: test run failed` footer. Padded counters such as `( 1/11)` and
the corrected E3 footer handling are ported and covered. A survivor, malformed
result or process-bound failure stops further mutations and fails the job.

## Blocking fixture gap: S11

The existing `s11_nnp_observed_in_child` asks `nnp()` whether false observation
should be skipped. Replacing `parse_nnp(&bytes)` in `nnp()` with `Some(true)`
also changes that parent decision: it prints `E4_SKIP_INHERITED_NNP`, skips the
false child, and the true child can pass. Therefore this mutation cannot be
credited as a native kill with the existing fixture. The parser-only test
would not exercise the observed-value function either.

The fixed manifest records `nnp_self_skip`. After ten actual kills, the driver
emits `blocked` / `fixture_gap_nnp_self_skip` for row eleven without applying
it, restores sources, runs final baselines, and exits 1. It cannot claim full
acceptance at this head. No skip guard was removed or weakened to fabricate
a failure, and the harness does not ask the owner to repair it.

Coordinator action: arrange a separately reviewed E4 fixture change that
determines inherited NNP independently of the function under mutation and
observes the required false/true behavior (including the shim-line coverage
advisory). Then re-pin affected bytes and the manifest, review the harness,
and obtain real ordinary-Ubuntu evidence before E4 merge. This local builder
did not edit those fixture bytes. No other definite fixture blocker was found
by source inspection; native baseline failures remain possible, including
S6's existing nine-second end-to-end assertion and S8's fixed 200 ms helper
deadline. Neither was changed or rehearsed.

## Checks actually run

On Windows, final source/test check:

- `python -I -B -m unittest discover -s tools/ci/e4-native -p test_driver.py`:
  **20 passed**. Synthetic Python children exercised exit/output capture,
  deadline termination and output overflow. Other tests exercised source and
  manifest guards, exact-fragment boundaries, restoration after exceptions and
  initial-baseline failure, both command scopes, parser rejection cases,
  visible skips, Windows/root/EUID refusal, ten synthetic kills followed by
  the blocked eleventh, and the separate workflow contract. No native/Git/
  network commands were executed by the synthetic runner.
- `git diff --cached --check`: PASS before both source and test commits.
- `git diff --exit-code 93046dd45f40ac38beb0e2a8d1e5b0da26585155 -- crates tools/ci/e3-native .github/workflows/e3-native-evidence.yml docs/PROGRESS.md`:
  PASS, confirming protected bytes unchanged.

Earlier 16/18-test runs passed during development; these are not additional
native evidence. No Cargo/native command, toolchain installation, WSL,
container, blind runtime rehearsal, hosted CI, GitHub push/watch/merge/rerun,
settings change or independent review was performed. No private state or
old Lab Manager was accessed. No existing privacy-flake, E_PROTECTION or
ExecutionFailed experiment was retried; their causes remain unresolved.

Native kills: **zero measured here**. Ordinary Ubuntu baselines and kills,
fresh Opus high review and the S11 fixture repair remain outstanding. M2 is
incomplete; production remains inactive/MechanismUnavailable. Native and
hardware paths remain **unverified on real hardware**.

## Coordinator S11 fixture correction before review

Sol medium correction `f17a7ac69892b2d6bd65502afac00428373f3756` reads PR_GET_NO_NEW_PRIVS directly
for the inherited-NNP precondition, independently of nnp(). An inherited
NNP1 still produces the explicit skip; invalid kernel observation fails.
The hard-coded nnp=true mutant can no longer manufacture its own skip.
Production bytes are unchanged. This is source reasoning, not a native kill.

The fixed manifest now has11 runnable cases and no blocked row. Source and
manifest hashes were refreshed; the portable suite validates all fragments
in memory.21 portable tests passed, including full11-kill synthetic flow
and the oldblocked-case fail-closed check. Fmt passed. No native execution,
push or CI action. Fresh Opus high review is required before publication.

Updated committed manifest:

| Path | SHA-256 |
| --- | --- |
| `crates/arxd/src/unix_shim_rights_linux_tests.rs` | `102ec5720cd1ecfd1236ebea4237025e155bcef974de27ad876b550525e059fd` |
| `.github/workflows/e4-native-evidence.yml` | `48122e681f227be7e7b2b6acc091c10782dd8685e9d71c81ffd4dcbb377834f8` |
| `tools/ci/e4-native/driver.py` | `7226a7df6e29eef8f8c8cb7e7be6b8f4c268c9a3181353de9352f42927fa9e37` |
| `tools/ci/e4-native/test_driver.py` | `57f33fe4b380119ae79dc3177bf640f6dc878a91c2b419cc78da262fdc1c5dfa` |
| `tools/ci/e4-native/mutations.jsonl` | `15980053ce88457578407686d309af95a6dd18cb175fd777d82ac0e70b850671` |

## Native harness integration-label correction

Opus high review23dee78 (retainedf78be2f) found B1 nextestintegration label
was wrong. Sol medium correction `d06e5418d502d29213f87d4d25d7d1c4d541af81` maps Cargo selector unix_shim
to reported label arxd::unix_shim using one binary configuration for both
parsing and command selection. Library label arxd remains unchanged.
The regression fixture accepts the qualified label, rejects the bare label
for baseline/kill and preserves --test unix_shim. The old driver fails it.

Portable22 tests passed. A separate owned scratch crate with no dependencies
was built offline on Windows using actualnextest0.9.151 and the driverflags:
a passing synthetic integrationtest (exit0) classified survived, and an
assertion failure (exit100) classified killed. This validates reporter format,
NOT E4 native behavior. No project test, nativeharness or CI run was executed.
Production/workflow/manifest/sourcepins unchanged. Fresh narrow Opus recheck
remains required before push. All11 native mutation kills remain unrun.

Updated committed hashes:
- `tools/ci/e4-native/driver.py`: `9b1dc47f83909fe493ed4910ddb9656c2656babe0e775b4e6cf1079daef4d658`.
- `tools/ci/e4-native/test_driver.py`: `38e14eb4fc92fff62a304d53473dffc80cbb7465a25b31b65084f613d3104eff`.

## First hosted publication checkpoint

Opus high label recheck0123317 passes B1, carrying forward the pre-push
security review23dee78. Coordinator publishes the reviewed head for first
hosted native E4 evidence. No native baseline, S6 timing or mutation kill
is claimed before the run completes. All11 kills, ordinary UID, explicit
skip/pass outcomes, source restoration and exact-head review/gates remain
required before merge. Production remains inactive.

## First hosted native E4 result

Run43504012486, attempt1, exacthead9937c16 succeeded at
2026-11-13 20:42:24-20:44:15Z (111s workflow elapsed). Root read the API
metadata and parsed the fixed JSON records from job logs. UID/euid1001.
All11 manifest labels reported killed with exit100. Source fingerprint
bdc038f1fcd64d72b3ad2c8fdab23239c0f926367d7c30fb4633e82a61895bdc
matched baselines and restored_sources.

Before/after library baselines passed in40.582/2.330s with inherited-NNP
false observation marked passed, not skipped. L8 passed in0.243/0.240s.
NNP-fenced S6 baselines passed in10.593/6.646s; the measured final baseline
is below its20s cap. Native results remain hosted owned-fixture evidence,
not real product hardware acceptance. Independent completed-evidence review
and the remaining required checks still precede merge. No retry or activation.
