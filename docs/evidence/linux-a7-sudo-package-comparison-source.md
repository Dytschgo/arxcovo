# A7 fixed sudo package comparison source evidence

2026-11-15; implementation: OpenAI gpt-6.1-sol / high.
Outcome: usable pure comparator plus fixed Linux read adapter, behind a refusing
CLI. Source-only delivery; no installed-host read or qualification was performed.
Only this note and the two new Python files changed. PROGRESS, ADRs, driver,
fixtures, workflows, gates and old policy collector are unchanged by instruction.

## Provenance and scope

Retained base `8f7092ed02a2da819756ca6194328e88757b752d` is an ancestor;
implementation parent is `afc08be` (existing progress record). Accepted Muse
review `15c5d0d` is represented by retained `e9d481f`,
[review](../reviews/m2-a7-ubuntu-package-path-muse-review.md).
Source provenance `eecf440` / retained `6d08139` is
[recorded here](../reviews/m2-a7-ubuntu-sudo-build-path-provenance.md).
Excluded local query `926d15b` is not an ancestor (merge-base exit 1), was not
cherry-picked, invoked or published. No sudo version/list query is revived.

This implements only the accepted bounded FILE leg under ADR0020 D8/A1 and D9b.
A1 trusts concurrent privileged writers/mounts and root-protected OS dependency
resolution. Held observations detect drift; they cannot defeat a privileged
adversary. Ubuntu source-to-binary association is trusted; no reproduced build,
transitive OS/library/NSS audit or external plugin qualification is claimed.

## Independently extracted official package data

