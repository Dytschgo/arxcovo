# M3 slice 3: acceptance-platform CA and member-key custody

Revised builder contract for adopted coordinator decision D1, 2026-11-19. Design sub-agent: OpenAI GPT-6.1 Sol / high. Source baseline is **fc83838d9eda05ab5e738cae8f8b7d3b53f3c606**, read through immutable Git objects. Independent design review: GO with required changes, SP1 persistent-format gap; this revision supplies the normative layout and independent structural hex fixtures below. Coordinator adopted section 8 items 1–5. This revision has not itself received independent re-review and grants no runtime qualification, live startup, Engine/founder or merge authority.

## Recommendation and decision boundary

Use the existing rustls/rcgen design with **locally materialized, owned rcgen keys inside an isolated daemon process**. For hosted Ubuntu acceptance, give each node a non-root daemon UID distinct from its arbitrary-build UID; keep an independently generated per-node symmetric wrapping key in a daemon-only persistent root, and AEAD-seal CA/member PKCS#8 bundles. Qualify actual access denials under the build identity. No remote signer, HSM, generic secrets vault or keychain integration is needed for this slice.

This resolves the owned `Issuer<'static, KeyPair>` requirement in `pki/ca.rs` and frozen `found(&ClusterCa, ...)` interface. A signer that never releases a key into the daemon would require a separate API/design change and is outside this proposal. The settled OS-keychain-backed **secrets vault is post-v1**. This proposal does not build it. The private wrapping-key file is a narrowly scoped acceptance backend, not a claimed universal replacement for native keychain policy.

The coordinator adopted this backend and lifetime policy. Draft ADR0037 is attached at `D:/Temp/arx-m3-custody-d1/adr-0037-acceptance-custody.md`; coordinator supplied 0037 as the next free main number. Record it in the repository before implementation, explicitly reconciling ADR0005's daemon/build separation and ADR0009's deferred M3 OS-keychain decision with ADR0029. ADR0006's historical root-file objection is not silently superseded. No Windows account default changes.

## 1. Security premise and limits

Protect CA and member private keys from arbitrary **standard-privilege agent build commands**, hostile workspaces, other cluster learners, unauthenticated network peers, substituted envelopes and accidental cross-node/cross-cluster loading. The trusted base comprises the pinned daemon binary/image/configuration, OS/kernel/container runtime and runner control plane, selected daemon identity, CSPRNG and crypto implementation. An authenticated agent principal is an application identity; it is neither a daemon identity nor OS permission to read keys. Peer membership likewise grants no owner or execution authority.

For a qualified hosted node:

- Daemon and build processes have different numeric UIDs, no shared supplementary access group to private roots, no CAP_SYS_PTRACE, CAP_DAC_OVERRIDE, CAP_SETUID or other capabilities; no-new-privileges applies. The daemon is non-root. A trusted runner starts the fixed processes as their explicit UIDs; it does not give the daemon a general privilege-changing helper.
- Build identity cannot traverse/read/write daemon roots, inspect daemon memory or duplicate its handles, read secrets from `/proc`, alter daemon executable/configuration/libraries, attach a debugger, signal the daemon, or access a container/host control socket. No key-bearing descriptor is inherited by a build. Workspace/tool search/environment are untrusted and never select daemon loaders/plugins. Disable core dumps; do not promise elimination of OS swap/crash-memory copies.
- The node's private persistent volume is unique to that node and absent from other nodes' mount namespaces. Build workspace is disjoint from state, credential, custody and wrapping-key roots. Runtime capabilities, mount table and deny probes are evidence; names such as `private` or `Qualified` are not evidence.
- The hosted control plane may use Docker exec with an explicit build UID to qualify access denial. That is a test observation, **not the production remote-execution mechanism**. Slice 14/D4 must provide actual A-to-B execution under the same isolated build identity before full acceptance. Running arbitrary commands as the daemon UID would invalidate D1 even if the file tests once passed.

Learners hold only their own member key and public trust/receipt material. **They never hold the CA, including an encrypted CA copy in a learner-mounted volume.** Only the legitimate fresh founder creates the initial CA; subsequent CA delivery is slice 12, after actual membership-qualified promotion. Do not add a `voter: bool` argument that confers membership authority. Custody primitives cannot independently prove committed voter eligibility; later trusted composition supplies it.

Private modes/ACLs block other identities, not arbitrary code running as their owner. Same-UID builds can read the wrapping key, unseal envelopes or inspect daemon memory. User-scope DPAPI alone shares the user's protection context; entropy stored alongside a blob does not fix this. Machine-scope DPAPI alone does not distinguish daemon from build. Root/admin, kernel/container-host compromise, compromised daemon, malicious trusted runner and physical theft of both envelope and wrapping-key storage are outside this backend's guarantee. Envelopes provide integrity/context binding and confidentiality when separated from the wrapping key; whole-volume encryption, anti-rollback counters and secure deletion are not claimed. A compromised voter may obtain the CA; existing committed member bindings and issuance guards remain necessary even when CA verification succeeds.

## 2. Hosted qualification and native mapping

Use three nonprivileged node containers in the eventual acceptance platform. Each has daemon/build identity separation as above; UID numbers may repeat across containers only when private mounts and runtime isolation prevent cross-node access. The hosted runner may prepare synthetic directories and ownership; no owner-PC account, admin or hypervisor step is authorized. This slice needs only a minimal custody process harness and short deny probes, not a cluster, Raft initialization or daemon-startup integration.

Use the acceptance brief's eventual private internal network, runner-side gateway/metadata/outside-cluster denies, no published ports, no host networking, no Docker socket or owner mounts, explicit CPU/RAM/disk/log/phase limits. A Docker bridge alone is insufficient. Slice 3's custody probes do not certify those future network fences.

Native mapping preserves the same semantic boundary; **native qualification remains owner-side setup**:

