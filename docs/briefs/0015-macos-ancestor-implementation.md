# ADR 0015 implementation brief (Sol)

Contract: [ADR 0015](../adr/0015-macos-stock-ancestors-and-runtime-placement.md).
Builder: Sol. A fresh independent review of ADR 0015 must come before this
work starts. Every installed path stays **unverified on real hardware**.

Base: the accepted ADR 0014 lifecycle line (`3ced94d7` or its merged
successor). `owner_unix.rs` and `Application` do not exist at `6dda6e34`.
Do not port them from another working tree. `ensure_endpoint_directory` is
specified by ADR 0014 but has no code at `3ced94d7`. Whichever PR implements
it uses the ADR 0015 parent. Do not implement it twice.

Change `layout.rs` (macOS endpoint constant, `check_unix_ancestor`),
`native_unix.rs` (the ancestor loop), `owner_unix.rs` (`Fs::parent` uses the
ancestor rule; leaf checks keep exact GID; Decision 2 `fchown` on create), and
the retained hosted fixture. Change no Windows or Linux policy, and add no CLI, setting or selector.

Required tests (ordinary user; the portable ones run on every OS):

1. **Pure policy, macOS ancestor.**
   - Accept: `{0, 0, 0755}`, `{0, 80, 0755}`, `{0, 1, 0711}`.
   - Refuse:
     - UID ≠ 0: 501 or 1, with any GID;
     - any `0o020` or `0o002` bit: `0775`, `0757`, `0777`;
     - `0o1755`, `0o2755`, `0o4755`;
     - `Protection::Windows`.
   - Linux ancestor: `{0, 1, 0755}` refuses. Leaf `BinaryIntegrity`,
     `PrivateDirectory`, `PrivateFile` and `EndpointDirectory` with GID 80
     refuse on macOS.
   - The old `runtime_parent` shape `{0, 1, 0775}` refuses.
   - `acl_empty` and `acl_present` keep refusing a nonempty or unknown ACL.
2. **Owned temp fixtures (macOS and Linux, `Fs` root seam).**
   - A `0755` traversal directory chgrp'd to another of the test user's own
     groups:
     - macOS: accepted as an ancestor, refused as a final `0700`/`0755`
       object;
     - Linux: refused.
   - These traversal directories refuse: `0775`, `0757`, a symlink, a
     `chmod +a` ACL (macOS), and a regular file in place of a directory.
   - A created child under a foreign-group parent ends with the expected GID
     and mode, with the same inode on rerun. An existing wrong child is
     unchanged and refused.
3. **Hosted macOS read-only positive controls.**
   - These must pass Decision 1: `/`, `/Library`,
     `/Library/Application Support`, `/private`, `/private/var`,
     `/Library/Logs`, `/Library/LaunchDaemons`, `/bin`.
   - `/bin/launchctl` must pass leaf `BinaryIntegrity`.
   - These must stay rejected:
     - `/private/var/run` as an ancestor;
     - `/Library/Application Support` as a leaf;
     - a trusted open of `/private/var/run/arxcovo`, which must refuse with
       `Permissions` before any lookup.
   - Print one bounded row per target, with fixed labels and booleans only.
     Rename the key to `per_directory_ancestor_accepts`. Add the resolved
     image only if it is copied exactly from the job log. Update the evidence
     file only from that exact capture.
4. **Layout and client.**
   - The macOS endpoint is under `Arxcovo/run`, is not under any
     `private_dirs` entry, is shorter than 100 bytes, and is component-disjoint
     from workspaces.
   - On macOS, `validate_address` of the old path refuses.
   - The service address requires expected identity `"0"`, and `peer_cred` is
     compared before any credential is written. The existing ordering test is
     kept. The real cross-user claim stays **unverified on real hardware**.
5. **Mutations that must fail a test:**
   - accept any GID on Linux;
   - drop the group-write check on macOS;
   - apply the ancestor rule to a leaf;
   - skip `fchown` on create;
   - chown an existing object.
