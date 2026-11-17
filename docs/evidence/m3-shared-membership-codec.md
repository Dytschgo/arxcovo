# M3 L2-A shared membership-v1 codec evidence

Current status: local verification and independent Sol source review PASS.
Hosted checks, exact-head review and Windows <=660s remain pending.
Sol6.1 high implemented; root Sol6.1 medium integrated and checked final format.
No engine test ran and no production join behavior is introduced.

## Pins and scope

- Prepared base: `9ddc1212deef0ea641f6707b8ffb23db73a660d1`.
- Source main: `fa7e3c019484bf2335bd26fbbafd2d65b207f480`.
- Source: `d4694d575f186541c5d6338230ea1287b3e252e9`.
- Original detailed evidence: `1117ba646368640f35f81f86da00959d829cf169`.
- Independent review: `1da2f46698125d38cdc5f4bd50adbe62465ca2b8`, retained byte-for-byte.
- Contract/root review: `e22dc933634fc7be8d391f8d749898d7578f5a4c` and the codec brief.

Four source paths changed: std-only membership.rs, one cluster module binding,
consensus codec wrappers/imports, and additive synchronous wrapper tests.
Private validated fields, immutable accessors and checked input preserve the v1
wire format, bounds, configuration order/duplicates, learner metadata and error
normalization. StateStore still accepts opaque bounded blobs as before.
No schema, command, enrollment, PKI, log, custody, transport or control change.
Consensus code from to_stored onward is unchanged. All old consensus test bytes
remain an exact prefix; prefix SHA256:
`0ea8b1055b47d1823e09cef5f6aef81ebb4c15bcb6a05c8e0ae856aeac2e6410`.
Source membership.rs baseline SHA256:
`6b0a0fad18f8588d920935f455e84b9957d16b8bf9a9443ca13706452e8950f3`.

## Actual verification

Windows, one bounded natural round; no engine/native/cluster loop.
Every dependency command used --locked --offline. Environment explicitly set
CARGO_NET_OFFLINE=true and CARGO_PROFILE_DEV_DEBUG=CARGO_PROFILE_TEST_DEBUG=0.
Task-owned target/temp/log root: `target/l2a-9ddc1212-20261117` in the assigned
builder worktree. No existing cache was deleted; owned command processes exited.

| Check | Result | Command wall seconds |
| --- | --- | --- |
| Initial fmt | PASS | 0.890 |
| Initial workspace all-target Clippy | E0433, inherited BTreeMap missing | 49.062 |
| Corrected workspace all-target Clippy, -D warnings | PASS | 5.643 |
| Focused selection listing | 11 pure cases | 57.859 |
| Focused nextest run | 11/11 PASS, test wall0.145s | 1.141 |
| Quarantine guard | 5 PASS | 0.328 |
| Sampler suite | 60 PASS | 0.391 |
| Full arxd listing | protected inventory unchanged | 13.234 |

The one allowed mechanical correction restored production BTreeMap and left
only BTreeSet under cfg(test). It happened before behavior tests; no algorithm,
acceptance or limit fix. The builder's fmt preceded it. Root checked final-source
fmt separately: exit0,0.875s; D:/Temp/arx-l2a-root-wr93ssym/fmt.log SHA256
`e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`.
No final-format or behavior rerun is attributed to the builder.

Focused expression (listed before run):

```text
test(/^cluster::membership::tests::/) | test(/^cluster::consensus::tests::shared_membership_codec_/) | test(=cluster::consensus::tests::maximal_codec_is_canonical_and_every_truncation_fails) | test(=cluster::consensus::tests::codec_rejects_noncanonical_and_out_of_bounds_memberships)
```

Exactly7 new pure tests,2 new wrapper tests and2 unchanged old pure tests.
Protected JSON confirms quarantine28/sample8/founder13/requiredTLS14, with the
joint proof ignored on Windows and founder/TLS cases unignored. These are
listings, not engine executions. No ignored-selection run occurred.

## Once-only compiled mutation probes

Each probe selected one new pure test, changed source bytes, compiled and exited
100 at the designated assertion. No compiler/unwrap/unrelated-red credit.
Preregistration, full patches and raw logs are retained in the artifact root.
Finally restored the exact baseline hash after EACH; no natural suite replay.

| Probe | Selected test | Observed semantic failure |
| --- | --- | --- |
| Omit learner encoding | literal_u_encode_preserves_learner | literal-U encode equality |
| Flatten identical configs | constructor_preserves_identical_configs | direct [{1}] vs [{1},{1}] |
| Admit exclamation address | constructor_refuses_well_formed_invalid_character | direct Ok vs Err |

