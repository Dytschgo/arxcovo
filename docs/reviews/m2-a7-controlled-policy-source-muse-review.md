# M2 A7 controlled-policy source review - Muse (independent source/security)

Scope: base `9398f72` to head `0648228` (`099a428` fixture + `0648228` progress-only).
Six files, 843 insertions, 9 deletions: [sudo_nnp.rs](/crates/arxd/src/exec/disposable/sudo_nnp.rs),
[driver.py](/tools/unix-exec-validation/driver.py), [test_driver.py](/tools/unix-exec-validation/test_driver.py),
[mutation_check.py](/tools/unix-exec-validation/mutation_check.py),
[linux-a7-controlled-policy-source.md](/docs/evidence/linux-a7-controlled-policy-source.md), plus one
PROGRESS line. Read AGENTS.md, KICKOFF, ADR0020 D9b, decision `dec7f9b` with the case1
transcription correction, the Sol independent review, and the delivery contract with its four
binding pins before inspecting the diff. No implementation, build, dispatch, account, sudo,
probe, push, or native operation ran. A7/A11, M2, full F/G and real-Runner PA6 remain
**unverified on real hardware**.

## Standards - PASS, no findings

- One-concern dormant source slice plus its evidence note; small conventional commits, UTF-8 LF,
  `git diff --check` clean (re-verified here).
- No product approval flow, publishing default, checklist, or audit framework added. The added
  checks are fixed technical fences (exact-set mapping/literal/entry admission, fail-closed
  refusals), proportionate to the user steering, not a new ceremony.
- Smell baseline: no new abstraction to flag. `controlled_literals` / `controlled_mappings` /
  `controlled_pin` / `stage` / `recheck` / `cleanup` mirror the existing owned-lifecycle
  vocabulary; the generator split (`sudo_nnp_accounts` shared, legacy `prepare_sudo_nnp` wrapper)
  removes the old dual-use function rather than adding a parallel one. No speculative
  generality: the plugin path constant is marked candidate-only/UNATTESTED, not wired anywhere.

## Spec - CONDITIONAL PASS (one integration-form finding, fail-closed; no source change required now)

Verified closed before effects: actual `sudo_nnp_host_attested() == False`
([driver.py](/tools/unix-exec-validation/driver.py):173), `sudo_nnp_eligible` requires `True/True`
(:177), both controller profile admissions remain `None`
([sudo_nnp.rs](/crates/arxd/src/exec/disposable/sudo_nnp.rs):14 and :800, `expected_profile?`
returns `None`), TEST stays the closed A4 omitted-kill literal
([driver.py](/tools/unix-exec-validation/driver.py):14) inside the closed selector set (:25-30),
and `run_body` (:1513) has no edge to any `controlled_*`/`prepare_controlled_*` function
(A4 helper/delivery path only). The dormant `disposable_linux_a7_a11` test stays `#[ignore]`
with an explicit unattested/no-grant reason (:1090).

Verified fixture closure: exactly the four D9b literal mappings, no optional source prefix or
extra mapping (`controlled_mapping_set` requires set equality of 4, :355; override renders
exactly 4 `BindReadOnlyPaths=` lines, :360). In-unit admission is actual view checks, not config
text: systemd mapping property (bounded 4096 bytes / 5 s), bounded mountinfo with exact
read-only target mounts including `statvfs`, source/view inode equality, root ownership/modes,
literal bytes and full include/entry sets, plus bounded libc NSS lookups for both pinned
accounts — executed before `Runner` construction and before EACH `run_phased` call
(:1127 initial, :1136 probe, :1151 reentry). Case0 alone carries `NOPASSWD: /usr/bin/true`;
case1 has no grant (literals keyed on `names[0]` only; test asserts the case1 name appears in no
literal). The plugin path is an explicit future candidate only, never self-proof (comments and
evidence state installed digest/default-path/floor proof UNATTESTED; no host hash invented).

Verified legacy isolation: the controlled preparer never reaches the host `write` path. The
AST test walks the full transitive call set of `prepare_controlled_sudo_nnp` and asserts no
`Path('/etc`, no `visudo`, and that neither `prepare_sudo_nnp` nor `run_body` is reachable;
the poison test forces the legacy writer to raise and asserts the controlled path still stages
two policy-`None` accounts without calling it (both re-ran green here).

Verified staging discipline: root-owned exclusive `0700` dirs / `0440` files, `O_EXCL |
O_NOFOLLOW | O_CLOEXEC`, `nlink == 1`, SHA256 over `<= 4096` bytes, held-descriptor pin plus
re-opened dev/ino recheck, full entry-set recheck, partial/uncaptured creation quarantines
everything (`cleanup_controlled_sudo_nnp` rechecks first and requires `unit_gone is True`;
reversed removal order files-before-dirs). No adoption, no guessed ownership, no foreign
deletion. Artifact `OwnerConfig`/`RunSpec` field pins, NNP/descriptor/identity/cleanup
discriminators and bounds are untouched (diff is purely additive outside the reviewed
delivery block).