| Platform | Mapping after separate native qualification | Refusal / limit |
| --- | --- | --- |
| Linux | Daemon-owned private roots; qualified distinct standard build UID, groups, proc/debug/handle and execution fences. The file-envelope backend can be reused after actual qualification. | Same UID, root/capable builds or missing execution isolation cannot claim this protection. Existing Linux account policy is unchanged. |
| macOS | Same key/envelope semantics after distinct execution identity, private-root ACLs and actual process/descriptor/debug qualification. Native keychain wrapping could be a later narrowly reviewed backend. | Do not infer Linux proc or service behavior. Existing macOS policy remains; no Keychain implementation or owner setup here. |
| Windows | Preserve ADR0029 `current_user` default and truthful shared-account reporting. A future qualified process/account boundary must protect the CA/member keys from the actual build token; private DACLs and optionally daemon-context DPAPI can then wrap/protect storage. | Current-user daemon plus arbitrary same-user builds is **not custody-qualified**. Inventory/current-user M2 behavior stays as settled. Optional dedicated execution is an owner-selected route, not an automatic mandate or silent fallback. No S4U retry, LocalSystem fallback or account creation. |

Do not ship an OS-name-based “qualified” switch. Slice 3 implements and proves the hosted backend; unsupported native composition returns Unqualified/Unavailable before generating or releasing keys. Native backend activation, installed paths and full execution integration are separate assignments. All native/multi-host paths here are **unverified on real hardware**.

## 3. Keys, authenticated envelope and context

Use P-256 ECDSA/SHA-256 for new CA and member keys, matching the existing CA test implementation and accepted CSR algorithms. Generate via rcgen's pinned crypto provider with OS randomness, never deterministically from cluster/node IDs. New wrapping keys are 32 CSPRNG bytes. Use the already locked ring 0.17.14 as an explicit direct dependency if necessary for AEAD; do not depend on a transitive import or add a second crypto stack. Builder verifies exact API support against pinned sources. Cargo/lock changes must be the minimal direct-dependency delta, with no upgrades or feature broadening elsewhere.

The normative v1 envelope below uses exact binary domain/magic `ARXKEY01`, version, closed kind (CA/member), installation ID (random 128 bits), cluster ID, pinned CA fingerprint, subject SPKI digest, fresh 96-bit random nonce, bounded ciphertext length, ciphertext and tag. Use ring ChaCha20-Poly1305 with a 256-bit key. Authenticate the entire 122-byte header as AAD. The CA kind's SPKI digest is the CA's, not a node binding. Each slot is created once; no rewrite/rotation loop under one nonce or unbounded seal endpoint. RNG failure refuses with no usable key result.

Maximum total envelope 16 KiB, PKCS#8 4096 bytes, certificate DER 4096 bytes, wrapping-key file exactly 32 bytes; strict lengths before allocation/decrypt, no trailing bytes, unknown versions/kinds/fields refused. Bound directory entry counts before collection. CA encrypted payload contains canonical length-delimited PKCS#8 and the **original CA DER**. Member payload contains PKCS#8; CSR is reproducible public data, not an authority record. No PEM text, environment variables, argv, stdin shell text, logs, MCP responses, image layers, build contexts or test artifacts carry private keys or wrapping keys.

Expected context comes from trusted private operation inputs and retained roots, never solely from the envelope being decoded. Compare header with expected context and authenticate before returning plaintext; recompute actual SPKI after parsing. A missing/untrusted expected context is refusal. Installation ID binds local placement, not hardware attestation: copying it and the wrapping key defeats that binding, within the stated privileged-attacker limit. Different node installations have different wrapping keys and IDs.

Member generation occurs before the issuer assigns a node ID. Its seal context therefore binds installation, cluster, pinned root and SPKI, **not an invented node number or origin E**. Return CSR only after durable sealing, generated from that same retained key. Later loading for TLS additionally requires expected node/key/root and the actual leaf/receipt validation. This avoids an unnecessary reseal on assignment. It does not authenticate the receipt's origin: only actual guarded Found or the original same-connection Redeem result can do that in slice 4/8. A successful decrypt or CSR proof cannot mint a Found/Redeem authority token.

### 3.1 Normative persisted bytes (SP1 closure)

This section is the sole byte definition for new slice-3 custody files. All offsets are zero-based byte offsets; intervals are half-open. `u8` is one octet; `u32be` is an unsigned 4-byte integer, most-significant byte first. IDs/digests/nonces are raw fixed-width bytes, never ASCII hex, UUID text, base64, JSON, DER integers or platform-native integers. Magic bytes include no NUL or newline. There is no padding, BOM, optional field or alternate encoding. All lengths count octets. Checked arithmetic and exact total-length equality are mandatory before slicing/allocation. Format refusal never triggers a migration or overwrite.

| Persisted object | Offset | Width | Exact value / interpretation |
| --- | ---: | ---: | --- |
| `install-context` | 0 | 8 | Magic ASCII `ARXCTX01`, hex `41 52 58 43 54 58 30 31` |
| `install-context` | 8 | 1 | Version `01` only |
| `install-context` | 9 | 16 | CSPRNG installation ID; every 16-byte value is structurally valid; no timestamp/UID inside it |
| `install-context` | 25 | 0 | Mandatory EOF: total exactly 25 bytes |
| `seal-key` | 0 | 32 | Raw uniformly generated symmetric key; no magic/version/length wrapper; total exactly 32 bytes |
| `custody-lock` | 0 | 0 | Empty regular file, exactly zero bytes; ownership is the actual exclusive OS lock, never file contents |

`install-context` grammar is exactly `4152584354583031 || 01 || installation_id[16] || EOF`. It is public but daemon-private and must match separately trusted expected installation context. It is not a bearer credential, UID attestation or source of authority. Fresh creation of ID/key is explicit; a changed/missing/partial context or key on recovery refuses. The raw wrapping-key file has no multi-version compatibility promise beyond its required 32 bytes.

