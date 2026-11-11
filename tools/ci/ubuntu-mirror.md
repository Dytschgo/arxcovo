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
exact file with the same native file guards and validating every entry as an
allowed Ubuntu HTTP/HTTPS URI with at most one bounded `priority:N` tag. A
foreign entry, malformed line, empty/oversized list or nested mirror URI refuses.
The mirror list itself is never modified. Suites,
components, architectures, signing-key selection, comments and unrelated
repositories remain unchanged. Existing global key selection is retained when
a source has no explicit Signed-By. No keys or insecure APT options are added.
[APT archive authentication](https://manpages.ubuntu.com/manpages/noble/man8/apt-secure.8.html)
continues to verify Release metadata and package checksums.

Both legacy `.list` and deb822 `.sources` are supported conservatively. Unsafe
trust options, foreign explicit keyrings, malformed target stanzas, unknown
Ubuntu hosts and mixed target/foreign URI stanzas refuse. Every file is bounded
to 64 KiB and at most 32 source files are read, plus the optional mirror list
bounded to 4 KiB and sixteen entries. The whole plan is validated first.
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
DNS failure. The root cause remains unknown. Product installed-service and
dedicated-account acceptance remain open.
