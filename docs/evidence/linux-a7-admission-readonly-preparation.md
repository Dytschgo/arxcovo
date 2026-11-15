# A7 read-only admission-chain preparation

2026-11-15; OpenAI gpt-6.1-sol, high; implementation sub-agent, no subagents.
Preparation only. Fresh independent Muse source/security and exact-head review
remain required. No runtime, publication, pin or dispatch authority was exercised.

## Baseline and binding correction

Local parent `fc4cedae3ef368b4c161b29244ca09121947f78a` atop main
`18b8fef58403e5ffadfd3f580d6a2a961c6d754c`; main tree
`215d75d3eeb09dd513846c73143f757454e6ce7a` equals reviewed
`291a08a6f4ca302eb35bd5fabe0238d0c735ab92` tree (locally verified).
PR130 independent review/all-five-green is supplied coordinator evidence, not
queried here. Parent differs from baseline only in coordinator PROGRESS and
the next-execution disposition; neither was edited by this builder.

Read AGENTS/KICKOFF, ADR0019 D7h/D7i consumed history, ADR0020 D9b, current
coordinator correction in `m2-a7-next-execution-disposition.md`, path-refusal
diagnosis/Muse review, failed run43715436515 record and hosted disposition.
Muse cfad96f retained2138752's original inference is corrected by fc4ceda:
mutable public recipes do not make controlled D9b fixtures impossible;
original-host policy reconstruction is not required. The historical code-only
E_PATH proves neither a specific ancestor nor a systematic cause. D7a–i remain
consumed, with no retries or reuse of their excluded hosts.

## Fixed implementation and call-graph audit

Before the first test execution, manually audited
`_main -> _gate -> _load_driver -> _module_bytes -> _diagnose ->
driver.a7_alias_c2 -> sudo_nnp_open/a4_directory`, plus fixed alias readlinkat
and optional `sudo_nnp_digest`. Gate precedes driver imports and source/root-down
reads. The loaded source hash/size is checked before compile; no search-path
import, path argument, selector or expected-hash override. Driver definitions
and standard-library imports have no adapter/native entry invocation.
Only a7_alias_c2 is invoked; a7_admission, package collector, cg_preflight,
staging, account/policy provisioning and main are never called. A source AST
test fixes that one driver entry. No shared driver/global selection changed.

The finite adapter admits exactly five directory paths and optional C2 file.
It stats each component without following links, emits fixed role and booleans
`owner_is_root`, `type_accepted`, `group_other_write_absent`, and refuses before
opening an unsafe component. No child traversal follows the first fault.
Held descriptors remain until final verification/closure; each open checks exact
flags, pre-stat/fstat and same-parent reopen identity. Parent-first verification
checks all held metadata and names; only atime is excluded. Fixed bounded /bin
readlinkat is the driver's existing operation; its accepted alias is compared
against held /usr/bin instead of following a mutable symlink again. This adapter
is local to the diagnosis module; original driver behavior is byte-identical.

Optional C2 must be regular, root-owned, single-link, no022, exactly282032 bytes;
the real driver digest logic is bounded by the adapter to six reads of65536,
and only the existing known sudo hash
`6ae6665cc51295ea8aaba94a3377dafa563f950aafc6296c0704fb33401a31ff`
can pass. Unknown digests never escape. Unsafe optional-file metadata is observed
as predicate E_PATH before the driver's later C2 check; this stricter local
diagnosis does not redefine production admission codes.

Output schema: validated source/workflow/run/attempt binding and a diagnosis
containing schema1, status read_only_diagnosis, fixed code, finite predicate rows,
qualified=false, native_credit=false. Codes are PATH_CHAIN_OBSERVED, E_PATH,
E_ALIAS, E_C2, E_ARTIFACT, E_OS or REFUSED. Gate/source/drift exceptions give
only constant `{"schema":1,"status":"refused"}`. Exit0 observes this chain only;
it grants no fixture admission. No raw IDs/modes/paths/inodes, unknown digest,
exception or traceback is printed. Closed roles: root, usr, usr_local,
usr_local_bin, usr_bin, c2_image. Root-protected dependencies/concurrent privileged
writers/mounts retain D8/A1's trust assumption; no whole-host attestation.

Manual-only workflow: ubuntu24.04, contents:read, 3min, pinned checkout with no
persisted credentials, mocked tests before fixed sudo/env-i isolated Python
transport. New A7_ADMISSION_REVIEWED_SHA is separate; both old pins must be empty
in workflow and wrapper. Runtime gate also enforces host/image/X64, root, argv,
run bounds/attempt1 and exact repo/actor/trigger/main/workflow/script binding.

