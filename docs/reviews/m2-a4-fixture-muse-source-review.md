# A4 fixed-helper dormant fixture: independent Muse source/security review

2026-11-15; Muse Spark 1.3 high (review sub-agent). Independent source/security
review of the fixed-helper dormant fixture slice against merged PR104.

- Base pinned: `c87387ea6d5680b9347d43f88b8d280884d26353` (PR104 merge).
- Head pinned: `f06f985f58cc9d68be701dfbd0f578b75089f` (branch
  `work/a4-after-helper-merge`, also `t3/a4-fixture-source-review`).
- `git log c87387e..HEAD --oneline`: `cb88795` (fixture source),
  `7b4c93b` (evidence/progress), `f06f985` (E3 pin alignment).
- Scope: `git diff c87387e...HEAD --stat` shows 9 files, +980/−1:
  `crates/arxd/src/exec/disposable.rs` (1 line: `mod a4;`),
  `crates/arxd/src/exec/disposable/a4.rs` (new, 406 lines),
  `crates/arxd/src/exec/hierarchy/a4.rs` (new, 398 lines),
  `crates/arxd/src/exec/hierarchy/fake.rs` (+17),
  `crates/arxd/src/exec/hierarchy/linux.rs` (+26),
  `crates/arxd/src/exec/stage_b_tests.rs` (+6),
  `docs/PROGRESS.md` (+4),
  `docs/reviews/m2-a4-fixture-source-evidence.md` (new, 121 lines),
  `tools/ci/e3-native/driver.py` (1 line: `SOURCE_SHA` refresh).
- Spec: ADR0020 §D9a bindings 1–4 (binding 5 is A6, untouched by this slice),
  the fixed-helper README/source/build evidence and its independent Muse
  review. Method: full read of all 9 diff files plus helper `README.md`,
  `image-pin.json`, and both prior reviews; full callgraph trace before any
  test; independent re-run of the permitted pure checks only. No native,
  ignored, helper-launch, fork, proc, cgroup, systemd, sudo, owner, dispatch,
  or network execution occurred in this review.

## Standards — PASS, no findings

- Small one-concern conventional commits: `cb88795` source only (6 files),
  `7b4c93b` docs/evidence only (2 files), `f06f985` E3 pin alignment
  (driver pin plus its evidence/progress record). No production, helper-owned,
  Cargo/package, manifest, workflow, policy, or ADR change: the diff
  name-list contains none of `tools/unix-exec-validation/**`,
  `crates/arxd/Cargo.*`, `package.json`, `pnpm-lock.yaml`, or
  `.github/workflows/**`, and sibling sources (`hierarchy/a5.rs`,
  `parse.rs`, `setup.rs`, `cgroup.rs`, `unix.rs`, `service/**`) have empty
  diffs against base.
- Every behavior ships with a test: 6 new pure model tests in
  `hierarchy/a4.rs` plus the ignored native fixture; the inventory test
  registers both new module declarations and passes.
- Fowler smell baseline: no finding. Names are literal (`helper`, `single_task`,
  `remaining`, `limits`, `finish`, `live/release/killed`); the shared
  `remaining`/`image` logic is extracted once per seam (pure helper vs mock
  `ChildOps`); no speculative abstraction (fixed path, fixed limits, fixed
  deadline, 4-method trait); no message chains or middle men (fixture calls
  `seed_a4`/`place_a4`/`finish` directly); one reason per file (native spawn in
  `disposable/a4.rs`, pure orchestration in `hierarchy/a4.rs`, model exposure
  in `fake.rs`).

## Spec — PASS, no findings (D9a bindings 1–4, fixture slice only)

Quoted against `docs/adr/0020-linux-execution-hierarchy-and-janitor.md:692-749`:

1. Fixed standalone helper, E4 descriptor-bound spawn, no own post-fork
   branch: `disposable/a4.rs:17-65` validates the staged
   `/usr/libexec/arxcovo/a4-helper` by trusted component traversal,
   no-follow regular executable descriptor, size/hash (`IMAGE_SIZE 3696`,
   `IMAGE_HASH fc814889…`, `SOURCE_HASH 770ab397…`, matching
   `image-pin.json`) and single link, failing before any seeding or spawn;
   no PATH/env/build fallback. Stdio is the verified root-owned char device
   1:3 at `/dev/null` (`:291-304`); only the two handshake fds survive exec
   (`:343-364`); absolute 15 s deadline travels as bounded decimal argv with
   empty env (`:305-321`); the child branch is prepared-pointers
   dup2/execveat/write/_exit only. This reuses the accepted E4 spawn-window
   basis exactly as D9a discloses; it proves no new linked-runtime closure.
