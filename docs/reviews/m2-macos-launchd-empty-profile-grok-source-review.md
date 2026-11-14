# M2 macOS launchd empty-profile source review

Disposition: **no source blocker.** The empty accept-set, macOS loaded-state
gates, pure status table, and injected capture seam match the accepted
fail-closed contract. This review grants **no merge, publication-as-acceptance,
native capture, launchctl run, or hardware credit**. The slice may enter
normal hosted gates. Those gates are the first real typecheck of the
`cfg(unix)` adapter.

Reviewer: xAI / grok-4.7, one pass, 2026-11-14. The assignment forbids
subagents, so Standards, Spec, and security are this single reviewer.
Independence is from the Sol implementation, not from a second reviewer.
Label: single-agent review. Coordinator merge remains separate.

M2 is **incomplete**. Real-Runner PA6 is **OPEN**. Linux Stage F evidence and
Stage G production wiring are **OPEN**. Installed, dedicated, and macOS
launchd paths are **unverified on real hardware**. Windows main71 run
`43545244266` attempt 1 has cause **unknown**; no retry is authorized. The
optional Windows replay harness stays parked. No native evidence or capture
authority is created here.

## Pins

Porcelain was empty on entry and stayed empty through the checks below. The
review cwd is the linked worktree top. Its gitdir is that worktree's own
gitdir and differs from the common gitdir. Paths below are repository-relative.
No owner path, account, or machine path is recorded. Inspection was source,
Git, documents, and the installed libc 0.2.191 sources as stubs. No launchctl,
native query, process, account, service, system, proc, sys, VM, private-state,
owner, install, network, GitHub, CI, or dispatch ran.

| Item | Identity |
| --- | --- |
| Review branch | `t3/m2-launchd-source-review` |
| Supplied head | `599af2163705faca075922b0af4ec486ee6d6008` |
| Head tree | `adcba8c6345628ce680afef33801908a5a8e310e` |
| Head parent | `e397796aea7707df082b4547aeb2d6f84314b65c` |
| Actual main | `4ec11b914039c81f5401fdfbe678201372742964` |
| Actual-main tree | `e3ac4fc4ea6ecbeb307a94610dc0a07e0ca089e7` |
| Sol source | `b6c1d74349d66d7f571025fe9150ac9e66737009`, parent `05aff28fd7df2aee73e966e4443b866f1d9bb149` |
| Retained source | `f2148cfaca7f7e976cc2876f2be98c827e340203`, same parent |
| Source tree at both | `953460500bfc15aca09716d5856c198c39e84d05` |
| Sol evidence | `57607413bb8e8e4539c1878a7735c616beb113a4`, parent `b6c1d74349d66d7f571025fe9150ac9e66737009` |
| Retained evidence | `e397796aea7707df082b4547aeb2d6f84314b65c`, parent `f2148cfaca7f7e976cc2876f2be98c827e340203` |
| Evidence tree at both | `e89c6ede709ec41575777b94c0641daaada50545` |

`b6c1d74` and `5760741` are not ancestors of this head. `f2148cf` and
`e397796` are. `git diff` between each supplied commit and its retained
commit is empty. `4ec11b9` is not an ancestor of this head. Service source at
the implementation parent `05aff28` matches `4ec11b9` (`git diff` for
`crates/arxd/src/service` was empty). Against `4ec11b9`, this head changes
four files: 766 insertions, 15 deletions. `git diff --check` on that range
was empty.

`599af21` changes only `docs/PROGRESS.md`, two inserted lines. Source blobs
are identical at `b6c1d74`, `f2148cf`, `5760741`, `e397796`, and `599af21`.

