# 0006: Separate service authority from build execution

Status: proposed from the Opus 5.5 high single-host security design. No service,
account switching or MCP acceptance has been implemented. Platform behavior is
**unverified on real hardware**. This records preparation while setup acceptance
finishes; it does not authorize agents to perform owner administration.

## Required boundary

Arbitrary agent build commands must run under the dedicated Arxcovo build
account, never under the daemon or the owner's login. The service's identity,
configuration, agent-token hashes, activity database and future CA keys need an
OS access boundary from that account. Encrypting keys under the same account
that runs arbitrary commands does not provide that boundary.

The owner's standard/admin setting controls build rights. Agents cannot change
that setting, account membership, service configuration or enrollment authority.
A missing account, changed pinned SID/UID, unavailable execution mechanism or
rights mismatch must refuse execution without falling back to another identity.
Admin build rights inherently weaken protection of local service secrets; the
cluster design must account for that rather than claim encryption solves it.

The advisory proposes a system service plus a separate standard build identity.
This requires a security review of the privileged surface and a working platform
probe before adoption. Prefer a narrow privilege broker where practical; never
read or modify attacker-controlled workspace paths using service privileges.

## Proposed interfaces

- Owner-only configuration names workspace roots, the pinned build identity,
  rights, limits and tool-search environment. Inventory reports observed facts.
- A local stdio MCP bridge connects to the service and verifies its OS identity
  before sending credentials. It never opens private state or executes builds.
- Streamable HTTP authenticates a separate agent principal and enforces bounded
  bodies, Host/Origin checks and loopback binding until protected remote transport
  exists. Agent tokens never authorize owner configuration.
- MCP exposes host inventory, bounded arbitrary command execution and the
  caller's activity. Executable arguments are passed directly; a shell is used
  only when explicitly requested as the executable. Tool discovery uses the
  build identity too.
- Durable activity starts before process creation; a storage failure prevents
  execution. Restart marks unfinished runs interrupted. Output is bounded and
  treated as untrusted; credentials and environment values are excluded from
  activity metadata.

## Platform probes before implementation acceptance

Windows: verify the proposed LocalSystem/S4U batch-logon mechanism for an
owner-created local standard account, actual child SID/integrity, required
privileges, environment/toolchain behavior, handle inheritance and Job-object
tree cancellation. S4U feasibility and private desktop requirements remain
unproven. Password-based execution would require a separate documented choice
and secure owner credential input, never a chat message or log.

Linux: verify identity switching, no-new-privileges, cgroup delegation and whole
tree cancellation on supported kernels. macOS needs its own verified privilege
and process-lifecycle implementation; Linux-only facilities cannot be assumed.
System service key storage must fit the chosen platform identity and the settled
OS-keychain design; a root-only file is an unapproved deviation until justified.

All platforms: test workspace traversal/reparse rejection, impersonated local
service endpoints, missing/revoked agent tokens, output floods, escaped
grandchildren, timeout/cancel/restart, denied access to service state and failure
to persist activity. A stub runner cannot prove account isolation.

Final milestone acceptance remains Claude Code using the real MCP transport to
list the host and run `cargo --version` as the dedicated account, with the
authenticated agent, observed execution identity and outcome in persisted
activity. Owner account/service setup is required on the real development PC;
disposable hosted-runner tests can prove software behavior separately.

## SDK feasibility check

On 2026-11-11, the official rmcp documentation and `cargo info rmcp` agree on
version 3.9.0 (Rust minimum 1.88, compatible with this repository's pinned 1.95).
The crate exposes `transport-io`, `transport-async-rw` and
`transport-streamable-http-server`; the generic transport supports an
AsyncRead/AsyncWrite pair. This resolves the advisory's uncertainty about the
generic transport feature and outdated 3.7 candidate. It does not pin a workspace
dependency or prove client interoperability: the first implementation must test
a real initialize/tools-list exchange and authentication through both transports.

Source: [official rmcp crate documentation](https://docs.rs/rmcp/3.9.0/rmcp/).
