# M2 A7 sudo package comparison review - Muse independent source/security

Scope: branch `t3/sudo-package-review` at HEAD `2b0fee3` over retained base
`8f7092ed02a2da819756ca6194328e88757b752d`; implementation parent `afc08be`.
Reviewed files ONLY (756 insertions, no other source touched):

- `tools/unix-exec-validation/sudo_package_profile.py` (289 lines)
- `tools/unix-exec-validation/test_sudo_package_profile.py` (322 lines)
- `docs/evidence/linux-a7-sudo-package-comparison-source.md` (145 lines)

Read before judging: AGENTS.md, KICKOFF, ADR0020 D9b (`D9b. Controlled A7
policy fixture`), the conditional GO review
(`docs/reviews/m2-a7-ubuntu-package-path-muse-review.md`, retained `e9d481f`
for `15c5d0d`), the build-path provenance
(`docs/reviews/m2-a7-ubuntu-sudo-build-path-provenance.md`, `6d08139` for
`eecf440`), the preflight DEFER review
(`docs/reviews/m2-a7-a11-runtime-preflight-muse-review.md`), and all three
scoped files in full. Excluded local query `926d15b` verified NOT an ancestor
(`git merge-base --is-ancestor` exit 1); no `sudo -V` query is revived, and no
source/contract gate is changed by this review. No PROGRESS/ADR/source edit,
test, build, host/native read, grant, dispatch, variable, push, PR or subagent
in this review. No Linux/root/proc/sys read, native/helper/ignored execution,
account/sudo/admin operation, or old Lab Manager access occurred.

## Standards - PASS, no findings

- One-concern dormant source slice plus evidence note; small diff, UTF-8, LF,
  no BOM, `git diff --check` clean and `py_compile` clean (both re-verified
  here; source files are 289/322 lines as stated).
- Every behavior has a test: 10 mock/AST tests covering literal pins, parser
  facts and mutants, adapter success/flags/reopens/budget, owner/type/mode/
  nlink/size refusals, link/metadata/content attacks, budget errors and
  private-leak silence, flag/ELF-table bounds, fake-libc readlink buffer, and
  the CLI/import call-graph fence.
- No product approval flow, ceremony, or audit framework added. Fixed technical
  fences only (exact pins, root-only ancestry, no022, bounded reads, fail-closed
  refusal), proportionate to the steering.
- Smell baseline: nothing to flag. `_LinuxPorts` thin delegation is a genuine
  injected-port adapter, not a Middle Man; `_ReadlinkAt` is the fixed syscall
  shape, not speculative generality. No duplicated logic, data clumps, message
  chains, or refused bequests. Test `elf()` fixture builder duplicates struct
  layout by necessity and never touches production pins (guarded by the
  literals test).

## Spec - PASS against the conditional GO leg

The GO review (`e9d481f`) authorized exactly the bounded FILE comparison
against official Noble amd64 sudo `1.9.15p5-3ubuntu5.24.04.4`, with mock
mutants and a fresh grant review still required. Every requirement maps:

- Three payload pins are the exact reviewed rows: sudo 282032 /
  `6ae6665c...a31ff`, sudoers.so 588080 / `151d681d...5d4c`,
  libsudo_util.so.0.0.0 133944 / `aae167b9...e26d` (`_PINS`, verified
  byte-identical to the provenance and GO-review rows). `compare` accepts only
  a 3-tuple of exact-size bytes, checks ELF facts, then SHA256, in fixed order,
  with no path/hash/fact selector for callers.
- Symlink chain: the GO review said "resolve the `.so` to `.so.0` to
  `.so.0.0.0` chain"; the implementation pins BOTH official links pointing
  DIRECTLY at `libsudo_util.so.0.0.0` and refuses anything else (including a
  chained `.so -> .so.0` hop). The evidence documents this package has no
  intermediate hop, so the pin is literal to the observed official bytes and
  fail-closed, not a relaxation. A future revision using a chained layout
  would refuse (availability cost only, never a false accept). Correct as is.
- RUNPATH wording: the GO review asked for "RUNPATH/RPATH absence"; actual
  struct inspection finds RUNPATH `/usr/libexec/sudo` present on sudo and
  sudoers.so, absent on util, RPATH absent everywhere (`_SEARCH`). The code
  pins exactly that. Assessed as a literal metadata correction, NOT a security
  expansion: `_SEARCH` is exact-equality matched, so the present RUNPATH adds
  no search-path trust - any other value refuses - and the constrained
  absolute directory stays under root-owned/no022 A1 protection.
- Required mutants all refuse in tests: wrong digest, truncation/oversize,
  arch/header/table mutants, search-path drift (including empty, `$ORIGIN`,
  `/tmp` variants), dangling/redirected/oversize/wrong-target links,
  owner/type/mode/nlink/size violations, missing nodes, descriptor/path swaps,
  metadata drift mid-read, budget exhaustion, and private-error leakage.
- Match semantics stay inside the GO ceiling: a match sets only
  `candidate_match`; `qualified` stays `False`. No host qualification, fixture
  admission, or native acceptance follows. The `sudo -V` DEFER stands: the
  module contains no query, policy, subprocess, socket, or account surface
  (only docstring words matched; no such call exists).

## Security - PASS as dormant source; any read invocation needs a fresh grant

- Privacy: refusals are constant-shape. `compare` on garbage returns `refused`
  with all three digests null (re-verified here); no unknown-byte hash oracle
  exists - known digests are emitted only on `candidate_match`. Exception text
  and port error payloads (`PRIVATE ...` probes) never reach output; the CLI
  prints only `root_reads_not_authorized` and exits 2 without inspecting
  arguments, environment, UID, or host files.
