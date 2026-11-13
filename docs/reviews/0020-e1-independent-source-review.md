# ADR 0020 Slice E1 independent source review

Reviewer: Anthropic / claude-opus-5-5 / high, 2026-11-13. Review sub-agent,
no subagents. This is the first independent source and security review of the
Sol E1 parsers. The old E1 source `2bbc427` was never independently approved,
so this is a full source review, not only an equivalence check. Opus wrote the
ADR 0020 architecture and correction 1, and those keep their own Sol reviews.
This review claims independence only for the Sol implementation.

**Standards: CHANGES REQUESTED.** S1: the `locate` trust-anchor rules that
enforce ADR 0020 T3 have no test that can fail. Removing the whole
NUL/backslash/newline/CR guard, or loosening the exact `arxd.service`
comparison to `ends_with`, leaves every focused test green. The fix touches
tests only. Five advisories follow. **Spec: no blocking findings.** Every E1
API and every P1-P10 fixture the brief names is present. The production
behaviour I read matches ADR 0020 T3, D4.2/D4.3, D8 R4/R5 and the D8 table.
Nothing is activated. Three spec advisories follow.

This review grants no publication, integration, activation, cgroup setup,
experiment or M2 acceptance. Contract PR56 still waits for its gates.
Publication still requires that merge, a final exact re-pin and independent
integration review on the merged base. Native, installed and dedicated paths
remain **unverified on real hardware**. M2 remains incomplete.
`E_PROTECTION` and `ExecutionFailed` remain unknown and unresolved. This
review claims no fix or retry.

## Pins

- Review target `105380f270d9929b1e8b897904e563360ede7b77` (evidence only).
  Source `55c22200e52d40ccb16d89d307c8764312cfa65c`. Base
  `fc1a6648515ef2227b59623df75e599da3bc9abe`, which is the contract stack on
  main Stage D `f91ae5a`. `git diff --stat f91ae5a fc1a664` over `crates/`,
  Cargo files, `.github/` and the toolchain file is empty.
- Isolation: the runtime-prepared T3 worktree. The working directory equals
  the git top level and is not the canonical checkout. Status was clean. Its
  HEAD is the later progress commit `2420c2e`, not the target. Every read used
  the target's Git objects or a private export made with `git archive` of the
  target. I created and switched no checkout. Every exported blob I checked
  hashed to the target's blob IDs.
- Scope: `git diff --name-only fc1a664 105380f` lists exactly the five crate
  files and `docs/evidence/linux-hierarchy-e1.md`. Each of the five blobs at
  the source equals the old E1 `2bbc427` blob: `hierarchy.rs` `4227261`,
  `hierarchy/parse.rs` `3917a2e`, `exec/mod.rs` `8969090`, `unix_bridge.rs`
  `4233dc4` and `unix_bridge/tests.rs` `fc9b8ce`. The three changed files had
  identical blobs at `2bbc427^` and at the base. Every other path keeps the
  main stack: no E2-E7, no E5 plumbing, no constructor or activation, and no
  Cargo, lockfile, CI or packaging change.
- Read: AGENTS, the dytschgo-review method, KICKOFF and ADR 0020 (D1, D2
  T2/T3, D4, D8, Bounds and the sequencing clarification). Also brief 0020
  §E1 P1-P10 and its mutation list, the Stage D review format and the E1
  evidence at the target.

## Diff read

- `exec/mod.rs`: adds `pub(crate) mod hierarchy;` and nothing else. The module
  compiles on every OS, as D1 requires for pure parsers.
- `exec/hierarchy.rs`: declares the module, applies
  `#![cfg_attr(not(test), allow(dead_code))]`, and re-exports `locate`.
- `service/unix_bridge.rs`: removes the private `cgroup_parent`. At the base
  only bridge T2 called it, because the bridge receives `parent` from its
  factory. Adds `#[cfg(test)] use crate::exec::hierarchy::locate;`.
  `clean_absolute` stays and is still used by preflight.
- `unix_bridge/tests.rs`: T2 now calls `locate(..).map(|l| l.runs())`. The
  accepted fixture keeps the same `runs` path. It adds the four A1 refusal
  fixtures the brief requires.
- `hierarchy/parse.rs`: the nine pure functions and eleven tests.

I found no filesystem, process, environment, libc or `unsafe` use. No
function calls `unwrap`, indexes or does arithmetic that can panic. Each
function checks its byte bound before parsing. No non-test code references
anything in `hierarchy`.

