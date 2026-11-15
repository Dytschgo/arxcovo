# A7/A11 qualification: injected source only

Sol / gpt-6.1-sol / high, 2026-11-15. Base main
`d18a721d601aa625698826399df0b0820d3f0290` (PR119); local parent
`3be59fe94a2c4225ae891a877d63e9d5f68597e2` changes PROGRESS only.
PR119 five-green/Muse PASS/tree equality to `991bc75` and hosted Linux
compilation are supplied coordinator evidence, not rerun here.

Read AGENTS/KICKOFF, current PROGRESS, the exact qualification section and
binding corrections in `docs/reviews/m2-a7-a11-delivery-contract.md`, its Muse
contract review, sudo-NNP plan and Muse source/delivery-source reviews.
Only the two new qualification Python files and this evidence file change.
Parent/coordinator owns PROGRESS. No workflow, driver, fixture, ADR, wiring,
profile, TEST, False/None latch or execution gate changes.

## Call graph and boundary

Audited the AST call graph before running mocks. Collector imports only hashlib,
itertools, json and stat; no OS read adapter, subprocess, environment selector,
network, driver import or production caller. `_collect(ports)` is private and
requires explicit injected syscall-shaped read ports. Module import performs no
collection; the only CLI entry returns exit 2 and the fixed
`root_reads_not_authorized` record. Tests invoke that function with reader
construction poisoned, without launching a process.

The private reader models root-down descriptor-relative read-only
O_NOFOLLOW/O_CLOEXEC/O_NONBLOCK opens, directory type, owner0, no group/other
write, single-link regular files, bounded chunk reads and end-of-snapshot stat
stability. No arbitrary path/command argument exists. Fixed policy files are
`/etc/sudo.conf`, `/etc/nsswitch.conf`, `/etc/sudoers`; each <=16KiB.
The entire `/etc/sudoers.d` set is mandatory, <=32 single-link regular files,
each <=16KiB and aggregate <=128KiB, including names sudo might ignore.
Bounded enumeration refuses overflow, duplicates, unsafe components and drift.
No includes are followed and no policy/plugin/NSS syntax is interpreted.

Installed C1 `/usr/bin/sudo` and optional C2 `/usr/local/bin/sudo` use the same
protected ancestry/type checks, executable bit, ELF magic and <=16MiB cap.
`protected_elf` is a bounded structural observation, not plugin/package trust or
proof that an image is a functioning sudo executable. `/bin` must be a
root-owned link exactly `usr/bin` or `/usr/bin`. The fixed injected floor port
requires systemd>=254, kernel>=5.14, cgroup2, memory+pids, user.delegate and
protected hierarchy booleans. It does not discover these on a real host or
substitute for the existing hierarchy/delegation checks. A Linux read adapter
and its exact floor measurement provenance remain unimplemented/unreviewed.
Concurrent privileged mutation is outside the snapshot trust assumption;
this output must never be used as an execution-time attestation.

Output keys/labels are fixed: version, status, qualified, policy_reviewed,
floor_observed, bin_alias_observed, c2_state, include_count, records. Records
contain hashes/sizes only under six fixed labels; include names+bytes are
length-framed into one aggregate digest, never a listing/per-entry oracle.
Exceptions and close failures reduce to one constant refusal, without paths,
identity metadata, raw bytes, exception strings or traceback. No raw capture,
storage, artifact server, callback to publish bytes, or audit framework exists.

## Exact policy review remains a blocker

There is no reviewed literal candidate or allowlist. `_REVIEWED_POLICY` is empty
and cannot enable success: all complete structurally safe snapshots return
`policy_unreviewed`, qualified=false and policy_reviewed=false. Unknown includes,
plugins, external NSS routes and all other unknown policy bytes therefore cannot
earn qualification. Hashes, ELF magic and syntax success are never policy proof.
No host bytes, future hash, installed-image equality or semantic proof is invented.

Minimal strategy for the next review: select a closed *public source-literal*
candidate, pin its actual source revision, and independently review the complete
exact literal bytes and include names, local plugin bytes/routing, NSS/defaults
and generated sole-group account applicability for both cases. A future narrowly
reviewed private comparison can check host bytes against those literals in memory
and release only equality flags/digests. No candidate has been selected/fetched
or assumed equal to runtime here. The existing generated case0 rule literal in
`driver.py::prepare_sudo_nnp` is a reference for that case only; it says nothing
about the host's other grants or case1 absence.

If the host differs from a public candidate, exact bytes need review through an
explicitly approved private channel without public artifacts, logs or owner-data
release. No such channel is settled by this change; digest-only output cannot
solve that review. This is a required acceptance blocker, not optional diagnostic
infrastructure. Do not invent a private upload/storage mechanism or enable a
literal/hash allowlist without that review.

## Checks actually run and remaining authority

- `python -B tools/unix-exec-validation/test_sudo_host_qualification.py`:
  10 tests PASS, including 17 in-memory security mutants assertion-killed by
  the nine base tests (153 mutant/base executions). No subprocess or host ports.
- Mock coverage: owner/write/type/link/ancestry, single-link files, file and set
  limits, early oversize refusal before reads, truncation, complete-set drift,
  unknown includes/plugins/routes, C1/C2 ELF/executable checks, both exact aliases,
  floor refusals, held-inode drift, silent read/close failures and refusing CLI.
- AST syntax compilation and UTF-8/LF checks on all three new files;
  `git diff --check` (including staged additions): PASS.

The first mutation run exposed missing early-size-read assertions; the second
exposed missing per-policy-file truncation coverage. Tests were strengthened;
no authority was relaxed. Final run killed all 17 mutants.

Fresh independent Muse source review is required before publication; none is
claimed here and no subagent was started. No push/PR/hosted checks, root read,
sudo/account/proc/probe/native/helper/ignored/LinuxFs/network/install/dispatch/
variable operation ran. A new narrow independently reviewed root-read-only grant
and adapter review must precede real capture. Execution requires its separate
fresh-host policy/profile and delivery grants. A7/A11/M2/full F/G/real-Runner PA6
remain open and **unverified on real hardware**.