| Envelope field (both `ca-key` and `member-key`) | Offset | Width | Encoding / rule |
| --- | ---: | ---: | --- |
| Magic/domain | 0 | 8 | ASCII `ARXKEY01`, hex `41 52 58 4b 45 59 30 31` |
| Version | 8 | 1 | `01` only |
| Kind | 9 | 1 | `01` = cluster CA; `02` = member key; all other values refuse |
| Installation ID | 10 | 16 | Raw ID, equal to trusted expected ID and `install-context` |
| Cluster ID | 26 | 16 | Existing raw `ClusterId`, exact expected value |
| CA fingerprint | 42 | 32 | SHA-256 of exact original CA DER, exact expected value |
| Subject key digest | 74 | 32 | SHA-256 of exact DER SubjectPublicKeyInfo; CA SPKI for kind01, member SPKI for kind02 |
| Nonce | 106 | 12 | Fresh random nonce; no byte-order interpretation |
| `L` | 118 | 4 | `u32be` **ciphertext length only**, excludes header and 16-byte tag; equals plaintext length for this AEAD |
| Ciphertext | 122 | `L` | AEAD ciphertext of exactly the kind-specific payload below |
| Tag | `122+L` | 16 | ChaCha20-Poly1305 authentication tag, immediately after ciphertext |
| EOF | `138+L` | 0 | Total file length must equal `138+L` |

AAD is **exactly bytes `[0,122)` as persisted**, including magic, version, kind, every ID/digest, nonce and the four length bytes. No prehash, string prefix, detached metadata or omitted length bytes. Pass nonce bytes `[106,118)` as the AEAD nonce; it is intentionally also included in AAD. Seal with the 32-byte `seal-key`. A successful structural parse does not authenticate any field. Compare trusted expected context, then require AEAD authentication; no key/certificate parser or owner is exposed until authentication succeeds. Header context changes with a recomputed outer length still fail authentication. There is no key-ID selector, legacy mode or algorithm-negotiation byte: version01 fixes the algorithm.

| Decrypted payload | Offset | Width | Encoding / rule |
| --- | ---: | ---: | --- |
| Both kinds: `K` | 0 | 4 | `u32be` PKCS#8 DER byte length, `1..=4096` |
| Both kinds: key | 4 | `K` | Exact PKCS#8 DER bytes, later strict crypto parsing required |
| Member EOF | `4+K` | 0 | Member payload length `L=4+K`, range `5..=4100` |
| CA only: `C` | `4+K` | 4 | `u32be` original certificate DER byte length, `1..=4096` |
| CA only: certificate | `8+K` | `C` | Exact original CA certificate DER bytes |
| CA EOF | `8+K+C` | 0 | CA payload length `L=8+K+C`, range `10..=8200` |

Kind-specific `L` bounds apply to the **header before decrypting**, in addition to file cap 16384 bytes and checked `138+L` equality. Maximum canonical v1 file is 8338 bytes (CA); the larger generic cap is not permission for padding. After authentication, payload lengths must consume exactly `L`; trailing plaintext, zero/oversized DER lengths and mismatched totals refuse. DER bytes must then be fully consumed by actual key/certificate parsing and pass the context/SPKI/signature/policy checks. ASN.1 parsing success cannot waive this framing.

Existing future `credentials-root/identity.json` and `ca.der` are **not created or given new formats in slice 3**. The later publisher retains the existing PublicIdentity v1 grammar and original root DER; its `member-key` artifact uses exactly the above member envelope bytes, with no extra publisher header. No other persisted record, generation counter, error latch, role marker or “seal succeeded” file is added. The creation failure latch is in-memory, not a recovery authority.

### 3.2 Independent hex fixtures (literal normative codec oracles)

These fixtures were authored here without a production encoder. Whitespace/newlines in the blocks are presentation only; remove ASCII whitespace to obtain bytes. Hex is case-insensitive in the fixture reader only; production files are binary. The builder must hard-code the expected byte strings and parsed field values in tests, and assert encoder output byte-for-byte against them. Expected bytes must not be produced at test time by the encoder under test. Decoder/encoder round-trip alone is insufficient.

Fixtures are **structural codec** positives. Payload sentinel bytes `30`/`31` are deliberately not usable PKCS#8/certificates; envelope ciphertext/tags are deliberately not authenticated. A full production load of these structural fixtures must fail. Real valid AEAD/key/load/fresh-process positives are required separately with runtime-generated disposable keys, never retained in fixture evidence. This keeps public framing oracles free of private key material while detecting encoder/decoder shared-layout mistakes.

**I1 — install-context**, exactly 25 bytes; parsed ID `000102030405060708090a0b0c0d0e0f`:

```hex
4152584354583031 01 000102030405060708090a0b0c0d0e0f
```

**P1 — member plaintext framing**, exactly 5 bytes, `K=1`, sentinel key `30`:

```hex
00000001 30
```

**P2 — CA plaintext framing**, exactly 10 bytes, `K=1`, key `30`, `C=1`, certificate `31`:

```hex
00000001 30 00000001 31
```

**E1 — member envelope framing**, exactly 143 bytes, version01/kind02, ID00..0f, cluster10..1f, CA digest20..3f, subject digest40..5f, nonce60..6b, `L=5`, ciphertext a0..a4, tag b0..bf:

```hex
4152584b45593031 01 02
000102030405060708090a0b0c0d0e0f
101112131415161718191a1b1c1d1e1f
202122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f
404142434445464748494a4b4c4d4e4f505152535455565758595a5b5c5d5e5f
606162636465666768696a6b
00000005
a0a1a2a3a4
b0b1b2b3b4b5b6b7b8b9babbbcbdbebf
```

The exact E1 AAD is its first 122 bytes, ending in `606162636465666768696a6b00000005`. Assert every field offset, ciphertext slice `[122,127)`, tag `[127,143)` and no trailing bytes; do not count the tag in `L`.

**E2 — CA envelope framing**, exactly 148 bytes; same fixed context/nonce pattern, kind01, `L=10`, ciphertext a0..a9, tag b0..bf:

```hex
4152584b45593031 01 01
000102030405060708090a0b0c0d0e0f
101112131415161718191a1b1c1d1e1f
202122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f
404142434445464748494a4b4c4d4e4f505152535455565758595a5b5c5d5e5f
606162636465666768696a6b
0000000a
a0a1a2a3a4a5a6a7a8a9
b0b1b2b3b4b5b6b7b8b9babbbcbdbebf
```

The CA digest/SPKI header values in E2 are public codec sentinels, not claims about an actual certificate. E1 and E2 reuse nonce bytes only as unauthenticated framing samples; production sealing always samples a new nonce.

