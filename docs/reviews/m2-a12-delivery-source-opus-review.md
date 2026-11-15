# Independent Opus review of Sol A12 closed delivery/driver source

2026-11-15, Anthropic Opus 5.5 / high, review sub-agent, no subagents.
Author: OpenAI gpt-6.1-sol / high.

## Target and provenance

- Branch `t3/review-a12-delivery-source`, review parent `8db16bb`, base main
  `cbab8b69f7204c81e4490da107dc1bca3282cf0c` (merged PR136).
- I checked that each retained commit is byte-identical to the original:
  `git diff X^ X` compared with `cmp`.

  | Original | Retained | Content |
  | --- | --- | --- |
  | `55bcf5d` | `ed94cc8` | Sol cwd handback: regression and evidence |
  | `3f31974` | `4259150` | Sol source, five paths, 1077+/28- |
  | `65c8dcd` | `79d1c0e` | Sol evidence appendix, 177+ |

- `32f685f` is the coordinator's cwd correction. It touches only PROGRESS and
  the evidence file. `8db16bb` touches only PROGRESS. Both are
  coordinator-owned, so I did not review them as A12 source.
- The evidence file keeps three parts in order: the handback, the coordinator
  resolution, then the implementation. The withdrawn absolute-cwd instruction
  is recorded as history, not as an open blocker.
- Hosted A7/A11 stays a FINAL NO-GO. This review does not reopen, debug or
  rebind it.

I read:

- AGENTS and KICKOFF;
- ADR0007 (relative cwd and identity);
- ADR0009 (dedicated identity and containment);
- ADR0017 (report boundary);
- ADR0020 (D6 residue);
- brief 0020 A12;
- the prior `m2-a12-residue-source-opus-review.md`;
- the full evidence file;
- all six changed paths.

I also traced every production callee these paths reach:

- `validate_spec` and `validate_phased_spec` (`mod.rs`);
- `unix::run`, which encodes the absolute workspace and maps empty cwd to `.`;
- `workspace_cwd`, which opens with `openat2` `RESOLVE_NO_SYMLINKS` and then
  resolves cwd with `RESOLVE_BENEATH`;
- `unix::walk`;
- the `OwnerConfig` checks for clean paths and protected/workspace overlap;
- these driver helpers: `gate`, `plan`, `copy_tree`, `exclusive_file`,
  `private_read`, `same_object`, `harness`, `test_argv`, `monitor_a1a2`,
  `verify_test_result`, `cleanup_a1a2` and `run_body`.

## Standards: one blocking finding (precise fix)

### B1. The Rust gate-wiring guard passes on its own text

`a12_fixed_targets_and_closed_source_gate` replaced the old literal-`None` body
check with three assertions:

```rust
const { assert!(!DELIVERY_ENABLED) };
assert!(closed_read::<()>(|| panic!(..)).is_none());
assert!(source.contains("closed_read(read_delivery)"));
```

