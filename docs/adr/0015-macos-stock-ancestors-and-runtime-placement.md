# 0015: macOS stock ancestors and runtime endpoint placement

Status: settled contract (Opus 5.5 high, architecture), 2026-11-11, at base
`6dda6e34` (PR31). It needs a fresh independent Sol review before any
implementation. It amends ADR 0009 and ADR 0014 (accepted at `3ced94d7`, not
in this base) only where stated. Boot and lifecycle rules are otherwise
unchanged. Every installed path is **unverified on real hardware**: no native
macOS runtime was available. The hosted facts describe one runner image only.

## Context

Hosted evidence (`docs/evidence/macos-stock-directory-observations.json`, run
43041133895, job 130592903303) and review comment 6799509282 (PR31) report:

| Stock directory | Owner | Group | Mode | ACL | Current ancestor rule |
| --- | --- | --- | --- | --- | --- |
| `/Library`, `/private`, `/private/var` | UID 0 | 0 | `0755` | empty | accepts |
| `/Library/Application Support` | UID 0 | nonzero | `0755` | empty | rejects: gid only |
| `/private/var/run` | UID 0 | nonzero | `0775` | empty | rejects: gid and group write |

The reviewer resolved the image from the job log as `macos-26-arm64`
`20261102.0473.1`. The evidence file records only `macos-latest`. That advisory
stays open; this ADR does not edit the evidence file. `/`, `/bin`,
`/bin/launchctl`, `/Library/Logs` and `/Library/LaunchDaemons` are on the
layout's trusted walks but were **not observed**.

Every trusted Unix walk applies the leaf `BinaryIntegrity` rule (UID 0, GID 0,
`mode & 0o7022 == 0`, empty ACL) to every traversed directory:
`native_unix::open(.., trusted=true)` and `owner_unix::Fs::parent`. So every
macOS state, config, binary and endpoint check fails on a stock system.

Primary sources:

- POSIX `rename`, `unlink`, `mkdir`, `chmod`, `chown`
  (pubs.opengroup.org/onlinepubs/9799919799/functions/): adding, removing or
  renaming an entry needs write and search permission on the directory. Only
  the file owner or a privileged process may change the mode. Changing
  ownership needs privilege.
- XNU `mkdir(2)` (opensource.apple.com/source/xnu/xnu-6153.61.1/bsd/man/man2/
  mkdir.2.auto.html): "The directory's group ID is set to that of the parent
  directory in which it is created." POSIX allows this. Objects that root
  creates under a nonzero-group parent therefore get that nonzero group.
- XNU `sys/un.h` and `unix(4)`: `sun_path[104]`.

I read these sources as search excerpts and mirrors, not full page captures.
The reviewer re-checks the cited lines.

## Decision 1: macOS namespace-ancestor rule (Mac only)

Add a pure `check_unix_ancestor(platform, &Protection)`. Apply it only to a
directory that a trusted walk passes **through** to reach a deeper component,
including `/`:

- **Linux:** exactly today's rule: UID 0, GID 0, `mode & 0o7022 == 0`.
- **macOS:** UID 0 and `mode & 0o7022 == 0`. The GID is **not compared**.
  The empty-ACL requirement is unchanged: on macOS, `protection()` returns a
  `Protection` only after `empty_acl` succeeds, and an unknown or nonempty ACL
  refuses first. MNT_LOCAL, `O_NOFOLLOW` and the lexical checks are unchanged.

Justification. With no group write bit and no ACL, the owning group has only
read and search on that directory. That is no more than its mode bits already
give everyone else. It cannot create, rename or unlink an entry, so it cannot
replace the protected next component. It also cannot change the mode or the
owner. The rule does not trust any group or group membership. It ignores the
GID only because, under these conditions, the GID carries no namespace
authority. Group or other write is refused with or without the sticky bit, and
so are setuid, setgid and sticky bits. An unknown or nonempty ACL is refused.
`/private/var/run` (`0775`) therefore stays refused.

Unchanged: every **final** object keeps its exact policy. That covers
`PrivateDirectory` (`0700`), `PrivateFile` (`0600`), `EndpointDirectory`
(`0755`) and leaf `BinaryIntegrity` on the binary, the binary directory and
`/bin/launchctl`. Each of these still requires root:wheel (GID 0). No caller
may check a stock shared directory as a final object. Windows (ADR 0013) and
Linux are unchanged.

