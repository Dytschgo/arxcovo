# M2 macOS absence collector source review

Disposition: **no source blocker.** The fixed collector publishes a closed
literal report, keeps raw streams private, and does not fill the production
accept-set. This review grants **no CI edit, hosted execution, fixture credit,
nonempty parser profile, merge, or hardware credit**. Later ordinary hosted
read-only execution is safe for the coordinator to authorize separately. The
coordinator, not this reviewer, grants that execution.

Reviewer: xAI / grok-4.7, one pass, 2026-11-14. The assignment forbids
subagents, so Standards, Spec, and security are this single reviewer.
Independence is from the Sol implementation, not from a second reviewer.
Label: single-agent review.

M2 is **incomplete**. Real-Runner PA6 is **OPEN**. Linux Stage F evidence and
Stage G production wiring are **OPEN**. Installed, dedicated, and macOS
launchd paths are **unverified on real hardware**. Windows main71 run
`43545244266` attempt 1 has cause **unknown**; no retry is authorized. The
optional Windows replay harness stays parked. No native evidence or capture
authority is created here.

## Pins

Porcelain was empty on entry and stayed empty through the checks below. The
review cwd is the linked worktree top. Its gitdir is that worktree's own
gitdir and differs from the common gitdir. The branch has no upstream. Paths
below are repository-relative. No owner path, account, or machine path is
recorded. Inspection was Git, documents, the collector and its tests, the
unchanged manager collector, `ci.yml`, ADR 0014, the production `launchd`
function, and CPython 3.12 `plistlib` for exception types. No launchctl,
collector CLI, native query, provenance file read, process, account, service,
system, VM, private-state, owner, install, network, GitHub, CI, or dispatch
ran.

| Item | Identity |
| --- | --- |
| Review branch | `t3/review-macos-absence-collector` |
| Supplied head | `f072a170724c9a2f194f41b65ce6e8f10ecc8f5e` |
| Head tree | `d3540e7af9e331a9333b3affbd639517d88cc406` |
| Head parent | `54e584f00986377e33f88c6c3691c827f0a0a54c` |
| Actual main | `cbc908ca54fce7a5f2d9c1dd922c928ce5ba0fec` |
| Actual-main tree | `7ab5610a4645a9ccfbb349504a214d85bf797897` |
| Merge-base of head and actual main | `aa49ce1e99475d07d63c6b0d23fa94bdb30bab74` |
| Sol source | `1cc2cd186eda4827147373b381d389e7b2816f83`, parent `fedee69bf8e2b244f787de15ead410412156bcfe` |
| Retained source | `ec5731894405f1c5399e8a426e82871f6a22e6bc`, same parent |
| Source tree at both | `44219cfb9f0a3a66194cd5ec47b7a2d89878dd5c` |
| Sol evidence | `d8cf865df1e250515b60a5b8bdefa88ef3289775`, parent `1cc2cd186eda4827147373b381d389e7b2816f83` |
| Retained evidence | `54e584f00986377e33f88c6c3691c827f0a0a54c`, parent `ec5731894405f1c5399e8a426e82871f6a22e6bc` |
| Evidence tree at both | `aad79bee7360d1a15da5350f313b4d45ffb32cda` |

`1cc2cd1` and `d8cf865` are not ancestors of this head. `ec57318` and
`54e584f` are. `git diff` between each supplied commit and its retained
commit is empty. `cbc908c` is not an ancestor. `f072a17` changes only
`docs/PROGRESS.md` relative to `54e584f`. Collector and evidence blobs are
identical at the Sol commit, the retained commit, and this head.

Against `cbc908c`, the collector scope is four files, 603 insertions.
`git diff --check` on that scope was empty. These blobs match `cbc908c` and
this head: `tools/ci/observe_manager.py`
`c7104fe1c827f992a90fa3d40668ada3455a0efa`, `.github/workflows/ci.yml`
`fc065af9786e75c52cfb7668b726d0a89931cd5b`, ADR 0014
`d3e03b9796d12e2a015d5dd343c2d622a6fb3731`, `owner_query.rs`
`20c51c17ba24e5d3b5cecfad2af5d2b662445888`. Service, workflow, and ADR diffs
against `cbc908c` were empty.