Fresh HTTPS retrieval of the official
[amd64 deb](https://archive.ubuntu.com/ubuntu/pool/main/s/sudo/sudo_1.9.15p5-3ubuntu5.24.04.4_amd64.deb)
returned exactly 948984 bytes, SHA256
`deaf622cdfa623d3efca7b797e38ceae461769b2ec427e8ecd8e6c0fcb745f3c`.
Python urllib retrieved bounded public data, parsed ar headers, and wrote only
data to a fresh temporary directory. Installed Windows tar read selected archive
members as data; no downloaded executable/script was run, installed or built.
No credentials or owner data were sent. The earlier signed-index authentication
is inherited from the provenance record, not reverified here.

Revision is `1.9.15p5-3ubuntu5.24.04.4`, architecture `amd64`.

| Fixed package payload | Bytes | SHA256 |
| --- | ---: | --- |
| `/usr/bin/sudo` | 282032 | `6ae6665cc51295ea8aaba94a3377dafa563f950aafc6296c0704fb33401a31ff` |
| `/usr/libexec/sudo/sudoers.so` | 588080 | `151d681d51293a040dc479cb0175d1a6d461a40ff455661cf6ad485adf305d4c` |
| `/usr/libexec/sudo/libsudo_util.so.0.0.0` | 133944 | `aae167b9da2436a31eba12d9fb20b36120fd204b2c720d62881d09e89eecd26d` |

Both official symlinks `libsudo_util.so` and `libsudo_util.so.0` point directly
to `libsudo_util.so.0.0.0`. There is no intermediate `.so -> .so.0` hop in this
package. Both names and their final target are pinned; alternative chains refuse.

Independent struct-based data inspection found these literal dynamic facts,
then the implemented parser/comparator accepted all three official members:

| Payload | Ordered DT_NEEDED | DT_RPATH | DT_RUNPATH |
| --- | --- | --- | --- |
| sudo | `libaudit.so.1`, `libselinux.so.1`, `libapparmor.so.1`, `libsudo_util.so.0`, `libc.so.6` | absent | `/usr/libexec/sudo` |
| sudoers | `libaudit.so.1`, `libpam.so.0`, `libssl.so.3`, `libz.so.1`, `libsudo_util.so.0`, `libcrypto.so.3`, `libc.so.6` | absent | `/usr/libexec/sudo` |
| util | `libcrypto.so.3`, `libc.so.6` | absent | absent |

Program-header counts: 13 / 11 / 11; dynamic-table bytes: 576 / 608 / 512;
dynamic-string bytes: 3808 / 4751 / 5719, respectively.
**Correction to tentative review wording:** RUNPATH is present for sudo and
sudoers, exactly as above. Universal RUNPATH absence would refuse the accepted
package. This is a literal metadata correction, not broader search-path trust.
Reviewed defaults remain `/etc/sudo.conf`, `/etc/sudoers`, `/etc/nsswitch.conf`,
plugin directory `/usr/libexec/sudo/`; fixture include closure supplies the fourth
mask. Later fixture binding must select the fixed absolute sudoers plugin with
no `sudoers_file=` override. This leg does not inspect fixture/policy files.

## Mechanism and boundaries

`compare` accepts exactly a tuple of three byte payloads, in fixed order, with no
expected-hash, path, revision or ELF-fact selector. All exact SHA256/size and
ELF facts must match. Output is a fixed schema of closed status/version labels,
booleans and known digests (null on refusal), never an unknown-byte hash oracle.
A match sets only `candidate_match`; `qualified` stays false.

The adapter uses read-only O_NOFOLLOW|O_CLOEXEC|O_NONBLOCK, O_DIRECTORY on all
root-down directories, root owner and no022 checks. Files must be regular,
nlink 1, and exactly the pinned size before reading. Chunk size is at most 4096;
per-file loops and aggregate returned-byte budget (1004059 including one EOF
probe byte per file) are finite. Short/truncated, oversized and non-byte reads
refuse. No arbitrary name enumeration, command, environment selector or policy,
account, proc or sys read exists. All descriptors close even on refusal.

Both symlinks use nofollow lstat on a held parent, exact root-owned link type,
size/nlink, and a **22-byte** readlinkat buffer (21-byte target plus one overflow
byte). The syscall adapter uses only stdlib ctypes with a fixed buffer and
rejects negative/full-buffer returns. Targets, link metadata and held final
regular target are pinned and rechecked. The commentary's earlier 24-byte count
was a reporting typo; code/tests use `len(target)+1`, exactly 22.

Metadata snapshots include device/inode/type/mode/uid/gid/nlink/size/mtime/ctime.
They are compared before/after reads, after all payloads, and against a fresh
root-down path reopen of every directory and file. Replacement is refused,
never adopted. These checks are observational under A1, not atomic provenance.

ELF inspection accepts only ELF64 little-endian amd64 ET_DYN with bounded,
file-backed program/dynamic/string tables: at most 32 headers, 128 dynamic
entries, 8192 string bytes, 16 dependencies, 255-byte strings. No loading,
relocation, disassembly, general ELF library or subprocess is used. Unexpected
search paths, duplicate facts, missing terminators and malformed offsets refuse.

`_read_linux` is private, has no ordinary caller, and was never invoked here.
It constructs the actual OS/ctypes adapters only on explicit future invocation.
Import and CLI have no edge to it. The CLI always emits
`root_reads_not_authorized` and exits 2, without inspecting arguments,
environment, UID or host files. No product approval flow was added.

## Checks actually run

- Audited imports/CLI/private-read call graph; AST regression preserves the
  refusing boundary. Linux syscall-shaped adapter exercised only with fake OS
  ports/FDs; bounded readlink buffer exercised with fake libc, no real syscall.
- `python -I -B -m unittest discover -s tools/unix-exec-validation -p test_sudo_package_profile.py -v`:
  final 10 tests PASS. Initial request-accounting assertion exposed a one-byte
  duplicate request per payload; fixed before final verification.
- `python -I -B -m unittest discover -s tools/unix-exec-validation -p 'test_*.py'`:
  155 tests PASS on final code. An earlier 154-test run also passed; repeated
  only after adding the bounded readlink syscall injection test/source change.
- Wrong digests/architecture, truncation/oversize, malformed tables and search
  paths, dangling/redirected links, owner/type/flags, descriptor/path swaps,
  budget exhaustion and private error payloads all refuse in synthetic tests.
  Synthetic success patches test-only expected digests; production pins remain
  literal. Actual downloaded official bytes also passed the pure comparator.
- In-memory Python compile, strict UTF-8/no BOM/LF assertions and final staged
  `git diff --check`: PASS. No bytecode artifact or downloaded data committed.

Source SHA256 pins (LF bytes; this note does not hash itself):

| File | SHA256 |
| --- | --- |
| `tools/unix-exec-validation/sudo_package_profile.py` | `12a122db7d30be04dd7523db046d49f4083882c318b375ccf0986ec0eb338e75` |
| `tools/unix-exec-validation/test_sudo_package_profile.py` | `c62f4558fdc5d2a61222599c41073c7c5140f8917f64625a71d80209602ae629` |

## Remaining authorization and acceptance

Fresh narrow Muse independent source/security review is required before any
publication, followed by normal exact-head gates and coordinator disposition.
A NEW reviewed read-only grant must precede any actual private Linux read entry
invocation. This task grants none. C2, floor, in-unit four binds/literals/view,
NSS/account observations and case0-allow/case1-deny under actual NNP remain open.
No host qualification, fixture admission or native acceptance follows from a
candidate match. A7/A11/M2/full F/G/real-Runner PA6 remain **unverified on real
hardware**. No dispatch, account/admin operation, variable, push, PR, subagent,
owner step or old Lab Manager access occurred.
