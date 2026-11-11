# Service config and owner-plan foundation

This M2 slice is library-only. It does not install, configure or start a service.
No agent-facing API or CLI selects these plans. The lifecycle production entry
continues to fail closed. No current-user execution config was added.

## API for later coordinator wiring

- `service::layout::Layout::system()` resolves fixed OS paths. Windows uses native
  `SHGetKnownFolderPath` with native x64 Program Files and ProgramData, never
  environment expansion. Its private test constructor is compiled only in unit
  tests; no binary override exists. Native local-disk/ancestor/reparse inspection
  remains a required adapter check, not a guarantee of this path constructor.
- `check_protection` checks explicit observations against separate private,
  binary-integrity and endpoint policies. Windows private objects require exact
  SYSTEM ownership, a protected present DACL and no foreign allow ACE, including
  inherit-only grants. Integrity permits read access and restricts write grants
  and ownership to SYSTEM, Administrators and TrustedInstaller. Unix policies
  require root:root, private dirs 0700/files 0600 and endpoint dirs 0755.
- `service::config::ServiceConfig::parse` caps input at 16 KiB, denies unknown
  fields recursively, requires explicit rights and validates pins, principals,
  hashes, limits, workspace disjointness and environment entries. `Resolver`
  observes user-vs-group identity, current administrative membership and existing
  metadata-only workspace safety. `revalidate` repeats those observations.
  Read-only getters expose validated runtime values for future integration.
  No production resolver is included. The Unix allowlist is explicitly PATH,
  HOME, TMPDIR, CARGO_HOME, RUSTUP_HOME, LANG, LC_ALL and TZ; the ADR0007 source
  is absent from this base and coordinator reconciliation is required before
  wiring that executor. Windows uses ADR0009's exact allowlist.
- `service::owner_plan::{owner_plan, configure_plan, uninstall_plan}` require
  owner-context evidence. Install additionally requires the fixed running binary.
  `Plan::operations` exposes typed operations; `Plan::execute` stops on the first
  fixed-category error through `OsManager`. `MissingNativeAdapter` fails closed.
  The only implemented managers are unit-test recording/in-memory models.
- Install verifies existing trees before creation and registers constant
  artifacts without starting. The future adapter must implement identical-only
  idempotency for bytes AND protections, including SCM registration. Constants
  and manager argv are embedded; no shell text or account values enter argv.
  SCM registration carries the fixed service name, account, privileges, delayed
  start, failure policy and shutdown timeout; its adapter must quote the image
  as a single executable before the two fixed arguments.
- Configure serializes validated values with serde. `AtomicConfig` describes
  same-directory private temp creation, explicit service owner/protection, flush,
  comparison with expected old bytes, and atomic publication. None means
  exclusive create; a concurrent edit must refuse. Previous expected bytes are
  canonical serde encoding, so noncanonical existing files conservatively refuse.
  There is deliberately no native write primitive pretending a test-owned file
  has SYSTEM ownership. Linux adds/removes only the fixed standard drop-in and
  requests daemon-reload. Windows grants only the pinned build SID if the native
  observation says the right was absent, recording its own change in config.
  A prior validated config preserves that record; changing pins requires uninstall.
- Uninstall plans stop and deregister, remove only identical manager constants
  and remove a Windows batch right only through `uninstall_plan` with the prior
  validated ownership record. They never delete config/state/account/workspace.
  Native status adapters must map results to ADR0009's six fixed status tokens;
  raw manager output must never be forwarded.

## Remaining native work and acceptance boundary

Owner context, resolver and protection observations are injected evidence, not
security attestations. Future native adapters must obtain them locally and
handle races with handles/no-follow checks, including all ancestors and the
binary directory. They must not trust booleans supplied by agents. No fake
establishes kernel ownership, privileges or account isolation.

A multi-step plan is not a transaction. It stops at the first failure and does
not repair or roll back existing objects. In particular, a Windows batch grant
can succeed before atomic config publication fails. Before enabling a native
configure adapter, implement durable recovery or compensation that preserves
whether this invocation actually added the right. The fake proves intent and
failure stopping, not crash-safe right ownership. Drop-in changes and config
publication likewise need native recovery/reconfiguration evidence. The adapter
must implement secure manager-artifact parent handling and exact existing
registration comparisons before mutating anything.

Production resolver/kernel ACL paths, atomic SYSTEM/root writes, SCM/LSA,
systemd/launchd operations, actual identity and installed-service behavior are
**unverified on real hardware**. Owner installation, account and rights setup
remain owner actions after integration/review; this slice authorizes none.

## Verification

Run `cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`,
and `python scripts/test-service-artifacts.py -v`.
The Python tests parse INI and plist bytes using standard parsers. Read-only
systemd-analyze verification requires Linux and the fixed binary to exist;
plutil lint requires macOS. Both are unrun on this Windows host, not installation
acceptance. No UI change requires a UI rerun.

## macOS rights policy correction

Configuration parsing and revalidation now reject macOS admin rights with
Error::Rights before account resolution or workspace observations. Configure
therefore returns no operation plan even for an object validated by an older
implementation. Standard mode still refuses an administrative build account.
Linux and Windows retain explicit admin-mode validation. This follows the
corrected ADR0009 containment limit; it does not establish native macOS cleanup.
The portable regression covers all twelve platform/rights/membership cases.