`source` is `include_str!("a12.rs")`, which includes this test. The needle
appears twice: once at `a12.rs:272` (the real body) and once at `a12.rs:1012`
(the assertion's own string literal). The third assertion is therefore always
true.

I demonstrated this with an in-memory Python string edit. No file was written
and nothing was compiled. Replacing the `prepared_delivery` body with
`read_delivery()`, which bypasses the gate, still leaves
`"closed_read(read_delivery)" in source` true.

No other test catches that mutant:

- The first two assertions test the constant and `closed_read`, not how they
  are wired.
- `a12_delivery_closed` is Linux-only. On CI it would still pass, because
  under nextest `current_exe` is not under `/var/lib/arx-ci-*`, so `layout()`
  returns `None` before any record is read.
- On Windows, `native` does not compile at all.

The Python and driver gates are independent and stay closed, so this is not an
exploitable gap today. But the Rust source fence is the stated primary gate,
and AGENTS requires a meaningful test for every behavior.

**Precise fix** (test-only, `a12.rs`):

1. Restore a body-equality extraction like the old one. The first match of the
   needle is the real definition, because the test comes later in the file.

   ```rust
   let body = source
       .split("fn prepared_delivery() -> Option<Prepared> {")
       .nth(1).unwrap().split('}').next().unwrap();
   assert_eq!(body.trim(), "closed_read(read_delivery)");
   ```

2. Assert that `read_delivery` has no other caller. For example, count
   `read_delivery(` occurrences outside the `fn read_delivery()` definition
   and the test's own literals, and require exactly the fixed number.
3. Optionally, record a Python in-memory check showing that the
   `read_delivery()` body mutant now fails the extraction.

### Standards items that pass

- Scope is exactly the six declared paths, with no change to:
  - `exec/mod.rs`, so the trailing `#[cfg(test)] #[path] mod a12;`
    declaration and the S2b pin are unchanged;
  - production, A7 logic, rights, package, workflow, helper, Cargo, ADR or
    PROGRESS (by the builder).
- The wrapper change is exactly the two consumed driver constants. I computed
  the SHA-256 of the committed LF `driver.py` blob as `138c0a64…1d75`, size
  96720, which equals both constants. No other repository pin names the old
  driver hash.
- Blobs match the evidence table. All changed files are `i/lf w/lf eol=lf`.
  Commit messages are conventional.
- Shared static fences still hold (see the full suite run below):
  - The new `subprocess.Popen` in `build_a12` passes both `stdout` and
    `stderr`, so F0 `test_subprocess_sinks` still holds.
  - `protected(...)` sites stay at 14, with three `run_parent`.
  - The legacy `test_exact_command_and_clean_environment` key set holds,
    because `cargo_env` is unchanged.
  - The A7 call-graph fence (`prepare_controlled_sudo_nnp` reaches no
    `Path('/etc`) holds.
  - W8 lists `a12.rs` in its inventory, and its declaration count is
    unchanged.

## Spec: one blocking finding (precise fix)

### B2. The 128 MiB harness cap probably cannot fit the Linux debug libtest built with this driver environment

Both sides cap the harness at 128 MiB:

- Rust: `decode` requires `harness.size` in `4..=128 MiB`, and `pin()` refuses
  anything larger.
- Python: `a12_pin` raises `E_ARTIFACT` above 128 MiB.

The harness is built by `build_a12` with the unchanged `cargo_env(stage)`.
That environment does not disable debuginfo. Only `ci.yml` sets
`CARGO_PROFILE_DEV_DEBUG: '0'`. So the hosted A12 harness would be a
full-debuginfo ELF, with DWARF embedded on Linux.

For scale, here is the builder's own build of this exact harness, the Windows
`arxd-0253deb079e95625`:

- `.exe`: 23,968,256 bytes;
- `.pdb`: 202,805,248 bytes (about 193 MiB).

Embedded Linux DWARF for the same crate graph is typically the same order of
size or larger. So the Linux harness very likely exceeds the cap. In that case:

- preparation fails closed with `E_ARTIFACT`;
- or, if it scrapes under the cap, every A12 attempt pays three full
  debug-build SHA-256 passes over more than 100 MiB, inside the 15 s
  preparation budget, the 5 s cancel-proof window and the 25 s call bounds.

This is an estimate, **not verified on Linux**.

`E_ARTIFACT` is also a multi-site code. A refusal would look like the
unattributable staged `E_ARTIFACT` in the A7 final record, so it would burn a
hosted attempt without a diagnosis. I make no claim about what caused that A7
miss.

**Precise fix** (A12-scoped, no shared change):

1. In `build_a12` only, pass an A12-only environment such as
   `{**cargo_env(stage), 'CARGO_PROFILE_DEV_DEBUG': '0'}`. This is the value
   CI already uses, and the test profile inherits it from dev.
2. Keep `cargo_env` and its legacy key-set test unchanged.
3. Assert the exact A12 build env in `test_one_bounded_build_and_clock`, and
   add an in-memory mutant that removes the setting.
4. Keep the 128 MiB cap on both sides.

If the coordinator prefers a different cap, it must be backed by a measured
hosted size under the exact driver environment, and changed in both Rust and
Python together.

### Spec items that pass

**cwd contract (ADR0007).**

- `RunSpec.cwd` stays empty, and `OwnerConfig.workspace` is the absolute
  derived `/var/lib/<account>/workspace`.
- `a12_delivery_workspace_relative_cwd_contract` checks the actual
  `validate_spec` and `validate_phased_spec`: empty is accepted, absolute is
  rejected with the exact error, then empty is accepted again.
- The per-call recheck also requires an empty cwd.
- The backend requires no ancestor ownership. As the dropped account,
  `/var/lib` (0755), then the account root (0700, owned), then `workspace`
  are traversable. So keeping the root outside the 0700 stage is correct and
  adds no path capability.
- Stage and workspace do not overlap, so `protected_directories=[stage]`
  passes the `OwnerConfig` overlap check.

**Closed gates before effects.** Four independent source edits would be needed
before anything ran:

- Rust `DELIVERY_ENABLED` is `false`, and `closed_read` returns before
  `current_exe` or any file, account or host read.
- Python `A12_DELIVERY_ENABLED` and `A12_EXECUTION_ENABLED` are both `false`.
- `run_body` refuses after the env-only `gate` but before any host probe.
- `main` refuses for every mode, including the internal `service` and
  `harness` modes.
- `prepare_a12` refuses before any import, read, build or mutation.
- Driver `TEST` is still the historical A7 literal, so `a12_selected()` is
  permanently false in this source. `test_argv` and `command` therefore
  cannot reach the A12 name.
- The ignored native test stays unselected.

**Record reader.**

- Fixed paths are derived from the stage. Nothing comes from the caller, the
  environment or the CLI.
- Stage layout: `current_exe` must equal
  `/var/lib/arx-ci-<id>-<attempt>/source/target/debug/deps/arxd-<hex>`, with
  decimal ids without leading zeros, attempt of at most 6 digits and an
  account name of at most 32 characters.
- The stage itself must be root:root with mode exactly 0700.
- Both records must be regular, root-owned, mode 0600, nlink 1 and at most
  4096 bytes, with a 4097-byte read ceiling and a byte count equal to the
  advertised length.
- `serde(deny_unknown_fields)` and serde's duplicate-field error reject
  unknown fields, nested unknown fields and duplicate fields. Trailing
  non-whitespace is also rejected.
- The run must match. `source_sha` and `workflow_sha` must both equal a
  separate 40-hex root-0600 file.
- The driver writes `GITHUB_SHA` with no newline, after `gate` has already
  enforced `GITHUB_SHA == ARX_REVIEWED_SHA == ARX_WORKFLOW_SHA`.
- The Python and Rust pin schemas match field for field: directory size 0 and
  an empty sha; root nlink 3 after `workspace`; harness root:root mode
  0o100700 after the explicit `chmod`.

**One owned account.**

- Collisions are checked through NSS and `lexists` before any mutation.
- `mkdir` is exclusive. `pending=True` is recorded before
  `groupadd`/`useradd`, and any failed capture quarantines.
- `useradd --no-user-group --gid`, nologin shell, home equal to the workspace,
  and no password are all checked.
- `a12_account` checks NSS, `getgrouplist == [gid]`, an empty `gr_mem` and
  UID/GID of at least 1000.
- The local passwd/group check is bounded to 1 MiB and checks UID, primary-GID
  and group aliases plus supplementary membership. The Rust side mirrors it
  exactly: 16 KiB NSS buffers, `getgrouplist` with two slots and exactly one
  allowed, and an identical local parse.

**Per-call recheck.**

- It runs inside the owned scoped worker, immediately before each
  `run_phased`.
- It covers NSS identity, local files, the reopened root and workspace pins
  (workspace opened relative to the reopened, compared root with
  `O_DIRECTORY|O_NOFOLLOW`), the harness bytes and metadata, and the fixed
  target's trust predicates (root, regular, executable, nlink 1, no
  setuid/setgid or group/world write).
