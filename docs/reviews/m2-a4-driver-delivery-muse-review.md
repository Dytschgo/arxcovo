# M2 A4 driver/delivery source review (Muse, independent)

Single-agent read-only review of the dormant A4 driver/delivery slice.
Base `fafb8d4f4bd460eff51328d781966d2e57f0fa22` (merged PR105),
head `5531fab` (`ba3eaf7` Sol implementation + `5531fab` coordinator CI gate).
Scope is the 9-file diff `fafb8d4..HEAD`:
`driver.py`, `test_driver.py`, `mutation_check.py`,
`a4-helper/README.md`, `a4-helper/build.py`, `a4-helper/test_build.py`,
`.github/workflows/ci.yml`, `docs/evidence/linux-a4-driver-delivery-source.md`,
`docs/PROGRESS.md`. No Rust, workflow (beyond the one CI step), Cargo/package,
production, or ADR change. Rust helper/fixture source, `recipe.json`, and
`image-pin.json` are byte-identical to base.

## Standards

- Follows the existing driver idiom throughout: `require(..., 'E_*')` gates,
  `O_NOFOLLOW|O_CLOEXEC` descriptors, exclusive `mkdir`/`O_EXCL` creation,
  retained `owned` identities with `same()` inode re-checks, quarantine
  (`E_QUARANTINE_CGROUP`) on replacement/extra/missing identity. No new
  error-code family, no new public helper beyond the A4-only path.
- `a4_input` allowlist is exactly five fixed checkout-relative names; anything
  else (e.g. `../foreign`) refuses with `E_PATH` before any open. New test
  `test_fixed_input_no_follow_bounded_regular_single_link` pins symlink, FIFO,
  oversize, and multi-link refusal plus `O_NOFOLLOW` on every open.
- `a4_image` enforces size AND hash conjunctively before any privileged open;
  mutation `omit image size and hash` is killed. Staged held-descriptor bytes
  are re-hashed after write (`a4_image(os.read(fd, ...))`); omitting it is
  killed. Short writes refuse before `fchmod`; covered and killed.
- Collision path owns nothing: `mkdir` `FileExistsError` leaves `owned`
  without `directory`, and `remove_a4_helper` returns early without
  unlink/rmdir. Covered by `test_collision_never_adopted_or_deleted`.
- Cleanup ordering is `stop -> helper -> runtime` (unit stop and cgroup
  disappearance first, `remove_a4_helper` before `remove_runtime`), pinned by
  `test_cleanup_after_stop_before_runtime_removal`. Fds (`fd`, `parent`) are
  closed in `run_body`'s finally. Pre-existing `os.execve` at `driver.py:870`
  is the unchanged harness launcher, not helper execution; the A4 path adds no
  fork/exec/system/subprocess-of-helper.
- Smell baseline (judgement calls, repo wins): mild duplication between
  `a4_directory` and the existing protected-ancestor checks is shared-shape
  reuse of the same fence, not a new abstraction; `helper_owned` as a dict
  beside the existing `owned` list is a justified divergent shape (fds vs
  paths), fully pinned by tests. No speculative generality: no launcher, path
  parameter, env override, or root helper compile. No findings.

## Spec

Against the review brief (closed A4->Standard only, TEST=A5 unchanged, fixed
delivery `root`/`usr/libexec/arxcovo/a4-helper`, known 3696-byte image
`fc814889...a92e`, independent source/recipe/pin preimage first, root-down
no-follow bounded single-link reads, exclusive parent/leaf with no adoption,
owned-descriptor cleanup only after unit stop/cgroup gone, collision owns
none, extra/replaced/unknown quarantine, preserved profile/limits/capture/
deadlines/schema/protected/F0/F0b; Linux candidate rejects root before
compiler/source, uses exact source/flags/Rust identity with Linux host,
records observed compiler/linker hashes but compares image/source/flags/size/
symbols to the existing cross-pin and never overwrites it; CI Linux-only step
runs 9 mock tool tests then unprivileged `linux-candidate`, never executes or
publishes the image):

