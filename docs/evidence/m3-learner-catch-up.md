# L1 learner catch-up: stopped local round

2026-11-17. Sol high, single builder, no children. Prepared base
`ac1465a5129a3c00215f91ebebfd1d024096b3d2`; source main
`fa8378889c05c6b87d191610e07e3bef68c2ebca`. Exact tested source commit:
`aa0c771a7826fb8b8f2b32955114b7b2349f46db`.
Contract: [learner brief](../briefs/m3-learner-admission.md), including the
superseding Root L1 contract check. **Incomplete; stopped on natural Clippy red.**

## Implemented source, not runtime evidence

One required all-OS test uses a real founder and enrolled node 2, distinct CSR
keys, private separate directories, one shared default MemRouter, and separate
local registries. Production TLS configs are retained before replay through
cleanup. Assertions cover the empty joiner registry and mutual TLS NotMember
refusal, leader enrollment guard, raw nonblocking test-only learner admission,
replay applied position and snapshot equality, voters {1}/learner {2} at
admission and after catch-up, own-view mutual TLS, isolated-learner Mint commit,
restored replication, zero snapshot transfers, router handle removal, Weak
expiry with configs alive, and isolated same-directory learner reopen with
persisted applied bytes and own registry bindings.

Catch-up uses only mem_net's default sender == source check and empty sender
authorization map. This is **not authenticated catch-up**. Production TLS
cannot start this replay with the joiner's empty registry: the known bootstrap
cycle remains. No joiner state was seeded, no production API or membership
policy changed, and no promotion is present in normal code. These are source
intent descriptions, not claims that runtime assertions passed.

Guard changes add the required test inventory and per-name required/forbidden
proof fragments and injection/removal probes, retaining F2's token checks.
The old TLS source is an unchanged prefix of the new file (checked against
prepared base). No production, helpers, Cargo, lockfile, workflow, sampler,
nextest, old bodies, quarantine27, sample8 or scheduling settings changed.
The one-voter/one-learner proof is required on all three OS, outside the
whole-fixture quarantine; no ignore or exemption was added.

## Actual validation and stop

Fresh target `D:/Temp/arx-l1-sol-bed2f82aa0/target`; process-local TEMP/TMP
`D:/Temp/arx-l1-sol-bed2f82aa0/tmp`. No cache deletion, install, native VM,
admin, owner-data, global settings or old Lab Manager action.

| Command | Exit | Wall seconds | Observed result |
| --- | ---: | ---: | --- |
| `cargo fmt --all --check` | 0 | 0.766 | PASS |
| `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` | 101 | 46.781 | FAIL: deprecated learner_ids at test lines 799 and 829 |

Clippy reports `Membership::learner_ids` deprecated and recommends
`non_write_voter_ids()`. The brief's cached-API feasibility list named the
former method; it cannot pass the required warnings-as-errors check unchanged.
No workaround, suppression, replacement API, fix, rerun or timeout increase was
made after this red. The round stopped immediately for coordinator disposition.
One routine preverification correction batch followed source API inspection
(fresh_read tuple extraction and generating a typed second Mint verifier);
formatting was applied before final verification. No failed fix attempts.

**Not run:** planned focused 27 unique cases (new1 + founder13 with correct
`cluster::consensus::issue::found::found_tests` prefix + current requiredTLS13),
guard unittest, M1/M2/M3. Mutation runner was prepared locally but never
executed; no mutant is claimed caught, no mutation/restoration proof exists.
No ordinary passed tests or historical mutants were replayed. All owned runner
and build processes exited. No independent review or hosted validation ran.

## Reused evidence and remaining work

Root supplied exact-main CI44069931184 attempt1 GREEN, wall472 / Windows467,
and F2 PASS on every OS. This builder did not fetch or independently verify
those observations. No new Linux/macOS/Windows runtime credit is claimed.
M2 remains open, M3 unchecked, recovery Sol3/3 / Opus0/2 unchanged. All hardware
paths are **unverified on real hardware**. No ADR deviation was implemented.
L2 production admission and D1/D2/D3 remain unapproved; no trust anchor or
promotion feature was added.

Local commits only; no push, PR, status/comment, watch, merge, protection or
label action. Root must dispose of the natural red before any further source
or verification round. Independent Opus review and exact-head publication
review, fresh main green, five normal gates and hosted Windows <=660 s remain
pending; current source is not ready for publication.

## Local artifact byte hashes

Artifacts are retained under `D:/Temp/arx-l1-sol-bed2f82aa0`.

| Artifact | SHA-256 |
| --- | --- |
| `fmt.log` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `clippy.log` | `e66e94608d4ab388ee8a5c925e938df206f94c59d99e7232e4ffaef06952f345` |
| `validation.json` | `dfed3bb619ff897ceaf837a6c473539f876aaedc90fc6cee405966d0b06b9ad9` |
| `validate.py` | `e877d0893a397ae4310c85b9c1a8af930d2d88a8a108fe47f6b01f6a8d364944` |
| `mutants.py` | `ae038cb9bade98cc25898391471ad89c15ea71916e8bd9001287d9e028f570ca` |

TLS source SHA-256: `605cb06641369c2601e61699a5fb1a02a8351be861ac8739ee749465830a2590`.