| File | Blob at head | LF SHA256 | Bytes | CR |
| --- | --- | --- | --- | --- |
| `crates/arxd/src/service/owner_launchd_capture.rs` | `af209f870bd9c63110a74ef8b677342aee3c5de9` | `16e70bff7e5580b1774354a57c21a13d1ec772fe3610cff36380ae649273a5ea` | 13235 | 0 |
| `crates/arxd/src/service/owner_query.rs` | `20c51c17ba24e5d3b5cecfad2af5d2b662445888` | `b9afb2b10f2c4737dac0908289597c31b63f7627a5a1c48511225740a3cdae5e` | 18701 | 0 |
| `crates/arxd/src/service/owner_unix.rs` | `bd991dd11c75f079bd3f689ec936baf8f3371084` | `17d2a2c892d4885c291f4ca8fa2377807ab28b42408e0f76cede715fa54ad8e7` | 82543 | 0 |
| `docs/adr/0014-unix-owner-lifecycle-corrections.md` | `d3e03b9796d12e2a015d5dd343c2d622a6fb3731` | `c75a0888f2e18734f4e0b31f5fd785e120e213265da924a0e915a65d33b5a376` | 35492 | 0 |

Baseline blobs at `4ec11b9`, unchanged through `05aff28`: `owner_query.rs`
`0fdf14996649262b7b077eb189fa365af086aa01`, `owner_unix.rs`
`282f7d373e2f9c1508bc8410ddfb3027c227bccb`, ADR 0014
`756b0156968c94cb4035a560594b2300a68334b5`. `owner_plan.rs` remains
`32a36454578553d7f8256be98e837bdb3f8885a1`. The plist and
`docs/service-install.md` blobs named in the design review are unchanged.
`docs/evidence/macos-manager-observations.json` remains
`da8326bfcb260fecc33de342be7fa179cac7f44a`.

| Document | Blob | LF SHA256 |
| --- | --- | --- |
| Proposal `docs/reviews/m2-macos-launchd-query-proposal.md` | `93f3c6ccc8725e40f542818826551e4b1fd71829` | `0c3a818ef0e78e859d3391adda3dcc7b8814d5eb5d2e830ae102a2bc250cbc62` |
| Design review `docs/reviews/m2-macos-launchd-query-grok-design-review.md` | `abf56af2045be6d3e243654918f6fd50d1799035` | `0c0ec0999d33f1fbd689da4fe54f8786c18f159a92dae31053cdc637de303584` |
| Evidence `docs/evidence/macos-launchd-empty-profile.md` | `8d98b81466d9978c20cada2c390f50348f501e30` | `ccd12e86ea30430998314fd521db37b6f357e01310ecd65fba145606c406d7e0` |

The proposal blob matches at `657d06a`, `f560c21`, and this head. The design
review blob matches at `b353a21`, `476fe06`, and this head. `657d06a` and
`b353a21` are not ancestors. `f560c21` and `476fe06` are. The three source
SHA256 values above match the evidence record.

Documents read: `AGENTS.md`, `CONTRIBUTING.md`, `docs/KICKOFF.md` (milestone
order, technical fences, hosted CI gates), the review skill, ADR 0014
including the new addendum, the proposal, the design review, and the evidence
record. Code read: the new capture module, the `owner_query` diff, the
`owner_unix` diff, macOS plan order in `owner_plan.rs`, and `Layout::for_test`.

## Standards

**PASS. No hard breach. One judgement-call smell.**

Repo rules that bind this change are `AGENTS.md` and `CONTRIBUTING.md`
(honest hardware labels, no claimed unrun checks, no secrets, no owner
administration, meaningful tests) and `docs/KICKOFF.md` (v1 order, technical
fences, hosted fmt/clippy/nextest). The change is one conventional source
concern plus the ADR note the design review required. `owner_plan.rs`
operations are untouched. New tests are ordinary `pure_` functions or the
five already-pure query tests. No credential or owner path was added.

Fowler baseline: one smell.