- `runs` is reopened, checked as a root-owned non-writable directory, and its
  dev/ino compared with the held descriptor.
- On failure, `checked_call` returns `NotStarted(Invalid)` without calling
  Runner. That makes `accepts` false, so the test panics before any marker.
- The remaining TOCTOU window between the recheck and the backend's own
  no-symlink open has the same shape as A7's `recheck_delivery`. It is bounded
  by the account having no processes and by root-only `/var/lib`.

**Timing and cancellation.**

- The existing 15 s preparation, 5 s ACK, 15 s Runner, 5 s proof, 25 s call
  and 90 s fixture bounds are unchanged, as are packaged startup 30 s, monitor
  120 s and runtime 140 s.
- A12 shares A7's monitor ACK clocks and attribution (runtime files, MainPID,
  sole PID in `tasks`).
- Proof-before-cancel, the explicit join, real `cleanup_residue`, an empty
  `runs` and the held-leaf literal `ENOENT` are unchanged from the reviewed
  residue source.

**Markers and summary.**

- Each case's marker is a raw fd-1 write emitted only after `exercise` has
  returned true. That happens after the join and after the residue/absence
  results.
- `COMPLETE` is written only after both cases.
- The verifier requires each marker exactly once, exactly one strict summary
  line `1 passed; 0 failed; 0 ignored`, the generic `test <TEST> ... ok` line,
  return code 0 and the harness PID.