**R1 — raw wrapping-key framing**, exactly 32 public sentinel bytes `000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f`. Test only exact-byte copy/length validation in memory; do not provision this as a real wrapping key or save it as run evidence. **Z1 — lock framing**, empty hex string, length zero. These are the only files with no magic/version.

Malformed fixtures below are exact byte recipes referencing the literal positives. `||` means concatenate byte strings; `X[a:b]` means zero-based half-open slice; `replace(X,a,b,h)` replaces exactly `[a,b)` with literal hex `h`. These operations do not invoke the production encoder. Freeze the expanded bytes in test fixtures where practical. Every case must refuse structurally, before any AEAD/key release; keep E1/E2/P1/P2/I1 positive parser controls in the same suite.

| Fixture | Exact bytes / recipe | Required refusal |
| --- | --- | --- |
| I2 | `I1 || 00` | Trailing install byte (26 bytes) |
| I3 | `I1[0:24]` | Truncated installation ID |
| I4 | `replace(I1,8,9,02)` | Unknown install version |
| I5 | `efbbbf || I1` | BOM / noncanonical install grammar |
| I6 | `replace(I1,0,8,4152584354583030)` | Wrong install magic |
| P3 | `00000000` | Zero-length key |
| P4 | `00001001` | Key length4097, oversized before allocation |
| P5 | `01000000 30` | Little-endian representation of1; BE length16777216 refuses |
| P6 | `P1 || 00` | Trailing member plaintext |
| P7 | `00000001 30 00000000` | Zero CA certificate length |
| P8 | `00000001 30 00001001` | CA certificate length4097 |
| P9 | `P2[0:9]` | CA certificate missing/truncated |
| P10 | `P2 || 00` | Trailing CA plaintext |
| E3 | `replace(E1,0,8,4152584b45593030)` | Wrong envelope magic |
| E4 | `replace(E1,8,9,02)` | Unknown envelope version |
| E5 | `replace(E1,9,10,00)` | Kind00 refused |
| E6 | `replace(E1,9,10,03)` | Kind03 refused |
| E7 | `replace(E1,118,122,05000000)` | Little-endian length: BE83886080, oversized |
| E8 | `replace(E1,118,122,00000015)` | Incorrectly tag-inclusive length21; total159 required but143 supplied |
| E9 | `E1[0:142]` | Tag truncated by1 |
| E10 | `E1 || 00` | Trailing ciphertext/file byte |
| E11 | `replace(E1,118,122,ffffffff)` | Oversized length / checked arithmetic refusal |
| E12 | `replace(E1,118,122,00000000)` | Member payload too short |
| E13 | `replace(E2,118,122,00002009)` | CA length8201 exceeds8200 |
| E14 | `replace(E1,118,122,00001005)` | Member length4101 exceeds4100 |
| E15 | `E1[0:121]` | Incomplete fixed header, including partial length |
| R2 / R3 | `R1[0:31]` / `R1 || 00` | Raw key must be exactly32 bytes |
| Z2 | `00` | Nonempty lock file |

Structural acceptance never permits loader acceptance. For real AEAD positives generated at runtime, flip **each header field** (including nonce/length), ciphertext and tag independently, and test wrong wrapping key/trusted expected context, with zero key release. Demonstrate at least one same-length header-context mutation that remains structurally well formed but fails authentication, alongside its unchanged valid control. Run all fixtures against the actual production framing implementation; a test-only duplicate parser cannot satisfy SP1. These document fixtures are authored specification data; no Rust/crypto/runtime test was run in this revision.

## 4. CA construction, loading and lifetime

Fresh founder operation generates a new CA key and self-signed certificate: P-256, CA basic constraints pathLen0, KeyCertSign/CrlSign, no node URI, no intermediate chain, bounded DER and a fixed non-secret Arxcovo cluster-CA name. Persist the exact DER with its key in one envelope. The cluster ID is authenticated by envelope context; a CA DN is not a cluster membership assertion. Retain the original DER bytes across restart: regenerating a self-signed certificate changes its fingerprint and breaks join-code/root bindings.

Production loader decrypts, strictly parses the full key and certificate, compares expected fingerprint and cluster context, requires supported algorithm/constraints/usages, issuer=subject and verifies the actual self-signature using the pinned crypto implementation. Require byte-exact derived-key SPKI equality with certificate SPKI before assembling an owned rcgen Issuer from the parsed CA parameters and key. No public arbitrary `(params, key, not_after)` constructor; bounds come from that original certificate. Do not silently self-sign a replacement certificate on load. Test loading by signing a leaf that verifies under the original persisted root; parsing alone is inadequate.

Adopted production lifetime is **365 days from checked creation time**. Set `t=floor(creation_ms/1000)`, notBefore=`t-120`, notAfter=`t+31536000`, using checked arithmetic/range conversion. Preserve existing leaf policy: notBefore = floor(now_ms/1000)-60, notAfter = that +2592000 seconds, both server/client EKU, node URI, proven CSR key, random serial and existing sealed-ticket/fresh-guard pipeline. Add a production-safe CA containment check for **both ends** of that leaf interval at every signing call; current `leaf_fits` checks only the upper end. A rollback before CA validity must never sign an out-of-range leaf. Preserve the existing sentinel/no-sign oracles and their positive controls. A healthy load-time clock does not authorize later signing without the actual sign-time containment/range checks.

Load/sign observations use fallible validated time in ADR0030's valid range, under its trusted-host-UTC premise and recovered floor when one exists. Do not turn a recovered public sample into a reusable signing clock. Fresh creation gets a fresh checked clock observation with the existing trusted-UTC premise, not a fabricated persisted floor. Exact clock plumbing to actual startup/founder remains later. New custody operations reject unavailable/invalid time; existing founder clock adapter/signature is frozen.

Issuer loading requires a full 30-day leaf interval to fit. Insufficient life returns CaExpiring; do not shorten leaves, extend CA time, ignore validity or auto-generate a CA. Already issued, still-valid member leaves may continue to authenticate with the original trust root; possession of the CA is not required for TLS. Member loading validates actual leaf time through the existing verifier at a fresh qualified observation. Certificate expiration and enrollment-code expiry remain different policies; TTL900000/skew30000 stay unchanged.