- **Duplicated Code**, judgement call. `Runner::query` verifies the literal
  `/bin/launchctl` and then calls `native_query`, which spawns that same
  literal with `LAUNCHD_QUERY` again (`owner_unix.rs` lines 769–781 and
  `owner_launchd_capture.rs` lines 128–129). The strings match today, and
  the program is absolute, so `PATH` cannot select a different binary. A
  later edit can verify one path and spawn another. Passing the verified
  program and argv into the adapter would remove the duplicate. This does
  not block the slice.

The status-matrix match inside the test is an independent oracle required by
the contract, not a second implementation called by the test. Tooling does
not enforce the duplicated literal. No other baseline smell is filed.
`Capture::query` exists so the capture tests kill a parser bypass; that is
the required seam.

## Spec

**PASS. No finding.**

Checked against the accepted design review, which narrows the proposal. The
ADR addendum at lines 579–604 records the same limits.

Implemented and inspected:

- `launchd` takes a borrowed `LaunchdObservation` and returns `Query::Unknown`
  for every input (`owner_query.rs` lines 216–228). The test crosses exit 0,
  exit 1, exit 113, `Signalled`, and `Incomplete` with empty, synthetic
  absence, success-shaped, NUL, and over-cap bytes, both completeness bits,
  and both truncation bits. There is no delimiter table, fixture, or absence
  constant. Exit 113 appears only as a test and mock input.
- On macOS, `active` is true for every `Ours` state, including `Stopped` and
  `Failed` (lines 82–85). `Installable` then returns `Error::Active`
  (`service-active`). `Unloaded` returns `Error::Manager`
  (`service-manager`). Only `Absent` passes those two expectations. Linux
  `active` is the previous match: `Stopped` and `Failed` stay inactive
  (lines 86–95). The new matrix test checks both platforms.
- `macos_status` returns `not-installed`, `stopped`, and `running` only for
  `(Absent, Absent)`, `(Identical, Absent)`, and
  `(Identical, Ours(Running))`. Every other pair returns `Error::Manager`,
  including `(Identical, Ours(Loaded))` and `(Absent, Ours(Running))`
  (lines 128–135). It deliberately does not call `safe`, because the accepted
  table maps the remaining pairs to `Manager`. Production `observe` still
  rejects `Unknown` and `Foreign` before `status` reaches the helper, so a
  real query that stays `Unknown` still halts at `service-manager`.
- `Gate::NotLoaded` is `Absent` only. `Gate::Loaded` is every `Ours`.
  `Unknown` and `Foreign` fail `safe` for every gate and every expectation.
- macOS install starts with `Expect(Installable)` before directories,
  provisioning, `disable`, or plist publication (`owner_plan.rs` lines
  262–292). The script model records a refusal and zero write-class
  operations for `Unknown`, `Foreign`, and every macOS `Ours` state.
- Start runs `bootstrap` only when the fresh gate query is `Absent`. Stop
  and uninstall run `bootout` for every `Ours` state and skip it only for
  `Absent`. After `bootout`, `Expect(Unloaded)` consumes a fresh query.
  `RemoveConstant` runs only when that query is `Absent` and the action is
  uninstall. `Ours`, `Unknown`, and `Foreign` leave the model artifact
  `Identical`.
- The macOS query still verifies `/bin/launchctl` with the existing binary
  policy, then uses the private capture helper. Linux still calls `bounded`
  with a 4096-byte stdout cap and parses with `systemd` only on success.
  Mutating `command` still calls `bounded` with no captured stdout and a 35
  second limit. `bounded` still clears the environment and sets `PATH`,
  `LANG=C`, `LC_ALL=C`, `SYSTEMD_PAGER=cat`, and `SYSTEMD_COLORS=0`. The
  macOS adapter clears the environment and sets only `PATH`, `LANG=C`, and
  `LC_ALL=C`, with cwd `/` and null stdin. No plan operation was added.

`docs/evidence/macos-manager-observations.json` is not referenced by the new
code. The historical exit 113 summary is not grammar. No new status token,
CLI, telemetry channel, workflow, or Windows harness file appears.
`Action::Status` still returns `Error::Manager` from the plan. Dedicated
execution stays inactive: the parser cannot return `Ours`, so real macOS
service operations still refuse `service-manager`.

