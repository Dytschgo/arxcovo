# M3 authenticated peer identity review

**Hold publication.** The production change retains the verified leaf binding
as specified. The new inventory guard can pass with a required test or its
module commented out. It also accepts an async signature. Both findings are
reproducible without a Rust build. TLS runtime evidence remains outstanding.

Independent Astra 6.2, high, 2026-11-18. Single reviewer under the owner's
stuck-work authorization. This review covers the following local Git objects.

| Role | Full commit |
| --- | --- |
| Main base and merge base | `e3e79089e6c36f6483cb3db8842e5894f26ee00d` |
| Reviewed root head | `092dcf9c729e3e34111fa33dd8bc753b3b98c944` |
| Original implementation | `a75962718aa6290681a2392c1ec825307af10bf0` |
| Integrated implementation | `be3f9281be28dc7239ac799df36aad1f1b355de8` |

## Standards

I compared the complete base-to-head diff in the prepared review checkout.
The six commits are a080d2cf and 3917865d (PR176 checkpoints), 431df42f
(contract), be3f9281 (implementation), 64dd8024 and 092dcf9c (evidence and
validation disposition). The diff contains four source paths and five docs.
The three PKI blobs at the reviewed head match the original implementation.

| File under `crates/arxd/src/cluster/pki/` | Git blob |
| --- | --- |
| `verify.rs` | `7f60471a863eb8423316e919c83bc254903c12f9` |
| `tls_tests.rs` | `8bed50125797725ed4bafc40b3836334ae0353dc` |
| `authenticated_identity_tests.rs` | `8bfa9a416bb757dce4e43d68b6f60532ede563d1` |

The TLS parent equals the base bytes plus precisely the required module
declaration and attributes. Removing the three new top-level Python items
reproduces the base guard byte for byte. Its 40 top-level AST nodes retain
their values and order, including the owned-query checks. The TLS14, founder13,
fixture28, sample8, atomic1, anchored-proof1 and query inventories retain
their existing definitions. Cargo, workflows, sampler, founder diagnostics,
production query and every source path outside the four-file allowlist match
main. All nine changed blobs match the checkout and decode as BOM-free UTF-8
with LF endings. The base-to-head whitespace check passed.

**Advisory.** The current-work summary at `docs/PROGRESS.md:43` still places
the builder on the older main with integration pending. Its final dated entry
correctly records integration on e3e79089. The coordinator can reconcile the
summary when recording this review. The brief and retained contract review
describe earlier design checkpoints. The merge evidence assigns the hosted
query and founder passes to PR176 and its main, with the founder cause still
unknown. I found no passage granting those passes to the identity tests.

## Spec

**B1. The guard counts commented-out declarations.**
At `tools/ci/test_windows_cluster_quarantine.py:518`, discovery searches raw
source, and the parent checks at line 530 count literal text. I called
`assert_identity_inventory` with three independent in-memory changes.

| Change from reviewed bytes | Required result | Observed |
| --- | --- | --- |
| Block-comment the entire final assumed-founder test, attribute through closing brace | Reject missing test | Accepted |
| Block-comment the child module binding and attributes in tls_tests.rs | Reject missing child binding | Accepted |
| Block-comment the tls_tests module binding and attribute in pki.rs | Reject missing parent binding | Accepted |

The first two edits would silently remove identity coverage from Rust test
discovery. The third isolates the guard's parent-binding check. The comparison
uses the actual guard function in each case. Other build or CI failures are
outside these probes. Count active declarations and add negative controls for
commented-out required tests and for each commented-out binding.

**B2. The named-test matcher admits async functions.**
In a fourth independent probe I changed only the assumed-founder signature
from `fn` to `async fn`. The optional async group at line 518 matched and
the inventory passed. The separate function-name check at line 516 matches
only lines beginning with `fn` and checks uniqueness without checking required
membership. The contract calls for exactly three synchronous tests. Require a
plain synchronous signature for each name and reject the async mutation in a
negative control. A subsequent rustc error would occur at a different gate.