- `TEST` remains `exec::disposable::disposable_linux_a5`
  (`driver.py:14`). A4 is purely additive in `standard_profile` and returns
  Standard (`True`); `test_closed_mapping_and_default` pins default, argv
  shape, `a4_selected()`, and `E_TEST_ARGS` outside A4 with zero opens.
- Fixed digests verified live: `helper.rs` `770ab...ada5a`, `recipe.json`
  `90a3d...39b47d`, `image-pin.json` `7ae3e...1df971`, `driver.py`
  `fa096...3ac1add1` — all match the literals `a4_delivery` requires before
  provenance/image admission. Image gate is exact 3696 bytes + SHA-256
  `fc814889c294db4216093e8f360201401be2c1b2dc6c4b01fd84617701f3a92e`.
- Driver delivery requires the Linux candidate record to equal the reviewed
  pin after substituting only the two observed compiler hashes and the
  `host:` line; any difference in source/flags/image/symbols refuses, and the
  staged `out/a4-helper` bytes are independently re-gated by `a4_image`.
  Observed compiler hashes are evidence fields, never admission pins.
- `build.py linux-candidate`: root (uid or euid 0) refuses before any recipe,
  source, or compiler access (also added to the original `build` path);
  non-Linux refuses; compiler identity must equal the recipe identity with
  only the host token substituted; the five compared fields must equal the
  reviewed pin or it raises, and it never writes `recipe.json`/`image-pin.json`.
  Pinned by `test_root_refuses_before_any_compiler_or_source_read` and
  `test_linux_candidate_compares_independent_pin_never_updates_it` (both
  matching and differing synthetic candidates).
- CI delta is one Linux-only `verify`-job step: mocked helper-tool tests plus
  `build.py linux-candidate` as the standard runner account. No sudo, no
  execution, no artifact publication, no permission/timeout/required-check
  change. The candidate-fulfilment question is answered by construction (exact
  equality to the known image, no new pin), but actual Linux-host equality
  evidence remains pending this PR — correctly labelled pending, not claimed.
- Preserved guards re-verified by execution here: 109 pure/mock tests OK
  (`tools/unix-exec-validation` discover), 9 helper-tool tests OK,
  36 driver mutation kills (29 retained + 7 new A4), F0 23 kills, F0b 36 kills
  + 4 documented equivalents, `MEASURED internal=757 outer=2447`, `git diff
  --check` clean (evidence file records the same commands).
- No findings; no spec item missing, partial, or wrong. No A4/M2/full F/G/PA6
  acceptance follows; hardware paths **unverified on real hardware**.

## Security

- Privilege order is correct: `a4_delivery()` (read-only opens from `/` down,
  `O_RDONLY|O_NOFOLLOW|O_CLOEXEC|O_NONBLOCK`, regular/single-link/bounded)
  runs before the `E_ROOT` gate and before reservation; `install_a4_helper`
  re-validates the image before touching `/`. Ancestor chain `/`, `usr`,
  `libexec` must each be root-owned directories unwritable by group/other;
  `arxcovo` is exclusively created (`mkdir` refuses existing — no adoption,
  no deletion of unowned entries), output is root-owned `0755` regular single-link of
  exact size, written through `O_EXCL|O_NOFOLLOW` and re-read back for hash.
- `build.py` executes only the pinned `rustc +1.95.0` with verbatim recipe
  flags and the sysroot-bundled `rust-lld`; bounded 60 s invocations, 1 MiB
  read bound, link/reparse rejection on every fixed path, ELF inventory
  rejects dynamic/TLS/relocations/forbidden symbols. The image is read, never
  executed, in either mode. No secrets, network, owner, cgroup, systemd, sudo,
  or dispatch surface added. No findings.

## Result

- Standards: 0 findings (worst: none; one noted justified dict-vs-list shape).
- Spec: 0 findings (worst: none; Linux-host equality evidence pending, not claimed).
- Security: 0 findings (worst: none).
- Pins: base `fafb8d4f4bd460eff51328d781966d2e57f0fa22`, head `5531fab`;
  checks run as above, all green; working tree clean before this review file.
