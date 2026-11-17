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


## Root disposition and completed corrected-source round

The builder stop above is historical and retained. OpenRaft0.10.0's cached
membership.rs lines228-240 show learner_ids() is exactly a deprecated alias
calling non_write_voter_ids(). Root replaced only the two test calls with the
supported method, source commit fa7bc545294d99eed71df822f9226503b721628f.
No warning suppression, timeout/retry change, engine repair or prior behavior-test
rerun occurred: the builder had run no behavior test before Clippy stopped.

Root used the existing task-owned Cargo target with CARGO_PROFILE_DEV_DEBUG=0,
offline networking and a fresh process-local temp directory. Original builder
logs were preserved. Logs/runners are under D:/Temp/arx-l1-root-u6lxwfst.

Actual corrected-source checks, each once:
- fmt: exit0, 0.969s; SHA256 `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`.
- clippy: exit0, 47.172s; SHA256 `9006468255ebf18c439c19c4525314ec5715862b4f66552f126a0f3b4dd398f3`.
- focused27: exit0, 63.937s; SHA256 `089249fd715effef21415ccb7cf2b11ea314d1ec4296c046310849b332c66ae4`.
- guard: exit0, 0.282s; SHA256 `35bba77e8d6dc989ba88b32c417dcc4fce299665512770fa676a2da9bcbd02bd`.

The focused selection executed all27 unique cases: new L1 plus13founder and13
existing requiredTLS. All passed; test wall5.533s, new L1 case0.679s. Guard4 passed.
No natural runtime red or further source correction. The supported-API correction
changes no assertion semantics. Production files remain untouched.

Once-only controls selected only the new L1 test:
- M1: runtime exit100 at line763/771, the expected pre-catch-up refusal no longer
  exists when the joiner server borrows the founder registry.
- M2: runtime exit100 at line814, applied/snapshot equality differs with a fresh
  joiner starting at first_free3.
- M3: runtime exit100 at line820, post-catch-up voter set is {1,2} instead of {1}
  after the injected valid promotion. This follows the root contract correction.
None was a compiler failure. No other mutants ran. Each source restoration in
finally was byte-exact; final source equals fa7bc54 without another behavior run.

M1: 12.063s command wall; restored source SHA256 `852d2e4c40bada793115505a78bcc098aed833fe630dfd47f9ce93ac127da622`.
M1.log: `9025ee4924592a3991209d52d334fa2b40c23615de4aa11119d01d401f1ed77f`.
M1.patch: `80c6018f8136c5094acfae1164d0a796da67fdc896cdd3ca98c91dfb15e1195a`.
M1-restore.json: `1a175ffc2f0989dfe4579571187b5b3125cba1b3caff89d8ea1fda3a4b7dce2a`.

M2: 12.359s command wall; restored source SHA256 `852d2e4c40bada793115505a78bcc098aed833fe630dfd47f9ce93ac127da622`.
M2.log: `9064b905dfaecc4e9480d14494a89564347a860953d49ba53f6399a1fed39913`.
M2.patch: `43bac5fb704caa94ac33369dea87e44aca1671a19355aad605a0473d22c5eee4`.
M2-restore.json: `1a175ffc2f0989dfe4579571187b5b3125cba1b3caff89d8ea1fda3a4b7dce2a`.

M3: 11.391s command wall; restored source SHA256 `852d2e4c40bada793115505a78bcc098aed833fe630dfd47f9ce93ac127da622`.
M3.log: `d2a4883cf4971a85f87ef356c3bfb07b041c934e446b7bfdfe8eb2d08ae6195f`.
M3.patch: `3debe257354b6d4501980a27c57e52dd66e7327b7427653733cd46f8c0931c7f`.
M3-restore.json: `1a175ffc2f0989dfe4579571187b5b3125cba1b3caff89d8ea1fda3a4b7dce2a`.

Independent Opus source review, exact-head publication review and all hosted gates
remain pending. Windows job <=660s and fresh green exact main remain required.
No publication, hosted rerun, fixture quarantine, production admission/promotion
or D1-D3 policy change. L1 catch-up is the unauthenticated mem_net test double,
not production TLS replication. M2 open/M3 unchecked; Sol3/3, Opus0/2 unchanged;
hardware paths remain **unverified on real hardware**.