| File | Blob at head | LF SHA256 | Bytes | CR |
| --- | --- | --- | --- | --- |
| `tools/ci/observe_launchd_absence.py` | `923b0374d28d3623c4cbb3a4d12c645721b8fe7d` | `f5748bcc0d370d63b80a1c364780ee17ee1d5399af0527d7b92303757ec1cc27` | 7457 | 0 |
| `tools/ci/test_observe_launchd_absence.py` | `d78684c1e6f4be95b9787fe12610cd2372e2c2c3` | `e4ef8f4d9bd64b0e5ad73aaf202563bdcdce46f00ac50191c8acfc787efc3d05` | 10608 | 0 |
| `tools/ci/README.md` | `2acc3dcf6aa55653378efcbd410cc9a65a1c1be0` | `e3564c7ebb2e27a7509ba2f8354970b0eed40f96d0863364401883ff2714dc53` | 4375 | 0 |
| `docs/evidence/macos-launchd-absence-preparation.md` | `95edca46e848f2a387c3be7a6000482cb8cd4852` | `23f67b0cbde8c80260f6a4845211018a41a172c7552ee543a14e4db47b0a958a` | 11702 | 0 |

The proposal blob matches at `657d06a`, `f560c21`, and this head:
`93f3c6ccc8725e40f542818826551e4b1fd71829`. The design-review blob matches
at `b353a21`, `476fe06`, and this head:
`abf56af2045be6d3e243654918f6fd50d1799035`. `657d06a` and `b353a21` are not
ancestors. `f560c21` and `476fe06` are.

Documents read: `AGENTS.md`, `CONTRIBUTING.md`, `docs/KICKOFF.md`, the review
skill, ADR 0014 including the empty-profile addendum, the accepted proposal,
the Grok design review, the evidence record, `tools/ci/README.md`, and the
current `verify` job. Code read: the new collector and its tests, the macOS
branch of `observe_manager.py`, and `owner_query::launchd`.

## Checks

- `python -B -m unittest discover -s tools/ci -p test_observe_launchd_absence.py`
  ran 12 tests, OK, 0.040 s. The pattern does not load the manager suite.
- Nine compile-valid mutants were applied to an in-memory copy. Disk source
  was not rewritten. Each mutant had zero test errors. Killing assertions:
  `first-excess-truncated`; `stream-identity` and `retained-bound`;
  `real-exit` and `real-exit-after-eof`; `complete-bounded-tuple` for both
  the EOF mutant and the truncation mutant; `whole-tuple-stream-code` and
  `closed-publication-schema` for code 113 alone;
  `closed-publication-schema` for published stdout;
  `query-two-seconds` and `absolute-query-bound`; `cleanup-one-second`.
- A pure `summarize` of the exact candidate returned `query` `Unknown`,
  `fixture_credit` false, `diagnostics` `withheld`, and `tuple`
  `synthetic-candidate-1`. The JSON contained neither the candidate
  diagnostic text nor a planted private marker.
- Metadata `fullmatch` rejected a path, a UID phrase, a secret marker, a
  short or uppercase SHA, and the historical label `macos-26-arm64`. It
  accepted only the closed shapes in the source.
- Four in-memory `plistlib` samples, not the host file: a missing-key
  `ValueError` embeds the key and is inside `main`'s handler; an array root
  raises `AttributeError` whose message has no planted marker; malformed XML
  raises `ExpatError` with a line/column message and no planted marker;
  an entity declaration raises `InvalidFileException` and is inside the
  handler.

## Standards

**PASS. No hard breach. One judgement-call smell.**

Repo rules that bind this change are `AGENTS.md` and `CONTRIBUTING.md`
(honest hardware labels, no claimed unrun checks, no secrets, no owner
administration, meaningful tests) and `docs/KICKOFF.md` (v1 order, technical
fences, hosted CI gates). The change is one conventional preparation concern.
It does not edit production service code, ADR 0014, or CI. The 12 tests are
behavior tests with mocked transport and metadata seams. No credential or
owner path was added.

Fowler baseline: one smell.

- **Mysterious Name**, judgement call. The public field `tuple` with value
  `synthetic-candidate-1` (`observe_launchd_absence.py` lines 119–120) can be
  read as an accepted grammar id. The value is one of two literals, and
  `fixture_credit` is the constant false. A name that says the id is only a
  source-literal reference would be harder to misuse. This does not block
  the slice.

The second length check in `summarize` is the contract's own recheck, not
duplicated code to extract. Tooling does not enforce the field name. No
other baseline smell is filed.

## Spec

**PASS. No blocker. Three advisories.**

Checked against the preparation evidence, which is the originating spec for
this collector, and against the accepted design review's evidence gate. The
ADR addendum at lines 579–587 still requires a reviewed full tuple before
any production absence proof and still says exit 113 alone proves nothing.

