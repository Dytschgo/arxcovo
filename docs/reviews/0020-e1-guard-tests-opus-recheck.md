# ADR 0020 Slice E1 guard-test correction recheck

Reviewer: Anthropic / claude-opus-5-5 / high, 2026-11-13. Review sub-agent,
no subagents. This is a narrow recheck of the S1 finding from the first full
E1 source review (e77cda7, retained as `88f4f2f`
`docs/reviews/0020-e1-independent-source-review.md`). It covers only the
test correction and its evidence. It is not a new full source review.

**S1: CLOSED.** The `locate` T3 guard and the exact `arxd.service` comparison
now each have a fixture that fails when that rule is removed. Each fixture
fails for the intended reason, not because of an unrelated check. **Standards:
PASS** for this correction. **Spec: PASS carries forward** from the first
review because no E1 production byte changed.

This recheck grants no publication, integration, activation, cgroup setup,
experiment or M2 acceptance. Contract PR56 must still merge. Publication still
needs a final exact re-pin and an independent integration review on the
current base, with its gates. Native, installed and dedicated paths remain
**unverified on real hardware**. M2 remains incomplete. `E_PROTECTION` and
`ExecutionFailed` remain unknown and unresolved. This recheck claims no fix or
retry.

## Pins

- Correction `ddc89fc4220de4b2a04ddc8e3f40cbe922e766cf` (test only).
- Evidence follow-up HEAD `4a4eb3d7bd50189000f10dc1f29ae57de9be3d02`
  (`docs/PROGRESS.md` and `docs/evidence/linux-hierarchy-e1.md` only).
- Prior review target `105380f`, source `55c2220`, base `fc1a664`.
- `parse.rs` blob at ddc89fc and HEAD: `54dd52043e863d12189fda7216da43e095c96ffb`.
  The working tree matched this blob after every mutation run.

## Scope check

- `git diff --name-only 105380f HEAD` lists `parse.rs`, `PROGRESS.md`, the
  E1 evidence file and the retained first review. The other four E1 source
  files (`hierarchy.rs`, `exec/mod.rs`, `unix_bridge.rs`,
  `unix_bridge/tests.rs`) did not change.
- The production part of `parse.rs` is everything before `#[cfg(test)]`. Its
  SHA-256 is `21fce461…8d10135` at 105380f, at ddc89fc and at HEAD, so those
  bytes are identical. All four ddc89fc hunks are inside `mod tests`.
- No production change, design fix or new activation. This means the
  first review's Spec PASS and production reading carry forward.

## Why the masking is gone

The first review found that every guard fixture was refused by a later rule,
because the forbidden byte sat in the last component or the fixture had no
`arxd.service`. Each new fixture keeps a complete `/arxd.service/supervisor`
suffix and puts the forbidden byte in an earlier component. Without the guard,
`split('/')` turns that byte into one more non-empty unit component, so
`locate` would return `Some`:

| Fixture | Rule it isolates |
| --- | --- |
| `0::/x\n1:name=systemd:/arxd.service/supervisor` | LF and the "no v1 line" rule |
| `0::/x\n0::/arxd.service/supervisor` | LF and the "exactly one `0::` line" rule |
| `0::/a\0/arxd.service/supervisor` | NUL |
| `0::/a\r/arxd.service/supervisor` | CR |
| `0::/a\\b/arxd.service/supervisor` | backslash |
| `0::/xarxd.service/supervisor` | exact unit name, not a suffix match |

This is the right test boundary. These are the parser's real trust-anchor
inputs, and the test checks them through the public `locate` result
(`is_none()`), not through a private helper.

## Independent mutation runs

I made my own edits with a scratch script. Byte matching used character codes,
so the shell could not alter backslashes. After each run I restored the file
from a scratch copy and compared SHA-256 hashes. All runs used
`cargo nextest run --locked -p arxd --no-fail-fast -E 'test(exec::hierarchy::parse::tests::<prefix>)'`.
Toolchain: cargo 1.95.0, cargo-nextest 0.9.151, Windows host. The parser
module builds in `cfg(test)` on every OS.

