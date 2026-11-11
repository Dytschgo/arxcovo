# Disposable Ubuntu runner mirror mitigation candidate

The workflow activation is prepared locally but has not been pushed or run.
It must receive independent review before publication. Never run
`ubuntu_mirror.py` on an owner machine. Its environment
checks express intent and are spoofable; they are not provenance or authority.

Observed PR32 job 130635247058 (source `b475bf1`) ran on `ubuntu-26.04`, image
`20261101.186.1`. Before application tests, three Ubuntu-domain mirror lookups
timed out while the Kernel Archives, GitHub and Microsoft controls resolved.
The later Playwright dependency step failed resolving Ubuntu names. This is a
mitigation candidate, not an established root cause or a verified CI fix.

The [Kernel Archives Ubuntu mirror](https://mirrors.edge.kernel.org/ubuntu/dists/)
lists resolute, updates, security and backports. The candidate rewrites only
the exact URI values for the three observed hosts to its HTTPS endpoint. Suites,
components, architectures, signing-key selection, comments and unrelated
repositories remain unchanged. Existing global key selection is retained when
a source has no explicit Signed-By. No keys or insecure APT options are added.
[APT archive authentication](https://manpages.ubuntu.com/manpages/noble/man8/apt-secure.8.html)
continues to verify Release metadata and package checksums.

Both legacy `.list` and deb822 `.sources` are supported conservatively. Unsafe
trust options, foreign explicit keyrings, malformed target stanzas, unknown
Ubuntu hosts and mixed target/foreign URI stanzas refuse. Every file is bounded
to 64 KiB and at most 32 files are read. The whole plan is validated first.
Only root-owned, private-against-write, regular single-link files under the
fixed APT directories are replaced through same-directory staging. Existing
source identity and bytes are checked before replacement. A failure leaves CI
red; a partial successful rewrite is not rolled back. Noncooperating root
writers are outside this ephemeral helper's guarantee.

Activation, after review, would preserve only the four non-secret intent fields
through sudo and run the helper in the disposable Ubuntu job before the
unchanged `npx playwright install --with-deps chromium`. APT update, signature
verification, dependency installation and smoke tests must still pass. Do not
change resolver configuration, hardcode IP addresses, disable checks, retry
blindly or increase the CI timeout. Current required checks remain enforced.

Portable tests use synthetic native file facts for root ownership, regular-file,
link-count, mode, read-size and no-follow guards. They do not open owner-machine
APT files. Actual root publication and package installation have not run and
remain unverified; no workflow has executed this candidate yet.