Implemented and inspected:

- The only child argv is `["/bin/launchctl", "print", "system/com.arxcovo.arxd"]`
  (lines 12 and 38–40). The test locks every `Popen` keyword: null stdin,
  pipes, `shell=False`, cwd `/`, `bufsize=0`, and the three-variable
  environment (test lines 55–59). There is no second command, Apple control
  target, label argument, or shell.
- The query budget starts before spawn and is 2.0 s (line 36). Each pass
  reads at most 1024 bytes once from each non-EOF stream, asks for one
  excess byte past a cap, then polls and sleeps at most 5 ms (lines 49–77).
  Caps are 65536 and 4096. Their sum is the aggregate 69632, and the
  aggregate is checked before a byte is stored (lines 14–15 and 61).
- `completed` requires a nonnegative `poll` result, both EOFs, and the
  deadline (lines 72–76). A negative code becomes `signalled` with a null
  exit code. Exit without both EOFs stays `timeout`. Spawn, setup, read,
  wait, kill, reap, and close failures use fixed labels (lines 41–103).
  Cleanup is attempted only when status is not `completed`, and its wait
  budget is at most 1 s. No cleanup label selects the candidate.
- `summarize` emits `query` `Unknown` and `fixture_credit` false on every
  path (lines 115–120). `synthetic-candidate-1` requires the whole
  code/stdout/stderr candidate. Exit 113 with empty or swapped streams,
  loaded-shaped bytes, truncation, a missing EOF, or an over-cap length
  stay `unsupported`.
- `owner_query::launchd` still ignores its observation and returns
  `Query::Unknown` (lines 225–227). This collector is not wired to it.

Advisories:

1. `main` catches `OSError`, `ValueError`, `TypeError`, and
   `plistlib.InvalidFileException` (lines 161–164). A non-dict plist root
   raises `AttributeError`. Malformed XML raises `ExpatError`. A deeply
   nested binary plist can raise `RecursionError`, which the binary parser
   does not convert. Those three escape the fixed provenance label and
   print a traceback. The missing-key `ValueError`, which does embed the
   key text, is caught and is not printed. See Security for why this does
   not publish launchctl bytes.
2. `tools/ci/README.md` calls the cleanup "at most one second". The source
   comment at lines 82–84 says blocking spawn, kill, and close latency is
   not preemptible. The evidence document states the same limit. The README
   sentence is the stronger claim.
3. The kill-fault case also plants a read `OSError`, so the test does not
   isolate `kill-error` (test lines 108–125). The candidate-success path
   checks the tuple id and not the whole dict (test lines 133–135). The
   16385-byte read sentinel, a non-regular file, and a rejected environment
   value are implemented and untested. Inspection and the reviewer oracle
   cover the success dict. These gaps do not show a wrong result.

## Security

**PASS. No blocker. The handled publication boundary holds.**

The public object is a fixed set of literals, booleans, one integer exit
code or null, and five regex-bounded strings. Stream bytes, byte lengths,
paths returned by the query, UID, account, home, hostname, and any other
environment value are absent from that object. Nothing hashes the streams.
`Capture` uses `repr=False`. Handled `OSError` text is not stored. A planted
marker in stdout, stderr, argv, and a provenance `ValueError` stayed out of
the serialized result and the entry stderr in the mocked tests.

Metadata reads one fixed public plist path with `O_RDONLY`, `O_NOFOLLOW`,
and `O_NONBLOCK`, requires a regular file, and stops at 16384 bytes with a
16385-byte sentinel (lines 125–134). Only `ProductVersion` and
`ProductBuildVersion` are taken from the parsed object. Only `GITHUB_SHA`,
`ImageOS`, and `ImageVersion` are read from the environment, each with a
`fullmatch` (lines 140–146). The alphabets are digits, dots, one bounded
Apple-build form, 40 lowercase hex digits, and `macos` plus two digits with
an optional `-arm64`. A hostile key or string that fails the match raises
an empty `ValueError` before `capture`. The historical evidence label
`macos-26-arm64` does not match `ImageOS`. This review did not read a live
runner environment, so a hosted mismatch remains possible. Mismatch exits 2
before the query.

`CANDIDATE` is a source literal (line 18). It is not a native fixture. The
historical manager summary records exit 113 and phrase booleans for an older
collector. It is not the stdout/stderr pair, and this collector does not
read that file. Exit 113 alone stays `unsupported`.

