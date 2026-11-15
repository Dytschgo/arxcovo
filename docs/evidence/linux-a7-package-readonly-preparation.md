# D7h fresh-host package read-only preparation

2026-11-15. Builder: OpenAI gpt-6.1-sol / high, implementation sub-agent.
Local preparation only. Fresh independent Muse architecture/security/source
review is **pending and required before publication**. No host read or workflow
run occurred. No new review approval or future main SHA is claimed.

## Baseline and review boundary

PR #123 main squash: `4f1a6db3f652291ffb94550fea85aa395f78109a`.
Locally verified tree: `6fb6ffd467d81a5f1d2cd0a39da6ae92aab5bc3b`.
Coordinator supplies equality with reviewed `40e24e7`, five green checks and
Muse PASS; these hosted facts were not fetched or re-run here. Existing source
review: [package comparator Muse review](../reviews/m2-a7-sudo-package-comparison-muse-review.md).
Read AGENTS/KICKOFF, ADR0019 D7a–g (all consumed), ADR0020 D9b, package proof,
conditional GO and comparator source review before implementation.

Local implementation parent is `4b14e15d99f78a0040e248ebe048386624db983c`, tree
`2f28d899d3b1fa6ff281b469ef83b18302a85d07`; it adds only a coordinator
PROGRESS note after the baseline. This task does not modify PROGRESS.
The exact candidate allowlist, source pins and future coordinator lifecycle are
in [ADR0019 D7h](../adr/0019-unix-native-execution-bridge.md#d7h-one-fresh-hosted-read-only-sudo-package-comparison-2026-11-15).
No driver/fixture/comparator/Cargo/package/helper/normal-CI/policy changes.

Baseline source identities (read from local git, not future identities):

| File | Git blob | SHA256 |
| --- | --- | --- |
| `tools/unix-exec-validation/sudo_package_profile.py` | `520d341633c67dc52ec483e9b3fcc92131a3da1f` | `12a122db7d30be04dd7523db046d49f4083882c318b375ccf0986ec0eb338e75` |
| `tools/unix-exec-validation/test_sudo_package_profile.py` | `00dbeb7b637df485492168e9199139f1dca85db1` | `c62f4558fdc5d2a61222599c41073c7c5140f8917f64625a71d80209602ae629` |

The comparator source is exactly 11180 bytes. Wrapper loading checks its
literal length and SHA256 before compiling those bytes. It does not change the
comparator CLI's refusing behavior or add an argument/hash/path override.

## Concrete wiring and privacy

New manual-only `.github/workflows/sudo-package-readonly.yml` uses Ubuntu 24.04,
three-minute timeout, contents:read, pinned checkout
`11d5960a326750d5838078e36cf38b85af677262` and persist-credentials:false.
There are no dispatch inputs, artifacts, secrets or permission expansion.
Distinct SUDO_PACKAGE_REVIEWED_SHA never opens the old native workflow.
Ten safe mocked tests precede the sole privilege command.

The existing coordinator-host sudo transport is explicitly covered by D7h:
`sudo -n /usr/bin/env -i` with fixed PATH and gate fields invokes system Python
`-I -B` on the absolute fixed repository script. This executes the transport
sudo, never a subject version/list/true oracle. No policy/account/proc/sys/
systemd read, native/helper/ignored execution or installation is added.

Call graph audited in source and guarded by tests:

1. Import defines the wrapper; no comparator import or private read occurs.
2. `_main -> _gate` validates repository, actors, trigger, ref, exact SHA
   triple, workflow ref, attempt 1, bounded run, fresh hosted OS/arch/image,
   real/effective root UID, no arguments and workspace/script binding.
3. Only after that, `_load_comparator -> _module_bytes` opens the fixed sibling
   source via held nofollow directories and a regular nlink1 file. Reads are
   at most 4096 bytes each, bounded by pinned size plus one EOF probe; wrong
   size/hash, drift, read/close errors refuse. All opened descriptors are closed,
   including when an individual close fails. Hash validation precedes compile.
4. Only the existing `_read_linux()` is invoked, once and with no arguments:
   three payloads, two links, ancestor metadata and fixed comparator bounds.
   No other wrapper or ordinary comparator CLI entry can select this leg.
5. `_comparison` validates every key/type/value of the fixed comparison
   schema, reconstructs known values and only then permits one JSON print.
   Match output binds source_sha/workflow_sha/run/attempt; qualified is false.
   Known refusal has null digests and binding; gate/errors/invalid schemas emit
   only `{"schema":1,"status":"refused"}` with exit 2. Private exceptions,
   partial result fields and unknown digests never print.

GitHub checkout/ref/SHA and the reviewed exact tree bind wrapper source; the
wrapper independently hashes comparator bytes and checks its script path.
These are not an attestation against a privileged concurrent writer. ADR0020
D8/A1 root-protected OS dependency resolution and privileged writer/mount trust
remain assumptions. No transitive OS/library/NSS audit or host qualification.

## Checks actually run

- `python -I -B tools/ci/test-sudo-package-readonly.py -v`: **10/10 PASS**
  on final source. Gate-field mutation, triple mismatch and old-variable-only
  authority, host/attempt/run bounds, OS/UID/argv/script/workspace failures,
  successful exact binding, sanitized refusal, missing/extra/wrong-type/schema
  fields, unknown digests and private exception silence are covered. Source
  ports are fake: fixed names/flags, length/hash/truncation/oversize/type/drift,
  bounded chunks, read/close faults and descriptor cleanup. The exact pinned
  repository comparator is compiled in memory but its private entry is never
  called by tests. No Linux package adapter or real UID call runs.
- `python -I -B -m unittest discover -s tools/unix-exec-validation -p
  test_sudo_package_profile.py -v`: **10/10 PASS**; existing fake adapter/ELF/
  readlink/privacy tests, comparator source unchanged.
- Initial discovery of the required hyphenated new test filename collected
  zero tests and failed. Corrected the workflow to execute that file directly;
  the direct command above executes all ten. No green zero-test claim.
- In-memory compile for both new Python files; strict UTF-8, no BOM, LF only;
  PyYAML BaseLoader structural assertions for the sole manual trigger/job,
  contents permission, timeout/runner, three steps, checkout credentials and
  distinct variable: **PASS**. No actionlint or hosted workflow validation run.
- AST/call-site audit: the sole private read edge is after `_gate` and pinned
  module loading; comparator import/CLI still has no real-read edge. Workflow
  test verifies tests precede privilege and prohibited surfaces stay absent.
- Final `git diff --check`, exact changed-path/mode and baseline immutable-blob
  comparisons, syntax/encoding checks: **PASS**, recorded before commit.

New executable-source SHA256 pins (LF bytes; this note does not hash itself):

| File | SHA256 |
| --- | --- |
| `.github/workflows/sudo-package-readonly.yml` | `fe7e423573e43299c794f58c3dff0a9b9cbc69512654a1c973a32c90e84f94a1` |
| `tools/ci/sudo-package-readonly.py` | `a40f58ae124b1d4136197cada6bb0cec08278ae04668d0ebca633b14a66f3f0c` |
| `tools/ci/test-sudo-package-readonly.py` | `ac114477eea3bc2863376c6c93954f0a9394527ca99b16002798edf4c3330ceb` |

## Coordinator work still pending

Fresh Muse architecture/security/source review before publication, exact-head
different-provider review, five required green checks, coordinator merge,
reviewed-tree/squash-tree equality and current-main equality are still required.
Reviewed head/tree, eventual squash/tree, pin readback, directly returned run
URL/attempt, terminal outcome and final independently verified variable HTTP404
are **not yet available**. Record their actual identities later; no speculative
SHA/run/approval is embedded here.

Only then: one pin/readback and one dispatch under D7h. Hold the pin until the
exact directly bound run is terminal; an empty/failing ancillary lookup is
never rejection or retry authority. Any accepted dispatch consumes the grant,
including skipped/failed runs. Finally delete and independently verify HTTP404;
no retry, rerun, redispatch or host reuse. Existing user authorization suffices;
this is a technical fence, not an additional owner approval flow.

This task performed no root/host package read, sudo/proc/native/helper/ignored
execution, account/admin/install operation, dispatch, variable operation,
network request, push, PR, subagent or old Lab Manager access. candidate_match
will establish only the read-only FILE leg; C2/floor/in-unit four-mask view,
fixture literals/NSS and actual NNP allow/deny cases remain open. A7/A11/M2/full
F/G/real-Runner PA6 remain **unverified on real hardware**.

## Coordinator review correction F1

Independent Muse `6d8dab4` passed all axes with fail-closed atime finding F1. Added an injected module-read regression: atime-only change must pass; each dev/ino/mode/uid/gid/nlink/size/mtime_ns/ctime_ns change must refuse with all descriptors closed. Exact regression reproduced the pre-fix false refusal (ValueError), then passed after comparing only those nine stable fields. All11 wrapper tests and10 comparator tests pass. The first invocation used a wrong test class name and earned no credit. No host read, privilege or dispatch occurred. Source pin table above is refreshed for this coordinator correction; comparator pin and workflow are unchanged.

## Coordinator candidate and dispatch binding

Reviewed head `895af0c3f0e1a28527088893e6f3c3958633555a` and main squash `47b440a5faeaa24434e155b81d5d03a5c977e4e9` share tree `b490c0479564ad895201e2a7872cb9fd11dad721`. Squash parent is baseline4f1a6db; exact seven allowed paths and new100644 modes match. All other execution bytes, comparator blob/11180-byte hash included, remain baseline. Accepted run43693642693 attempt1/job132749349754 binds this candidate. Main/actor/ref and absent new+old pin variables checked before once-set/readback/once dispatch. Monitor uses the returned URL directly. Terminal result and finally404 remain pending; this local note does not advance main or grant another run.
