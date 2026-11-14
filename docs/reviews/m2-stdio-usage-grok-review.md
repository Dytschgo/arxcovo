# M2 stdio usage review

Disposition: **no source blocker.** Retained `7316b8f` is the same patch as Sol
`be10b78`. Malformed `mcp-stdio` now exits 2 with a fixed usage line on stderr
before the runtime starts. The accepted three-argument grammar is unchanged.
**This is a source review for normal publication. It grants no merge, no
authenticated config-to-MCP acceptance, and no hardware credit.**

Reviewer: xAI / grok-4.7, one pass, 2026-11-14. The assignment forbids
subagents, so Standards, Spec, and security are this single reviewer.
Independence is from the Sol fix, not from a second reviewer. Label:
single-agent review. Coordinator merge remains separate.

M2 is **incomplete**. Real-Runner PA6 is **OPEN**. Linux Stage F evidence and
Stage G production wiring are **OPEN**. Installed service and dedicated
execution remain **unverified on real hardware**. The optional Windows replay
harness and launchd-profile evidence stay parked. No daemon, native executor,
owner, account, service, network, VM, private-state, install, GitHub, or CI
operation was run.

## Pins

Porcelain was empty on entry and after the checks below. The review cwd is
the linked worktree top. Its gitdir is that worktree's own gitdir and differs
from the common gitdir. The branch has no upstream. Paths below are
repository-relative. No owner path, account, or machine path is recorded.

| Item | Identity |
| --- | --- |
| Review branch | `t3/review-mcp-stdio-usage` |
| Branch tip at review | `f1ab6bded2effef151a5636dd361099c5c773ac6` |
| Tip tree | `8982591ba3e76028588ff59de37f603b3b8bc2a7` |
| Retained fix | `7316b8fa1a0aa404ba1f3d0557d3e3bd4da91427` |
| Sol fix | `be10b78e1e625e40e1fcd347dd9718bd712a5474` |
| Shared parent | `4ff5909e82f3af32d56f2e7a13b019ecb5034cdf` |
| Fix tree, both commits | `b73f15e7695944c5719b17c75bd5eb2bfb755ee4` |
| Parent tree | `cee4d97bf6f8457615c462b2d9512a8c39991ee1` |
| Stable patch-id, both commits | `198e3ffc358dfd4110d81db942681877f98560a2` |

`7316b8f` and `be10b78` share the parent, the tree, the message, and the
patch-id. They differ only as commit objects. `git diff 4ff5909 7316b8f` is
100 insertions and 0 deletions across three files. `git diff --check` on that
range was empty. The branch tip adds only the two-line `docs/PROGRESS.md`
note; that file is outside this source diff and was not edited here.

| File | Blob at retained fix | Parent blob |
| --- | --- | --- |
| `crates/arxd/src/main.rs` | `cae709bba9d106f0494fec656b356edfe7602634` | `a83727d168bee8fdd836a2341c1d322de0c08685` |
| `crates/arxd/tests/cli.rs` | `f65a1b502295d7d97adb9d2714d7fc1f2ababf95` | `ce14ee57e2744c1ff3d62877464889805fad1a46` |
| `docs/evidence/m2-client-setup-roundtrip.md` | `43109a5d247efff6a73da48bc303e5f941a5c7f4` | absent |

Numstat is 6, 44, and 50 insertions. Documents read: `AGENTS.md`,
`CONTRIBUTING.md`, the review skill, `docs/service-install.md` (the exact
`mcp-stdio --client-config FILE` sentence), ADR 0018's route-stability line,
and the slice sentence in `docs/PROGRESS.md`. Code read: the `main` guard and
the unchanged accept path, `ClientConfig::load` / `private_file` /
`checked_path`, and both CLI tests.

## Checks

The diff was read against parent `4ff5909`. These commands were run on the
clean tip, whose `main.rs` and `cli.rs` blobs match the retained fix:

- `cargo test -p arxd --locked --offline --test cli -- --test-threads=1`:
  2 passed, 0 failed, 0.43 s. The new test launches `arxd` for a bare
  `mcp-stdio`, a missing value, a misspelled option, an extra argument, and
  the accepted shape with an absent owned temp file whose name contains a
  space. The old test launches `arxd` with no arguments.