## Security and correctness read

Each item below was checked against the ADR text, not against the tests.

- `locate` (T2/T3): bound 4,096, UTF-8 only, at most one trailing `\n`
  removed, then the prefix `0::/`. It refuses NUL, backslash, LF and CR
  anywhere in the path, and empty, `.` or `..` components. The last component
  must be `supervisor` and the one before it exactly `arxd.service`, at any
  slice depth including none. Every Stage D refusal still holds. Stage D's
  `//` and root-only rules are covered by the empty-component check and the
  two-component requirement. Every input `locate` accepts was also accepted
  by Stage D. A second line can only enter through the LF guard, which keeps
  T3's "exactly one `0::` line, no v1 lines" rule. `runs()` and
  `unit_components()` come from the same validated components.
- `leaf_name`: `arx-` followed by exactly two `-`-separated canonical
  decimals (no leading zero except `0`), with at most 10 and 20 digits.
  The 35-byte guard is redundant with those limits. It cannot overflow
  because nothing is converted to a number.
- `events` (D4.2): every line is `<[a-z_]+> <[0-9]+>`, with no duplicate key,
  exactly one `populated` line valued `0` or `1`, and bound 4,096. CRLF, blank
  lines, a doubled space, `+1`, uppercase keys and a missing `populated` line
  are refused. Missing final newline is accepted, which is harmless.
