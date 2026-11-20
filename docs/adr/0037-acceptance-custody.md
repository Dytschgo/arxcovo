# ADR 0037: Qualified acceptance-platform CA and member-key custody

Date: 2026-11-19.
Status: **adopted decision; implementation and runtime qualification pending**. Coordinator has adopted D1 and contract section8 decisions1–5; independent design review was GO with required change SP1. The attached revised contract supplies the persistent byte definition and independent structural fixtures. Independent Sol design re-review is GO (D:/Temp/arx-m3-custody-d1-review2/review.md); implementation review and runtime qualification remain pending.

Source baseline: `fc83838d9eda05ab5e738cae8f8b7d3b53f3c606`, read only through immutable Git objects. Author: OpenAI GPT-6.1 Sol / high, design sub-agent. ADR0037 is the next number supplied by coordinator, not inferred from another builder's moving checkout.

## Context

M3 requires a real cluster CA and member-key custody before production founding/joining can be composed. Current `ClusterCa` owns `rcgen::Issuer<'static, KeyPair>` and original CA DER, but its constructors are test-only. Existing `found(&ClusterCa, ...)` requires an actual owned signing key in the daemon. A remote non-exporting signer would require a separate frozen-API change. Public receipt consistency and an opaque artifact filename do not prove key possession, issuance provenance or isolation.

ADR0005 requires CA sealing under daemon identity inaccessible to arbitrary build commands, permits CA custody only on eligible voters, and forbids learners holding it. ADR0009 explicitly reserved M3 storage for a separate decision and refused an assumed root/SYSTEM-only file substitute; ADR0006 is historical corroboration of that concern. ADR0029 later selected Windows current-user execution as default, with honest shared-account access and optional dedicated execution. The settled post-v1 secrets vault/OS-keychain design must not become a v1 dependency.

The acceptance platform is three hosted Ubuntu containers, allowed by the kickoff. This ADR chooses the smallest concrete backend compatible with locally owned rcgen keys. It does not claim native clustered execution or a complete three-container acceptance path.

## Decision

Each node uses a **non-root daemon identity distinct from its standard arbitrary-build identity**, qualified through actual permissions, process, descriptor and mount-denial probes. No shared private-root group, capabilities enabling identity change/DAC bypass/debug, inherited secret handles, Docker/host-control socket, owner mounts or ability to alter daemon image/configuration. No-new-privileges applies; core dumps are disabled. Daemon code and loaders do not use untrusted workspace/tool paths or build-supplied environment.

The daemon generates a random 256-bit wrapping key in a separate daemon-only persistent root and AEAD-seals new P-256 CA/member PKCS#8 bundles. The algorithm is ChaCha20-Poly1305 through the already pinned ring0.17.14, with a minimal explicit dependency if required. Materialize the authenticated/validated keys locally as owned rcgen/rustls signing material inside that boundary. No generic vault, remote signer, raw unseal/export API or CA transfer endpoint is added.

AEAD binds exact version/kind, installation ID, cluster, original CA fingerprint, subject SPKI, nonce and length. Decrypt only under separately trusted expected context. Compare actual derived SPKI with key/certificate identity before returning a usable owner. Persist the original CA certificate DER; never regenerate a self-signed certificate during load. Validate its actual self-signature, algorithm, constraints/usages, exact bounds and fingerprint. Existing committed bindings, sealed issuance subjects and fresh guards retain authority; possessing a CA/member key does not create authority.

Each node has its own wrapping key/installation ID and private volume. Learners receive **neither plaintext nor encrypted CA**, nor mounts/descriptors reaching founder/voter storage. Fresh private founder CA generation is distinct from future membership-qualified voter transfer. Slice12 must qualify transfer outside Raft after real promotion; this ADR implements no CA distribution or voter-eligibility token.

## Persistent format and failure rules

The normative attached brief is the revised custody contract, sections3.1–3.2 and9. For repository adoption, retain it as `docs/briefs/m3-acceptance-custody.md` and this ADR as `docs/adr/0037-acceptance-custody.md` in the same change. Before that adoption the exact attachment is [contract.md](D:/Temp/arx-m3-custody-d1/contract.md). Keep one normative byte definition: builder tests reproduce the attachment's literal hex fixtures, independently of the production encoder.

The chosen wire-independent on-disk layout is summarized here for reviewers:

