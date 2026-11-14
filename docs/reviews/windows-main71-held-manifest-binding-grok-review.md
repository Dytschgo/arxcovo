# Windows main71 native replay: held manifest binding review

Disposition: the Sol high held-manifest slice is **acceptable as source**.
`Admission.Bind` reads a fixed disposition and a fixed manifest through
`IAdmissionFiles.ReadPinned`, checks the externally supplied disposition
SHA256, and binds held source bytes to Git blob ids, provenance hashes, and
the three lock/selector aliases. The final refusal remains. **Native run
readiness is NOT CLEARED.** This review grants no publication, PR, CI
admission, registration, launch, or selected-environment provenance.

Reviewer: xAI / grok-4.7, one pass, 2026-11-14. The assignment forbids
subagents, so Standards and Spec/security are this single reviewer.
Independence is from the Sol implementation, not from a second reviewer.
Label: single-agent review.

M2 is **incomplete**. The cause of run `43545244266` attempt 1 is **unknown**.
No historical retry was issued. Phase 1 still has no focused red loop that
reaches that failure. Real-Runner PA6 record/replay remains **OPEN**. Native,
installed, and dedicated paths are **unverified on real hardware**.

## Pins

Porcelain was empty on entry and empty after the pure checks. The review cwd
equaled the linked worktree top level. The Git directory is that worktree's
directory under the common Git directory. Paths below are repository-relative.
No owner path, account, or machine path is recorded.

| Item | Identity |
| --- | --- |
| Review branch | `t3/held-manifest-review` |
| Reviewed head | `a449bb96d9c2a6af9512817e1519ab5da4dff1c8` |
| Head tree | `920b6bb8c6a2a0a1efdf2250809708e9763fc099` |
| Head after evidence | `a449bb96` changes `docs/PROGRESS.md` only |
| Supplied source | `06a257ee57e0fc604ac5dc69ba4665565ec24dde` |
| Retained source | `b4ea46ec816bac315508714e13a625d04e275b09` |
| Source tree | `5674e9dd621a52c2fb312d82f10ac743fda9043b` (both commits) |
| Source parent | `7dc9134600e4a58a845b4da30a86b7a5ba56115b` |
| Supplied evidence | `7b2828e747e5d8fb87a0656df0ca89fe31af9e9a` |
| Retained evidence | `d71bdcd7fd64ba77989f42fcd59bac3c04b8eefd` |
| Evidence tree | `f7ed9d3381134628b813bd03a2edefe6871d872d` (both commits) |
| Integration review | `984d345c71ea91dfce069c2000da2caa1f8556d4`, retained `620b3be91ee90dc957b8e03b6f0b53177b6e77a0`, same tree `f4f2751ffe109f2a4ba3d0cc3e9ea7525baee3a0` |
| Exclusive-name review | `852b9b133b70739f3c9c6bf75df431ea09413f91`, retained `66018b230c50b6b56743377e9907e582be6c5e81`, same tree `350ed2714e7deaa67f8995818a53f230581c93dd` |
| Accepted proposal blob | `366876f711595c14bde42763e2a263cfe44ca9c2`, unchanged |
| Source diff | `git diff 7dc9134600e4a58a845b4da30a86b7a5ba56115b b4ea46ec816bac315508714e13a625d04e275b09` — 5 files, 350 insertions, 42 deletions |

`git diff 06a257ee b4ea46ec` and `git diff 7b2828e7 d71bdcd7` are empty. The
supplied commits are not ancestors of this head. Each retained commit has the
same tree. `git diff --check` on the source diff reported no whitespace
errors. `crates/`, `HeldFiles.cs`, `NativeJob.cs`, `Prepare-Provenance.py`,
`Invoke-BoundedReplay.ps1`, and `selectors.json` are unchanged from the source
parent. Every changed file has CR count 0.

| File | LF SHA256 | Git blob |
| --- | --- | --- |
| `tools/windows-native-replay/Admission.cs` | `b6c1aa6b39d618a85e2183988cc95b44c965dfbb44670b2c2c4382d336754289` | `f26127281e627d3cca76312846a18243206cd063` |
| `tools/windows-native-replay/Controller.cs` | `0941fa6fb8add01470e87e7084d753acc8b967ffd76c2f0bc621bee24cc1075a` | `77080352f9061522de94efca18cb9137371203e0` |
| `tools/windows-native-replay/Check-Source.py` | `b3e9075a8dceed2c38ae4d6a032322f3d8dd1c77c7fbfa48200415593e3cef18` | `f90717b6331a786fd108446d417359a50a0284ca` |
| `tools/windows-native-replay/PINNING.md` | `cc0472cf5d1e82b2280a9915a04802a5b72876bcfa19340e179bac4354890bfa` | `630581e2c7269608ff5c66d3b454a18586ac60ab` |
| `tools/windows-native-replay/Test-PureController.ps1` | `2e9b07ff121769509e65797c285f262cb255938d0a4f17b643b900c46d17cb30` | `df6e4c8ce9cb8a846c4d37f72ab46be319e7bb1c` |

