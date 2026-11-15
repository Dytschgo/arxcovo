# M2 A7 Ubuntu package-path review - Muse independent (conditional GO)

Scope: provenance candidate `6d08139` (= `eecf440` content) on branch
`t3/ubuntu-sudo-package-path-review` at HEAD `bde9fa1`, over retained
`8f7092e` + PROGRESS + preflight DEFER `97ff38e`; local `926d15b` stays
unpublished/unrun. Question: GO or NOGO for the MINIMAL bounded FILE
comparison against expected official Noble amd64 sudo
`1.9.15p5-3ubuntu5.24.04.4` bytes - no `sudo -V` query, no host-policy
reconstruction. Read AGENTS/KICKOFF, ADR0020 D9b, the candidate, the
preflight review and D9b decision sources before judging. No PROGRESS/ADR/
source edits, tests, builds, host/native reads, grants, dispatch, push,
PR or subagents in this review.

## Standards - PASS, no findings

- One-concern docs-only provenance note; conventional subject, small diff,
  UTF-8 LF, `git diff --check` clean (re-verified here).
- Every claim is a checkable hash/flag/inventory row; no product approval
  flow, ceremony or audit framework. Proportionate to the steering.
- Smell baseline: nothing to flag in a data note; no abstraction added,
  no speculative generality.

## Spec - what the candidate establishes (and its ceiling)

- Authenticated chain as recorded: GPG-signed noble-security InRelease
  (exit 0, GOODSIG `F6ECB3762474EDA9D21B7022871920D1991BC93C`) to
  Packages.xz/Sources.xz SHA256+size to exact sudo stanzas to dsc/orig/
  debian/deb bytes; updates index agrees on version/deb hash (its own
  signature unchecked, recorded as such).
- Four upstream files at the pinned commit byte-equal to Ubuntu orig
  members; the delta patch inventory touches none of them. `lp2143042`
  changes mailer identity only; `CVE-2025-32463` changes sudoers Makefile
  object lists, not path CPPDEFS. Reviewed build flags: `--with-sssd`,
  apparmor/selinux/linux-audit, tmpfiles; plain sudo has no ldap/nsswitch/
  plugindir/sysconfdir overrides. Compiled defaults: `/etc/sudo.conf` and
  `/etc/sudoers`; NSS `files` routing; plugindir `/usr/libexec/sudo/`.
- Trust ceiling (explicit, accepted): Ubuntu's source-to-binary association,
  NOT a reproduced build, NOT a transitive dependency audit, NOT a
  libc/NSS attestation, NOT installed-host qualification.

## Independent verification actually run

- Fresh HTTPS downloads to TEMP, data-only extraction, nothing executed or
  installed - a second route corroborating the recorded hash chain:
  dsc `18138493...009ff836` MATCH; deb `deaf622c...c745f3c` MATCH;
  delta `4be7dcbc...12ef0` MATCH; rules `0a364a99...96bc7aa` MATCH;
  `usr/bin/sudo` `6ae6665c...a31ff` MATCH (282032 bytes);
  `sudoers.so` `151d681d...5d4c` MATCH (588080 bytes);
  `libsudo_util.so.0.0.0` `aae167b9...e26d` MATCH (133944 bytes).
- `debian/rules` flags corroborated line by line (sssd/sssd-lib,
  `--enable-admin-flag`, which is not `--enable-adminconf`,
  apparmor/selinux/linux-audit, tmpfiles, build-simple vs build-ldap
  `--with-ldap`); `series` contains `lp2143042.patch` and
  `CVE-2025-32463.patch` as recorded.
- NOT re-run here (taken from the candidate record): the GPG verification
  itself, Sources/Packages stanza matching, the four upstream byte
  comparisons, and the configure.ac line pins. Any recheck failure in
  that list flips this verdict.

## Security - PASS as a bound leg; residual risks named

- GPG trust is undefined (official-HTTPS keyring bootstrap, no pretrusted
  local key) - honestly recorded in the candidate, not a blocker for a
  drift-detection leg under A1 distro-build trust.
- Strings are not semantic proof: the ELF-observed configure string and
  NUL-terminated `/etc/sudo.conf` corroborate but never replace the digest
  comparison plus in-unit view plus native oracles.
- Concurrent privileged writers stay outside the trust boundary; A1 states
  privileged mounts/writers as assumption, so held snapshots are
  observations, never provenance. Targeted privileged tampering is out of
  scope; this leg detects drift, wrong revision and wrong architecture.

## Loaded-plugin/dependency disposition (finite, no OS-audit recursion)

- The fixture pins a fixed absolute Plugin path with no `sudoers_file=`
  override, so loaded-plugin identity reduces to: (1) compiled-default
  sudo.conf path `/etc/sudo.conf` from this build, (2) package-byte equality
  of installed sudo, (3) in-unit view proving the fixture bind mapping is
  effective, (4) held-descriptor digest of the plugin file at the pinned
  absolute path. The loader takes absolute plugin paths directly (recorded
  `load_plugins.c` reading); no further plugin-search audit is needed.
- `libsudo_util.so.0`: resolve the `.so` to `.so.0` to `.so.0.0.0` chain
  under held descriptors, digest the final target against the member row,
  and read-only-inspect sudo's ELF NEEDED/RUNPATH for unexpected search dirs.
- ADR0020 D8/A1 allows this bound: the paths are root-owned without
  group/other write, so the comparison needs no whole-OS audit - it refuses
  drift while A1 bounds malice.
- Exact minimal facts for the future grant: held-fd SHA256+size of the three
  files; bounded nofollow readlink chain of the `.so.0` links; read-only ELF
  NEEDED plus RUNPATH/RPATH absence; in-unit proof of the four bind mappings
  and fixture-literal bytes. No arbitrary package attestation or framework.

## Verdict - CONDITIONAL GO for the file-comparison leg only

- GO: compare protected held-descriptor SHA256+size of installed
  `/usr/bin/sudo`, `sudoers.so` at the pinned absolute path, and the
  resolved `libsudo_util.so.0` target against the three member rows above;
  reject any drift, other architecture or other revision. Package-byte
  equality plus reviewed build defaults CAN satisfy the compiled-default
  half of the D9b four-mask proof, with in-unit view and native oracles
  (actual NNP case0-allow/case1-deny) supplying the behavioral half later.
- Binding: expected digests are exactly the three rows verified above for
  revision `1.9.15p5-3ubuntu5.24.04.4` amd64. Source ceiling is distro-build
  trust as stated. C2 absent-or-trusted disposition, in-unit view, and
  native oracles remain open and are NOT granted here.
- Required alongside: mock mutants in the future wiring (wrong digest,
  truncated file, swapped-arch member, dangling/redirected `.so.0` link,
  extra mapping, missing bind - each must refuse), and a fresh narrow
  read-only grant review with its own independent review.
- The `sudo -V` query DEFER (`97ff38e`/`1292d33`) stands and is not revived
  by this GO; no privileged execution is authorized by this review.
- NOGO flip conditions: any recheck failure in the NOT-re-run list above,
  a fixture override of the plugin path, or scope creep past the three files.

A7/A11/M2/full F/G/real-Runner PA6 remain **unverified on real hardware**.
No gate, execution or native grant changes; any comparison wiring still
needs independent review, exact-head gates and coordinator merge.

## Checks run

- Fresh downloads + SHA256 of dsc/deb/delta/rules/three members: all MATCH.
- `rules` flag lines and `series` inventory corroborated; `git diff --check`
  clean; line/encoding fence on this note. No tests, builds or host reads.