- `classify_entry` (D4.3): `.` and `..` give `Dot`. An empty name, a name over
  255 bytes, or one containing `/`, `\` or NUL is `Refused`. `DT_DIR` (4)
  gives `Child`. `DT_REG` (8) gives `Control` only for
  `[a-z0-9_]+(\.[a-z0-9_]+)+` of at most 64 bytes, and `Refused` otherwise.
  `DT_UNKNOWN` (0) takes its kind from the exact `S_IFMT` comparison
  (`0o040000` or `0o100000`). Any other mode, a missing mode or an `fstatat`
  error is `Refused`. Every other `d_type` (links, devices, FIFOs, sockets,
  `DT_WHT`, unknown values) is `Refused`. The ABI constants are correct.
- `controllers`: whitespace-split tokens matched exactly; over 4,096 bytes
  gives neither controller. `pids`: empty means none; otherwise each line is a
  canonical decimal of at most 10 digits that fits in `u32`. Bound 4,096.
- `hardening` (R4): bound 8,192, UTF-8. Any line starting `NoNewPrivs:` or
  `CapBnd:` must equal `NoNewPrivs:\t1` or `CapBnd:\t00000000000000c1`, and
  each must appear exactly once. A subset, a superset, a different case, a
  duplicate or a missing line gives `None`. An injected line can only make
  the result fail closed.
- `bin_alias`: exact `usr/bin` or `/usr/bin`.
- `rights_line` (R5): bound 96. A `Value` pre-check makes `no_new_privs`
  mandatory, even though it is nullable. Typed decoding with
  `deny_unknown_fields` at both levels rejects extra keys, wrong types,
  unknown or retired `sudo` values, and duplicate keys at either level.
  `serde` derive reports `duplicate_field`, and the tests confirm it.
- `classify_rights`: I checked all 60 cells by hand against the D8 table.
  Standard with none gives `MechanismUnavailable`. Admin with standard gives
  `ServiceReconfigurationRequired`. Standard with standard is ok only for
  (`true`, `absent` or `exit_nonzero`). Admin with none is ok only for
  (`false`, `exit_zero`). Everything else is `RightsMismatch`. `unknown` and
  `untrusted` never pass. The admin ok row is pure logic. Nothing calls it,
  and Linux admin stays refused upstream by ADR 0019 D6 and `Runner::new`.
  macOS availability is unaffected, and `capability()` and production
  selection are unchanged.

## Fresh verification (this review)

Host: Windows / x86_64-pc-windows-msvc, rustc 1.95.0, cargo-nextest 0.9.151.
Every command ran on the private export of target `105380f`, with its own
target directory.

- `cargo fmt --all --check`: passed.
- `cargo clippy -p arxd --all-targets --locked -- -D warnings`: passed. This
  is narrower than the builder's workspace run, which I do not re-credit.
- `cargo nextest run --locked -p arxd -E 'test(exec::hierarchy) | test(service::unix_bridge)'`:
  **26 passed, 310 skipped**. That is 11 parser tests and 15 bridge fake-port
  tests. I did not run the full suite, because no code outside the five files
  changed and the base code equals main.
- **Independent compiled mutants.** I ran eleven, each against
  `test(exec::hierarchy) | test(service::unix_bridge::tests::t2_)`. The
  original bytes were restored after each one, and the final blob was checked
  as `3917a2e`. All eleven compiled; none was credited for a compile failure.

| Mutant in `parse.rs` | Result |
| --- | --- |
| Drop the `.` component check (control) | killed by `p1_` |
| Drop the `no_new_privs` presence pre-check (control) | killed by `p7_` |
| Delete the NUL/`\`/LF/CR guard in `locate` | **survived** |
| Drop only LF from that guard | **survived** |
| Drop only NUL from that guard | **survived** |
| Unit check `!= "arxd.service"` becomes `!ends_with("arxd.service")` | **survived** |
| Leaf PID at most 9 digits | **survived** |
| Leaf total length at most 34 | **survived** |
| `events` key allows uppercase | **survived** |
| `DT_UNKNOWN` directory chosen by bit test `m & 0o040000 != 0` | **survived** |
| `NoNewPrivs` line accepted by prefix | **survived** |

- **Discriminating probes.** These were reviewer-only tests, not committed. I
  appended them to the exported `parse.rs`. On the unmodified source all
  probes pass, so the code is correct on these inputs. Each of the seven
  surviving mutant shapes was then killed by its probe:
  `0::/x\n1:name=systemd:/arxd.service/supervisor`,
  `0::/x\n0::/arxd.service/supervisor`, `0::/a\0/arxd.service/supervisor`,
  `0::/a\r/arxd.service/supervisor` and `0::/xarxd.service/supervisor` are
  refused. `arx-1234567890-12345678901234567890` is accepted.
  `Populated 0\npopulated 0\n` is refused. `DT_UNKNOWN` with a socket, block
  or FIFO mode is refused. `NoNewPrivs:\t10` gives `None`.
- Characterisation, current behaviour: `controllers("memory\u{a0}pids")`
  reports both controllers. `rights_line` accepts
  ` { "rights" : {"no_new_privs":true,"sudo":"absent"} } ` as
  `(Some(true), Absent)`.

The builder's 35-test run, the old 334/3 workspace run and the old 13-mutation
ledger are not re-credited here. Nothing here exercised Linux or macOS
compilation, a real runner, a production native path, root, an owner account,
a service, a cgroup, system paths, a VM, Lab Manager, an experiment, private
state or an ignored native fixture.

## Standards

Sources: AGENTS ("Every behavior needs a meaningful test"), CONTRIBUTING
("meaningful behavior tests") and the smell baseline.

**S1 (blocking, tests only): `locate`'s T3 guards cannot fail a test.**
`parse.rs:29-31` enforces T3's "exactly one `0::` line, no v1 lines, no
NUL/CR" rule, and `parse.rs:37` enforces "exactly `arxd.service`". Deleting
the guard, or replacing the exact comparison with `ends_with`, leaves all 12
selected tests green. The cause is the A1 tightening. Each inherited Stage D
fixture that once tested these rules no longer has `arxd.service`, so the new
rule refuses it on its own: `0::/a/supervisor\n0::/b/supervisor`,
`0::/a\0/supervisor` and `0::/a/../supervisor`. The new `p1_` fixtures put the
forbidden byte in the last component (`supervisor\0`, `supervisor\\`,
`supervisor\r\n`, `supervisor\n1::/x`), where the `supervisor` comparison
refuses them anyway. With the guard deleted, `locate` accepts
`0::/x\n1:name=systemd:/arxd.service/supervisor` with unit
`["x\n1:name=systemd:", "arxd.service"]`.

This is not exploitable on a real host. The kernel refuses `\n` in cgroup
names, it lists the v2 line last, and only privileged writers create
cgroups. The code is correct; the gap is that removing a trust-anchor rule
goes undetected. Fix: in `p1_` or bridge T2, add refusal fixtures that keep
`arxd.service/supervisor` intact and put the fault earlier. Use the five
`locate` probe inputs above, plus `0::/a\\b/arxd.service/supervisor` for
backslash. Each must kill the matching mutant.

Advisories, in priority order. I recommend fixing A1-A3 in the same test-only
correction.

- **A1:** No test covers the leaf name's maximum accepted bound. P2 tests
  over-limit PID and serial lengths. It accepts only a 7-digit PID, so PID at
  most 9 digits and total length at most 34 both survive. Add
  `arx-1234567890-12345678901234567890` as accepted. The effect of either
  mutant would be fail-closed (`LeafStructure`), costing availability only.
- **A2:** `DT_UNKNOWN` is tested only with directory, regular, symlink,
  character and zero modes. A common `S_ISDIR`-by-bit mistake would classify
  sockets (`0o140000`) and block devices (`0o060000`) as `Child`. Add socket,
  block and FIFO modes to P9. Downstream, E3's `O_DIRECTORY` open would
  likely still fail closed, but E1 should pin it.
- **A3:** The `Populated 0` fixture in P3 is refused because `populated` is
  missing, not because of the key charset. Add `Populated 0\npopulated 0\n`.
- **A4:** In `hardening`, P6 does not separate exact line equality from a
  prefix match for either line. Its CRLF case fails on both lines at once.
  Consider adding `NoNewPrivs:\t10` and `CapBnd:\t00000000000000c1x`. The
  kernel never emits these, so the risk is low.
- **A5 (possible Speculative Generality / broad suppression):**
  `hierarchy.rs` applies `#![cfg_attr(not(test), allow(dead_code))]` to the
  whole module tree. E2/E3 code added under it will lose dead-code warnings in
  non-test builds. This matches the bridge's existing `#![allow(dead_code)]`
  pattern, so it is not a breach. Consider narrowing it when E2 lands.