- Interleaving with libtest's flushed `test … ` prefix is handled the same way
  as the hosted-proven A4 marker.

**Cleanup order.**

1. Stop the owned unit.
2. Confirm the `arxd.service` cgroup is absent.
3. Require `pending` to be false.
4. Run the account check.
5. Scan live UIDs: at most 20000 `/proc` entries, the `/proc/<pid>` owner and
   all four `Uid:` fields of a status read of at most 4096 bytes.
6. Check root and workspace inode identity.
7. Run `userdel` and `groupdel`, then the symlink-safe `rmtree` of the account
   root.
8. Only after that, remove the runtime files and the stage.

Any refusal propagates before the shared deletions, so the stage and the
reservation are retained. There is no static VM or backup deletion. The
build captures at most 4 MiB through a drained pipe (no pipe deadlock), waits
360 s, kills/waits/joins, rejects a backwards or late clock and requires
exactly one test artifact at the fixed path.

## Security: PASS, no blockers

- Test and validation code only, with no production surface, MCP, route,
  secret or owner data. Nothing in the source reaches the native path.
- Trust in records and artifacts rests on a root-only 0700 stage. Every
  descriptor walk refuses non-root or group/world-writable ancestors and opens
  with `O_NOFOLLOW|O_NONBLOCK`, so a FIFO cannot hang the reader before the
  mode check rejects it.
- `unsafe` use is sound:
  - `openat` gets NUL-free `CString`s and valid held fds;
  - `getpwnam_r`/`getgrnam_r` only call `assume_init` after a zero return
    and a non-null result;
  - `pw` pointers into the shared buffer are consumed before `getgrnam_r`
    reuses it;
  - `gr_mem` is null-checked before it is dereferenced;
  - `getgrouplist` gets the true capacity;
  - the `write(1)` return value is checked.
- No sudo, NNP, profile, policy, view or A7 artifact is reused. The AST fence
  forbids `sudo_nnp`, `a7_admission`, `a4_delivery`, `package`, `policy` and
  `/sudo` in all six A12 driver functions.
- Account provisioning makes no admin, password or supplementary-group grant.
  Cleanup deletes only objects whose identity it re-verifies.

## Advisories (non-blocking)

1. `a12_uid_released` treats only `FileNotFoundError` as an exited process.
   An `ESRCH` (`ProcessLookupError`) between `open` and `read` would cause a
   spurious quarantine. This fails closed; catching it as "gone" is optional.
2. Per-call full re-hashing of the running harness adds little: Linux refuses
   writes to a running executable (`ETXTBSY`). A held metadata pin
   (dev/ino/mode/owner/nlink/size/mtime/ctime) per call, with the full hash at
   admission, would cut budget pressure. B2 already makes the hash small.