The script model accepts `Verify` and `StartPreflight` through `_ => Ok(())`
(`owner_unix.rs` line 1921). Current macOS plans run those checks before
`Expect`. Both are reads in the real manager. The refusal tests still stop
on `Expect` before `EnsureDirectory`, `ProvisionState`, `Constant`, or
`Command`. A new write-class operation inserted before `Expect` would not
increment `mutations`. That is a test-limit, and the current plan does not
contain such an operation.

## Security

**PASS for this fail-closed slice. No blocker. Four advisories.**

The production accept-set is empty. Capture stores at most 65536 stdout
bytes and 4096 stderr bytes. A read that crosses a cap sets that stream's
truncation bit, stops the capture, and does not keep the excess bytes of
that read. Each pass reads both streams, at most 65 reads of at most 1024
bytes each. `WouldBlock` on one stream still reaches the other. Exact cap
plus EOF is complete and not truncated. Process exit without EOF on both
streams stays `Incomplete`. EOF before exit waits for `try_wait`. A nonzero
exit, including 113, is preserved as `Exited` and still maps through
`launchd` to `Unknown`. `ExitStatus::code() == None` becomes `Signalled`,
not `128 + signal`. Spawn failure returns `Incomplete` without `kill`.
Setup, read, wait, deadline, overflow, and a never-closed pipe become
`Incomplete`, then one `kill` and at most one second of `try_wait`. The
result stays `Incomplete` / `Unknown` whether or not the reap observes an
exit. No errno text, stream bytes, or `Debug`/`Display` of the observation
cross the boundary. Assertion messages are fixed strings. `Query::Unknown`
carries no payload.

The native adapter, `cfg(unix)` only, sets `O_NONBLOCK` with `F_GETFL` then
`F_SETFL` of the existing flags OR `O_NONBLOCK`, and fails closed if either
call fails. That is the same shape as `exec/unix.rs` `nonblocking`, and it
is stricter than `bounded`, which replaces the flags with `O_NONBLOCK`
alone. Installed libc 0.2.191 stubs, labeled as crate sources and not a
compile of `arxd`, declare `fcntl(c_int, c_int, ...) -> c_int`, Apple
`F_GETFL`/`F_SETFL` as `c_int`, and BSD `O_NONBLOCK` as `c_int`. This review
did not compile those calls.

Advisories, none of which authorize a native run:

1. **Real Unix typechecking is missing.** Host rustc 1.95.0
   `59807616e1fa2540724bfbac14d7976d7e4a3860` is `x86_64-pc-windows-msvc`.
   The fmt, Clippy, and test commands below compile the Windows `cfg`. They
   do not compile `native_query`. Hosted macOS and Linux gates are the first
   real typecheck. No cross-target compile was claimed.
2. **The synthetic seam is not kernel proof.** Tests drive a `Transport`
   mock. They do not open a pipe, set a descriptor flag, or reap a process.
   The adapter does not read back `O_NONBLOCK` after `F_SETFL`. `Child::kill`
   signals that child only. The deadline is checked at each dual-stream pass,
   including after reads that themselves advance time, matching `bounded`.
   One blocking read would not notice the deadline if the flag were not
   actually set; setup failure never enters the read loop.
3. **An unfinished reap drops the child handle.** The installed toolchain
   has no `rust-src`, so the 1.95 `Child` destructor was not read. Public
   `std::process::Child` documentation says dropping the handle does not
   wait. After the one-second `try_wait` loop, `native_query` returns
   `Unknown` and drops the handle. A child that survives `SIGKILL` can
   remain live or unreaped. Cleanup is not absence. `bounded` already has
   this shape. The pure reap bound is an assertion after return, so a
   non-returning loop would hang the test rather than fail it.
