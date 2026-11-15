# M2 A7 controlled policy-fixture decision

Design sub-agent: Muse Spark, high effort, 2026-11-15. Documentation only.
Sol coordinator independently reviews; no contract or gate changes here.
Standing results retained: query-oracle NO-GO `25bbbdc` (exit-1/NOPASSWD
ambiguity); public provenance `00e4b2a` (no complete host literals).
User steering: proportionate fences, working end-to-end M2, no safety overhead.
A7/A11/M2/full F/G/real-Runner PA6 remain **unverified on real hardware**.

## Decision: CONDITIONAL GO on B (fixture-local policy view)

(A) Complete-host-policy private byte review: attest the fresh host's own
`/etc/sudo.conf`, `/etc/nsswitch.conf`, `/etc/sudoers`, full `sudoers.d` set,
resolved plugin paths and NSS routes, then review exact bytes privately.
(B) Source-pinned fixture-local view on a fresh disposable host: systemd
test-only bind mounts present reviewed literal config bytes to installed real
sudo; two owned accounts give closed allow/deny cases. No host-policy
reconstruction, no public raw-policy capture.

B is the one practical next path, subject to the binding amendment and the
fresh-host grant below. A stays valid but is heavier: it must close unknown
host bytes per image. B generates the closed set from reviewed source instead.

## Why B is equivalent for the A7 security intent

A7 proves the fence, not production policy: the NNP Runner must yield sudo
exit-nonzero both where the viewed policy allows NOPASSWD `/usr/bin/true`
(case0) and where it allows nothing (case1). A known-closed viewed policy is
sufficient; it need not be the host's original policy. B keeps every
load-bearing probe fact unchanged: installed sudo ELF, real-descriptor C1
alias `O_PATH` + `execveat(AT_EMPTY_PATH)` at the build account, NNP,
64MiB/swap0/pids32, 20s/4096-byte bounds, UID/GID `>=1000` distinct checks,
sole primary groups, nologin, `admin=false`, owned-only cleanup, both cases.

## Feasibility against the actual repo and systemd/sudo semantics

Packaged `arxd.service` plus `50-arxcovo-standard-rights.conf` set
`ProtectSystem=full`, `PrivateTmp=yes`, `NNP=yes`. B changes neither file.
Only the disposable test-only `ExecStart` override gains explicit
fixture-only `BindReadOnlyPaths=` entries (exact mappings in the amendment).
`ProtectSystem=full` implies a private mount namespace; extra bind mounts
are the documented hole-punch mechanism, applied after the read-only
remounts, and stay inside that namespace, so the host is unmutated.
`PrivateTmp=yes` masks only `/tmp`/`/var/tmp` and does not interact.
Fixture files are staged root-owned outside the service (driver prep, as
today), satisfying sudo's root-owned `0440` enforcement and the existing
no-in-service-`/etc`-mutation rule. No kernel/systemd blocker found on paper;
fresh-host behavior still needs the grant below, not assumed here.

## Trusted-source boundary without reconstructing packages

B does not rebuild or byte-prove distro packages. Trust rests on: reviewed
image revision provenance, protected-ancestry admission of the installed C1
ELF (owner0, no group/other write, nlink1, exact size/hash/dev+ino, held FD,
per-host digest pin), C2 absent-or-trusted, and the fixture literals
themselves reviewed as source. Real remaining risks, each checkable on the
fresh host: (1) an unmasked compiled-in sudo default path (sysconfdir or
plugin path differing from `/etc`) bypassing the four masks, which breaks
case1 closure; establish via the installed ELF's compiled paths plus
fresh-host verification. (2) The interpreting plugin (`sudoers.so`) is the
host's bytes, so a drifted plugin enforces differently than reviewed text;
per-host plugin digest pin required, same as route A. (3) Fixture `nsswitch`
must keep `passwd`/`group` on files so owned-account resolution is intact.

## Minimal exact binding amendment (reviewed test-contract change, no waiver)

1. Packaged unit and drop-in bytes unchanged. Only the disposable override
may add exactly: `BindReadOnlyPaths=<stage>/fx/sudoers:/etc/sudoers`,
`<stage>/fx/sudo.conf:/etc/sudo.conf`,
`<stage>/fx/nsswitch.conf:/etc/nsswitch.conf`,
`<stage>/fx/sudoers.d:/etc/sudoers.d`. No other masks; production never sees
them. Any extra mapping or production activation refuses.
2. Fixture literals are reviewed source bytes: `sudoers` (no user grants,
single `#includedir /etc/sudoers.d`), `sudoers.d/10-a7-case0` (exclusive
`0440`, exactly `<case0> ALL=(root) NOPASSWD: /usr/bin/true`), `sudo.conf`
(local sudoers plugin only, fixed pinned path), `nsswitch.conf`
(`sudoers: files`, `passwd`/`group` on files, no `sss`/`ldap`). Closed set:
exactly one include file; unknown names/routes/plugins refuse.
3. Case1 closure is by construction plus static literal review: no grant for
case1-name-or-group, no `ALL`, no UID/user-alias match, no external source.
No runtime policy inference; list-mode oracles stay banned per `25bbbdc`.
4. Binary/plugin/floor pins per execution host: C1 ELF digest, resolved
plugin digest, compiled-default path equality, systemd>=254, kernel>=5.14,
cgroup2 memory+pids delegation. Drift refuses before effects.

## Source ceiling, tests, and needed fresh-host grant (no implementation here)

Ceiling: `tools/unix-exec-validation/driver.py` (fixture staging + override
rendering) and its tests; dormant `sudo_nnp.rs` gains only the fixture-digest
binding when a later slice wires it. Tests: pure rejection/mutation for mask
completeness, literal drift, case1-grant absence, collisions, partial
cleanup; retain S8-S14, E4 pins, driver guards. Grant: one fresh disposable
host execution grant naming image revision, both owned accounts/cases,
fixture+artifact+source/workflow pins, one dispatch, private bounded capture
(digests/flags only), and cleanup/variable lifecycle. D7a-g stay consumed;
contract and gates unchanged until Sol review and green required checks.