No renewal, CA/key rotation, migration, revocation-system redesign or automatic recovery is implemented here. Coordinator adopted the 365-day finite-lifetime/stop policy with its availability limitation. Full operational renewal availability is not claimed; the eventual inability to issue is a material limitation, not a hidden fallback. Tests use controlled time at boundaries with no wall-clock waits.

Keep keys only in daemon memory while needed. ClusterCa retains its owned rcgen KeyPair through authorized issuer lifetime; member owner retains its key through CSR/credential use, and rustls necessarily owns signing material during connections. Limit temporary PKCS#8 copies, do not derive Debug/Serialize/Clone for secret owners, and release temporary buffers promptly. Best-effort clearing is useful but **complete rcgen/rustls/allocator/process-memory zeroization is not promised**. No core/key dump diagnostics. Process exit drops in-memory ownership; restart decrypts the same key, never replaces it to make progress.

## 5. Narrow interfaces and persistent layout

Proposed interfaces describe real key/file operations, not public capabilities or a new lifecycle framework. Keep them cluster-private and minimize exported methods:

```text
Custody::open_existing(checked roots, expected local installation) -> Custody
Custody::create_fresh_installation(checked empty roots) -> Custody
Custody::create_founder_ca(expected cluster, fresh checked time) -> owned ClusterCa
Custody::load_ca(expected cluster/root, fresh checked time) -> owned ClusterCa
Custody::create_member(expected cluster/root) -> retained MemberKey + bounded CSR
Custody::load_member(expected seal context) -> retained MemberKey
MemberKey::check_leaf(expected cluster/node/key/root, leaf, checked time) -> matching TLS key owner
```

There is no generic `unseal(bytes) -> Vec<u8>` for callers, string key lookup, arbitrary path, arbitrary signer or CA export endpoint. A private checked file owner supplies bounded bytes and retains actual handles. Member/CA loading authenticates and validates before returning usable objects. A caller may borrow `&ClusterCa` for existing issuance. Production export/transfer of CA PKCS#8 is withheld until slice 12's membership-qualified channel exists; normal serializability is not added now. `check_leaf` establishes key/leaf consistency and cryptographic validity, not issuance-origin provenance or normal registry admission. Future TLS integration still uses strict registry/receipt-scoped authorization in its own slice.

Proposed roots (all daemon-only, disjoint from workspace and state):

```text
wrap-root/             # separate private persistent mount/root
  install-context      # bounded public ID, version; private permissions
  seal-key             # exactly 32 secret bytes, create-once
custody-root/          # staging/issuer storage, not startup Plan.credentials
  custody-lock         # real exclusive OS lock
  ca-key               # CA envelope; founder/voter only
  member-key           # pre-publication member envelope
credentials-root/     # owned by later actual publisher, not written here
  identity.json        # existing PublicIdentity v1
  ca.der               # original public root
  member-key           # opaque basename in PublicIdentity.artifact
```

No fourth CA file in `Plan.credentials`: startup_plan currently requires exactly three entries and fresh credential emptiness. Keep CA and wrapping roots outside that directory and outside the build workspace. Slice 3 exercises separate disposable custody roots and does not mutate startup preparation or pretend its Owner accepts this new delta. Slice 4 must settle actual owner-bound publication/handoff: move the retained staged member envelope into its final slot without untrusted path recapture, retain original key context, and preserve exactly-three-entry recovery. Do not copy/write the final `identity.json` here. The staging member slot may be absent after confirmed later publication; no operation treats that absence alone as permission to generate another key.

File rules: canonical absolute roots, bounded components, no symlink/reparse points/alternate streams/nonregular objects/hardlinks, no ancestor/workspace overlap or alias, restrictive creation modes/private DACLs from creation. Inspect the actual opened handle, identity, ownership/permissions and single-link status, rather than pathname metadata followed by ordinary reopen. Retain checked directory/file objects and recheck namespace identity at effect boundaries under the qualified no-external-writer premise. Reuse repository primitives only where they actually provide these properties; no claims of continuous exclusion from same-account writers. Unqualified filesystem/durability semantics refuse.

Creation writes a fixed-slot create-new envelope, flushes it and synchronizes its parent directory before returning success. Fresh install context and wrapping-key creation are explicit operations under exclusive custody ownership, not recovery fallback. A short/crashed create leaves a partial artifact; it is never overwritten or automatically deleted. No “transaction succeeded” marker can substitute for reading/authenticating the real object. Whole-bundle CA storage avoids a separate cert/key publication pair. Opening existing state performs no create, repair, migration or key generation. Existing destination, unknown file, unexpected staging remnant, lock conflict, malformed key or unsupported context is a refusal.

An operation failure retains first static error and marks that retained creation attempt spent before any create-capable effect; no retry/reset API. On restart, incomplete installation or CA/member blobs refuse and remain for disposition. Complete envelopes reload through the actual authentication/key/cert checks. Missing wrap key alongside envelopes refuses rather than generating a new one. Loss/corruption may make recovery impossible; restoring/removing owner data or backups is not an agent operation. No auto-delete even of an apparent orphan. Errors expose closed codes (Unqualified, Layout, Ownership, Context, Corrupt, KeyMismatch, Clock, CaExpiring, Unavailable), never OS error strings, private paths or plaintext bytes.

This is custody restart behavior only. Committed-Found/no-published-leaf remains D2 and cannot be “recovered” by decrypting a key. A loaded CA does not establish node1/first_free2, fresh genesis, current voter eligibility, a committed grant, cleanup certainty or authority to call found again.

## 6. Builder fence and exclusions

One-concern PR under the exact builder fence in section 9. Custody/member helpers stay in the new custody module; existing CSR/verify implementations remain unchanged and are called for real validation. Minimal explicit ring dependency and lock delta only if required. Coordinator adopted the focused hosted harness scope; its exact launcher/image/resource/phase/cleanup/CI constants must still be pinned in the bounded brief **before build**. Measure resulting cost before claiming CI fit; no general provisioning system. Coordinator records adopted ADR, PROGRESS provider/effort/outcome, actual checks and remaining gaps. This read-only task changes none of those repository files.

