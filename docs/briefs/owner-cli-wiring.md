# Brief: owner CLI wiring (ADR 0012 PR-D, ADR 0018)

Status: **accepted contract; implementation remains gated by §0**.
Original source pin: `2f53332b4f4d3e1584f2efb80317856588576e3a`.
The retained Sol reviews cover the Opus proposal/first correction, and the
Opus correction-2 recheck covers the Sol correction at d11dcdc. Coordinator
disposition 29068df accepts the core contract, with every implementation gate
retained. The N1–N5 clarification passed independent Opus review at dfcb643.
The small follow-up advisories are included for published-head recheck before
implementation. No Rust was built or behavior test run for this document.

Historical correction checkpoint:
Correction 1 (Opus 5.5 high), 2026-11-12, on prepared root `780ca41`. It
answers the Sol review
[0018-owner-cli-sol-review.md](../reviews/0018-owner-cli-sol-review.md).
Sections 3 (S2), 4, 6, 7 and 8 change for A-SP1, A-SP2, A-SP3, A-ST1, A-A1
and A-A2 ([dispositions](../reviews/0018-owner-cli-opus-correction.md)). The
status is still **proposed** and needs a fresh independent Sol recheck. The
scope is unchanged.

## 0. Gate before any code

Implementation starts only when all of these hold:

1. E1, E2, N3, client and artifact libraries are published and their
   hosted required gates pass on the actual integrated revisions. At this
   refresh E1 #40, N3 #41, client #39, artifact #43 and E2 #46 are merged
   through all required gates. E2's explicit merge disposition retains the
   open baseline intermittency and claims no native/hardware acceptance.
