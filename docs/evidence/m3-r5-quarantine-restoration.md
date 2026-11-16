# M3 R5 quarantine restoration: local implementation evidence

Provider: GPT-6.1 Sol, high. Prepared HEAD
`8e662ecf625e8e7fb26a9dde5d5896ff694aacf7`; main source base
`c6121523417089bc0afaf307470183dd4a7f47ba`. Assigned isolated checkout retained,
branch `t3/m3-r5-quarantine-restoration`; no checkout or child created. No policy-branch
commits imported. Local code commit: `c7d1367549158232ae5b25dde7c0886f106a0e13`.

## Outcome and stop condition

Implementation is present, verification incomplete. First natural unittest failure
stopped verification; no rerun or subsequent test command was executed. One routine
correction changed the mutation fixture to match the multiline rustfmt attribute.
The corrected final code has **not been tested**. No production failure was reproduced,
no hosted credit exists, and this quarantine is an exception, not a fix.
Sol shared fixes remain 3/3; Opus remains 0/2. M2 open, M3 unchecked;
Windows three-node **unverified on real hardware**.

## Exact tested source and failed check

The initial tested source is Git tree `c460cbba244d53dc9f6066e8304e4c0f18ef3228`
(recorded with `git write-tree` before the correction). It is not the final code commit.

Command (Windows, repository cwd):

```text
python -m unittest discover -s tools/ci -p test_windows_cluster_quarantine.py
```

Result: exit 1, 2 tests, 3 failing subtests (`removed gate`, `unix family`,
`all platforms`), elapsed 0.364 s. The real-source guard passed and issuance tests
passed. Those three fixtures did not alter source because their search string was
single-line. Their explicit `assertNotEqual(mutant, source)` detected the no-ops;
they are test implementation failures, not meaningful intentional mutant rejections.
Other intentional mutant rejections are not a green overall check.

Failure log SHA-256:
`4579585f56704e1a4d55a3953fac10e7d1ecd8025800cc2b99cf11968539363b`.
Logs/manifest remain local in `D:/Temp/arxcovo-r5-_dlemzoi/` (`guard.log`,
`checks.jsonl`, `failed-source-sha256.json`); no raw log committed.

Tested file SHA-256 pins:

| File | SHA-256 |
| --- | --- |
| `crates/arxd/src/cluster/consensus/three_node.rs` | `d733a4d86c27636f26d998a9cfe40e9f089a1aba354b247954a6fa58c704b9bb` |
| `tools/ci/test_windows_cluster_quarantine.py` | `9ece9591a359da371f5e25729483eee901534d031856f3650a1389ed6837ba3c` |
| `tools/ci/windows_diag_sample.py` | `44ea3710d461c1525d1c08860996dd01c22a4617a36bba67ad20be14bd69ad7e` |
| `tools/ci/test_windows_diag_sample.py` | `d3526dffe3219cf7a5ec0edce4a8ecbf0aaf45a4c483bbb67905537518bff1a3` |

Final code file SHA-256 pins (after the one routine fixture correction, untested):

| File | SHA-256 |
| --- | --- |
| `crates/arxd/src/cluster/consensus/three_node.rs` | `d733a4d86c27636f26d998a9cfe40e9f089a1aba354b247954a6fa58c704b9bb` |
| `tools/ci/test_windows_cluster_quarantine.py` | `28cff2968acf1e007c2388bc89543ef744fe739eaa650fb788849e0ed5c9fa16` |
| `tools/ci/windows_diag_sample.py` | `44ea3710d461c1525d1c08860996dd01c22a4617a36bba67ad20be14bd69ad7e` |
| `tools/ci/test_windows_diag_sample.py` | `d3526dffe3219cf7a5ec0edce4a8ecbf0aaf45a4c483bbb67905537518bff1a3` |

## Inventory and scope

Declared guard inventories: 8 `EXPECTED` sampled names + 1 `WINDOWS_UNSAMPLED`
R5 name = 9 Windows ignores. Sample inventory remains 8 IDs; only the fixed `FILTER`
constant changes in the sampler. New tests capture the namespace executed through
`load_expected`'s real verified-bytes path, compare the fixed exclusion to the unsampled
inventory, and cover ignored R5 expression-mismatch acceptance / match rejection.
The full sampler tests were not run, so these are implementation descriptions.

Manual `git diff c612152 -- crates/arxd/src/cluster/consensus/three_node.rs` inspection
shows only four added attribute lines before R5. Its body, the existing eight attributes,
and production bytes are unchanged. `EXPECTED` and `REASON` declaration lines were retained.
No profile, grammar, budget, workflow, Cargo, policy, helper, deadline, retry or scheduling edits.
Only contract-allowlisted files plus this evidence file are changed from the prepared HEAD.
The prepared brief is the only pre-existing difference from main.

**Coverage loss:** Windows R5 does not execute in ordinary required CI or the diagnostic
sample. Linux/macOS R5 remains required. No real nextest list was collected, and no
ordinary R5 skip was observed locally. No ignored test was executed.

## Environment and unrun checks

All planned Cargo output and temporary directories were isolated under the new
`D:/Temp/arxcovo-r5-_dlemzoi/target` and `/tmp` directories. The logging runner set
`CARGO_NET_OFFLINE=true`, `CARGO_TARGET_DIR`, `TEMP`, `TMP`, `TMPDIR` and
`PYTHONDONTWRITEBYTECODE=1`. No existing cache was deleted. No install, admin,
native, VM, repeat loop or external action was performed.

Repository toolchain pin read: Rust 1.95.0, rustfmt/clippy components, minimal profile.
Actual Rust/Clippy/nextest executable versions were not collected before the stop.
The sampler brief names nextest 0.9.151; this is a documented pin, not a local observation.
All changed text was written as strict UTF-8 with LF by Python.

Unrun due to stop:

- Corrected guard and full sampler unittests.
- `cargo fmt --all --check`.
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`.
- Real Windows `cargo nextest list -T json` using the sampler's exact argv (must show
  exactly 8 matched/ignored and R5 expression mismatch). No altered argv was used.
- Ordinary `cargo nextest run -p arxd --lib -E 'test(=cluster::consensus::three_node::r5_shutdown_read_owners)'`
  (must show R5 skipped without ignored execution).
- Hosted Linux/macOS R5; all hosted PR checks and review.

## Remaining gates

Root inspection, independent Opus high source review, and completion/disposition of the
stopped local verification are required before publication. No push, PR, remote status,
comment, watch, merge, label or protection action was taken.

Known main after PR161 is `c612152`, CI43880904887 RED, Windows R5 timeout at
three_node.rs:1195, 1475 s wall / 738 s charged, credit withheld because red.
These facts are taken from the accepted contract, not newly queried or measured here.
The active budget gate cannot pass the known 1475 s main. ADR0025 acceptance alone
cannot clear it; bootstrap remains unauthorized. All five hosted gates remain pending;
a concrete reviewed PR precedes any later owner exception decision. No new-main green
claim or extension of the previous restoration exception is made.