- `cargo clippy -p arxd --locked --offline --bin arxd --test cli -- -D warnings`:
  passed, 26.71 s.
- `rustfmt --edition 2024 --check crates/arxd/src/main.rs crates/arxd/tests/cli.rs`:
  passed.
- `git diff --check 4ff5909 7316b8f`: passed.

The pre-fix binary was not executed again. Parent source confirms the claimed
baseline: a short `mcp-stdio` misses the three-argument accept, falls into
`invalid arguments`, and `foreground` maps that to `transport-frame-or-io` /
exit 1. Linux and macOS were not run. Revert of the retained commit is a
two-way door.

## Standards

**PASS. No hard breach. No baseline smell that should change this diff.**

`AGENTS.md` requires a meaningful test per behavior, no secrets, no unrun
check presented as run, and a hardware label. `CONTRIBUTING.md` requires the
same focused test and forbids credentials. The commit is one conventional
`fix(cli)` concern. The new test drives the package binary and checks exit
code, exact stderr bytes, empty stdout, and that the absent file was not
created. The evidence file labels installed and dedicated paths **unverified
on real hardware** and does not claim the checks it did not run.

The reject predicate in `main` and the accept predicate in `run` state the
same grammar. That is sequencing, not Duplicated Code to extract: the reject
path has to leave before `tokio::runtime::Builder`, and a later accept-path
widening would still fail closed at this guard. No new helper, flag, or
framework was added. The Fowler baseline does not otherwise match this hunk.
`()` on the `||` keeps a one- or two-argument `mcp-stdio` from indexing
`args[1]`. The bare and missing-value cases passed.

## Spec

**PASS. The reproduced diagnostic defect is fixed inside the requested slice.**

Slice B in `docs/PROGRESS.md` asks to "reproduce one concrete existing-flow
defect and fix it with a bounded end-to-end regression, no new auth/CLI
product scope." `docs/service-install.md` says the entry "accepts exactly
`mcp-stdio --client-config FILE`." ADR 0018 says the `mcp-stdio
--client-config` and `--config` routes stay as they are.

The defect is the malformed setup diagnostic, which happens before any config
or transport exists. The regression covers that boundary on the real binary:
four malformed shapes expect exit 2 and exactly
`arxd: usage: arxd mcp-stdio --client-config FILE\n`; the accepted shape with
a spaced absent path expects exit 1 and exactly `arxd: client-config\n`.
Stdout is empty in every case. The only `context("client-config")` site is
`ClientConfig::load`, so that last result is the existing loader. No new
command, flag, auth step, or confirmation flow was added. A config-to-bridge
roundtrip was not this defect and stays unclaimed.

## Security

**PASS. No new authority and no usage-path disclosure.**

The new branch compares the first argument with `mcp-stdio` and, only when
there are three arguments, the second with `--client-config`. Otherwise it
prints one fixed literal and exits 2. It does not interpolate argv, open a
file, or start the runtime, bridge, listener, or executor. The test feeds the
spaced path and the extra token `extra` into usage cases and requires stderr
to equal that fixed line, so those bytes are not in the diagnostic.

The accepted shape still calls the unchanged `ClientConfig::load` path:
`checked_path`, `security::private`, ownership, and the existing read cap.
An absent leaf fails inside that loader. On Windows the walk reaches the
missing final component and returns the fixed `client-config` category; a
space is an ordinary path component. The test requires that exact stderr and
that the leaf was not created. File protections, token handling, and endpoint
selection are untouched. Usage exit 2 matches the existing owner-CLI usage
convention and does not weaken the accept path.

## Advisories

1. `docs/evidence/m2-client-setup-roundtrip.md` is named like a roundtrip.
   Its body correctly denies an authenticated config-to-MCP run. The same
   file calls the pre-existing zero-argument test a missing-server-config
   regression. That test expects `arxd: transport-frame-or-io` for no
   arguments, not exit 3 `server-config`. Neither wording changes the source
   fix. Do not read the filename as acceptance.

## Blockers

None.

Standards: 0 findings. Spec: 0 findings. Security: 0 findings.
