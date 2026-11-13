# Disposable Ubuntu runner mirror mitigation candidate

Source `400903d` received independent review before publication and its first
hosted execution passed. Exact evidence is retained in
`docs/evidence/ubuntu-signed-mirror-run.json`. Never run
`ubuntu_mirror.py` on an owner machine. Its environment
checks express intent and are spoofable; they are not provenance or authority.

Observed PR32 job 130635247058 (source `b475bf1`) ran on `ubuntu-26.04`, image
`20261101.186.1`. Before application tests, three Ubuntu-domain mirror lookups
timed out while the Kernel Archives, GitHub and Microsoft controls resolved.
The later Playwright dependency step failed resolving Ubuntu names. This is a
mitigation candidate, not an established root cause or a verified CI fix.

The [Kernel Archives Ubuntu mirror](https://mirrors.edge.kernel.org/ubuntu/dists/)
lists resolute, updates, security and backports. The candidate rewrites only
the exact URI values for the three observed hosts to its HTTPS endpoint. It
also recognizes only `mirror+file:/etc/apt/apt-mirrors.txt`, after reading that
exact file with the same native file guards. Every active entry has at most
one bounded `priority:N` tag and must be either a retained HOSTS HTTP/HTTPS
URI or an omitted entry matching the narrow grammar below. A foreign entry,
malformed line, empty/oversized list or nested mirror URI refuses.
Sources that use that mirror list keep their URI unchanged (their trust options
are still validated). Instead the validated list gains the Kernel mirror as
`priority:1`, and every original Ubuntu entry is kept with its priority raised
by one. Unsupported entries matching the omission grammar are removed.
Comments, existing priority ties and untagged entries are preserved, the result stays within
4 KiB, and a second run refuses because the list then has a non-Ubuntu entry.
[apt-transport-mirror](https://manpages.ubuntu.com/manpages/noble/man1/apt-transport-mirror.1.html)
tries the lowest priority first. If a mirror is inaccessible, it tries the next
entry until the file is retrieved. Suites,
components, architectures, signing-key selection, comments and unrelated
repositories remain unchanged. Existing global key selection is retained when
a source has no explicit Signed-By. No keys or insecure APT options are added.
[APT archive authentication](https://manpages.ubuntu.com/manpages/noble/man8/apt-secure.8.html)
continues to verify Release metadata and package checksums.

Both legacy `.list` and deb822 `.sources` are supported conservatively. Unsafe
trust options, foreign explicit keyrings, malformed target stanzas, unknown
Ubuntu hosts and mixed target/foreign URI stanzas refuse. Every file is bounded
to 64 KiB and at most 32 source files are read, plus the optional mirror list
bounded to 4 KiB input/output and sixteen active entries, including omissions.
At least one HOSTS fallback must remain. The whole plan, including every entry
after an omission, is validated before publication.
Only root-owned, private-against-write, regular single-link files under the
fixed APT directories are replaced through same-directory staging. Existing
source identity and bytes are checked before replacement. A failure leaves CI
red; a partial successful rewrite is not rolled back. Noncooperating root
writers are outside this ephemeral helper's guarantee.

The reviewed activation preserves only four non-secret intent fields
through sudo and runs isolated Python (`-I -B`) in the disposable Ubuntu job before the
unchanged `npx playwright install --with-deps chromium`. APT update, signature
verification, dependency installation and smoke tests must still pass. Do not
change resolver configuration, hardcode IP addresses, disable checks, retry
blindly or increase the CI timeout. Current required checks remain enforced.

Portable tests use synthetic native file facts for root ownership, regular-file,
link-count, mode, read-size and no-follow guards. They do not open owner-machine
APT files. Real root publication and package installation were exercised only
in the disposable hosted Ubuntu job at `400903d`, not on an owner host.

The complete plan/recheck/publication loop is also tested against owned temporary
trees using injected root facts and a recorded fchmod request. Real staging,
byte replacement and cleanup run there; this provides no root-owner or Unix
permission proof. Tests cover the observed 144-byte priority mirror list,
preservation, all-plan refusal, stale bytes/identity and publication failure.

The first hosted Ubuntu job updated one source, fetched InRelease metadata for
all four suites from the HTTPS mirror with no signature/fetch warning, and
passed dependency installation plus browser and Electron smoke in 130 seconds.
All three Ubuntu-domain DNS controls also resolved in that run. It therefore
proves the signed-mirror path works, not that it reproduced or fixed the earlier
DNS failure. The root cause remains unknown.

## Kernel-only mirror failure and fallback correction

On November 12, fresh main job 130963746682 (source `b5e6556`) and four sibling
Ubuntu jobs all printed `ubuntu-mirror-updated:1`, and all six DNS controls
resolved. Each job then failed in APT because it could not connect to
`mirrors.edge.kernel.org:443`. IPv6 reported network-unreachable (101), and in
every job the IPv4 connection also timed out. Three jobs failed at InRelease
(no installation candidate). Two fetched indexes, then failed all nine `.deb`
downloads. An IPv4-only change would therefore not have helped, because the
single Kernel host was the only source.

In the PR32 failure, APT on this image went through all three Ubuntu entries
(`Ign` and then `Err`), spending about 20 s per file and entry on name
resolution. A list with Ubuntu first and the Kernel mirror last would face that
per-file cost under the 12-minute limit. The correction therefore puts the
Kernel entry first and keeps the Ubuntu entries behind it. In the fresh runs,
Kernel connect failures failed all queued items together after about 16 s.
Tests model only the documented order. Real fallback timing and the sync of
packages fetched from a different mirror are **pending hosted confirmation**.
Sanitized facts are in `docs/evidence/ubuntu-kernel-mirror-failures.json`.

Product installed-service and
dedicated-account acceptance remain open.

## Narrow unsupported-entry omission

For the indirect list only, `_rewrite_mirror_list_counted` returns bytes and
an omission count; `rewrite_mirror_list` retains its bytes API. A full raw-token
match runs before URL parsing: lowercase ASCII HTTP(S), ASCII LDH labels of
1–63 characters under `.ubuntu.com`, total hostname at most 253 characters,
and exactly `/ubuntu` or `/ubuntu/`. Bare domains, `xn--` labels, non-ASCII,
C0/DEL, escapes, backslashes, userinfo, ports, query/fragment delimiters
(including empty delimiters), other schemes and paths cannot be omitted.
ASCII line/token separators avoid Unicode whitespace/control normalization.
Parsed checks retain independent scheme, authority, hostname, length, label,
path and query/fragment guards. Only unsupported hosts outside HOSTS are
omitted; Kernel entries always refuse, anywhere, including a second run.

All retained and dropped active entries count toward sixteen; an all-omitted
list refuses. The public 144-byte list still yields exactly the same 195 bytes.
Priority+1 and existing ties are unchanged; no universal uniqueness is promised.
Direct sources still refuse unsupported Ubuntu-domain URIs. Third-party source
preservation, HOSTS, Kernel endpoint, keyring, signature and Valid-Until checks,
file guards, rechecks and publication-error behavior remain unchanged.
Success prints exactly `ubuntu-mirror-updated:N` and `ubuntu-mirror-omitted:M`
(on separate lines), where M is 0–15 and direct mode reports 0. Refusal prints
only `ubuntu-mirror-refused` to stderr; invalid arguments print only
`ubuntu-mirror-arguments`. No host, source bytes, index or error is printed.

This is the approved ADR 0021 availability tradeoff: removed hosts are never
contacted or trusted. If only an omitted host was reachable, installation can
still fail. Representative synthetic fixtures establish parser behavior only;
the actual historical hostname and unchecked tail remain UNKNOWN. This local
correction is **unverified on real hardware**, without a hosted fetch trial.