Do not edit Engine/SDK/Owners, consensus start/shutdown/adapters, founder or issue authority, clocks/founder wrapper, store binding, startup_plan/recovery, join_receipt formats, peer/preproof verifiers, transport/network, enrollment wire, config/main/services, executor or public MCP. In particular no raw LeafSubject implementation, loosened registry, fake publisher, fabricated GatedFound, dormant supervisor reducer or live startup wiring. Existing `issue::test_ticket` can supply **test-only synthetic CA-sign behavior**, not committed-Found evidence. Production key construction does not grant production lifecycle activation.

Respect D6: Engine Sol **3/3 exhausted**, historical Opus **0/2 unchanged**. No Engine runtime, founder fix/diagnostic/retry, timing/deadline change, test grouping/quarantine expansion or retry-to-green here. Custody cannot close the founder bridge review's SP2 retained-operational-owner/publication gap; SP1's actual no-sign oracle is already present in pinned ca.rs. ST1's speculative lifecycle implementation remains excluded. Native account/executor/VM setup, CA transfer, renewal and the post-v1 secrets vault stay outside this slice. No babysitter/schedule changes, Lab Manager or protected VM access.

## 7. Test contract: observable positives and actual refusals

All entries below are **proposed, NOT RUN**. Name each concrete test in the builder brief; assert selected nonzero counts. Direct tests use disposable synthetic keys and the actual production codec/crypto/file loader. No Engine, network cluster or long sleep is needed.

| Case | Required observable oracle |
| --- | --- |
| Fresh CA/member and restart | Create real files, close process/owners, reload in a fresh process with the same roots; original root fingerprint and member SPKI unchanged; generate/parse CSR with valid proof; actual CA signs a matching synthetic test ticket and resulting leaf verifies under original root and matching member key. No ephemeral/test CA constructor on the positive production-load path. |
| Wrong context | Change expected cluster, installation ID, CA fingerprint, kind and SPKI independently; valid envelope refuses. Swap envelopes between two actual installations/wrap keys. Editing header to match new context still fails AEAD. Counters/outputs show zero returned signer/member owner and zero sign calls. Healthy original remains usable. |
| Corruption/codec | Flip nonce/ciphertext/tag/header; truncate at every structural boundary; append bytes; unknown version/kind, oversized lengths, invalid DER/PKCS#8 and RNG failure. Real loader refuses before key release, bounded reads/allocations, bytes unchanged on load failure. |
| Mismatched keys | In a module-private fixture, AEAD-seal a syntactically valid CA payload containing certificate A/key B; actual loader refuses SPKI mismatch. Member A plus valid issued leaf B also refuses. A permanent always-fail loader cannot pass: paired matching fixtures must succeed. |
| CA policy/time | Real wrong/self-signature, non-CA, wrong usages/pathLen/algorithm, extra DER, expired/not-yet-valid roots refuse. Test full 30-day interval containment at equality and one-second outside each bound; unavailable/range/overflow time refuses. Keep existing sentinel finite/conversion no-sign tests with real positive sign controls. No sleep, lifetime shortening or founder invocation. |
| Durable partial/error state | Fail writes/flush/directory-sync at real seams; success requires completed authenticated bytes and observed durability calls. Reopen partial/missing wrap/context/key blobs refuses without rewrite or regeneration; create-new collision/second create refuses, first error retained. Test exclusive lock from another process and reacquisition only after deliberate owner drop. |
| Unsafe filesystem | Actual symlink/hardlink/reparse/nonregular/unsafe-ancestor/permission/overlap and replacement fixtures plus stable positive. Distinguish OS prevention from exercised identity-mismatch detection. Missing native fixture capability is a gap, not pass. |
| Actual build-identity denial | Hosted subprocess under distinct build UID tries open/read/write wrap key, CA/member envelopes, private roots and daemon config; attempts to inspect daemon memory/FDs/debug or signal it fail under the selected process policy. Prove it can read/write its own workspace and that the daemon can load/sign, so a broken probe is not credit. Capture errno/status only; no secret bytes. Attempt identity escalation without capabilities fails. |
| Learner inventory | Custody harness creates only member slot for two learner fixtures; neither has a CA blob nor mount/descriptor to founder private storage. Generic member load never yields ClusterCa. Do not claim actual promotion authorization/transfer coverage; that belongs to slices 11/12. |

Include a negative-control same-UID disposable fixture showing that privacy/sealing alone does not qualify isolation, or explicitly document that limit if process-test availability prevents it. Do not report same-UID refusal caused only by a fake API permission flag. Log only bounded case name, status, denial class, elapsed time and image/source hash; no keys, envelope contents, cert dumps, join secrets or owner data.

Hosted process qualification needs its own exact image digest, observed daemon/build identity relation, bounded harness lifecycle, mounts/capabilities/no-new-privileges facts and cleanup report. No daemon privilege escalation to run probes. If runner launch under distinct identities is unavailable, report custody qualification blocked; a mocked UID check is not a substitute. Full A-to-B application execution must repeat the key-denial premise under its **actual** child identity later.

CI: short pure/crypto/filesystem cases join existing normal platform checks; Linux-only process-isolation harness gets hosted Ubuntu credit only. No Docker dependency in Windows/macOS Rust tests. Prefer one compiled helper reused for subprocess cases; build exact image/binary once. Avoid thousands of expensive key generations or a new heavyweight dependency tree. Measure compilation and test time, not just case duration. No 900-second expiry wait or full three-container acceptance journey in this PR.

Windows Rust once took 10m36s of its unchanged 12-minute timeout (84s margin); later recorded runs varied. No claim that this proposal fits before measurement. Keep required CI, ci-time-budget, agent-review, ADR0031's exact job layout/900 charged-second rules and existing stricter feature limits. If new cost threatens margin, narrow/split the custody harness; do not extend timeouts, skip meaningful refusals, change founder scheduling or rerun a founder red. A normal required CI run may execute existing Engine tests; it is not permission for local Engine experiments or repair. Preserve any first failure for independent disposition.