`HeldFiles.cs` remains blob `9526c04b368664ee23e6a6efbb27e554cf5b10b9`, LF
SHA256 `9470b8f11a37ad22ecc7eb28926de444d0c8d5cd6e25e64b9e0131459ada1643`.
These seven rows match the evidence table. This pass recomputed them from the
file bytes with SHA256 and with SHA1 over `blob {length}\0` plus those bytes.

The evidence file at the source commit is 64796 bytes, SHA256
`096747fd5fcccc0f2db43326f6985d4ea8d2188f9cde78774a3770a02e2fb767`. The current
file begins with those exact bytes. Offset 39689 is `0x96`. The evidence
commit appends 16522 bytes. This pass did not rewrite that prefix.

## Prior blocker disposition

| Prior item | This tree | Credit |
| --- | --- | --- |
| Held manifest read | `Bind` reads fixed `run-disposition.json` and `run-manifest.json` through `ReadPinned`, with a 65536-byte cap | Source interface accepted. `HeldFiles` handle behavior remains UNRUN |
| External trust hash | Constructor and `Bind` take the disposition SHA256 as an argument. Neither closed schema contains that hash | Accepted as coordinator input. It is not a signature and not execution permission |
| Git blob | `GitBlob` hashes ASCII `blob {length}\0` plus the held bytes and compares that id with the manifest and the disposition | Accepted for the pure oracle. No Git process and no commit-object check |
| Provenance | `source_sha256` covers `Sources`, `runtime_authority` is false, `native_executed` is false, helper hash matches the held helper | Accepted as a document check of held bytes. The provenance utility was not run |
| Lock aliases | `workspace_lock`, `helper_lock`, and `selectors` must match the corresponding source role in path, id, size, SHA256, and bytes | Accepted |
| `id128` order | `IdText` prints the first qword, then the second. The pure oracle fixes that order | Internal consistency accepted. Kernel id layout remains UNRUN |
| Caller `cleanCheckout` / blob map | Both parameters are gone. `Check-Source.py` asserts their absence | Accepted |
| Final refusal, `GATE2_ONLY`, selectors, `OwnedSession` | Throw remains. Entry still throws. `registration_enabled` is false. No `new OwnedSession(` construction | Preserved |
| SP-TRANSPORT, SP-GRAMMAR, SP-DEADLINE | Unchanged open gates | Not this slice and not closed |
| SP-EXCLUDE, SP-SYSROOT | `HeldFiles.cs` unchanged. `RevalidatePins` still scans both directories and calls `SystemRootSeparate` | Prior source acceptance stands. Kernel behavior remains UNRUN |

No three-attempt handoff trigger was present in `984d345` or `852b9b1`, and
none is present here. The evidence ledger's four managed corrections are four
different defects: a duplicate local name, a seal that rewrote a bad size, a
missing per-mutant type rename, and one hash mutant that the length check
already refused. Each has one recorded correction. They are not three failed
repairs of one production defect.

## Standards

**0 hard findings.** Baseline applied. One judgement, not a blocker.

Repo rules that hold: one conventional preparation commit for the behavior;
meaningful pure checks for the new binder, aliases, provenance fields, id
order, and Git framing; no credentials or owner paths; no product approval
flow; no workflow, ADR, root Cargo, or Rust edit. `Check-Source.py` remains a
string inventory and is not the only check of the new decisions. The pure
runner refuses a CR byte in its own sources and checks exact LF SHA256 of the
five compiled inputs before and after the mutants. Mutants are in-memory
copies with renamed types. A compiler error does not count as a kill. This
pass did not run the README Cargo, Clippy, or full test commands. The
assignment forbids those runs, and the diff does not change Rust.

Fowler baseline:

- **Divergent Change**, judgement only. `Controller.cs` still holds the wire
  decoder, the session types, and the pure suite. This slice adds
  `BindingPureChecks` there. The runner compiles that file as the entry, so
  splitting the oracle would fight the current harness. Not a hard structure
  violation.