At this head the declarations themselves are correct. The new class exercises
omission, rename, literal duplicates, weakened lists, ignore and cfg hiding
on tests, helpers and parents. It checks both module bindings and the child
path. I ran the Python quarantine file once here. All nine tests passed.
The four additional probes above changed strings in memory only.

The Rust tests implement the requested behaviors on inspection.

- The first fixture uses the existing CA, issued leaf, production configs and
  strict registry callback. It checks both incomplete handles, then pumps TLS
  until both ends finish and reads each handle. Its expected cluster and node
  are fixture constants. The expected digest comes from the generated key's
  SPKI independently of parsing the peer certificate.
- The second fixture generates a separate CA and signed leaf carrying cluster
  [3; 16] in the node URI. Both production config builders install PeerVerifier
  with the new root and a cluster/node/key check. Both clocks use 1_800_000_000
  Unix seconds, within rcgen's default certificate validity. Both completed
  handles must identify that cluster, node 4 and the independently hashed key.
- The binding test applies Found to four two-member states with distinct keys,
  ordered nodes below first-free 10, and checks four non-replay Founded outcomes.
  It authorizes both handles on the matching state, then checks WrongCluster,
  KeyMismatch and NotMember on the respective controls. It applies Remove at
  the next log position, checks non-replay Removed and rechecks the same handles
  for NotMember. No registry fields are seeded directly.
- The assumed-founder test checks node 4 and the exact Incomplete refusal.
  Designated assertion sites consume booleans with static messages. TLS and
  registry fixtures have returned before those assertions execute. They do
  not format identity tuples, keys, digests or certificate bytes.

## Security

In `verify.rs:282`, AuthenticatedPeer keeps private node and optional
cluster/key fields. Clone and Copy, the public constructor signatures and
node() behavior are preserved. Both constructors still check handshake
completion and TLS 1.3 and require exactly one peer certificate. The common
constructor parses that leaf once with peer_identity and stores its returned
components. The cluster-visible accessor maps None to static Incomplete.
Only the test-only assume constructor creates the synthetic node-only handle.

The diff adds no raw constructor, setter, Debug/serde path or retained
certificate, session or store. Signature verification, chain and size checks,
registry/target checks and resumption settings match main. The existing
trusted-config warning applies to both constructors. Arbitrary same-crate
rustls configs can forge evidence. A copied tuple records what the handshake
identified. A future admission caller still needs fresh authoritative state.
Local registry observations can be stale without bound. The change confers
no owner authority, custody, admission, or bootstrap privileges. Reverting
the slice requires no stored-data, wire-format or configuration migration.

## Evidence and release disposition

The implementation record reports fmt, Clippy, guard8 and sampler60 on the
older parent. Its sole TLS command exited 101 during rustc compilation after
the C volume filled. Zero tests ran. Neither designated semantic mutant ran.
The old logs are absent from this checkout. I reused the committed account
and did not independently authenticate its log hashes. I verified the current
verify.rs SHA-256 as
`a65991d2ebe729fd9071ab66e36e760fc88d888f79039df1d0d48b33f4b1f5ba`.

The independent Sol validation assignment owns the D-volume baseline and
two designated mutation runs. Their outcomes and restoration evidence are
pending in this review. I ran no Rust, Cargo or engine commands. Resolving
B1 and B2 and obtaining the bounded runtime evidence precede publication.
The eventual publication head still needs independent exact-head review,
all five normal hosted gates, feature Windows <=660 s and fresh green main.
The PR176 main result is historical context. Only the coordinator merges.

M2 remains open, M3 unchecked. Sol 3/3 and historical Opus 0/2 remain
unchanged. Hardware paths are **unverified on real hardware**.

Standards: one advisory (stale summary). Spec: two guard blockers, including
silent omission of required coverage. Security: zero production findings.
