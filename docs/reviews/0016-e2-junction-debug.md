# ADR0016 E2 junction fixture debug (other-provider handoff)

Status: **diagnosis complete; test-only patch proposed, not applied**. Source
remains the incomplete E2 draft; do not publish or wire CLI.
Opus 5.5 (claude-opus-5-5), high effort, debug research sub-agent, 2026-11-11.
No delegation. Triggered by the Sol three-fix stop recorded in
`0016-e2-local-implementation-evidence.md`.

- Exact base checked out: `e67e9cb7e6e9645b2632c8e4f978eaece4c53034`.
- Runtime/test source at base is byte-identical to the E2 draft
  `fb61a883f681d0dd5db5094f64c3faaadbfc6586`; at the end of this work it still
  equals base (`git diff --quiet HEAD -- crates`).
- Deliverables: this report and `0016-e2-junction-fixture.patch` (one test file).
  `git apply --check` passes on base; applying it in the worktree reproduces the
  tested candidate byte-for-byte (post-image blob `b1a3655`).

## Shortest repro

```
cargo nextest run --locked -p arxd --no-fail-fast -E \
  'test(=service::owner_install_windows::tests::i5_junction_state_and_member_refuse) | test(=service::owner_windows::tests::n3_junction_parent_refuses)'
```

At base: I5 FAIL `owned-temp junction setup failed: exit=Some(1), syntax=true,
denied=false` (run `80d62c1b-ade5-4785-8ec5-778a27c3ee2e`, plus three more
identical reds); N3 PASS every run. Deterministic 4/4.

Minimised with a temporary tagged probe (removed): the symptom persists with
the I5 command builder run in a plain `tempfile` directory using
`std::fs::create_dir` (no Fixture, protection policy, state/member layout or
leaf-parent working directory). The load-bearing element is the program path
`PathBuf::from(SystemRoot).join("System32/cmd.exe")`.

## Ranked hypotheses (stated before probing) and evidence

| # | Hypothesis | Prediction | Probe result | Verdict |
|---|---|---|---|---|
| H1 | Forward slash in the cmd.exe image path: cmd parses its own quoted argv[0] (`.../System32/cmd.exe"`) as switch `/c` followed by `md.exe" ...`, so mklink never runs | Only changing the path to `join("System32").join("cmd.exe")` turns green; the original path fails even with `/d /c exit 0` | P1: path-only change → exit 0, junction exists. P2: original path + `/d /c exit 0` → exit 1; E1-style path → exit 0. P3: original path + lone arg `zzprobe` in an empty owned tempdir → exit 0 and one new directory whose name contains `exe` and `zzprobe` (cmd's `md` builtin ran); with `/d /c echo hi`, stdout has no `hi` and an `exe…` directory appears | **Supported** |
| H2 | raw_arg target quoting (`\"` lead from the evidence doc) | Changing quoting alone changes the result; original path with a quote-free command is green | P2 fails with no mklink and no quotes; P1 is green with identical quoting | Falsified |
| H3 | `to_string_lossy().replace('/', "\\")` vs `display()` | `display()` alone turns green | P1 green with unchanged `replace`; target has no `/` | Falsified as cause |
| H4 | `/d` switch or leaf/current-dir form | Dropping `/d` or using the temp root turns green | M2 uses temp root and still red; P1 keeps `/d` and leaf form and is green | Falsified as cause |

Supported cause: the I5 fixture builds cmd.exe's path with a literal
`System32/cmd.exe` segment. Rust passes that path quoted as argv[0]; cmd.exe
reads `/cmd.exe` in its own command line as the `/c` switch and executes its
`md` builtin on the remainder, which rejects it with the syntax message and
exit 1. All three Sol corrections changed only the mklink arguments and kept the
`System32/cmd.exe` program path, which is why each failed. The passing N3 joins
`System32` and `cmd.exe` separately. The exact internal cmd.exe parse rule is
inferred from P1–P3 behaviour, not from documentation.

## Fix (test-only)

`0016-e2-junction-fixture.patch` replaces the single path expression with
`.join("System32").join("cmd.exe")` plus a one-line comment. No assertion,
loop, refusal expectation, permission, retry or skip changes. Sol's earlier
argument changes (leaf name with parent working directory, slash replacement)
are harmless and left in place to keep the patch minimal.

## Checks run on the candidate (then reverted)

- Focused loop above: 3/3 green (runs `365f451b-09b9-4229-abef-1a56f8ecf7c2`,
  `5900579a-d6ec-418b-b54c-bd0b1f9452ec`, `26cdcf0e-ab75-4569-9abe-f243e6194169`).
- `cargo fmt --all -- --check`: PASS.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: PASS.
- `owner_install_windows`, `owner_windows`, `owner_scm` test modules: 49/49 PASS
  (`a186a8fe-9d05-4cd2-8231-64146241de47`).
- `cargo nextest run --locked --workspace --all-targets --no-fail-fast`:
  **241 passed, 0 failed, 2 skipped** (`87022a38-d1fc-4a64-8a89-ab0694389810`),
  versus Sol's 240/1/2 at the same source.

Bounded guard mutations, one at a time, each restored by checking out the file
(production diff count 0 after each):

| Mutation | I5 | N3 | Meaning |
|---|---|---|---|
| MB: installer `open()` drops `FILE_FLAG_OPEN_REPARSE_POINT` | FAIL, state iteration, at the `f.run()` refusal assert (`c71278ad`) | PASS | I5 state case pins the installer's no-follow open |
| MC: installer member loop drops `policy.check(&f, false)` | FAIL, member iteration (state iteration passed), same assert (`d8abe637`) | PASS | I5 member case pins the installer's member classification check |
| MA: `structural` ignores only `FILE_ATTRIBUTE_REPARSE_POINT` | PASS | FAIL (`03a53cbe`) | At the installer layer the reparse-attribute bit is redundant with the type check (member) and, inferred, the owner/protected-DACL check (state); only N3 pins that bit |

The iteration attribution for MB/MC used temporary `[DEBUG-j5k2]` lines, all
removed (grep clean). MA is reported as a coverage observation, not changed
here; deciding whether E2 needs a reparse-only installer assertion belongs to
the pending E2 guard-mutation work.

Not run: hosted CI, any guard/SCM/LSA/restore/production constructor, ignored
fixtures, M2 acceptance. Owned-temp cleanup: no fixture directories or reparse
points remained in recent temp directories after the runs; junctions were
removed with `remove_dir` or tempdir drop, which does not follow them.

## Handoff to Sol (production E2 routing)

1. `git apply docs/reviews/0016-e2-junction-fixture.patch`, re-run the focused
   loop and full nextest, commit as a test fix.
2. Remaining E2 work is unchanged from the evidence doc: meaningful E2 guard
   mutations (consider MA above), exact-token/error-boundary and owner-lock
   identity assertions, `service-install.md` step 4 and `service-foundation.md`
   updates, independent Opus review, green hosted gates, E1 exact-head
   follow-up. PROGRESS was not edited here.

All installed/native/dedicated provisioning and SCM paths remain **unverified
on real hardware**. Raw paths, SIDs, environment values and stderr were not
logged; only fixed classifications and redacted owned-fixture evidence.