`PINNING.md`'s early "Remaining integration blocker" paragraph still says no
sealed admission object binds the held manifest. The new "Held manifest
binding follow-up" section describes `Bind` and says the old caller-byte path
is superseded. The follow-up matches the code. The earlier sentence is stale.
It under-claims the source work. It does not claim a native run.

## Spec

**0 findings.** Checked against the exclusive-name review's held-manifest
slice, `PINNING.md` items 1 and 5's boundary, and the questions in this
assignment. Later gates stay open on purpose. Source binding is not native
proof and is not a selected-environment provenance result.

### Held disposition and manifest

`Bind` requires an absolute root, a 64-character lowercase SHA256, and a
nonzero 32-character lowercase id. It reads
`{root}\tools\windows-native-replay\run-disposition.json` through
`IAdmissionFiles.ReadPinned` with limit 65536, then requires a nonempty buffer
of at most 65536 bytes whose SHA256 equals the argument. That argument is not
a field of either document. `Keys` rejects an extra property, so neither
document can carry its own trust hash.

The disposition's closed schema is `schema` 1, `runtime_authority` false,
`reviewed_commit` (40 lowercase hex), `root` equal to the caller root,
`manifest` (`sha256`, `id128`, `size`), and `source_blobs` with exactly the
`Sources` paths. Manifest `size` must be 1..65536. `Read` then loads the fixed
`run-manifest.json` path at that hash, exact size, and id. `Read` refuses a
null buffer, a length other than `size`, or a SHA256 mismatch. `Parse` repeats
the manifest hash check at the 65536 cap and requires `runtime_authority`
false. Its external hash is the disposition's manifest hash, not a property
inside the manifest. A self-hash field still fails the closed key set.

`Parse` remains a schema function. Manifest schema tests call it with bytes
they just serialized. The admission constructor has no byte, blob-map, or
`cleanCheckout` overload. `Bind` is the method the constructor and
`Revalidate` call. There is no `new Admission(` in this tree, so that
constructor is unexecuted source. The pure binder calls the same `Bind`.

### Git blob, provenance, and aliases

For every source role, `Bind` requires the disposition blob id to be 40
lowercase hex, equal to the manifest blob id, and equal to `GitBlob` of the
bytes just read for that role. `GitBlob` feeds ASCII `blob {decimal-length}\0`
and then those bytes to SHA1, using invariant decimal digits. An independent
SHA1 of `blob 6\0` plus `hello\n` is
`ce013625030ba8dba906f756967f9e9ca394464a`, the constant the pure oracle
requires. The binder does not invoke Git and does not parse a commit object.
The disposition's `source_blobs` map is the coordinator's attested mapping,
checked against held bytes.

Every pin's held buffer is hashed again at its exact size. Source pins are at
most 2 MiB and must contain no CR. Provenance pins are at most 65536 bytes.
The provenance object is closed: schema 1, `source_commit_claim` equal to the
disposition commit, `runtime_authority` false, `native_executed` false,
`source_sha256` exactly the `Sources` keys, and `helper_sha256`. Each source
hash and the helper hash are SHA256 of the same held buffers used for the Git
id. `Prepare-Provenance.py` was not executed. Its document shape is compatible
with this checker. Compatibility is not a run of that tool.

`Alias` requires `workspace_lock` and `source:Cargo.lock`, `helper_lock` and
`source:tools/windows-native-replay/child/Cargo.lock`, and `selectors` and
`source:tools/windows-native-replay/selectors.json` to share path, id, size,
SHA256, and bytes. Any other case-insensitive shared path is refused. The
three allowed pairs are the only path aliases.

### Identity, copies, and preserved gates

`IdText` is the first qword's 16 lowercase hex digits followed by the second.
The pure oracle requires `IdText(0x0123456789abcdef, 0xfedcba9876543210)` to
equal `0123456789abcdeffedcba9876543210`. Directory records still read those
qwords at offsets 72 and 80, and `FileIdInfo` still stores them in that order.
Pin ids must be 32 lowercase hex and not all zeros. This fixes the formatter
the source already uses. It is not a native identity query.

`Facts` stores a cloned pin list and a readonly commit string. `CopyPins`
returns another clone. The pure check clears a returned list and requires the
next copy to still have `Sources.Length + 8` pins. That kills a return of the
live list. The clone copies role, path, sha256, id, blob, and size into new
pin objects.

`RevalidatePins` still requires separate helper and image directories, two
`ExclusiveNames` scans, `SameScan`, the leaf id, `SystemRootSeparate`, and the
source CR check. `HeldFiles` still has the relative open, reparse, type,
link-count, and post-hash identity checks. Those files and mutants were not
rewritten. `Revalidate` binds, then `RevalidateBoundary` runs the pin checks
and throws `remaining admission gates unproved`. The second pin pass, used by
the older directory fixtures, checks length and does not itself hash. The
throw is still unconditional, and `Bind` has already hashed on the constructor
path.

