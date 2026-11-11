# ADR 0015 independent contract review

Reviewer: OpenAI / gpt-6.1-sol / medium, 2026-11-11.
Pinned architecture range: 6dda6e342766ae89ae5473f35c6e2aa4cb73ede0 through
26364b98c2ab4c1391ffe120397687827ee22cfc. Complete ADR, brief and three scoped
amendments inspected. Later PR31 image metadata was integrated separately.

## Standards

No blocking findings. The change is bounded, the new rule is Mac-only and only
for pass-through directories, and all leaf/Linux/Windows rules remain strict.
The brief supplies explicit positive/negative controls and mutations. UTF-8,
Markdown structure and whitespace checks passed. No Rust/runtime suite ran.

## Spec

No blocking findings. UID0 ownership, no group/other write, no special bits and
an empty known ACL exclude non-root namespace writers without trusting a group.
The old writable runtime parent remains refused. The new fixed endpoint shares
only traversable parents and leaves config/state locations unchanged. Existing
objects are never repaired, no fallback address is introduced, and credentials
still follow the kernel-root identity check. Stale sockets and unobserved stock
paths remain explicit acceptance gaps rather than claimed working paths.

Minor wording corrections applied during integration: socket ownership remains
the configured client UID (the new root-ownership rule concerns regular files
and directories), the image advisory is already resolved by PR31, and the
compatibility note claims no recorded acceptance rather than asserting nobody
could ever have run a macOS service.

Primary-source checks: Open Group pages returned403, so full Apple XNU sources
were used instead of relying on search excerpts:

- [mkdir](https://raw.githubusercontent.com/apple-oss-distributions/xnu/main/bsd/man/man2/mkdir.2): parent-group inheritance and umask restriction.
- [rename](https://raw.githubusercontent.com/apple-oss-distributions/xnu/main/bsd/man/man2/rename.2): directory-write/search requirements and additional Darwin directory/sticky restrictions. The new rule conservatively excludes writable parents; it does not claim every protected directory could be renamed under every filesystem.
- [chmod](https://raw.githubusercontent.com/apple-oss-distributions/xnu/main/bsd/man/man2/chmod.2) and [chown](https://raw.githubusercontent.com/apple-oss-distributions/xnu/main/bsd/man/man2/chown.2): ownership restrictions on changing mode/owner.
- [Unix socket header](https://raw.githubusercontent.com/apple-oss-distributions/xnu/main/bsd/sys/un.h):104-byte sun_path storage. The proposed address is50 ASCII bytes and stays below the current100-byte client limit.

Accepted for implementation, subject to the required hosted positive controls.
Any rejected unobserved stock path returns to architecture before a policy is
relaxed. No installed-service, cross-user or M2 acceptance is established;
those paths remain unverified on real hardware.

Standards:0 blocking findings. Spec:0 blocking findings; native evidence remains
an explicit implementation/acceptance prerequisite.