S1 mutants (all in `p1_location_and_single_parser`, assertion at `parse.rs:299`):

| Mutant | Fixture that failed | Run ID |
| --- | --- | --- |
| whole guard disabled (`if false && …`) | `0::/x\n1:name=systemd:/arxd.service/supervisor` | 595a9436-265b-4dbf-88e6-87141d3ff8c4 |
| `'\0'` removed | `0::/a\0/arxd.service/supervisor` | 0e66781d-8b79-4350-804a-69761339cff7 |
| `'\\'` removed | `0::/a\\b/arxd.service/supervisor` | 62dec273-2d68-48da-80a2-2c2a46ff9140 |
| `'\n'` removed | `0::/x\n1:name=systemd:/arxd.service/supervisor` | a2e5ee85-2f02-4c82-9541-81a2f6740856 |
| `'\r'` removed | `0::/a\r/arxd.service/supervisor` | 31400a2e-998b-4794-a324-8cc654162663 |
| `!= "arxd.service"` changed to `!ends_with("arxd.service")` | `0::/xarxd.service/supervisor` | f0357812-8053-4cb9-a68b-20293621a9d5, a960bb3f-9ff9-4912-86d9-ca69f3f7fe66 |

These match the builder's five reported kills. I did not reuse their run IDs.
I also added the whole-guard mutant, which the first review named.

Advisory fixtures (the builder added these but did not claim kills for them;
I ran one mutant each):

| Advisory | Mutant | Test, line | Run ID |
| --- | --- | --- | --- |
| A1 longest leaf | `len() > 35` changed to `> 34` | p2, `parse.rs:323` | a6d0281e-5c11-456b-9662-d9d965d4bf50 |
| A2 DT_UNKNOWN socket/block/FIFO | unknown `st_mode` maps to regular (`_ => 8`) | p9, `parse.rs:581` | 5cc87e10-53ed-4799-9552-17f57fcd5537 |
| A3 `Populated` with `populated` present | key check `is_ascii_lowercase` changed to `is_ascii_alphabetic` | p3, `parse.rs:368` | 07bc8fe7-5ba7-4bcd-8cc4-0fbb04e9ce0f |
| A4 NoNewPrivs suffix | `!=` changed to `!starts_with` | p6, `parse.rs:433` | a5b8c5c5-661e-4979-b58c-2119f5730e96 |
| A4 CapBnd suffix | `!=` changed to `!starts_with` | p6, `parse.rs:433` | ec2024f2-5608-46f7-a785-5fa4742935ff |

Each mutant compiled and failed an assertion. All five advisories are now
covered by tests that can fail.

## Checks run by this reviewer

- Focused parse tests at HEAD: 11/11 passed (`027de3c8-b408-4db9-b76c-ac897e5b26c2`).
- `cargo fmt --all -- --check` passed after restoration.
- Final restored focused selection
  `-E 'test(exec::hierarchy) | test(service::unix_bridge) | test(exec::stage_b_tests)'`:
  35/35 passed, 301 skipped (`e79ceb52-ced0-41d4-9aa9-595b16735d54`).
- `git status` is clean, and the `parse.rs` blob equals HEAD.

Not run: Clippy (no production change; the builder reported a locked
workspace Clippy pass, which I did not repeat), the full workspace suite, and
CI.

## Note (non-blocking)

The correction evidence reports "focused 26 passed, 310 filtered". My run of
the selection documented earlier in the same evidence file reports 35 run and
301 skipped on this Windows host. Both add up to 336. The 26-test selection
or platform is not named. A later evidence edit could name the exact filter
and host. This does not affect S1.

## Verdict

S1 closed. Standards PASS for ddc89fc. Spec PASS carries forward, since the
production bytes match the reviewed 105380f. The first review's remaining
advisories (for example SA1, the meaning of "exactly" for the rights line and
`controllers` whitespace) remain open for the E4/E6 decisions and do
not block. No publication before PR56 merges, a final re-pin and an
independent integration review on the current base with its gates.