`Invoke-BoundedReplay.ps1` still throws `GATE2_ONLY` before `Add-Type`.
`selectors.json` still has `registration_enabled` false. `Check-Source.py`
reports 28 oracle rows, five pure Rust registrations, and zero native
registrations or callers. `OwnedSession.Run` remains in the file above
`public static void Run()`. A search found no `new OwnedSession(`,
`new Admission(`, or `new HeldFiles(` construction. The pure `Run` does not
call `OwnedSession.Run`.

The 17 new mutants cover the disposition hash, the shared held-read hash, Git
bytes, the reviewed blob id, the Git header, provenance source, commit, and
helper hashes, all three authority flags, `native_executed`, alias path, lock
bytes, an unlisted role alias, qword order, and the facts copy. The direct
same-size `Admission.Read` assertion is what kills `binding-held-manifest`.
The older directory, path, link, type, and final-refusal mutants remain.

## Security

**0 findings reachable from the current entries.**

The PowerShell entry throws before compilation or a process API. Parameter
validation cannot enable execution. `PureControllerChecks.Run` does not
construct `Admission`, `HeldFiles`, `OwnedSession`, or `NativeJob`. The only
`Stopwatch.StartNew` is an instance field of `OwnedSession`, and that type is
not initialized by `Run`. `Path.GetFileName` and `Path.GetDirectoryName` on the
binder path are string splits. Compiling the files defines `DllImport` and
does not call it. `Bind` returning facts is a pure document result. It does
not select an environment, open a native handle, or authorize a launch.

Removing the final throw would still leave the dedicated transport, the
accepted command grammar, the deadline supervisor, and every kernel open
unproven. The entry would still throw, and no constructor of `OwnedSession`
would appear. That deletion would not be clearance.

No account, service, VM, owner profile, or Lab Manager state was inspected.

## Failure ledger

No three-attempt handoff trigger.

Historical main71 run `43545244266` stays a separate unknown cause. This
review issues no retry. The 62 assertion kills are negative controls, not
failed production fixes.

## Advisories

- The baseline paragraph in `PINNING.md` still denies a sealed manifest
  binder. The follow-up section is the one that matches `Bind`. A later docs
  edit can replace the stale sentence. It is not a source repair.
- The facts assertion checks the copy's count after `Clear`. It does not read
  the mutated `sha256` back from a second copy. The implementation clones pin
  fields, and the live-list mutant is killed.
- `RevalidatePins` without the content dictionary checks length and the source
  CR, then the boundary throws. Hashing of that second pass is not a separate
  success path while the throw remains.

## Next implementation slice

Sol high, one slice, no launch authority. Do not reopen this binding contract
unless a later authorized run disproves it. Do not implement the deadline
waiter, and do not treat the dormant libtest vector as accepted grammar.

Dedicated telemetry pipe, the prior SP-TRANSPORT slice:

- One inherited write handle that is neither stdout nor stderr.
- The parent reads it with `PeekNamedPipe` and a bounded `ReadFile`.
- Stdout and stderr are drained and discarded.
- `emit_frame` writes only that telemetry handle.
- Do not put the handle in the four-value environment block.
- The pure `frame()` oracle stays the byte oracle and must not call
  `WriteFile`.
- Do not register the filters.

Keep the entry on `GATE2_ONLY`, keep `registration_enabled` false, do not
construct `OwnedSession` from `Run`, and do not remove
`remaining admission gates unproved`. Environment selection, nested-Job proof,
budget measurement, hardware, the historical main71 cause, and PA6 stay open.

## Close

Standards: 0 hard findings. 1 judgement, divergent change in `Controller.cs`.
Spec: 0 findings. The slice binds held disposition, manifest, Git, provenance,
and lock bytes, then still refuses.
Security: 0 reachable findings. The worst dormant limit is that deleting the
final throw would still leave transport, grammar, deadlines, and unexecuted
native opens.
**Acceptable source may stay local. Native run readiness is NOT CLEARED.**

Performed: read-only Git inspection, LF SHA256 and Git-blob recomputation,
evidence-prefix comparison, static reading of the five-file delta and the pure
call graph, `Check-Source.py`, one managed pure run with 62 assertion mutants
whose stdout matched the evidence block, and this file's write and commit.
Not performed: PROGRESS, source, design, or evidence edits; native execution;
provenance preparation; launch; registration; publication.
