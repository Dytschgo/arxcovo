# D9b Ubuntu sudo build-path provenance

2026-11-15; OpenAI gpt-6.1-sol / high; public-source research only.
Read AGENTS/KICKOFF, ADR0020 D9b, accepted decision/reviews, controlled source evidence,
public-policy provenance and local `926d15b` preflight evidence; main supplied as `8f7092e`.

## Result and scope

**A small official-package comparison candidate exists for Noble amd64 sudo `1.9.15p5-3ubuntu5.24.04.4`.**
Both official noble-updates/security indexes still advertise this revision. No installed host was queried.
This supports build-specific default-path review without executing `sudo -V`; installed equality remains unknown.
It trusts Ubuntu's association of source and binary packages, not a reproduced build or full policy reconstruction.
A7/A11/M2/full F/G/real-Runner PA6 remain **unverified on real hardware**; no gate or execution grant changes.

## Retrieved primary evidence and authentication

Archive root [pool/main/s/sudo](https://archive.ubuntu.com/ubuntu/pool/main/s/sudo/):
[descriptor](https://archive.ubuntu.com/ubuntu/pool/main/s/sudo/sudo_1.9.15p5-3ubuntu5.24.04.4.dsc),
[upstream tar](https://archive.ubuntu.com/ubuntu/pool/main/s/sudo/sudo_1.9.15p5.orig.tar.gz),
[Debian delta](https://archive.ubuntu.com/ubuntu/pool/main/s/sudo/sudo_1.9.15p5-3ubuntu5.24.04.4.debian.tar.xz),
[amd64 deb](https://archive.ubuntu.com/ubuntu/pool/main/s/sudo/sudo_1.9.15p5-3ubuntu5.24.04.4_amd64.deb).
Actual HTTPS downloads; web descriptor fetch failed unsupported content-type, so Python urllib retrieved bytes.
Read archives as data with tarfile/ar parsing and installed Windows tar; no downloaded code/binary executed.

Verified [noble-security InRelease](https://archive.ubuntu.com/ubuntu/dists/noble-security/InRelease)
using existing Git GPG: exit 0, GOODSIG/VALIDSIG fingerprint `F6ECB3762474EDA9D21B7022871920D1991BC93C`.
Key bootstrap: official HTTPS [ubuntu-keyring deb](https://archive.ubuntu.com/ubuntu/pool/main/u/ubuntu-keyring/ubuntu-keyring_2023.11.28.1_all.deb),
read-only extraction of `usr/share/keyrings/ubuntu-archive-keyring.gpg`; no key imported into personal stores.
GPG trust is undefined: authentication relies on this official HTTPS key bootstrap, not an independently pretrusted local key.
Initial GPG Windows-path invocation failed; corrected MSYS paths produced the successful verification above.
Verified InRelease SHA256+size for [Packages.xz](https://archive.ubuntu.com/ubuntu/dists/noble-security/main/binary-amd64/Packages.xz)
and [Sources.xz](https://archive.ubuntu.com/ubuntu/dists/noble-security/main/source/Sources.xz), then exact sudo stanzas:
binary SHA256+size matches deb; source SHA256+sizes match dsc, orig and Debian tar; dsc also matches both tar hashes.
Separate [updates index](https://archive.ubuntu.com/ubuntu/dists/noble-updates/main/binary-amd64/Packages.xz) agrees on version/deb hash (its signature not checked).
No separate dsc/uploader or upstream detached-signature verification claimed; signed Sources authenticates these exact source bytes.

| Download/member (SHA256; members are uncompressed bytes) | Bytes | Digest |
| --- | ---: | --- |
| InRelease (snapshot; mutable URL) | 126127 | `58c274e629044455eff0a76d09c6b30e1b4bebb6253ca67c4e23cdddba4e7f64` |
| security Packages.xz | 1195092 | `b54fa6f53e08407e5289e8723a16ed6b13482bb5bb93a988e5768e2c78c82956` |
| security Sources.xz | 321116 | `e527bda51efcfa51e5e160ad7747498910c7c45629d875f423e74743a62f7fc7` |
| ubuntu-keyring deb | 11124 | `36de43b15853ccae0028e9a767613770c704833f82586f28eb262f0311adb8a8` |
| sudo dsc | 2763 | `18138493fb0f1780753c61adc1170ba2424ebb594a6614dfcfbf7e7c009ff836` |
| sudo orig tar.gz | 5306611 | `558d10b9a1991fb3b9fa7fa7b07ec4405b7aefb5b3cb0b0871dbc81e3a88e558` |
| sudo Debian tar.xz | 78028 | `4be7dcbcf4bd8daf530255440fe3fbcebc0fc4c5a1e4fa830d7790a3aeb12ef0` |
| sudo amd64 deb | 948984 | `deaf622cdfa623d3efca7b797e38ceae461769b2ec427e8ecd8e6c0fcb745f3c` |
| deb `usr/bin/sudo` | 282032 | `6ae6665cc51295ea8aaba94a3377dafa563f950aafc6296c0704fb33401a31ff` |
| deb `usr/libexec/sudo/sudoers.so` | 588080 | `151d681d51293a040dc479cb0175d1a6d461a40ff455661cf6ad485adf305d4c` |
| deb `usr/libexec/sudo/libsudo_util.so.0.0.0` | 133944 | `aae167b9da2436a31eba12d9fb20b36120fd204b2c720d62881d09e89eecd26d` |
| delta `debian/rules` | — | `0a364a99793de453193e5bcd330bc324cab27303243ea3a807c992d0296bc7aa` |
| orig `configure.ac` | — | `7a83a67de69115ccda0f573fc6afac55af82360c8adce04756680a3acba28b7f` |
| orig `lib/util/sudo_conf.c` | — | `f20043bf1bd9f98156698e71a3ec785492a1b7037efc999aceef287a27aecd8f` |
| orig `plugins/sudoers/sudo_nss.c` | — | `dc99e9744e097ad88c6f076b929cb783414963c009c38257368ed223c437a6d8` |

## Exact path derivation and packaging

Pinned upstream [configure.ac](https://github.com/sudo-project/sudo/blob/fc9369cb442c8c5f6612d1831294569f1db75028/configure.ac),
[sudo_conf.c](https://github.com/sudo-project/sudo/blob/fc9369cb442c8c5f6612d1831294569f1db75028/lib/util/sudo_conf.c),
[sudo_nss.c](https://github.com/sudo-project/sudo/blob/fc9369cb442c8c5f6612d1831294569f1db75028/plugins/sudoers/sudo_nss.c)
and [load_plugins.c](https://github.com/sudo-project/sudo/blob/fc9369cb442c8c5f6612d1831294569f1db75028/src/load_plugins.c)
were fetched at that commit and each byte-compared equal to its Ubuntu orig member.
Inspected delta patch target inventory: these four files are untouched. `lp2143042.patch` changes policy mailer identity only;
`CVE-2025-32463.patch` changes sudoers Makefile object/dependency lists, not its path CPPDEFS. No whole-security-review claim.

- `debian/rules`: `build-simple` produces sudo; separate `build-ldap` adds `--with-ldap --with-ldap-conf-file=/etc/sudo-ldap.conf` for sudo-ldap.
  Common exact relevant flags: `--with-sssd --with-sssd-lib=/usr/lib/$(DEB_HOST_MULTIARCH)`; Linux adds `--with-apparmor --with-selinux --with-linux-audit --enable-tmpfiles.d=/usr/lib/tmpfiles.d`.
  The deb's sudo ELF contains its actual configure string: `--prefix=/usr --sysconfdir=/etc --libdir=${prefix}/lib/x86_64-linux-gnu --with-sssd --with-sssd-lib=/usr/lib/x86_64-linux-gnu` (selected exact arguments).
  That string has no `--enable-adminconf`, `--with-plugindir`, `--with-nsswitch`, `--with-ldap` or alternate sudoers path; `--enable-admin-flag` is a different option.
- `configure.ac:134,136`: `sudo_conf='$(sysconfdir)/sudo.conf'`, `sudoers_path='$(sysconfdir)/sudoers'`.
  Adminconf defaults **no** (:1579–1590), so no alternate colon search path. `SUDO_DEFINE_PATH` (:4427–4429) emits `_PATH_SUDO_CONF` and `_PATH_SUDOERS`.
  `src/Makefile.in:55` and `lib/util/Makefile.in:70` compile `-D_PATH_SUDO_CONF=\"@sudo_conf@\"`; sudoers Makefile :76 compiles `-D_PATH_SUDOERS=\"@sudoers_path@\"`.
  Result for this build: `/etc/sudo.conf` and `/etc/sudoers`; both also observed as NUL-terminated package ELF strings, not runtime output.
- `configure.ac:229,3525–3527`: `_PATH_NSSWITCH_CONF=/etc/nsswitch.conf` by default. `--with-sssd` defines `HAVE_SSSD` (:391–398),
  enabling `sudo_nss.c:52–129` to open that macro and select only `files` for literal `sudoers: files`. SSSD support compiled in does not force SSSD routing.
  Plain sudo's version-print NSS label is guarded by HAVE_LDAP, explaining why `-V` may omit it despite active NSS parsing.
- `configure.ac:232–233,4353`: `plugindir="$libexecdir/sudo"`, `_PATH_SUDOERS_PLUGIN="sudoers.so"`, `_PATH_SUDO_PLUGIN_DIR="$plugindir/"`.
  Default libexecdir under prefix `/usr` yields `/usr/libexec/sudo/`; package sudo/plugin strings corroborate. Loader accepts absolute plugin paths directly.
  Actual deb contains root:root `usr/bin/sudo` mode 04755, `sudoers.so` and `libsudo_util.so.0.0.0` mode 0644; util symlinks `.so` and `.so.0` point to `.so.0.0.0`.
  `debian/rules` installs build-simple into debian/sudo and explicitly chmods `usr/libexec/sudo/*.so` 0644; `debian/sudo.install` supplies `debian/etc/*`.
- `/etc/sudoers.d` is **fixture literal include closure**, not a separate compiled directory default.
  Upstream `plugins/sudoers/toke.l:396–406` parses `#includedir`; D9b's sole directive plus exact sole case0 entry supplies this fourth mask.
  No original-host policy set, image recipe equality, libc NSS attestation or full transitive dependency audit follows from these facts.

## Minimal later runtime comparison (not performed or authorized here)

Under separately reviewed bounded file-read wiring/grant, compare protected held-descriptor SHA256+size of installed `/usr/bin/sudo`,
`/usr/libexec/sudo/sudoers.so` **and resolved libsudo_util.so.0** against the three amd64 member rows; reject drift/other architecture/revision.
The util library matters because it implements sudo.conf reading; also validate its protected symlink chain and actual dependency resolution.
Preserve C1/C2, alias, ancestry/identity, floor and per-host pins; existing candidate-only digest observations need trusted expected values.
In-unit four-mask/literal/entry/mount and NSS account checks remain required before each case. Fixed absolute fixture Plugin has no `sudoers_file=` override.
This binds compared bytes to the official package's reviewed source/default-path interpretation under distro-build trust; strings alone are not semantic proof.
It does not prove arbitrary loader/libc replacement safe, reproduce the package, qualify an installed host or replace native behavior evidence.

Validation: source/archive hash and size assertions, GPG verification, four upstream equality comparisons; report UTF-8/LF and git diff checks.
No tests/build/install, native/proc/root/owner reads, account/sudo/admin operation, dispatch, variable, push, PR or subagent action.