## Source blobs and raw artifacts

- `crates/arxd/src/cluster.rs`: `1f5828b1cae80678ff31a4bc6ab106d6abc39d4c`.
- `crates/arxd/src/cluster/consensus.rs`: `ab95f2eea891abf02b869dce56f6f8131ab0f4a5`.
- `crates/arxd/src/cluster/consensus/tests.rs`: `7721992935651cd9a419fbadfa01c7c2ea493c0c`.
- `crates/arxd/src/cluster/membership.rs`: `78f23118696b34547affd351171ffebbe2d89107`.

The reviewer compared frozen source/workflow/Cargo/fixture/guard/sampler blobs,
recomputed hashes and reconstructed all mutation patches. It reused compile/test
results with attribution and ran no Rust or engine case. Advisory A1's verbose
escaped log decorations and trailing whitespace are removed from this summary;
raw logs and the original detailed evidence commit above remain unchanged.

Artifact SHA256 manifest:

```text
05ab6c148ea6bb0340d1920f5c5640e5a899c173488bbccb01a40c871f877468  address.log
d0994fd5083acc6b1339c321fe0bc07376a6b0e068fa4397371087a7005f8c28  address.patch
b3712324a0bfb4283af0c483fd09b2a8188864add18f651502667655bfaad1d7  clippy-correction1.log
56ce7c509aebde4a40440c1dcaf043a08b2099ae3f56de4b4b99194bd2827232  clippy.log
a3cd85c994f35fa0d81f18421b337f6a1e3762f279d0eaee1f4eca6c228ee9b9  duplicate.log
06a8c3206f5fbe6dcc396fb2bab764661627034217449819a8e558ccf1a46449  duplicate.patch
e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  fmt.log
be7e29f43e4286dbb3cd96c130f8c018d5244f1609bf0f521dafd60d9bdcb45a  focused-list.log
1ea8226a091c67289f8ce55739c34505f9e8735566390d361820b2d231ecd468  focused-run.log
1632dcc03294ad0a6f731dd28a326324cc0fd2f2f1cadadf9a9a9d2423f7c7a2  guard.log
65d2a11abb22a49e6c537eed917216f2576da643a8df90366e25e7f0000d9715  inventory.json
0f9b0d5f3c9b31f3e0b717ae4e2b5b4e52f14a61f6700468b08d3a24fb968751  inventory.log
050029a01b1340c27cbe32973228b5608cf49cb456413f6f0604f1db8c0a4598  learner.log
43a998c6822cd782b056c75739a02d0ccced71f51d58f266a8391c1c565d8864  learner.patch
6b0a0fad18f8588d920935f455e84b9957d16b8bf9a9443ca13706452e8950f3  membership.baseline.rs
475086a95e9d9f72cd6bd543471fabc74e4cae6f04e84c99c3a9706456afafa9  mutation-log.txt
e7c777b9623ecbc2eb3e4bf712af088cb3c4a03857bcc19757d5adba5b95bf17  mutations.py
44d8492ea3bb8e83ba466988ada994091940c26882a938dec25a0f866f7449ac  natural.py
2c56b0cdd7fd43cf915a7b8c78f8c1749de03648a8b7949ff831c170140a2a4b  protected-inventory.json
1ea79f6e36406d6aa7a68b4b3625a9af15a982a946982f99c2724649fe339707  results.txt
80f943be4c173197cfb1b0fe6ee2f23d39fdf5474c58a2965b24d1d672edf9f8  sampler.log
b4a70e90c0be69d704fcaa3e940f460c15417185db1304ddc16b9ebaaa6a21d2  selection.txt
3660a1e59bff224249bfa71cdb454d9afb44fc38a77ada26cb90ce4b4ad4e58f  session.txt
9affb12fc53af31208b7157e9207b9e9eb3a216639d2a41c58b612c76dae6d6d  verify.py
```

## Remaining gates and limits

Root rechecked exact mainfa7e3c01 and CI44090513191 attempt1 SUCCESS,604s wall /
Windows600s, before publication preparation. This does not replace hosted checks
on the new feature head. Hosted new pure tests on all OS, all five normal gates,
exact-head independent review and Windows<=660s remain required.
Reverting the source commit restores the old codec with no DB migration.
No source mutant remains. No public action, install, admin/VM work, timing,
workflow, capacity, quarantine or repair-budget change in this local task.
L2 B3 apply-time decoding/anchors, schema, custody, transport/control remain out
of scope. M2open/M3unchecked; Sol shared3/3 and Opus0/2 unchanged.
Hardware paths remain **unverified on real hardware**.