Before future build/test commands verify TMP/TEMP, CARGO_TARGET_DIR, scratch/observer roots and tool caches on task-owned D:, pinned tool/dependency provenance and C: free >=20,000,000,000 bytes. This design task runs no checks beyond document/path/hash inspection. Reviewer must be a separate Sol 6.1 agent; coordinator alone merges after exact-head independent review and green normal gates.

## 8. Coordinator decisions (adopted)

1. **D1 — DECIDED:** hosted per-node daemon-only file wrapping plus authenticated envelopes is the first backend. Native remains unsupported until qualified. Record ADR0037's reconciliation with ADR0005/0009/0029; post-v1 vault scope stays unchanged.
2. **Lifetime — DECIDED:** fixed 365-day CA, unchanged 30-day leaves, refusal when the whole leaf interval no longer fits, no renewal in this slice; finite issuance availability is an accepted limitation.
3. **Hosted evidence — DECIDED:** minimal actual distinct-UID hosted Ubuntu harness is slice-3 evidence. Exact launcher/image/resource/phase/cleanup bounds and CI placement are pinned before build. No local PC provisioning or Windows container work.
4. **D2 / slice 4 — DECIDED requirement, disposition still due:** slice 4 settles the fail-closed committed-Found/no-published-credentials disposition and actual owner-bound publication **before any startup claim**. Custody does not settle restartable founding or receipt provenance; no D6 crossing is authorized.
5. **D4 / slice 14 — DECIDED:** actual remote execution retains daemon/build separation. An executor unable to meet it is an integration blocker for a separate reviewed slice; it cannot run arbitrary builds as daemon or weaken D1.
6. **Owner native route — NOT REQUESTED:** no native setup work or owner question now. Preserve Windows current-user default, shared-account facts, optional dedicated selection and D7 gates; no automatic dedicated-account mandate or owner administration.

These are bounded adoption/integration decisions, not invitations to redesign TLS, rcgen, Raft or the joining journey. No external interview is required to finish this proposal.

## 9. Builder-ready brief