## Exact source inventory

Canonical Git index bytes (regular100644), not text-decoded hashes:

| Path | Bytes | Git blob | SHA256 |
| --- | ---: | --- | --- |
| .github/workflows/a7-admission-readonly.yml | 2342 | 5568feb19d9391ace8c6434909d4f19e9a4e36e8 | 7878e894b9487183dd8d9cdc4788b0ee73ad100b75b5410617b22ab23b5bbc33 |
| tools/ci/a7-admission-readonly.py | 9172 | 4a355207ec8cabfbfe833398654e3c3ac75f56da | afb8987cb316bd370d7ab84fd0ab4414523416f243be478f524f95409ab6f029 |
| tools/ci/test-a7-admission-readonly.py | 17064 | e70649566ce39d0840df85696c6479d3ff1586ff | d5ee530c1f537be1b907eb0dca4ad764ef8f13c7b40892f9328187f6698a770b |
| tools/unix-exec-validation/driver.py (unchanged) | 82576 | 4974515d777fa63b64532afded20d9de8cc3b6ee | 43518beda2536f9ee4c39ffc158fe6fe56921aa08d31fa0f4aced26cd6f0f75e |

All other executable source bytes/modes equal the baseline recursive tree above;
the strict D7j allowlist in ADR0019 admits only the three new source/test files,
ADR, this evidence, inherited disposition, retained fresh Muse review and
coordinator-only PROGRESS. No deletion/rename/type/mode change is admissible.
Source inventory includes the exact unchanged full baseline tree as the complement
of that allowlist, rather than an unbounded host inventory. Reviewer/coordinator
must append actual reviewed head/tree and merged main/tree later, require full
tree equality and revalidate this inventory; no future SHA is invented here.

## Checks actually run

`python -I -B tools/ci/test-a7-admission-readonly.py -v`: final13 tests PASS,
1.689s. Tests use only repository source reads plus mocked OS/ctypes/digest ports;
real alias/traversal/digest logic is exercised where stated. All descriptors are
asserted closed. Coverage includes every gate field before import/read, SHA
binding and old pins, UID/argv/run/attempt/workspace bounds, every directory's
owner/type/group-write/other-write/symlink fault and no unsafe opens, fixed path
allowlist, flag checks, alias mismatch, C2 absent/known mocked match/unknown hash,
digest bounds, all nine non-atime stat fields refusing drift, atime passing,
loader size/type/link/hash/extra-byte failures, closed output/privacy and workflow
test-before-privilege transport. Seven in-memory mutants are assertion-killed
with zero test errors: root owner, write bits, first-fault stop, exact flags,
environment gate, SHA equality and source hash fence. No on-disk source mutation.

First invocation failed during import because Windows lacks os.O_NONBLOCK;
Linux constant fallback for the isolated port fixed mock portability. No Linux
gate or host fence was relaxed. Nine tests passed before expanded coverage;
final13 passed after additions. `git diff --check` and staged diff check passed.
Recursive baseline-to-index audit passed: exactly seven allowed paths (five
builder files plus the two inherited coordinator docs), all regular100644,
no deletions/mode changes; every complement blob including driver is unchanged.
No Cargo/build, temporary
build output or cache deletion. No real root/host/adapter/policy/account read,
network, sudo/native execution, variable, dispatch, push, PR, merge, watch or
owner-admin operation occurred. No old Lab Manager state was touched.

## Remaining coordinator gates

D7j is a new inactive proposal, never a renewal of D7a–i. Fresh Muse source/security
review and different-provider exact-head review, all five green required checks,
coordinator merge, complete exact inventory and reviewed/main tree equality precede
the first eligible main squash's once-only pin/dispatch. Read back main immediately
beforehand; require unchanged exact candidate. Independently verify all three live
pins absent before once-setting the new pin. Capture returned accepted URL, monitor
that exact run through terminal, then finally clear and independently verify404;
any accepted dispatch consumes including skip/failure. No retry, host excluded.
No checks/review/merge/live pin absence are claimed by this preparation.

This diagnoses one fresh host's role/predicate only. A later D9b combined attempt
still needs its own reviewed disposition on a NEW host and all actual NNP/oracle/
runtime checks. A7/A11, M2, full F/G and real-Runner PA6 remain
**unverified on real hardware**.
