# ADR 0020 E3 local inactive implementation evidence

Builder: OpenAI / gpt-6.1-sol / high, 2026-11-13. Implementation sub-agent,
no delegates. Independent Opus 5.5 high security review is **pending and
required before publication**. This evidence grants no activation.

## Isolation, dependencies and source

Get-Location and git rev-parse --show-toplevel matched the supplied isolated
checkout, outside the canonical checkout. Initial git status --short was empty;
HEAD was exactly `be629c810a08ce30e4b0812d53bcba89f62677be` (reviewed E2 PR60).
The canonical checkout was never accessed. Read AGENTS, KICKOFF, ADR 0020,
the entire implementation brief, E1 parsers, E2 port/driver and prior evidence,
and the existing P-SCAN tokenizer. The direct task overrides coordinator,
babysitter and PROGRESS duties. No GitHub state was refreshed.

Supplied dependency disposition: E2 review
`dd4dad86ead64e5b3cc4947254d27a33ab86bb13`, recheck
`352a8c1d762461b38e0f90223e9ec5712b32e975`, PR60 comment `6861791400` pass;
all gates except Windows pending at handoff. These are dependency pins, not
an E3 review. E2 merge, current-base checks, exact re-pin and independent
integration review remain required before publication/integration.

Source commit: `eb9362089162d248e9d40081d9b393f5a25b6f97`.
Contract blobs at that commit: ADR `fae2048431c77141503d0fec7fbdaa54288d4f52`;
brief `07ecbf698b210752c12c9c14ab8e8b3a15e74f6a`.

Git blobs and SHA-256 of **committed bytes** (git show, avoiding worktree
newline conversion):

| Path | Git blob | SHA-256 |
| --- | --- | --- |
| `crates/arxd/src/exec/hierarchy.rs` | `01897160fe5514f5119c01589702b0a4661c46df` | `3fc924602dca90afa7444c50f03b3786704a3570bf5f8d79e34344ee7e957100` |
| `crates/arxd/src/exec/hierarchy/linux.rs` | `667a2830e70ea103ffcabe7ff29aa724428dc513` | `babcfb786e0b580adadd5e68677638a9fdd34376212c42ab1a58efa3bc5326ea` |
| `crates/arxd/src/exec/stage_b_tests.rs` | `c00f33f1e79c998e525e58ef9b542533a2a08e84` | `d67fd687b360c1a78c1702a4bdf7d54049f674d7a82a320d4f1a6fd1862dcb56` |

The source diff against the initial pin contains exactly these three paths.
Hierarchy root gains only the Linux module declaration. Stage B test source
has an appended E3-only extension using the actual existing tokenizer and
payload scanner. All original Stage B test bytes and all E1/E2/E5 bytes are
preserved except the explicitly permitted root declaration. Cargo, CI,
PROGRESS, service wiring, production selection and capabilities are unchanged.
Production stays MechanismUnavailable. No E4/E6/E7 implementation.

## Adapter and test-source coverage

System constants are UID 0, procfs 0x9fa0, cgroup2 0x63677270. The private
cfg(test) owned(uid) bypass affects only UID/magic equality in the common
trust check; mode, type, no-follow, read bounds and listing code remain shared.
No test constructor exists in production. Stat returns actual kernel facts.
All cgroup operations use a bound Dir containing OwnedFd. Root and protected
sys/fs ancestors are checked before reaching cgroup2. Opens use openat with
NOFOLLOW/CLOEXEC and directories add DIRECTORY. Own proc observations bind
/proc then the numeric own PID directory (the descriptor-relative equivalent
of kernel /proc/self), avoiding following the self symlink; only own status
and cgroup are exposed. Each proc file is fstat/fstatfs checked and bounded.

Control opens add NONBLOCK before fstat rejects non-regular objects, so FIFO
substitution cannot wait for a peer. Reads consume at most max+1, clamped to
4096 for controls, and oversized reads map EFBIG to the port's TooBig.
Writes are at most 4096 bytes and one syscall transaction; short writes fail.
Successful file operations explicitly close and propagate close errors;
Linux close is never retried. OwnedFd closes directories and failure paths.
Each listing opens a fresh description for dot, transfers it once to a new
DIR stream, checks readdir errno, counts dots against the raw budget, and
propagates successful-path closedir errors; early failures close via Drop.
There is no descriptor duplication or cached stream. Unknown entry types use
fstatat with AT_SYMLINK_NOFOLLOW; classification delegates to E1.
Xattrs use fgetxattr with a two-byte buffer. mkdirat passes exactly 0700.
Errors are fixed FsError values; no paths, file bytes or panic payloads escape.

These Linux test sources exist but were **not compiled or executed** here:

| Brief | Test | Intended assertions |
| --- | --- | --- |
| L1 | l1_bounds_and_bounded_writes | 4096 pass, 4097 TooBig through text/read; oversized write leaves file intact; valid write and unsafe component refusal |
| L2 | l2_nofollow_and_nonblocking_fifo | File/child symlinks refused, target unchanged, FIFO read/open_ro/write refuse without waiting |
| L3 | l3_raw_budget_includes_dots | 510 files plus two dots pass at 512; one more fails |
| L4 | l4_entry_kinds | E1 classifier on directory, regular, link and unknown stat outcomes |
| L5 | l5_system_constants_and_shared_mode_trust | UID/magics, production owner refusal, shared writable-mode refusal; real runs mode 0700, Exists and rmdir |
| L7 | l7_xattr_bounded | One-byte xattr, oversized xattr refused; explicit skip only for filesystem refusal |
| L8 | l8_unprivileged_hardening_zero_changes | Own status precondition; Hardening with exactly one S0 read, zero mutations, no later observations; explicit root/unexpected Standard skips |
| L9 | l9_fresh_descriptions_and_owned_lifetime | Complete repeated listings, addition/removal visible, observer FD distinction/CLOEXEC/unchanged handle offset; handle survives path rename |
| L10 | l10_real_entry_kinds | Directory/control accepted; leaf-like file, symlink, FIFO, bound socket refused; target unchanged |

Owned native fixture tests skip root with a fixed explicit marker. L8 runs
only on supported non-root Linux, reads own status and performs no mutation.
L7 was **not run** here; no xattr pass or runtime skip is claimed.

L6 `l6_e3_tokenized_source_inventory_and_mutants` runs portably. It scans
linux.rs and hierarchy.rs with P-SCAN, excludes their trailing test modules,
and rejects the required path, signal, Command, hook and duplication tokens.
Synthetic negative controls cover each forbidden token (including F_DUPFD
variants) and diagnostic payload; comments and strings are positive controls.
It does not modify the broader E6 inventory. The native mutation obligations
map to L1 (raised read limit), L2 (NOFOLLOW), L3 (record bound), L9 (shared
listing description or missing CLOEXEC), L10 (symlink classification), L5
(system test UID). **No Linux mutation kill is claimed:** these are source
coverage pins requiring actual compiled-target execution and review on Linux.
Only L6 synthetic scanner controls were executed on this host.

## Commands and limits

Host: Windows x86_64-pc-windows-msvc; rustc 1.95.0
(59807616e 2026-04-14), nextest 0.9.151 (6b3f99457).

- `cargo check --locked --workspace --all-targets --target x86_64-unknown-linux-gnu`:
  blocked in native dependency build: cc-rs could not find
  `x86_64-linux-gnu-gcc`. Installed Rust targets were inspected; no compiler,
  WSL, toolchain or dependency was installed. This is **not a real Linux crate
  typecheck**. No substitute/stub typecheck was used or credited.
- `cargo fmt --all --check`: passed on final source.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: passed;
  final source run 2.68 s. Earlier run passed in 39.49 s.
- `cargo nextest run --locked -p arxd -E 'test(exec::hierarchy) | test(service::unix_bridge) | test(exec::stage_b_tests)'`:
  final commit source **60 passed**, 303 skipped, 2.337 s,
  run `e677ae9a-1dc1-4f41-ba1a-12148db7db5c`.
  Initial focused run also passed 60/303, 2.026 s,
  `92841830-465f-4c55-8fe6-73cc659eda6f`, before the final test-source
  lifetime assertion refinement. No failing test was retried.
- Exactly one final normal `cargo nextest run --workspace --locked`, after
  source settled at the source commit: **362 passed, 3 skipped**, 11.648 s,
  run `2e08a4cf-7521-4158-834a-59e577f6a82b`, exit 0. No retries or green chasing.
- `git diff --check` and staged whitespace check: passed. Source tree clean
  after source commit. Git diff/name and tree blobs establish preservation.

Known unchanged dispatcher privacy flake 65762554 did not recur. Its cause
remains unknown; no service fix or database dump was made. Windows results
prove no Linux syscall path. L1-L5/L7-L10 and native mutation runs remain
unverified; Linux Clippy and actual ordinary-user native execution are still
required. Full hosted checks were not run or requested by this builder.

No provisioning, system cgroup mutation, owner/account/service/VM operation,
experiment, private-state access or ignored native fixture was used. No push,
PR, CI action, watch, merge, rerun or subagent. No architectural deviation is
claimed. E2 merge/re-pin and independent Opus high security/integration review
remain publication gates. M2 remains incomplete. Native paths remain
**unverified on real hardware**. E_PROTECTION/ExecutionFailed remains unknown;
no diagnosis, retry or activation is authorized or claimed.

## B1 test-only correction

Opus high source review de2cc44 (retainedf38d082) found L1 did not detect
over-reading followed by TooBig. Sol medium correction `c13fc98cc494a836676e39acff70a4494b477171` adds a
Linux-only pipe test: 32 bytes are written, text(reader,7) must return TooBig
and leave exactly24 unread through a second read handle. Writer closes before
reading, so EOF is bounded and the32-byte preload fits the minimum pipe capacity.
Production text and every other production byte are unchanged.

Windows fmt passed. Portable L6 and P-SCAN tests passed4/4, run
2a369d29-5763-41ce-8aaf-79e6b4771a50. The pipe test was NOT compiled or run
on Linux here; no native mutation kill is claimed. No full-suite repeat for
Linux-test-only changes. Hosted Linux must run the new test and its read-limit
mutants. Narrow independent recheck is pending before publication.

Corrected linux.rs blob `945e34ccc1ab47c0836da5d2b4d5747df12594c6`; committed SHA-256 `c45d2a416f8504cba34368c5d11558f00d149d4cc31b208ecb41a4b4bbeeb863`.