- Call-graph audit (re-ran here): CLI calls are exactly
  `{print, json.dumps, _result}`; module level has no call edge to `_collect`,
  `_read_linux`, or `_LinuxPorts`; no `import subprocess`; no reference to
  `sudo_package_profile` anywhere outside the two new files. `_read_linux` is
  defined, has zero callers, and was never invoked.
- Adapter is fail-closed and bounded: `O_RDONLY|O_NOFOLLOW|O_CLOEXEC|`
  `O_NONBLOCK` with `O_DIRECTORY` on all root-down directories (constructor
  refuses zeroed flags); root owner plus no-`0o022` on every ancestor and leaf;
  regular files require `nlink == 1` and exact pinned size before reading;
  at most 4096-byte chunks with a finite loop plus one EOF probe per file and
  an aggregate returned-byte budget of exactly 1004059
  (282032+588080+133944+3, recomputed here). Short, oversized, and non-byte
  reads refuse; every descriptor closes in reverse in `finally`, and a close
  failure collapses even success to `refused`.
- Symlink handling: nofollow lstat on the held parent, exact root-owned link
  type, `nlink == 1`, `st_size == 21`, fixed 22-byte (`len+1`) readlinkat
  buffer rejecting negative and full-buffer returns, exact-target match, and
  before/after snapshot equality - then rechecked after all payloads and
  against a fresh root-down reopen that compares every identity and all nine
  metadata fields (dev/ino/type/mode/uid/gid/nlink/size/mtime/ctime) without
  adopting replacements. The evidence's earlier 24-byte mention is an
  acknowledged reporting typo; code and tests use `len(target)+1` = 22.
- ctypes readlink port: buffer allocated by injected `create_string_buffer`
  only (no real CDLL/syscall in tests; fake-libc test asserts the exact
  `(parent, name, 22, 22)` call shape and bound violations), fixed name
  allowlist, ascii-only. FD lifetime is fully covered: the success test
  asserts zero leaked descriptors, and every failure injection still closes.
- ELF inspection is data-only and source-independent: ELF64 LE amd64 ET_DYN
  magic/type/machine/version, bounded file-backed program/dynamic/string
  tables (at most 32 headers, 128 dynamic entries, 8192 string bytes, 16
  dependencies, 255-byte ascii strings), single PT_DYNAMIC with terminator,
  single STR TAB owner, at most one RPATH/RUNPATH each. Offset arithmetic was
  traced here: every segment satisfies `size <= len-off` before use, so
  dynamic-entry and string-table reads cannot run past the buffer. No sections,
  relocations, loading, execution, or subprocess.
- Residuals are explicit and contained while dormant: A1 trusts concurrent
  privileged writers/mounts (snapshots are observations, never provenance);
  root-protected OS dependency resolution is assumed, not audited - but no
  arbitrary full-OS audit is needed because paths, flags, tables, and facts
  are all fixed literals. Ubuntu source-to-binary association is trusted, not
  a reproduced build. None of this widens with the RUNPATH correction above.

## Checks actually run

- New suite: `python -I -B -m unittest discover -s
  tools/unix-exec-validation -p test_sudo_package_profile.py -v` - 10/10 PASS.
- Whole directory: `-p 'test_*.py'` - 155/155 PASS.
- AST call-graph probe: CLI call set `{_result, json.dumps, print}`; zero
  `_read_linux` name loads; module-level callers absent; `subprocess` absent.
- Arithmetic: `_TOTAL` 1004059 recomputed; readlink bound 22; `_SEARCH` and
  `_LINKS` as stated above.
- Behavior probes: garbage `compare` refuses with null digests; `_cli` exits
  2 with `root_reads_not_authorized`; both source SHA256 recompute to the
  evidence pins (`12a122db...`, `c62f4558...`).
- Hygiene: LF-only, no BOM, `git diff --check` clean, `py_compile` clean,
  `926d15b` not an ancestor, no wiring references outside the two files.
- NOT re-run (inherited, dual-verified): the fresh official-deb download and
  member-hash corroboration from the provenance and GO review (deb
  `deaf622c...`, same three member rows MATCH on a second route there). A
  third download was not needed for this scope; no bytes were executed or
  installed anywhere in this chain.

## Publication eligibility and minimum next grant/wiring

- Eligible: publication of exactly these two Python files plus the evidence
  note as dormant source, through normal exact-head review and all required
  gates. This closes nothing by itself.
- NOT eligible: any invocation of `_read_linux`, any host qualification or
  `qualified` claim, any fixture admission, and any A7/A11/M2/full F/G/
  real-Runner PA6 credit. Actual reads, the four-mask in-unit view, and native
  NNP oracles (case0-allow/case1-deny) remain open and ungranted.
- Minimum next readonly grant/wiring (fresh narrow review required, not
  granted here): explicit one-shot invocation of the private entry on a
  disposable host only, fixed to the 6 root-down directories, 3 files, and 2
  named links with the stated flags and bounds; read-only ports only - no
  `sudo -V` execution, no policy/account/proc/sys/network surface, no mapping
  or file outside the pinned set, no retained privilege. Wiring must still add
  the D9b in-unit four-mask view and fixture-literal checks before any Runner
  case.

A7/A11/M2/full F/G/real-Runner PA6 remain **unverified on real hardware**. No
gate, execution, or native grant changes; any comparison wiring still needs
independent review, exact-head gates, and coordinator merge.