2. The original four-PR queue has drained (#9, #26, #34 and #36 merged).
   This does not authorize bypassing any later queue or review gate.
3. State-permission exit correction #45 is merged as 2b805c4, retaining the
   code-4 rule in section 5. No duplicate classifier is required.
4. The core contract reviews and the advisory clarification recheck are
   accepted, with their cross-provider scope attributed correctly.
5. This contract PR47 is merged after its exact-head independent review and
   all five required checks (d9b6fa3 -> d70e808). Every implementation slice
   still needs its own independent review and green hosted gates.

E2's original ExecutionFailed remains open. Publication and local
passes are not a fix, waiver or implementation permission. No valid owner
command or production NativeOps may run during implementation tests.

## 1. Settled sources (do not reopen)

- ADR 0009: subcommands, fixed layout, config schema, `client-config new`.
- ADR 0012: owner record, recovery, `RightReport` tokens, SCM boot-intent
  amendment.
- ADR 0014: Unix plans, gates, typed query, status mapping, install convergence.
- ADR 0015: macOS endpoint and ancestors.
- ADR 0016: `OwnerGuard`, guard-first install with registration last, session
  pins, proof-gated uninstall.
- [SCM brief §13](windows-scm-adapter.md#13-pr-d-prerequisites-not-part-of-pr-c).
- [docs/service-install.md](../service-install.md): the owner command examples
  are the source of the flag names.

## 2. Grammar (exact; `args_os().skip(1)`)

| argv after the program name | route |
| --- | --- |
| `service run` (exactly two) | `service::run()`, unchanged, before Tokio |
| `service install` / `start` / `stop` / `status` / `uninstall` (exactly two) | owner verb |
| `service configure` + flag pairs | owner configure |
| `client-config new --principal P --out PATH` (either flag order, exactly six) | client credential |
| any other argv starting with `service` or `client-config` | `arxd: usage`, exit 2 |
| anything else | the existing foreground Tokio path, unchanged |

Configure flags:

| flag | count | value |
| --- | --- | --- |
| `--build-account` | exactly 1 | account name (resolved, then pinned) |
| `--client-account` | exactly 1 | account name or identity (resolved) |
| `--rights` | exactly 1 | `standard` or `admin`, lowercase only |
| `--workspace` | exactly 1 | path, passed verbatim (never canonicalized) |
| `--env` | 0–16 | `KEY=VALUE`, split at the first `=`; the key must be nonempty |
| `--agent` | 0–64 | `PRINCIPAL=SHA256`, split at the first `=`; both parts nonempty |

Parser rules. A violation is usage (exit 2), with no native call:

- Check the counts before converting anything. More than 170 arguments, any
  single argument over 4200 bytes (`OsStr::len`), or more than 32 KiB in total
  is refused.
- Every argument must convert with `to_str`; non-UTF-8 is refused.
- Flags and values are separate tokens. Refuse `--flag=value`, short flags,
  abbreviations, `--`, `--help` and unknown flags. Matching is case-sensitive.
- A value may not be empty or start with `--`.
- A missing or duplicated single-occurrence flag is refused. Flag order is
  free. Repeated `--env` and `--agent` values are kept in a `Vec` in input
  order and are never deduplicated by the parser.
- Semantic checks belong to the library, not the parser, and exit 3: the
  allowed `--env` keys, a duplicate env key, at least one agent, principal
  syntax, hash syntax and uniqueness.
- Parsed types (`ConfigureArgs`, `ClientArgs`, `Command`) have no
  `Debug`/`Display` and are never formatted. No `clap` and no new crate.

## 3. Library additions (exact)

**S1. `service::config`:**

```rust
pub const DEFAULT_LIMITS: (u8, u64, u32) = (1, 8_589_934_592, 512);
pub(crate) fn valid_principal(p: &str) -> bool; // extracted from validate_inventory; same rule
pub struct ConfigureRequest<'a> {                // no Debug/Clone
    pub build_account: &'a str, pub client_account: &'a str, pub rights: Rights,
    pub workspace: &'a str, pub environment: &'a [(String, String)],
    pub agents: &'a [(String, String)],
}
impl ServiceConfig {
    pub fn from_request(r: &ConfigureRequest<'_>, l: &Layout, res: &impl Resolver) -> Result<Self, Error>;
}
```

`from_request` runs in this order:

1. macOS with `admin` → `Rights`, before any resolver call.
2. Duplicate env key → `Config`.
3. `res.account(build_account)` must be `User`, else `Identity`; errors pass
   through, so `AccountMissing` stays `AccountMissing`.
4. `res.account(client_account)` must be `User`.
5. Build the `Wire`: schema `arxd-service/1`; client identity; build `{name:
   build_account verbatim, identity, gid}`; rights; workspace verbatim;
   environment; limits from `DEFAULT_LIMITS`; agents in input order;
   `granted_batch_right: false`.
6. `serde_json::to_vec` → `ServiceConfig::parse(&bytes, l, res)`.

There is no other constructor. `configure_plan` then sets the mirror.

**S1. `service::owner_unix`:**

- `pub fn require_owner() -> Result<(), Error>`: the existing `identity()`
  check, giving `OwnerRequired`.
- `pub fn read_config() -> Result<Option<Vec<u8>>, Error>`: `Manager::new()`,
  then the pinned no-follow parent, `open_at`, absent → `None`, then
  `bytes(f, 0o600, MAX_CONFIG_BYTES)` and the `same` check.

**S1. `service::native` (Windows):**

- Split `state_guard` into `state_members(layout)` (identical checks, no
  identity requirement) and `state_guard` = `require_service_identity()` +
  `state_members`. Service behaviour is unchanged.
- Add `pub(crate) fn owner_start_preflight(l: &Layout) -> Result<(), Error>`:
  `observe_layout(l)`, `read_config(l)` (that is, `parse_inventory`), then
  `state_members(l)`. All handles drop on return.

**S1. `service::owner_windows`:** add a `pub(super) trait ConfigureSession {
read_config; observe_batch; configure }` with the existing signatures. Its
only production implementation is `WindowsOwnerSession`. Add no constructor for
`BatchObservation`, store or right.

**S1. `service::executor`:** extract the existing Windows per-component
device-name and trailing dot/space predicate into a shared `pub(crate) fn`,
with no behaviour change.

Until S3 wires these items, any unused ones carry
`#[cfg_attr(not(test), allow(dead_code))]` with a comment naming S3, following
the existing `uninstall_flow` pattern. S3 removes those attributes, plus
`#![allow(dead_code)]` in `owner_install_windows.rs`.

**S2. `crate::client_config` (new; Windows and Unix parts as in `local`):**

```rust
pub(crate) enum ClientError { Argument, Elevated, Parent, Exists, Io, Partial } // no payload
pub(crate) fn create(principal: &str, out: &str) -> Result<String /*sha256 hex*/, ClientError>;
#[cfg(unix)]
type ClientHandle<'a> = std::os::fd::BorrowedFd<'a>;
#[cfg(windows)]
type ClientHandle<'a> = std::os::windows::io::BorrowedHandle<'a>;
trait PostCreateOps {
    #[cfg(unix)]
    fn mode(&mut self, file: ClientHandle<'_>) -> std::io::Result<()>;
    fn write(&mut self, file: ClientHandle<'_>, json: &[u8]) -> std::io::Result<()>;
    fn flush(&mut self, file: ClientHandle<'_>) -> std::io::Result<()>;
    #[cfg(unix)]
    fn sync_parent(&mut self, parent: ClientHandle<'_>) -> std::io::Result<()>;
    fn verify(&mut self, file: ClientHandle<'_>, expected_len: u64)
        -> std::io::Result<()>;
}
fn create_with(principal: &str, out: &str, address: &str, server: &str,
               elevated: impl FnOnce() -> Result<bool, ClientError>,
               random: impl FnOnce(&mut [u8; 32]) -> Result<(), ClientError>,
               ops: &mut impl PostCreateOps)
               -> Result<String, ClientError>;
```

`create` passes `Layout::system()?.endpoint()`, the server identity
(`S-1-5-18` on Windows, `0` on Unix), the native elevation check,
`getrandom::fill` and the private concrete `NativeFileOps` adapter implementing
`PostCreateOps`. NativeFileOps is distinct from the forbidden owner CLI
NativeOps. Its context holds only the current-user comparison facts needed
by Verify; it adds no owner authority and has no Debug output. No flag,
environment variable or build feature can select a test adapter in production.

`PostCreateOps` is a private operation seam, not a pre-step fault hook (A-ST1).
Its platform-specific methods perform Mode, Write, Flush, SyncParent and
Verify on the already-created handle (and held parent for SyncParent).
The native adapter has no path arguments and cannot create, reopen, replace
or delete a file. Methods borrow the already-created handle (or the retained
parent for SyncParent); they do not take ownership of it. Tests may use a
cfg(test) step enum solely to record method order; no unused production tag
enum remains. Owned-temp fault wrappers may deliberately mutate fixture
mode/length or add a hard link before forwarding Verify, as specified below;
those fixture operations are not available in the native adapter.
Each method returns its operation's `io::Result`; Write returns the result of
writing all bytes, including an error after writing a prefix. The shared
`create_with` sequence immediately maps every method's Err to Partial through
one classifier. The native adapter must propagate syscall/write/check errors
unchanged into that path; it must not classify or suppress them itself.
Tests wrap the native adapter in owned temp fixtures to return native-shaped
errors or write a prefix and then Err. A private adapter unit test also
supplies failing syscall results to the same result-propagation code used by
native methods, so a native-only Io mapping or ignored Err is detectable.

Windows Verify separates handle-fact acquisition from a private pure checker.
Native `GetFileInformationByHandle`/`GetSecurityInfo` results feed that checker;
synthetic facts use the same checker, and its Err traverses the same Verify
method/result classifier. Facts include owner, type, reparse status, link
count, length, DACL presence and every ACE's type and recipient.
Both acquisition errors and checker rejections are required negative cases. No NativeOps, OwnerGuard,
session, elevation or service path is used by these tests. Unix retains a
real owned-temp hard-link fixture. `create_with` runs in this order, with no
filesystem call before step 4:

1. `valid_principal`, else `Argument`.
2. Lexical `--out` check, else `Argument`. The path must be absolute and
   normalized, at most 4096 bytes, and end in a plain final name. On Unix it
   starts with a single `/` and has no `\`. On Windows it is in `X:\` form, has
   no `:` after the drive, and passes the shared device predicate on every
   component.
3. `elevated()?` true → `Elevated`. Before calling RNG, construct the
   sole wipe owner with initialized `raw: [u8; 32]`, `token: [u8; 64]` and
   preallocated JSON storage. The owner is neither Clone nor Debug. Its Drop
   overwrites all initialized secret storage with volatile zero writes before
   releasing it (no new crate). All secret-bearing work below operates in
   this owner's storage; no temporary owned secret copy is introduced.
   - Call `random(&mut owner.raw)`. Production maps a getrandom error to Io.
     If RNG fills even one byte and then returns Err, return through the
     owner's Drop. No filesystem call or output occurs. Unwinding after owner
     construction also drops it; aborts/OS or allocator copies remain outside
     the best-effort guarantee.
   - Encode raw once into the owner's token, lowercase hex.
   - Compute `digest = sha256(&owner.token)` and retain this non-secret
     `[u8; 32]` outside the owner, before clearing any token byte.
   - Serialize the private serialize-only struct `{address,
     expected_server_identity, token: &str}` directly into the owner's JSON
     storage, borrowing the same token text. Reserve an escaping-aware bound
     before RNG: the compact empty-values object length plus six times the
     sum of the three value byte lengths (including 64 token bytes), using
     checked arithmetic. Bound arithmetic overflow returns Io before the wipe
     owner is built and before RNG; no unreachable overflow fixture or new
     fault hook is required. Six bytes per input byte bounds JSON escaping,
     including the Windows pipe address's backslashes. No post-RNG growth or
     intermediate secret String/Vec is allowed.
   - Serialization of these three string fields into a Vec is structurally
     infallible under the current model. If serde unexpectedly returns Err,
     map it to Io and drop the wipe owner before any filesystem call. Do not
     add an artificial serialization fault seam or require an unreachable
     error fixture. Allocator failure/abort is outside the guarantee. All
     reachable ordinary error returns still traverse Drop.
4. Walk the existing ancestors from the root. Unix uses `openat` relative to
   the previous retained descriptor with `O_NOFOLLOW|O_DIRECTORY`. Windows
   uses `FILE_FLAG_OPEN_REPARSE_POINT | BACKUP_SEMANTICS`, refuses the
   reparse attribute, accepts fixed drives only, shares read and write but
   never delete, and holds every ancestor handle until return, so no held
   component can be renamed or deleted. A missing grandparent or any link →
   `Parent`.
5. The parent:
   - If absent, create exactly it: Unix `mkdirat(grandparent_fd, name, 0700)`;
     Windows `CreateDirectoryW` with `O:<TokenUser>D:P(A;OICI;FA;;;<TokenUser>)`
     while the ancestor handles are held. A race returning `EEXIST` or 183
     continues as "existing".
   - Then open it without following links: Unix `openat(grandparent_fd, name,
     O_NOFOLLOW|O_DIRECTORY)`; Windows the same flags as step 4, under the held
     ancestors. Check the opened handle only: a directory, not a reparse
     point, owned by the current user. On Unix also require
     `mode & 0o077 == 0`. On Unix a newly created parent then gets
     `fchmod(fd, 0700)` on that descriptor, which only removes bits. An
     existing parent is never chmodded. On Windows apply the allow-only rule
     (A-A2): DACL present, every ACE `ACCESS_ALLOWED_ACE_TYPE`, recipients
     only TokenUser, SYSTEM or BUILTIN\Administrators, and any other ACE type
     (deny, object, callback, compound, unknown) refuses. Any failure →
     `Parent`. A parent created here is left in place on every failure.
   - Keep the parent pinned until return.
6. **The boundary (A-SP2).** Create the file: Unix
   `openat(parent_fd, name, O_WRONLY|O_CREAT|O_EXCL|O_NOFOLLOW|O_CLOEXEC, 0600)`;
   Windows `CreateFileW(CREATE_NEW)`, `GENERIC_READ|GENERIC_WRITE|READ_CONTROL`,
   share 0, `SECURITY_ATTRIBUTES` with owner TokenUser and
   `D:P(A;;FA;;;<TokenUser>)`. `EEXIST`, `ERROR_FILE_EXISTS` or
   `ERROR_ALREADY_EXISTS` → `Exists`. Any other create error → `Io`. Neither
   case mutates the existing object: it is never opened for writing,
   truncated, chmodded, re-ACLed or deleted. Once the create returns a
   descriptor or handle, the command never returns `Exists`, `Io`, `Parent`
   or success except as step 8 allows.
7. Post-create steps, each invoked through `ops` in this order:
   1. `Mode` (Unix only): `fchmod(fd, 0600)` on the created descriptor, so a
      restrictive umask cannot leave the owner without read access.
   2. `Write`: write all of `json` to the created handle.
   3. `Flush`: `fsync(fd)` or `FlushFileBuffers(handle)`.
   4. `SyncParent` (Unix only): `fsync(parent_fd)`.
   5. `Verify`: check the created handle only (`fstat`, or
      `GetFileInformationByHandle` plus `GetSecurityInfo` on the handle): a
      regular file, not a reparse point, link count 1, owned by the current
      user, Unix `mode & 0o777 == 0o600`, Windows the step 5 allow-only rule,
      and length equal to `json.len()`.

   Any operation `Err`, including native-call or checker failure, at any of these steps →
   `Partial`. The function then returns immediately. It returns no hash,
   writes no further bytes, and does not unlink, set a delete disposition,
   set delete-on-close, truncate or rename. The file stays in place, private
   since its first byte, holding whatever bytes the completed steps wrote.
   Production never reopens the created file by path.
8. Only after all of step 7 succeeds, return `hex(digest)`. The step 3 owner
   then clears the token buffers on drop. The token never leaves the function
   except into the file. No code path hashes after clearing, and none returns
   the digest after a post-create failure.

## 4. Per-verb composition (exact order)

`ctx` is `OwnerContext { admin: true, running_binary: &current_exe()? }`. It is
built only after the native owner check of its platform has passed: on
Windows, possessing a guard; on Unix, `require_owner()`.

| verb | Windows | Unix |
| --- | --- | --- |
| install | `OwnerGuard::acquire()` → `owner_install_windows::install(guard)` | `require_owner` → `Layout::system` → `owner_plan(Install)` → `owner_unix::execute` |
| configure | guard → `layout = guard.layout().clone()` → `ConfigureArgs` → `from_request(NativeResolver)` → `WindowsOwnerSession::open(guard)` → `windows_configure(session, …)` (below) | `require_owner` → layout → `from_request` → `owner_unix::read_config()` → previous `ServiceConfig::parse` → `configure_plan(…, None, NativeResolver)` → `execute` |
| start | guard → `layout = guard.layout().clone()` → `native::owner_start_preflight(&layout)` → drop guard → `WindowsScm::system(&layout)?.start()` | `owner_plan(Start)` (contains `StartPreflight`) → `execute` |
| stop | guard → `WindowsScm::system(guard.layout())?.stop()` | `owner_plan(Stop)` → `execute` |
| status | guard → `scm.status()` → print the token | `owner_unix::status()` → print the token |
| uninstall | guard → `scm = WindowsScm::system(guard.layout())?` → `uninstall_flow(&mut scm, \|p\| WindowsOwnerSession::uninstall(guard, p))` → print the `RightReport` token | `owner_plan(Uninstall)` → `execute`; prints nothing |

Every Unix row begins with `require_owner()`, then `Layout::system()` and
`ctx`. `owner_unix::execute`, `status` and `read_config` then repeat root, the
fixed binary and the protections natively.

`windows_configure<S: ConfigureSession>` runs these steps:

1. `prev = session.read_config()?`.
2. `old = prev.map(|b| ServiceConfig::parse(&b, l, res)).transpose()?`.
3. `batch = session.observe_batch(new.build_identity())?`.
4. `plan = configure_plan(l, ctx, &new, old.as_ref(), Some(batch), res)?`.
5. `req = Configure::from_plan(l, &plan, res, batch)?`.
6. `session.configure(&req)`.

A failure at any step makes no later session call. The CLI never builds a
store, right, guard, proof or `BatchObservation`. It never calls `parts`,
`Plan::execute`, `uninstall_plan` or any raw LSA, SCM or file primitive.

**Previous config revalidation is kept (A-SP3).** On both platforms the
previous config goes through the full `ServiceConfig::parse` (Windows step 2;
Unix "previous `ServiceConfig::parse`"), and `configure_plan` calls
`old.revalidate` again. The CLI does not replace either call with
`parse_inventory`, does not pass `None` for an existing previous config, and
does not change `configure_plan`, `parse` or `revalidate`. A previous config
whose pinned accounts, revisions, execution preflight or workspace no longer
validate refuses configure with the library's error (exit 3), even if the new
flags are valid. That happens before any `observe_batch` or `configure`
session call on Windows, and before `execute` on Unix. The manual owner
remedies are in ADR 0018 decision 4: restore the old workspace, or uninstall,
delete the config by hand, install and configure. No command, flag, prompt or
override is added.

## 5. Output and exit table (exhaustive `match`, no wildcard)

| error | stderr token | exit |
| --- | --- | --- |
| usage | `usage` | 2 |
| `Error::{Config, Identity, AccountMissing, Rights, Workspace}` | its `Display` token | 3 |
| `Error::{Permissions, OwnerRequired}` | its token | **4** (ADR 0009 code-4 rule) |
| `Error::{Layout, NativeAdapterMissing}` | its token | 7 |
| `Error::{Manager, NotInstalled, Foreign, Active}` | its token | 1 |
| every `Failure` | `service-owner-*` | 1 |
| `OpenError::Layout(e)` / `OpenError::Store(f)` | as `e` / as `f` | as `e` / 1 |
| `ClientError::Argument` | `client-config-argument` | 3 |
| `ClientError::{Elevated, Parent}` | `client-config-elevated` / `client-config-parent` | 4 |
| `ClientError::{Exists, Io, Partial}` | `client-config-exists` / `-io` / `-partial` | 1 |

On success the exit code is 0, and stdout follows these rules:

- `status` prints exactly one token from `{not-installed, stopped, starting,
  running, stopping, failed}` and a newline.
- Windows uninstall prints exactly one `RightReport` token and a newline.
- `client-config new` prints exactly `<principal>=<64 lowercase hex>` and a
  newline, the same form `--agent` accepts.
- Every other success prints nothing.

Neither stream ever carries an argument value, path, account name, SID or OS
code. Windows `status` returning `Err(NotInstalled)` (Ours but not Installed)
is an error line, not a token.

## 6. Test seams (no production selector)

The code goes in `service::owner_cli` (it needs `pub(super)` access). It
exposes only `pub fn main(args: &[OsString]) -> i32`. `main.rs` calls it for
the two owner prefixes, then calls `std::process::exit`.

- `parse(&[OsString]) -> Result<Command, Usage>` is pure.
- `dispatch(cmd, ops: &mut impl OwnerOps, out: &mut impl Write, err: &mut impl
  Write) -> i32` contains the real output and exit mapping. `trait OwnerOps`
  has one method per verb and is `pub(super)`. `NativeOps` is its only
  non-test implementation; no flag or environment variable selects anything
  else.
- The compositions are generic over existing seams:
  `windows_configure<S: ConfigureSession>`; `windows_start(preflight: impl
  FnOnce() -> Result<(), Error>, start: impl FnOnce() -> Result<(), Error>)`;
  the deferred start closure constructs the SCM only after preflight succeeds
  and its guard and pins are released. The fake start must record exactly the
  same call sequence as a direct PR-C start. Native guard release and SCM
  construction order remain source-reviewed. The uninstall
  closure through `uninstall_flow`; `unix_verb(action, layout, ctx, execute:
  impl FnOnce(&Plan) -> Result<(), Error>)`; `unix_configure(read_previous,
  res, layout, ctx, args, execute)`. `Layout::for_test`, `FakeResolver`, the
  `owner_scm` fake calls and clock, and the `owner_transactions` recording
  models stay `#[cfg(test)]`.
- `client_config::create_with` takes the private `PostCreateOps` adapter
  from section 3. Tests exercise operation results and the production Windows
  fact checker, including partial writes and native-result propagation.
  `create` always passes the native adapter. No product setting selects it.
- **No test constructs `NativeOps`, calls `create` (only `create_with`), or
  spawns a valid owner argv.** Every test of a valid owner command goes
  through `dispatch` with `RecordingOps` or through a generic composition with
  fakes. Production `NativeOps` is never instantiated under `cfg(test)`.
- **The usage vector changes before any wiring.** The first S3 commit changes
  only `tests/service.rs`. It replaces `["service", "install"]` with
  `["service", "install", "extra"]` and keeps the current expectation
  `arxd: usage: arxd service run\n`, which current `main` already prints for
  that argv. Only the later commit that adds owner routing changes the
  expectation to `arxd: usage\n`. No commit in the stack routes owner verbs
  while that file still spawns a valid owner argv, so no intermediate test run
  can execute one. A unit test asserts that `parse` rejects every argv that
  file spawns, except the unmanaged `service run` cases, which keep their
  existing coverage. Hosted Windows runners are elevated, so this rule is a
  safety rule, not a style rule.

## 7. Required tests (portable unless marked)

1. **Parser matrix**, table-driven. Accept every service-install.md example
   form. Reject each rule in section 2, including a non-UTF-8 argument (Unix
   `OsStrExt::from_bytes`; Windows `OsString::from_wide` with an unpaired
   surrogate), 171 arguments, a 4201-byte argument, 33 KiB in total,
   `--rights=admin`, `--rights Admin`, a duplicate `--workspace`, 17 `--env`,
   65 `--agent`, a missing `=`, an empty key, `service run extra`, and
   `client-config new` with an extra argument. Repeated `--env`/`--agent` keep
   their order and duplicates.
2. **Dispatch.** Use `RecordingOps` with marker strings in every argument.
   Each verb calls exactly one method with exactly the parsed values. Every
   error variant (exhaustive list) produces its token and exit code. The
   success stdout is exact. No stream contains a marker, except the principal
   on the client success line.
3. **Routing.** `service`/`client-config` argv never reaches the foreground
   parser. `service run` reaches `service::run` only with exactly two
   arguments. The existing binary tests for unmanaged `service run` (10 on
   Windows, 4 on Unix) stay unchanged.
4. **`from_request`**, using `FakeResolver` on Windows, Linux and macOS test
   layouts:
   - the bytes are canonical serde and reparse equal;
   - the limits equal `DEFAULT_LIMITS`;
   - macOS `admin` gives `Rights` with zero resolver calls;
   - a duplicate env key gives `Config`;
   - a group account gives `Identity`;
   - `AccountMissing` passes through;
   - the previous agents are never merged (no value is copied from the
     previous config).
5. **`windows_configure`** with a recording session, and **`unix_configure`**
   with a recording `execute`:
   - the call order is read, parse, observe(new SID), configure;
   - an unparseable previous config makes no observe or configure call;
   - a changed build pin is refused before `configure`;
   - the mirror equals `add || recorded_grant`;
   - **stale previous config (A-SP3)**, using a counting resolver: same pinned
     build account in both configs, `workspace_safe` false for the old
     workspace and true for the new one. Configure returns `Workspace`
     (exit 3). The resolver records a `workspace_safe` call for the old
     workspace. Windows makes zero `observe_batch` and `configure` calls, and
     Unix makes zero `execute` calls. The same fixture with the old workspace
     valid succeeds, and its compare-and-replace expectation equals the
     previous bytes;
   - a previous config whose build account is now missing returns the library's
     `Identity` (`service-identity`, exit 3): full validation folds execution
     preflight `AccountMissing` into `Identity`. Both platform compositions
     record `account:old-builder` and no `workspace:/old`; Windows session
     calls equal only `read`, and Unix makes zero execute calls. The same
     zero-mutation assertions apply. New-account lookup in `from_request`
     still passes through `AccountMissing`; no parser/revalidation change.
6. **`windows_start`** (Windows) with fake SCM calls: a preflight `Err` makes
   zero SCM calls, and `Ok` runs exactly the PR-C start sequence. **Inventory
   preflight invariant**, using a counting resolver: `parse_inventory` makes
   zero `workspace_safe` calls and no build-account `account` call.
7. **Uninstall.** For every injected SCM fault, the session closure is never
   invoked. On Unix, the plan passed to `execute` equals `owner_plan(Uninstall)`
   and `read_previous` is never invoked, even with no config.
8. **Unix verbs.** The plan passed to `execute` equals `owner_plan(action)`
   exactly, and start contains `StartPreflight`.
9. **`client-config` `create_with`**, run natively as the current user in an
   owned temp directory:
   - a missing parent is created private; an existing private parent is
     accepted unchanged;
   - each of these is refused and left unchanged: a non-private or foreign
     parent, a symlinked or junction ancestor, an existing target (same bytes);
   - elevated → `Elevated`, with no filesystem change;
   - an RNG Err creates nothing. A deterministic RNG writes a nonzero prefix
     then returns Err; assert Io, no file/parent creation, no hash or output,
     and that the wipe owner ran. A private test-only observer in the shared
     wipe routine reports only a boolean after checking all initialized
     storage is zero and before releasing it; it exposes no secret bytes.
     Check that same observer on success and each ordinary post-RNG error
     class (Parent, Exists, Io and Partial). An unwind fixture
     after a partial RNG fill also checks Drop; do not test allocator/OS copies;
   - the file is private, has one link, contains exactly three keys and
     deserializes as `ClientConfig`; `security::private` and `owned` pass on
     the file and its parent. These path-based helpers are independent test
     oracles only;
   - **hash order (A-SP1).** `random` fills deterministic nonzero bytes
     (`raw[i] = 0xA5 ^ i`). The test reads the file as the current user,
     parses `token`, and asserts that it equals the lowercase hex of those
     bytes, that the returned hash equals `sha256(token text from the file)`,
     and that it differs from `sha256` of 64 zero bytes, of 64 `'0'`
     characters and of the raw 32 bytes;
   - **JSON allocation bound (N5/A3).** The single private
     `serialize_owned` routine called by `create_with` contains a cfg(test)
     `AllocationObserver` seam. A scoped, per-thread test registration receives
     Before/After snapshots containing only pointer and capacity; it resets
     on Drop, and production has no observer or registration interface. No
     bytes, token or Debug of the secret owner are exposed. Tests drive the
     real `create_with` serialization path and assert unchanged capacity and
     pointer with boolean/fixed-label assertions, never printing addresses.
     Run both fixed Windows and Unix endpoint strings on every CI platform;
     parse the JSON and assert exact address/server/token values. The Windows
     case must contain escaped backslashes and round-trip correctly. The
     bound is checked before RNG and needs no overflow fault seam.
   - **post-create operation matrix (A-ST1, A-SP2).** The generic byte,
     privacy and one-link expectations below apply to operation/acquisition
     errors without deliberate fixture corruption. The invalid Verify cases
     use their explicit end states below instead. The private adapter
     returns an operation error at each platform step: Unix Mode, Write,
     Flush, SyncParent, Verify; Windows Write, Flush, Verify. Test both a
     Write error before any bytes and one after a known JSON prefix has been
     written to the real owned-temp file. Assert all of these:
     - exactly Err(Partial), no hash, no later operation, exact prior order;
     - the file remains private, one link, current-user owned; before Unix
       Mode succeeds require only no group/other permissions, since umask may
       remove owner bits. Exact mode 0600 is required after Mode succeeds;
     - empty bytes for Mode/pre-write errors, the known prefix for a partial
       write, and complete JSON for Flush/SyncParent/Verify errors;
     - no parent change or extra entry, no reopen, rollback or deletion;
     - dispatch emits exactly `arxd: client-config-partial\n`, empty stdout,
       exit 1, and wipe observation is true.
   - **native-result propagation.** Feed actual Err results through the
     private native adapter's syscall-result seam for fchmod, write, flush,
     parent fsync and handle-fact acquisition as applicable. They must reach
     the shared Partial classifier. Mutating only native-result propagation
     to Io, Ok or ignored failure must fail a compiled assertion, not merely
     the adapter fake. The production syscall and fixture result must meet at
     the same result-handling statement; no parallel test implementation.
   - **verification predicates.** Unix: use three separate real owned-temp
     fixtures. Before forwarding native Verify, (a) create a hard link (link
     count 2), (b) fchmod the held file to 0640, or (c) append one extra byte
     through the held handle. Each returns Partial, no hash, and retains the
     file/names without repair or rollback. These deliberately invalid Verify
     fixtures use synthetic credentials under a private 0700 parent. Their
     explicit retained states are: (a) complete JSON, two names/two links,
     current owner and mode0600; (b) complete JSON, one link and mode0640,
     deliberately failing the strict file-privacy/mode assertions; (c) JSON
     followed by one fixed `x` byte, one link, mode0600 and expected length
     json.len()+1, deliberately failing the generic complete-JSON assertion.
     All three retain every unaffected assertion: no later operation, no
     reopen, empty stdout, no hash, fixed stderr/exit and wipe observer true.
     No file is repaired or removed; only the intentionally invalid mode,
     link or payload expectations differ. Real owner/type negatives
     remain hardware limitations; do not add privilege or foreign-account
     operations. Windows: require independent negative fact fixtures for foreign owner,
     wrong type, reparse, link count 2, wrong length, absent DACL, deny ACE,
     other unsupported ACE type and foreign allowed recipient, plus a valid
     control. Each uses the production checker and reaches Partial through
     Verify. A deny ACE must actually be present in its fixture. Record any
     inability to exercise real GetSecurityInfo as a hardware limitation;
     synthetic predicate coverage is mandatory and cannot be replaced by a
     pre-Verify injected Err. Tests must also catch skipping the checker or
     suppressing its rejection. Do not change a real account's permissions.
   - **pre-create errors make no change.** For `Exists`, the existing
     target's bytes, length, mode or DACL, owner and modification time are
     unchanged, and no post-create adapter method is called. The same holds for `Parent`,
     `Elevated`, `Argument` and an RNG `Err`;
   - the output never contains the token.

**Mutations.** Each must fail a test; restore it afterwards and record the run:

- accept `--flag=value`;
- deduplicate `--env`;
- print an argument value in an error;
- map `Permissions` to 1 or 5;
- ignore a `deregister` `Err`;
- run the Windows start preflight after `start`, or skip it;
- use `ServiceConfig::parse` in `read_config`;
- use `uninstall_plan` on Unix;
- merge the previous agents;
- open with truncate or `CREATE_ALWAYS`;
- skip the elevation check;
- chmod an existing parent;
- echo the token;
- clear the token buffers before hashing, or hash the raw bytes instead of
  the token text (A-SP1);
- skip previous-config parsing, replace it with `parse_inventory`, or pass
  `None` for an existing previous config (A-SP3);
- map a post-create operation failure to `Io` instead of `Partial`
  (A-SP2, A-ST1);
- return success (the hash) after a post-create failure, or ignore a failed
  `Verify` (A-ST1);
- unlink or delete the file, or set delete-on-close, after a post-create
  failure (A-ST1);
- run Unix `fchmod` before the exclusive create, or apply it to an existing
  target (A-SP2);
- accept a deny ACE in the Windows client-file rule (A-A2);
- construct the wipe owner after RNG, bypass Drop on a partially filled RNG
  Err, or skip wiping any initialized secret buffer (A-SP1);
- map only native-operation Err to Io or Ok, ignore a prefix-write failure,
  bypass the Windows checker, or ignore its rejection (A-ST1);
- omit Unix Verify's mode or length check, each caught by its real fixture;
- start JSON storage without the required capacity (or below actual encoded
  length), caught by the unchanged-pointer/capacity tests (N5).

## 8. Slices (one concern each; S1 and S2 may run in parallel)

1. **S1** `feat(service): owner configure and start preflight prerequisites`:
   section 3 items for `config`, `owner_unix`, `native`, `owner_windows` and
   `executor`, with tests 4–6. No CLI.
2. **S2** `feat(client): private client credential creation`: `client_config`
   with test 9. Not wired.
3. **S3** `feat(cli): wire owner service and client-config commands`:
   `owner_cli`, `main.rs` routing, `NativeOps`, the compositions, tests 1–3, 5
   (wiring), 7 and 8, and the `tests/service.rs` change. That test change is
   S3's first commit, before routing (section 6). It also updates three docs:
   - service-install.md: the owner commands are wired and **unverified on real
     hardware**; add the ADR 0009 manual pin-change step, the
     `client-config-partial` remedy (delete the private file by hand and
     rerun; no registration hash was supplied) and the stale-previous-config
     refusal with its two manual remedies (ADR 0018 decision 4);
   - service-foundation.md;
   - the README sentence "arxd generates no credentials" (ADR 0009).

Each slice runs only fmt, locked all-target Clippy and normal nextest as an
ordinary user. No slice runs an SCM, LSA, account, privilege, pipe-connect,
system-path, service-manager, VM, Lab Manager or S4U operation.

## 9. Open facts (all **unverified on real hardware**)

- A Windows start preflight against a running service: the state-file share
  modes.
- An elevated admin reading SYSTEM-private state.
- `CreateFileW`/`CreateDirectoryW` with an explicit TokenUser owner from an
  unelevated token on the owner's filesystem.
- Whether the guard's read-only image pin interferes with the SCM image load
  (start drops it first).
- Every SCM, LSA, systemd and launchd item in ADR 0012/0014 and the SCM brief
  §12.
- macOS stays `Unknown`, so install, start, stop, uninstall and status stop
  with `service-manager` before any mutation.
- The pin-change gap in ADR 0018 stays open, with no new authority.

Coordinator disposition: see ADR0018 acceptance and retained final Opus recheck.
Earlier proposed-status paragraphs are historical checkpoints.

## Advisory clarification disposition (2026-11-12)

Coordinator Sol medium resolves final Opus recheck N1–N5: remove the unreachable
serialization-error fixture; remove the stale production tag; specify borrowed
handle methods and distinct NativeFileOps; require real Unix mode/length
negative cases; bound JSON escaping and assert no allocation growth. Tests
and mutations described here are future requirements, not executed evidence.
The core contract and every authority/privacy/Partial/wipe gate stay intact.
Independent Opus review passed at dfcb643; small follow-up advisories are
specified above for the published-head recheck.