The uncaught provenance exceptions in Spec advisory 1 happen before
`capture`, so the traceback cannot contain launchctl output. In the checked
samples, `AttributeError` and `ExpatError` did not contain the planted
marker. Entity declarations become the caught invalid-file error. The
residual is a traceback of collector source in the job log if the public
plist is not a shallow dictionary. That is not a raw private payload.

Production capture is a different component and is unchanged: 10 s deadline,
PATH `/usr/sbin:/usr/bin:/sbin:/bin`, and `launchd` returning `Unknown`.
This diagnostic's PATH is only `/usr/bin:/bin` and its budget is 2 s. The
argv is absolute, so PATH does not select the binary. A launchctl failure
from the narrower environment stays `Unknown`. Integration must not widen
the environment or the budget inside the workflow.

### CI proposal

The current step is `.github/workflows/ci.yml` lines 38–42: `if: runner.os != 'Windows'`,
then the manager unit tests and `python3 tools/ci/observe_manager.py`.
On macOS that program prints both `system/com.arxcovo.arxd` and
`system/com.apple.syslogd` (`observe_manager.py` lines 14–15 and 127–130).
On Linux it runs the existing `systemctl show` property list only.

The evidence proposal changes that condition to `runner.os == 'Linux'`,
leaves the `run` block as it is, and inserts one macOS step after it and
before `cargo fmt`: the new unit tests, then
`python3 -B tools/ci/observe_launchd_absence.py`. Evaluated against the
current job, that edit:

- removes the Apple control query and leaves one arxd `print` on macOS;
- leaves the Linux commands unchanged;
- does not add a second native print;
- does not add `sudo`, a secret, `workflow_dispatch`, an artifact upload, a
  shell pipe, or a new permission;
- does not change `permissions: contents: read`, `timeout-minutes: 12`, the
  stock-directory step, the required-check set, or `ci-time-budget`.

`workflow_dispatch` is already on this workflow (line 6). The proposal does
not add a dispatch path. The new step would run only as part of the existing
`verify` job. The stock-directory test opens `/bin/launchctl` as a file
(`native.rs` lines 561–565) and does not execute it. The existing Linux
mirror `sudo` step is untouched. Default `bash -e` stops the new step before
the query when the unit tests fail. The script returns 0 for a sanitized
`Unknown` report and 2 for a provenance failure. Exit 0 does not mean the
candidate matched. Exit 2 fails `verify` closed. Neither result is absence
proof. No job-duration measurement was made here.

The proposed commands use `-B` and not `-I`. The job does not set
`PYTHONPATH`. Isolated mode would reject an injected path and is optional
hardening, not a missing control in the current job.

## Native proof remaining

Native proof from this review is **empty**. The mocked tests and mutants do
not establish pipe EOF, kill, reap, signal delivery, truncation, or the
candidate bytes on any OS. They do not establish that exit 113 means the
service is absent. A later exact match may set `tuple` to
`synthetic-candidate-1` while `query` stays `Unknown` and `fixture_credit`
stays false. That id is a pointer to the source literal. It is not a
compatibility fixture, not production `Absent`, and not a nonempty parser
profile. `owner_query::launchd` still discards every observation.

No installed Apple manual was read. No live `ImageOS`, `ImageVersion`,
product version, or product build was read. Architecture is present only
when `ImageOS` itself carries the optional `-arm64` suffix. The public
source pin is an environment claim, not a git attestation. Installed
lifecycle, boot, restart, and dedicated execution remain **unverified on
real hardware**.

## Publication and CI integration

Publish this review. Do not publish the candidate bytes, the historical
summary, or a future job log as a fixture. Do not copy the literal into
`owner_query`. The production accept-set stays empty.

The exact CI edit in the evidence document is safe for a later
coordinator-authorized change to `.github/workflows/ci.yml` and nothing else.
Apply it as written: Linux-only condition on the existing manager step, the
existing Linux `run` block unchanged, and the new macOS step in the same
`verify` job. Do not keep `observe_manager.py` on macOS. If provenance
exits 2, stop and return to source review. Do not print the environment or
widen a regex from the workflow. A green job is a sanitized report, not
acceptance.

This review does not make that edit and does not run the collector. The
coordinator grants any later execution after this review. M2 remains
incomplete.

## Close

Standards: 1 judgement-call smell, 0 hard breaches; worst is the public
field name `tuple`. Spec: 3 advisories, 0 blockers; worst is provenance
exceptions outside the fixed-label handler. Security: 0 blockers; worst
residual hazard is treating `synthetic-candidate-1`, exit 113, or a green
verify job as absence or a nonempty profile.