Coordinator split: slice 3a excludes hosted_probe.rs, tools/ci/custody/* and all workflow edits. Slice 3b adds those hosted qualification pieces later. PROGRESS is coordinator-owned.

**Build:** `feat(pki): seal and reload acceptance cluster credentials`. Builder Sol6.1/high; separate independent Sol reviewer; coordinator-only merge. Re-pin intervening main changes before assignment. This contract is the settled custody specification; ordinary exact-source review and green checks remain required.

**Exact source fence (repository-relative):**

| File | Allowed change |
| --- | --- |
| `crates/arxd/src/cluster/pki.rs` | Private custody module declaration and accurate module description only |
| `crates/arxd/src/cluster/pki/ca.rs` | Narrow private production CA generation/loading from actual authenticated bytes; store actual notBefore/notAfter; sign-time full-interval containment and direct tests; preserve ticket authority and existing sentinel tests |
| NEW `crates/arxd/src/cluster/pki/custody.rs` | Concrete secret owners, closed contexts/errors, member generation/CSR/matching-leaf checks; hosted Linux file backend, explicit unsupported-native refusal; no public API or lifecycle framework |
| NEW `crates/arxd/src/cluster/pki/custody/codec.rs` | Sections3.1/3.2 exact framing and AEAD; private functions, bounded parsing |
| NEW `crates/arxd/src/cluster/pki/custody/tests.rs` | Literal fixtures plus meaningful generation/crypto/context/file/error/restart/unsupported-platform tests |
| NEW `crates/arxd/src/cluster/pki/custody/hosted_probe.rs` | `cfg(test)` Linux-only fixed process probe, using the real custody implementation, no production command/entry point; bounded inputs and scalar output |
| `crates/arxd/Cargo.toml`, `Cargo.lock` | Explicit exact ring0.17.14 dependency if needed; no dependency upgrade or unrelated lock change |
| NEW `tools/ci/custody/Containerfile`, `tools/ci/custody/run.py`, `tools/ci/custody/test_run.py` | Minimal synthetic hosted image/launcher and pure launcher refusal/cleanup tests; reuse the compiled arxd test executable's exact named probe for fresh-process key operations; no general command broker |
| `.github/workflows/ci.yml` | Once pre-build harness pins are recorded: one Linux-only custody step in existing Rust worker using already compiled helper; no new job/topology, gates, timeouts, scheduling/quarantine or existing-step changes |
| `docs/adr/0037-acceptance-custody.md`, `docs/briefs/m3-acceptance-custody.md`, `docs/PROGRESS.md` | Coordinator adopts draft ADR and normative contract/brief, records actual tests/cost/provider/effort/outcome and remaining gaps |

Everything else is excluded, including existing `csr.rs`, `verify.rs`, `preproof.rs`, `join_receipt.rs`, startup/consensus/Engine/founder/issue/executor paths. Use their existing private interfaces; inability to do so is a source-fence blocker, not permission to widen visibility/authority. New secret owners may use only the narrow access needed inside PKI; no generic export/decrypt API.

**Persistent inventories:** before any mutation, validate both roots, their disjoint identity/privacy and all entries under a retained exclusive `custody-lock`. Bound wrap-root enumeration to at most3 entries to detect more than its exact2; custody-root to at most4 to detect more than its max3. Fresh initialization alone may create the initially absent lock using create-new after checked-empty-root validation, then acquire it before other creates. Existing operation requires that zero-length lock already exists and acquires it without CREATE. No deletion/repair on lock failure.

| Observed inventory | Allowed interpretation / operation |
| --- | --- |
| Both roots empty | Explicit create-fresh-installation only; never open-existing or implicit regeneration |
| Wrap exactly `{install-context,seal-key}`, custody exactly `{custody-lock}` | Complete installation; explicit preassignment member creation, or private fresh-founder CA creation before member creation; no Found authority inferred |
| Same wrap, custody `{custody-lock,member-key}` | Member load; learner fixture has no CA; founder-CA creation refuses occupied member state |
| Same wrap, custody `{custody-lock,ca-key}` | Authenticated founder/issuer fixture load and explicit matching member creation; a learner operation refuses any CA presence |
| Same wrap, custody `{custody-lock,ca-key,member-key}` | Authenticated CA/member loads under trusted expected contexts; learner operation refuses CA presence |
| Any missing/extra/partial/nonregular/oversized/unsafe file or context/key mismatch | Refuse, preserve bytes/first error; no new key, overwrite, role inference or reset |

The inventory table does not authorize later voter import or staged-member publication; slices4/12 define those effects and their owner handoff. Each existing envelope must pass its exact codec/authentication/crypto policy before inventory is called usable. No native platform is declared qualified by a successful pure codec test.

**Invariants to implement:** original DER/fingerprint and derived SPKI preserved; no key release before context/authentication/key/cert/time checks; member preseal precedes CSR release; learners lack CA bytes/mounts/handles; actual distinct build identity denied; full 30-day interval checked every sign; exclusive ownership/create-new/durable success/no regeneration; creation error spent before effects and original cause retained; exact v1 framing; no secret debug/output; no Engine/founder/live startup.

**Required test groups:** literal I/P/E/R/Z fixtures above, actual AEAD control plus field/tag/wrong-key mutations, real production CA/member fresh-process restart and leaf verification, authenticated key/cert mismatch, all expected-context changes, both CA-time bounds and sentinel controls, partial-create/write/sync failures with unchanged recovery bytes, actual lock/unsafe-file/replacement/permission fixtures, unsupported-native refusal, actual hosted build-UID file/process/FD denial with workspace/daemon positive controls, learner CA absence and same-UID limitation. Proposed test names/counts and exact selections must be supplied before execution; no compile-only/zero-selected credit. No Rust behavior test has run for this revision.

**Pre-build hosted pins:** coordinator brief must contain immutable base-image digest, exact source hash/build target, reviewed Containerfile/build inputs; explicit daemon/build UIDs/groups, launcher argv and exact libtest probe name; no inherited key FD or environment; mount/capability/no-new-privileges/dump/network policy; CPU/RAM/PID/disk/log caps; create/probe/restart/cleanup phase and total deadlines; ownership-checked disposable cleanup with first-failure preservation; Linux-only CI insertion point. Record resulting test-binary and built-image digests after the single build and before execution; those output hashes cannot be invented before compilation. This file deliberately invents no image digest or measured runtime. Until the pre-build pins are present, implementation of pure codec/key components may proceed under its source assignment, but image builds and harness execution remain unqualified and must not start. No owner question or native provisioning is needed for these coordinator pins.

**CI measurement:** measure the normal exact-head first CI's full wall/charged time, every Rust worker and added compile/helper/image/probe cost, selected counts and first failure; compare with unchanged12-minute workers and900 charged-second budget. The known Windows10m36s leaves only84s margin, with variability; measure direct-dependency and all-target test-compilation cost even though Docker/probes run Linux-only. No fit claim without those observations. If margin fails, narrow/split under coordinator disposition; preserve normal gates and first failures, never raise timeout, change founder scheduling or retry-to-green. Ordinary CI's existing Engine tests grant no local Engine experiment or exhausted-fix budget.

## Read and execution record

**Revision after independent review:** verified review SHA256 `4a3ac97112c12cdc019ca2cffb4b299ff259a04c009b90a9071c71dedb5f7cfe`, GO with required SP1 changes. Added normative byte tables, independent literal structural fixtures/malformed recipes, exact file inventories, decided section8 and bounded builder brief; wrote the requested ADR0037 draft. Coordinator decisions1–5 are adopted; native item6 is not requested. Repository rereads in this revision used only `git show fc83838d9eda05ab5e738cae8f8b7d3b53f3c606:<path>`, never working-tree files or branch commands. Both requested scratch files are the entire revision write scope. Every revision shell command explicitly sets TMP/TEMP/scratch/target/observer/cache paths under this task root. Mechanical Markdown/hex-length/hash inspection is document validation only; no build, Rust test, crypto/runtime check, CI observation, container or delegation occurred. This self-revision has not been independently re-reviewed. The original design execution record follows for provenance and is not a new acceptance claim.

Read pinned AGENTS, KICKOFF, dev-scratch, v1-readiness, top current-state PROGRESS, acceptance brief; all ADR blobs were searched for custody/identity policy and decisive ADR0005/0006/0009/0029/0030/0031/0033/0034/0035 content inspected. Read ca/verify/preproof/CSR, join_receipt production validation, startup_plan production ownership/layout, pki declarations and dependency pins. Read requested plan items and D1/D2/D6/D7, founder contract and independent review. Some combined outputs truncated; decisive production/policy ranges were reread. This is not an exhaustive semantic audit of all historical ADR/test bodies or fresh main CI health.

Input SHA256s: plan `4111c9caf2e80b219a34e4ce4144effc9f8c3164b263fa97148a91858edfca1a`; founder contract `99254b61d6528d7305377c29e74f95c7f639f8ba4a123a8998f43361757dbf3a`; founder contract review `09aa49f15668c7bd96a2524f9350233b94df838a390075aebe46cb7e01031c6b`.

Only scratch directory creation and this contract write are mutations. Shared checkout was read through immutable git show/ls-tree only; no checkout/branch/index edits, commits, builds, tests, pushes, daemons, containers, external requests or delegation. No secrets/private owner data, protected resources or owner admin actions. No source/runtime pass claimed. PROGRESS's current-state main label is historical relative to supplied fc83838d baseline; do not infer fresh hosted health from it.

Every shell command set requested TMP=TEMP=`D:/Temp/arx-m3-custody-d1/tmp` and ARX_SCRATCH_ROOT=`D:/Temp/arx-m3-custody-d1`. Initial two read-only batches omitted explicit target/observer/cache paths; subsequent commands set CARGO_TARGET_DIR=`.../target`, ARX_OBSERVER_ROOT=`.../observer`, CARGO_HOME=`.../cargo-home`; no build/test/cache tool ran. Effective paths and non-reparse scratch root/tmp were inspected; C: free observed 687629557760 bytes. Only this D: root is writable. M2 OPEN / M3 UNCHECKED; **unverified on real hardware**. Existing exhausted counters and stop rules unchanged.