- `install-context`: magic hex `4152584354583031` (`ARXCTX01`), version byte01, raw16-byte random installation ID, EOF at25. No BOM/newline/text encoding.
- `seal-key`: exactly32 raw random bytes, no wrapper. `custody-lock`: exactly zero bytes plus an actual exclusive OS lock, not a file-content ownership marker.
- CA/member envelope: magic hex `4152584b45593031` (`ARXKEY01`), version01, kind01=CA or02=member, installation16/cluster16/CA-digest32/SPKI-digest32 bytes, nonce12, ciphertext length `u32be`. Fixed header is122 bytes. AAD is exactly those122 persisted bytes. Ciphertext follows, then tag16, then mandatory EOF; total=`138+L`. `L` excludes the tag and equals plaintext size.
- Decrypted member payload is `u32be K || PKCS#8[K]`, `K=1..4096`, `L=4+K` (5..4100). CA adds `u32be C || originalDER[C]`, `C=1..4096`, `L=8+K+C` (10..8200). File cap16384, kind-specific bounds before decryption, exact inner/outer lengths, no padding/trailing bytes or alternate kind/version/algorithm.

The attachment includes literal structural positives I1/P1/P2/E1/E2/R1/Z1 and exact malformed recipes for magic/version/kind/endian/length/BOM/truncation/trailing/oversize errors. Its public sentinel payloads/tags **must not successfully authenticate/load**. Structural parser/encoder acceptance is separate from real runtime-generated AEAD/key/fresh-process positives. No private key is retained in fixture evidence.

Roots are disjoint from state and workspaces. Wrapping/context root, issuer/staging custody root, and later credentials root have separate purposes. Preserve startup_plan's fresh-empty and exactly-three-file recovery rules: slice3 creates no credentials record and adds no CA file there. Slice4 supplies actual owner-bound publication of the member artifact and original Found/Redeem provenance, before live startup claims.

Inspect actual opened objects, ownership/privacy/link count/type and ancestor/path safety; reject unsafe aliases, symlinks/reparse points, hardlinks, alternate streams and unsupported filesystem/durability semantics. Retain exclusive custody ownership and checked objects. Creation is create-new, with flush and parent-directory durability before success. Lock acquisition and exact inventories follow contract section9; unknown/partial layouts refuse unchanged. Existing opens have no create/repair/migrate/delete behavior. Record first error and spend the creation attempt before effects. Complete restart reloads the same key; missing/corrupt wrapping or sealed material never causes regeneration or overwrite.

## Reconciliation and security limits

This ADR explicitly authorizes **this qualified hosted non-root file-envelope backend** as the first M3 backend, satisfying ADR0005 through actual daemon/build separation. It resolves ADR0009's deferred CA storage decision narrowly rather than assuming a root/SYSTEM-only keyfile is safe. It neither implements nor pulls forward the post-v1 secrets vault/keychain system.

ADR0029 is unchanged: Windows current-user remains default and shares that account's access. Same-UID private files, user-scope DPAPI and machine-scope DPAPI alone do not isolate daemon secrets from arbitrary builds under that protection context. No automatic dedicated-account mandate, fallback identity, S4U work or native setup follows. Native Linux/macOS/Windows composition is unsupported until independently qualified through owner-side setup; item6/native route was not requested. Existing Linux/macOS account policies remain unchanged.

The trusted base includes daemon/kernel/container runtime/host runner, pinned image/configuration, RNG and crypto. Privileged/root/admin attacker, compromised daemon or runner and theft of both wrapping key and ciphertext are outside this backend's guarantee. Envelopes do not establish anti-rollback, hardware binding, complete memory zeroization, disk encryption or secure deletion. Same-UID code can read/decrypt/debug; report that limit honestly. Keep temporary private copies short-lived, secret owners non-Debug/non-Serialize/non-Clone, and logs/argv/environment/MCP/image layers free of keys. Rustls/rcgen necessarily retain private signing material while in use.

## Adopted lifetime and availability limitation

New production CA: creation observation `t=floor(checked_ms/1000)`; notBefore=`t-120`, notAfter=`t+31536000` (365 days), checked arithmetic and supported-date conversion. Leaves retain existing policy: notBefore=`floor(now_ms/1000)-60`, notAfter=notBefore+2592000 (30 days). Both leaf interval ends must fit actual original CA bounds at **every sign**, and a full leaf interval must fit on issuer load. A load-time observation is not authority for all subsequent signs. Keep actual key binding, sealed ticket/guards, sentinel no-sign/conversion tests and positive controls.

Use ADR0030's fallible validated clock/range and conditional trusted-host-UTC model, with actual recovered floor when available; no fabricated floor, cached signing time or founder-interface change. TLS member loading still checks actual time/signature/identity through existing verification. Code TTL900000/skew30000 are unchanged.

No renewal, rotation, automatic CA extension, shorter replacement leaf or key replacement is included. Refuse `CaExpiring` when a whole unchanged leaf no longer fits. Existing still-valid leaves can authenticate without CA private-key possession until their own validity ends. Eventually issuance and then authentication become unavailable without separate maintenance work: **finite availability is an adopted limitation**, not a completed renewal design or a fallback. Controlled-time tests require no sleep and do not prove real fifteen-minute expiry acceptance.