Finding S1 (integration form mismatch, fail-closed, blocks activation only): the two sides
pin DIFFERENT mapping normal forms. Driver `controlled_mappings` renders
`<stage>/fx/<n>:/etc/<n>` (no suffix); Rust `controlled_mappings` expects
`/var/lib/arx-ci-{run}/fx/{n}:/etc/{n}:rbind` (mandatory `:rbind` suffix). Each side's tests
pass against its own form, so the pair can never jointly accept the same systemd echo unless
the actual `systemctl show --property=BindReadOnlyPaths --value` bytes happen to carry exactly
`:rbind`. Worse, the token shape itself is unattested: `controlled_view` splits the property
value on whitespace and matches whole tokens, but the real echo format (one `src:dst:opts`
token per mapping vs. space-separated fields) has never been observed. Either way admission
refuses rather than widens (safe direction), but the in-unit mapping check cannot succeed until
this is pinned. Minimal fix: no source change now (dormant, fail-closed). Under the SEPARATE
fresh-host grant, capture the exact `--value` bytes first, then pin one identical normal form
in `controlled_mappings` (driver), `controlled_mappings` (Rust), and both test suites.
Do not "accept optional suffixes" as a shortcut: that reintroduces the optional-mapping hole
D9b forbids.

Note S2 (strictness, fail-closed): Rust `controlled_mounts` refuses on ANY `/etc/*` mountpoint
outside the four-set and on any malformed line. Real hosts often carry extra `/etc` mounts
(`resolv.conf`, `hostname`); those will refuse admission rather than bypass it. No change now;
the fresh-host grant must record actual mountinfo so the closed set is confronted with reality,
not assumed.

Note S3 (robustness, fail-closed): `controlled_pin` does a single `os.read(fd, 4097)` plus one
confirming read; a short first read on a legal file would false-refuse with `E_POLICY_BOUND`.
Safe direction; a read-loop is optional later, not a publication blocker.

## Security - PASS WITH HELD CONSTRAINTS (no new exposure)

- Fail-closed posture retained everywhere: `False` host latch, `None` profiles, closed TEST,
  `prepared_delivery() -> None`, owned-only cleanup with reservation/quarantine on partial
  capture, changed inode, or uncertain stop. No per-run sudo, no in-service `/etc` mutation,
  no installation/relaxation, no policy inference, no list-mode oracle (rejected route stays
  rejected). Legacy dormant host-writer keeps its `visudo -c` syntax-only check and is
  unreachable from the controlled path (AST + poison-mutant proof).
- Trust boundary honest: staged bytes bind to reviewed source literals; installed C1 ELF/plugin
  digest, compiled-default-path equality, floor, and per-host pins are explicitly deferred to
  the fresh-host grant, not claimed. Bounded output discipline holds (`<= 4096` pin reads,
  `<= 1 MiB` mountinfo cap, `4096`-byte property cap, 5 s deadlines with kill-and-reap on the
  systemctl child).
- Linux-only integration (`systemctl` echo shape, mountinfo field positions, `statvfs`
  `ST_RDONLY`, `getpwnam_r`/`getgrnam_r`/`getgrouplist` buffer discipline, `/proc/self/fd`
  directory listing) is type- and logic-inspected but UNCOMPILED locally; I found no
  memory-unsafety beyond the usual audited `unsafe` (TOCTOU between recheck and use is closed
  by re-open + dev/ino + mode/owner revalidation, and the load-bearing assumption is stated:
  only root-writable ancestry). S1 above is the one real open item and it fails closed.

## Publication eligibility and check ledger

- Eligible now: docs-only publication of this dormant source through normal exact-head review
  and green required gates. No TEST change, no selection, no native execution.
- Not eligible: any A7/A11 acceptance, M2/full-F/G/PA6 credit, native grant, or host-policy
  claim. Those need the S1 observation, installed plugin/default-path/floor attestation, and
  the separately bounded fresh-host grant with both cases, pins, one dispatch, and
  cleanup/variable lifecycle.
- Real checks I ran (not reported): `ControlledPolicy` 9/9 green and `SudoNnp` 15/15 green via
  `python -I -m unittest discover` (mocked effects only); the four source blob hashes recomputed
  and matching the evidence table (`7dbb9a9`, `9738eaa`, `850ebbe`, `fbac127`);
  `git diff --check` clean; all five files LF-only.
- Reported by the builder, NOT rerun here: full 145-test suite, 22 portable E4 tests, service
  artifact/inventory checks, 6 Python + 5 Rust controlled mutants, `cargo fmt --check`, offline
  Windows Clippy. No Linux compilation, host read, `/proc` read, helper, ignored-test, sudo,
  account, network, install, dispatch, variable, push, PR, or subagent action ran from this
  review.

Findings: Standards 0; Spec 1 finding (S1 mapping normal-form mismatch, fail-closed) + 2
fail-closed notes (S2/S3); Security 0 new issues. Worst item is S1: it cannot grant anything
today, but activation is impossible until the fresh-host echo bytes pin one shared form.
A7/A11/M2/full-F/G/PA6 remain **unverified on real hardware**.