No other baseline smell applies: names are clear and there is no duplicated
logic shape (the shared `decimal` helper is reused).

## Spec

Source: [brief 0020 §E1](../briefs/0020-linux-hierarchy-implementation.md) and
[ADR 0020](../adr/0020-linux-execution-hierarchy-and-janitor.md) D1, D2, D4
and D8.

Implemented as specified: `locate` with `Location::{unit_components(), runs()}`
(T3), `leaf_name`, `events`, `classify_entry`, `controllers`, `pids`,
`hardening`, `bin_alias`, `rights_line` and `classify_rights`, with the
specified signatures and bounds. The bridge-local parser is removed. P1-P10
contain every named fixture, including the four A1 refusal fixtures, the two
new accepted shapes and the source pin. The brief's mutation list is covered
by the old 13-row ledger, which is not re-credited; the two controls I re-ran
were killed. Production stays `MechanismUnavailable`, and no slice beyond E1
is present. S1 is a test-strength issue; the spec's behaviour is met.

Advisories:

- **SA1 (canonical form):** The brief says `rights_line` accepts "exactly
  `{"rights":{"no_new_privs":…,"sudo":…}}`". The code accepts JSON whitespace
  around and inside the object, and `\uXXXX` escapes. Keys, types, values,
  duplicates and the 96-byte bound are still exact. The line is never
  released, persisted or logged, so this has no safety effect. Before E4/E6,
  decide whether "exactly" means canonical bytes (compare with the shim's
  serialised form) or semantic equality, and record the decision. Similarly,
  `controllers` uses Unicode `split_whitespace` (so NBSP separates tokens),
  where `split_ascii_whitespace` would match kernel output more strictly.
- **SA2 (carry to E6):** "The bridge calls `locate(bytes).map(|l| l.runs())`"
  is met only in bridge tests, because the base had no non-test caller. The
  source pin is a substring check on `unix_bridge.rs` alone (`fn cgroup_parent(`,
  `strip_prefix("0::`). When E6 adds the real `/proc/self/cgroup` read, that
  path must call `locate`, and the pin should cover `unix_bridge/readiness.rs`
  and match on behaviour, not one spelling.
- **SA3 (stricter than the D4.3 table, fail-closed):** `classify_entry`
  refuses names over 255 bytes or containing `\` even for directories. D4.3's
  table puts every directory in the "child" row. This only refuses entries
  that could not be a valid leaf anyway, and E2 maps them to `LeafStructure`.
  Note it in the E2 evidence.

## Result

Standards: one blocking finding (S1: `locate`'s T3 guards and exact
`arxd.service` comparison can be removed without failing any test; tests-only
fix) and five advisories. Spec: no blocking findings and three advisories; the
largest is SA1, the meaning of "exactly" for the rights line. After the S1 test
correction, a focused recheck needs only the correction diff, the focused
tests and the S1 mutants.