## Hosted harness and CI

Slice 3b implements the coordinator-adopted 2026-11-20 hosted pins (SHA256
`28f882cb018eb956c60c190f8718fb982923cd096adb608e15e194c51fa2e167`,
design baseline `85eccce8a917446efc1ea9df6d9bd4b0c256c128`). Hosted
execution and exact-head qualification remain pending; native and multi-host
paths remain **unverified on real hardware**, and process restart does not
qualify crash durability. The coordinator retains the full adopted revision.

Implementation is split: slice 3a adds codec, CA/member owners and file-backend primitives; slice 3b will add the hosted distinct-UID harness and CI step. Slice 3a alone makes no custody-isolation qualification claim.

Implementation is split: slice 3a adds codec, CA/member owners and file-backend primitives; slice 3b will add the hosted distinct-UID harness and CI step. Slice 3a alone makes no custody-isolation qualification claim.

Coordinator adopted a minimal distinct-UID **hosted Ubuntu custody harness** as direct slice3 evidence. Use real production codec/key/file operations in a fixed `cfg(test)` probe, separate process restart and actual build-UID denial, with successful daemon-load/sign and workspace controls. Learner probes inspect real mounts/descriptors/CA absence. Docker exec under explicit UID may drive custody probes; it does not prove production A-to-B execution. Slice14/D4 must preserve and repeat the isolation premise under the actual executor.

Before any harness/image build, the coordinator's bounded brief pins immutable base-image digest, source hash/build target and reviewed Containerfile/build inputs, launcher argv and fixed probe name, daemon/build UIDs/groups, mounts/caps/no-new-privileges, network/dump policy, CPU/RAM/PID/disk/log limits, phase/total deadlines, ownership-safe disposable cleanup, failure-preservation policy and Linux-only CI insertion point. Measure resulting binary/image digests after the single build and before execution; output hashes cannot be known before compilation. No invented digest or duration is accepted as a pin. Missing capability/pin blocks that harness, not an owner interview or silent same-UID fallback.

Measure new compile/dependency/helper/image/probe cost and exact-head first normal CI wall/charged/worker time; report actual selected test counts and first failures. Windows Rust has reached10m36s of its unchanged12-minute timeout, only84s margin. Reuse one compiled helper; no Docker in Windows/macOS Rust tests, new job topology or long expiry wait. Preserve ADR0031 budget semantics, required checks and ci-time-budget/agent-review. Narrow/split if cost threatens margin; no timeout increase, quarantine/scheduling change or retry-to-green.

## Exclusions and remaining prerequisites

No Engine/SDK/Owners, consensus start/shutdown/adapters, founder/issue authority, live clocks/startup/config/main/service/executor, transport/preproof, public MCP, receipt format or storage change. No fake publication/provenance capability, speculative lifecycle reducer or raw LeafSubject implementer. Test-only synthetic tickets exercise actual CA behavior, not committed Found.

D6 stays fixed: Engine Sol **3/3 exhausted**, historical Opus **0/2 unchanged**. No founder diagnostic/fix/retry, timing/deadline change or new repair allowance. Existing normal hosted CI may run existing Engine cases; it grants no local experiment or repair authority.

D2's fail-closed committed-Found/no-published-credentials disposition and real owner-bound publisher must be settled in slice4 before any startup claim; custody alone closes neither. Real promotion/CA transfer, A-to-B authorization/executor, clock qualification and isolated three-container survival/reuse/expiry acceptance remain separate slices. No native setup, owner administration, protected Lab Manager/VM access, secrets-vault implementation or babysitter change.

Coordinator records adoption and per-task provider/effort/outcome, actual tests/costs and gaps in PROGRESS. Independent exact-head Sol review and green required checks precede coordinator-only merge. M2 OPEN / M3 UNCHECKED; native and multi-host paths **unverified on real hardware**.

## Design evidence (historical draft record)

Independent review input: `D:/Temp/arx-m3-custody-d1-review/review.md`, verified SHA256 `4a3ac97112c12cdc019ca2cffb4b299ff259a04c009b90a9071c71dedb5f7cfe`, GO with required SP1 format changes. Coordinator decisions1–5 were supplied explicitly in the revision request. This draft and attached revision supply those changes; no independent re-review or behavior/CI pass is claimed.

Read-only design revision only. Writes are limited to the two requested scratch Markdown files; no repository mutation, checkout/branch change, commit, build, test, push, container, daemon, external request or delegation ran. The historical original design record remains in the contract. Every revision shell command sets task-owned D: TMP/TEMP/scratch/target/observer/cache paths explicitly. Document byte-layout inspection and SHA256 checks are artifact validation, not implementation tests.