3. `record_bytes` does not check gid 0. The driver creates root:root records;
   tightening is optional.
4. On a build timeout, only `cargo` is killed, not its process group. This is
   the same as the existing A1/A2 path. Orphan `rustc` processes would only
   delay the drain join (`E_BUILD_LOG`) and stage removal.
5. Evidence completeness:
   - The builder did not run the full CI step
     `unittest discover -s tools/unix-exec-validation -p 'test_*.py'`. I ran
     it (below).
   - The Linux-only, non-ignored `a12_delivery_closed` will run in Ubuntu
     nextest.
   - The non-required S2b workflow does not trigger, because neither
     `exec/mod.rs` nor `tools/ci/s2b-native/**` changed.
   - The A7 wrapper and unix-execution-experiment workflows are
     `workflow_dispatch` only, so a PR triggers neither.

## Checks I ran

All TEMP/TMP and outputs were on `D:/Code/`. There were no installs, no cache
deletion and no host, account, cgroup, proc, sudo or network actions.

| Check | Result |
| --- | --- |
| `cargo test --locked --offline -p arxd --lib exec::a12::`, reusing the builder target `D:/Code/arxcovo-a12-delivery-source/target` | 7 passed, 409 filtered, 0.04 s (21.04 s incremental build) |
| `python -I -B -m unittest discover -s tools/unix-exec-validation -p 'test_*.py'`: the whole CI pure-guard step, run because the builder ran only subsets | 194 OK, 1.175 s |
| `python -I -B tools/ci/test-a7-admission-readonly.py` | 13 OK, 1.728 s |
| `mutation_check.a12_assertion_mutants()` (in memory) | 7/7 killed |

Notes on these checks:

- The 194 tests include all 14 A12, F0 `test_subprocess_sinks`, the F0b and
  legacy driver guards. I audited them: they use patched ports or in-memory
  source. The one subprocess test spawns `driver.py run` on non-Linux, which
  refuses at `E_PLATFORM` before any effect.
- I recomputed the driver blob's SHA-256 and size to check the wrapper pins.
- I did the in-memory gate-bypass mutation demonstration described in B1.

I did not rerun the builder's fmt, Windows Clippy or exact S2b applicability
checks (`mod.rs` is unchanged). I accept them as reported.

**Linux compile was not run.** It needs a Linux C cross toolchain for bundled
`rusqlite`, and installing one is out of scope. Windows Clippy does not
type-check `cfg(target_os = "linux")`.

Instead I audited every native name and signature:

- `OwnerConfig`'s ten fields and their types;
- the `Clone` derive;
- `MetadataExt`, `AsRawFd` and `FromRawFd` in scope;
- `sha2` 0.10 default `std`, which provides `io::Write`;
- `libc` 0.2.191 Linux signatures for `openat`, `getpwnam_r`, `getgrnam_r`
  and `getgrouplist` (`*mut c_int`), and `gr_mem: *mut *mut c_char`;
- the `Send`/`Sync` captures of the worker closure (`Prepared`, `Metadata`,
  `Runner`; `RefCell` is not captured);
- `needless_borrows_for_generic_args` on `join(&name)`, which does not fire
  because `name` is used more than once.

I found no mismatch. Hosted Ubuntu Clippy and nextest are still **mandatory**
before merge.

## Verdict

**Not yet eligible for publication.** Two precise, A12-scoped fixes are
required:

- **B1:** a non-vacuous Rust gate-wiring guard;
- **B2:** an A12-only debuginfo-off build env, with a test and a mutant.

Security passes with no blockers. After those fixes, a fresh different-provider
re-check of just those deltas, plus the hosted Linux compile, is enough. No
full re-review is needed.

This review grants nothing. There is:

- no candidate, dispatch, account or execution permission;
- no A12, A7/A11, M2, full F/G or PA6 credit;
- no A7 retry or counter reset.

Delivery stays closed and unselected. A12 remains **unverified on real
hardware**. M2 remains open.