4. **No capture authority.** A nonempty profile, absence fixture, running
   fixture, installed lifecycle, and dedicated execution remain closed.
   Root between two observations is still not an atomic pair. The query
   does not open a path from print output.

No new owner or admin action is present. The only new process image is the
existing read-only `/bin/launchctl print system/com.arxcovo.arxd` argv, and
only inside the unrun Unix adapter.

## Tests this review ran

Call graph before execution: the nine `pure_` tests call `launchd`,
`macos_status`, `expect`, `gate`, `capture` with `Mock`, or `owner_plan`
through the in-memory `Model`. `Layout::for_test` builds literal paths.
`Mock::pause` adds to a counter. The five existing query tests call
`systemd`, `expect`, `gate`, and `linux_status`. None of the 14 call
`native_query`, `bounded`, or `Command`. `ordinary_process_timeout_and_output_bounds`
was not selected.

Toolchain: rustc 1.95.0 as pinned above. Commands, all `--locked --offline`:

- `cargo fmt --all -- --check`: exit 0.
- `cargo clippy -p arxd --lib --tests -- -D warnings`: exit 0, dev profile
  finished in 37.63s. Windows `cfg` only.
- `cargo test -p arxd --lib -- --exact` with the 14 names below: exit 0.
  Result: **14 passed, 0 failed, 0 ignored, 362 filtered out, 0.00s.**

The 14 names:

- `service::owner_query::empty_profile_tests::pure_empty_profile_every_observation_unknown`
- `service::owner_query::empty_profile_tests::pure_macos_state_and_status_matrix`
- `service::owner_unix::launchd_capture::tests::pure_capture_exact_caps_nonzero_and_signal`
- `service::owner_unix::launchd_capture::tests::pure_capture_fair_flood_and_overflow`
- `service::owner_unix::launchd_capture::tests::pure_capture_failures_deadline_and_bounded_reap`
- `service::owner_unix::launchd_capture::tests::pure_capture_eof_before_exit_and_wouldblock_fairness`
- `service::owner_unix::launchd_lifecycle_pure_tests::pure_uncertain_and_loaded_install_never_mutates`
- `service::owner_unix::launchd_lifecycle_pure_tests::pure_post_bootout_requires_fresh_absence_and_keeps_plist`
- `service::owner_unix::launchd_lifecycle_pure_tests::pure_bootstrap_and_bootout_gates`
- `service::owner_query::tests::bounded_queries_reject_ambiguous_or_foreign_output`
- `service::owner_query::tests::not_found_cannot_hide_an_active_or_transitioning_unit`
- `service::owner_query::tests::active_unknown_and_foreign_cannot_pass_installable_or_stop_proof`
- `service::owner_query::tests::installed_absence_differs_between_platforms`
- `service::owner_query::tests::gates_and_status_keep_unknown_errors_fixed`

No mutant was applied in this review. The evidence's eight named assertions
are present on the implementation under test: `empty-profile-refuses-every-observation`,
`mac-all-ours-block-install`, `exhaustive-macos-status`, `uncertain-gate-refuses`,
`only-absence-proves-unloaded`, and `cleanup-only-after-spawn`. Source bytes
were not edited, so no restore was required. The builder's mutant runs stay
that record's claim; this review did not repeat them.

## Limits

Hosted Unix compilation and the full nextest/UI gates did not run here.
Native pipes, `O_NONBLOCK`, `SIGKILL`, and reap behavior are unrun. No
launchctl grammar was inferred. M2, PA6, Linux F/G, and main71 are unchanged.
The optional Windows harness stays parked. Real hardware remains
**unverified on real hardware**.

## Close

Standards: 1 judgement-call smell, 0 hard breaches; worst is the duplicated
launchctl program literal. Spec: 0 findings. Security: 0 blockers; worst
residual is the Windows-hosted gap over the `cfg(unix)` adapter and the
unproven reap. Ready for normal hosted gates only.