## Decision 2: new objects get GID 0 on macOS

Because of BSD group inheritance (above), every directory or file that the
owner adapter or the service **newly creates** on macOS gets
`fchown(fd, 0, 0)` on its new handle before `fchmod` and before the final
check. This covers `Fs::directory`, `Fs::create`, the config temporary file and
`ensure_endpoint_directory`. Existing objects are never chowned, chmodded,
repaired or removed. A wrong existing object is still refused. Linux behaviour
is unchanged. A portable `fchown` to the expected IDs is allowed, but it is not
required there.

## Decision 3: runtime endpoint moves; config and state stay

| macOS | Before | After |
| --- | --- | --- |
| endpoint | `/private/var/run/arxcovo/arxd.sock` | `/Library/Application Support/Arxcovo/run/arxd.sock` (50 bytes) |
| binary, config, state, logs, plist | unchanged | unchanged |

- The parent `.../Arxcovo/run` is the `EndpointDirectory`: root:wheel, `0755`,
  empty ACL.
- Its ancestors are `/`, `/Library`, `/Library/Application Support` and
  `/Library/Application Support/Arxcovo`. They pass Decision 1.
  `Arxcovo` is owner-created (`install -d -o root -g wheel -m 0755`). It must
  be traversable, so the endpoint is never placed under a `0700` private
  directory. If it is `0700`, client traversal fails closed.
- 50 bytes is within the client's 100-byte `validate_address` limit and below
  `sun_path` 104.
- **ADR 0014 decision 2 is amended.** `ensure_endpoint_directory` uses
  `layout.endpoint().parent()`, derived from the generic `Layout` with no
  macOS literal. It walks to `.../Arxcovo` under Decision 1 and never creates
  that directory. It then calls `mkdirat("run", 0755)`:
  - **Created:** `fchown(0,0)`, then `fchmod(0755)`, then the check.
  - **`EEXIST`:** open no-follow and check. Never repair it.
  - **Any other failure:** exit 7.

  The start preflight still accepts a missing `run` directory. Install still
  never creates it, and uninstall never removes it.
- **ADR 0014 remaining evidence is amended.** Whether `/private/var/run` is
  cleared at boot no longer matters to Arxcovo. The stock-ancestor item is
  settled here, subject to the hosted controls below. The stale-socket item
  now also survives a reboot.
- Rejected alternatives:
  - Keep `/private/var/run`: group-writable.
  - Trust `admin` or another named group: blanket group trust.
  - `chgrp` stock folders: the design never repairs them.
  - Move config and state (for example, to `/private/var/db`): a migration,
    plus new unobserved ancestors.
  - Put `run` under `state`: `0700` is not traversable for clients.

**Consequence: stale sockets.** `/private/var/run` was said (unverified) to be
cleared at boot. `run` persists, so after a crash, SIGKILL or power loss, the
stale socket also survives a reboot. Bind keeps refusing it (ADR 0009, no
automatic reclaim) until the owner removes that one socket by hand. A clean
stop still unlinks the socket. Lock-proven stale reclaim inside `run` is an
open M2 item for a later ADR, not part of this contract.

## Compatibility and cleanup (no automatic action)

- No working macOS service existed before this ADR, because startup was
  blocked. Config and state paths are unchanged, so there is no migration.
- An old `/private/var/run/arxcovo` (from any pre-0015 build, or created by
  hand) is never read, used, repaired or deleted. Its parent stays refused. The owner may
  remove it by hand: `sudo rm` a stale socket, then `sudo rmdir` the directory.
- There is no fallback or dual path. An old client config with the old address
  is not the service address. It fails the owner-only parent check and refuses
  before connect, so the owner reruns `client-config new`. Old clients against
  a new service, and new clients against an old one, fail closed.
- No HTTP endpoint, development fallback, MCP mode override, setting or
  approval flow is added.

## Remaining evidence (all **unverified on real hardware**)

- **Hosted (implementation PR):** the unobserved stock paths named in Context
  must pass. If any fails, the PR stops and returns to architecture. No ad hoc
  path move.
- **Real hardware:** a root service creating and binding `run`; cross-user
  client traversal; the root peer check before credentials; launchd's
  `StandardErrorPath` under a nonzero-group parent; stale-socket recovery.

## Implementation

See [docs/briefs/0015-macos-ancestor-implementation.md](../briefs/0015-macos-ancestor-implementation.md):
one Sol PR plus its required tests and mutations.