2. Only S7 kills on success: `hierarchy/a4.rs:128-206` calls the unchanged
   `a5::finish` exactly once with no fixture kill on that path; the
   `populated_leaf_requires_s7_and_one_removal` test asserts exactly one
   `rmdir` and one `cgroup.kill` write, and `omitted_kill_residue_and_non_
   sigkill_never_accept` refuses absent kill effect, fresh residue, and
   non-SIGKILL. Native `Drop` (`disposable/a4.rs:243-260`) SIGKILLs only an
   unreaped owned child after failure, with `ECHILD` clearing ownership.
3. Parent survives, holds identity, waitpid-SIGKILL before original expiry:
   `live()` (`disposable/a4.rs:160-222`) requires one numeric procfs task,
   leaf procs equal to exactly that child, empty runs procs, and sole parent
   in supervisor; `finish()` requires `>6 s` remaining both before and after
   `G`; `killed()` requires `WIFSIGNALED` + `SIGKILL`. Fifteen precondition
   cases refuse without setup or reap, with zero `reaps`.
4. Created-only, no adoption, exact postconditions: `seed_a5` refuses unless
   `mkdir_runs` returns `Created::Created`; `seed_a4`/`place_a4`
   (`linux.rs:103-127`, `#[cfg(test)]`) reopen literal `arx-0-0` and compare
   device/inode to the retained handle before the PID write; `finish()`
   checks uid/mode/magic, 512-record lists, no nested/unknown leaves, exact
   `67108864`/`0`/`1` limit readback, empty runs procs, fresh S9, one
   removal via unchanged A5, then `READY`/identity hold. A1/A2/A3/A5 files
   are untouched.

## Security — PASS, no blockers

- Reachability: the native path is only the `#[ignore]`d
  `exec::disposable::a4::disposable_linux_a4`, behind
  `#[cfg(all(test, target_os = "linux"))] mod disposable`; `seed_a4`/
  `place_a4` are `#[cfg(test)]`; `hierarchy::fake` (incl. `mod a4`) is
  `#[cfg(test)]`; `hierarchy::linux` is `#[cfg(target_os = "linux")]`.
  Caller trace confirms no production or ordinary-test caller of
  `seed_a4`/`place_a4`/`disposable_linux_a4`; ordinary tests reach only mock
  `ChildOps`, `Fake`/`Manual`, and pure hashing/parsing.
- Descriptor binding: every open uses `O_NOFOLLOW | O_CLOEXEC`
  (+`O_NONBLOCK`, `O_DIRECTORY` for dirs); each path component is
  root-owned, group/other-unwritable, type-checked; the helper additionally
  requires exact size, exec bit, `nlink == 1`, and a bounded
  `take(IMAGE_SIZE + 1)` hash match.
- FD/SIGPIPE/stdio: parent inventories `/proc/self/fd` (bounded 513
  entries, unrelated fds must be `CLOEXEC`); exactly the two handshake ends
  are made inheritable around `fork` and restored immediately after; pipes
  are `O_CLOEXEC | O_NONBLOCK`; `SIGPIPE` is queried read-only and required
  to be inherited-ignored, never changed.
- Cleanup attribution: success-path wait is `waitpid` on the retained owned
  PID only; `ECHILD` clears ownership so no foreign identity is ever
  signalled; blocking `waitpid` + `SIGKILL` run solely on the failure-only
  `Drop` path; `close` is never retried.
- Concrete residual notes (non-blocking): `f06f985` bundles the driver pin
  with its evidence/progress record — one pin-alignment concern, acceptable;
  `S2b` pinned sources, `MANIFEST_SHA`, guards, and manifests are unchanged;
  `SOURCE_SHA` becomes `75b174ac10c7c3dd3be5984f507ba891ae4dccd953c49dfb12327007c0b907bd`,
  the exact Git-LF blob hash for this source commit; the refresh is
  source-`#[cfg(test)]`-only and outside the closed driver mapping.

## Checks actually run in this review

| Check | Result |
|---|---|
| `cargo test -p arxd --locked --offline --lib exec::hierarchy::fake::a4::` | 6 passed |
| `cargo test -p arxd --locked --offline --lib exec::stage_b_tests::w8_e7_crate_wide_inactive_source_inventory` | 1 passed |
| `cargo fmt --all -- --check`, `git diff --check` | clean |
| `cargo clippy -p arxd --locked --offline --all-targets -- -D warnings` | finished, no warnings |
| `python -I -B tools/ci/e3-native/test_driver.py` | 25 tests, 1 skip, OK (no A4/helper execution) |
| Linux native/ignored fixture compile or run | not attempted here: `x86_64-linux-gnu-gcc` absent on this host; normal hosted compile later |

Working tree is clean except this new review file (to be committed alone
next). No source/`PROGRESS`/evidence edits, no push, no PR, no subagents.

No A4, M2, full F/G, or PA6 acceptance follows; native/hardware paths are
**unverified on real hardware**.
