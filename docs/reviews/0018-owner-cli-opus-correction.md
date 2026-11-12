# ADR0018 owner CLI: Opus correction 1 and finding dispositions

Author: Anthropic / claude-opus-5-5 / high, design sub-agent, 2026-11-12. No
subagents. This is a docs-only correction. It is not implementation authority
and not acceptance. The corrected proposal **needs a fresh independent Sol
recheck** before any implementation. This is correction attempt 1 of at most 3.

## Exact scope

- Prepared root: `780ca4164494f6c3da00a340f4c4c67e3ac9d305`. Owner libraries
  there are context only. No runtime file changed, and no part of an older
  branch was ported.
- Original proposal: base `2f53332b4f4d3e1584f2efb80317856588576e3a`, head
  `ed208f27f01077a88993dc88feb8f01ce306b126`. Only
  `docs/adr/0018-owner-cli-wiring.md` and `docs/briefs/owner-cli-wiring.md`
  were copied, blob-equal, in the first commit of this branch. The proposal
  branch's `PROGRESS.md` delta and runtime were not merged. The status stays
  **proposed** with the original source pin. A correction note is added.
- Review: `6a6faae7a43831fa58a896a4b0cc722107ef9195`,
  [0018-owner-cli-sol-review.md](0018-owner-cli-sol-review.md), read in full
  and retained blob-equal. The ADR0019 review in that commit is out of scope.
  It was not copied, and ADR 0019 was not edited.
- Context read: the ADR 0009 owner-subcommands section and the ADR 0012 and
  0016 status lines. ADR 0014 and 0015 and the E1/E2 owner-session sources
  were not reread. The parts of the proposal that depend on them are
  unchanged, and the Sol review found them satisfied. These sources were read
  at the prepared root: `service/config.rs`
  (`parse`, `revalidate`, `validate`, `parse_inventory`,
  `execution_preflight`), `service/owner_plan.rs` (`configure_plan`,
  `uninstall_plan`), `service/layout.rs` error tokens, `security.rs`
  (`private`), `local.rs` (`ClientConfig::load`, client #39 boundary) and
  `tests/service.rs`.

## Dispositions

| Finding | Kind | Disposition | Where |
| --- | --- | --- | --- |
| A-SP1 hash before clearing | blocker | **Fixed.** The token text is encoded once. Its `sha256` digest is computed and kept immediately, before the JSON is built, and the JSON borrows that same text. A single drop owner clears the raw bytes, the token text and the JSON on every return after the RNG. Success returns the kept digest. The test uses deterministic nonzero RNG bytes and compares the returned hash with `sha256` of the token text read back from the file. Mutations: clear before hashing; hash the raw bytes. | ADR 0018 decision 7; brief §3 S2 steps 3 and 8, §7 test 9, mutations |
| A-SP2 partial boundary | blocker | **Fixed.** The successful exclusive create is the boundary. Every later failure (Unix `fchmod`, write, flush, Unix parent fsync, handle checks) returns `Partial`: no hash, no token, file left private, no rollback. Pre-create failures never mutate an existing object. Checks use the created handle only; there is no path-based reopen of the file. Unix mutations are relative to retained descriptors. Windows holds every ancestor handle without delete sharing. | ADR 0018 decision 7; brief §3 S2 steps 4–7 |
| A-ST1 post-create test coverage | blocker | **Fixed.** `create_with` gains an unprivileged deterministic seam `before: FnMut(PostCreate) -> Result<(), ()>`, called before every post-create step. Production passes `\|_\| Ok(())`; nothing selects anything else. The fault matrix covers every tag and asserts `Partial`, the tag order, a retained private file, the exact bytes written so far, an unchanged parent, the `client-config-partial` line with empty stdout, and exit 1. A real Unix verification failure uses a hard link. Mutations: partial mapped to `Io`; success after a fault or ignoring `Verify`; delete or delete-on-close; `fchmod` before create or on an existing target. No product approval is added. | brief §3 S2, §6, §7 test 9, mutations |
| A-SP3 replacement versus old-config revalidation | blocker | **Fixed by narrowing the claim.** The previous config still goes through the full `ServiceConfig::parse`, and `configure_plan` still calls `old.revalidate`. The library is unchanged. The counterexample (same pinned account, old workspace removed, new valid workspace) refuses with `service-workspace`, exit 3, and makes no mutation. Manual owner remedies: restore the old workspace and rerun, or uninstall, delete the config by hand, install and configure. Counting-resolver tests cover Windows and Unix. A mutation skips or weakens the old parse. `configure_plan` is not weakened, and no API or privilege behaviour changes. | ADR 0018 decision 4, deviations, gaps; brief §4, §7 test 5, mutations, §8 S3 docs |
| A-A1 "token unusable" overstatement | advisory | **Fixed.** The text now says the command supplied no registration hash and claims no usable completed credential. An owner could read the file and compute its hash; that is not an authorization fence. Success is defined precisely. The remedy is to delete the file by hand and rerun. | ADR 0018 decision 7; brief §8 S3 docs |
| A-A2 allow-only versus the helper's deny handling | advisory | **Fixed: the stricter allow-only rule.** For the parent and the created file, every ACE must be `ACCESS_ALLOWED_ACE_TYPE` for TokenUser, SYSTEM or BUILTIN\Administrators; deny ACEs and other types refuse. Anything this rule accepts, `security::private` accepts too, so `ClientConfig::load` still passes. The path-based helper is never used in production. Mutation: accept a deny ACE. | ADR 0018 decision 7, deviations; brief §3 S2 step 5 |

The review's implementation handoff also applies. The `tests/service.rs`
vector becomes `["service", "install", "extra"]` in the first S3 commit,
keeping today's usage expectation, before any owner routing exists. Every
test of a valid owner command uses fakes or `RecordingOps`, never
`NativeOps`. No test spawns a valid owner argv.

## Unchanged

Routing, the Windows guard on every verb, the inventory-only start preflight,
config-free uninstall, the exit table and the code-4 rule, the fixed limits,
the pin-change gap, the slices and the gates are as originally proposed. No
new product setting, approval, prompt, recovery verb, MCP or HTTP owner
entry, account creation, password or S4U path appears.

## Limits

Only Git object reads, text reads, text edits, link checks and `git diff
--check` ran. No cargo, build, test, native probe, process, pipe, system path,
account, SCM, LSA, privilege, service, VM, Lab Manager, S4U, ignored fixture or
private-state action occurred. No GitHub write, push, PR, rerun or merge
happened. The proposed tests and mutations were **not run**. Every native
path, including Windows `CREATE_NEW` with an explicit owner from an
unelevated token and a hard-link or DACL fixture with the handle held, is
**unverified on real hardware**. Gate 1 (hosted-green E1/E2) is unmet at the
prepared root. No milestone 2 acceptance is claimed.
