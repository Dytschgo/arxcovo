# 0035: Bind SQLite identity through a pinned native extension

Accepted for the first dependency and binding proof slice, 2026-11-19.
Store construction and operational startup remain later, separately reviewed work.

The pinned SQLite public API exposes its native Windows handle but no corresponding
Unix descriptor accessor. Retained paths and HAS_MOVED cannot establish that the
main database opened by SQLite is the originally checked object. Independent Sol
reviews support a narrow native extension rather than a general VFS implementation.

Maintain one complete libsqlite3-sys0.35.0 upstream crate as a local Cargo patch,
preserving its license and archive provenance. Patch only build.rs, the SQLite
amalgamation/header and bundled bindings; add a checked patch inventory and notes.
A private versioned file-control result measures the actual Unix descriptor.
Windows uses the borrowed handle from opcode29. Require effective library,
native VFS/method and original retained-object identity checks before trusting
the binding. Unsupported results fail closed. Preserve existing SQLite versions,
store schemas, durability, limits and timeouts.

The reviewed V2 contract is retained at
`D:/Temp/arx-bound-contract-correction-v2/contract.md`, SHA256
`1d92b5bc4ec64ea8e8078235f8cf677d6704aa216bb6c94eb0d012f504728b86`.
Independent review `D:/Temp/arx-bound-contract-review-v2/report.md`, SHA256
`35061d21e7779ed7c27cf39f11cc1b6bad77365e4c5d8a50f60455a69a7f5a4d`,
reports Standards0/Spec0 and closes SP1-SP3. The first build grants only the
dependency/verifier/low-level binding and real disposable-file test scope.

This choice adds a maintained dependency patch and upgrade checks. It establishes
object identity at measurement time, not continuous content immutability or
exclusion of same-account writers. Current-user Windows execution retains its
shared account access. No dedicated-account mandate or product approval flow.

Do not integrate stores, Engine/start/shutdown, founder, networking, custody or
production activation in the first slice. The later store-only design retains
spent state on the original Owner and borrows resources during fallible
initialization; it depends on this slice's source review and qualification.
PR189 remains held after its unknown founder failure. No new founder diagnostic,
fix or retry authority follows. M2 OPEN / M3 UNCHECKED;
**unverified on real hardware**.

PR190's first macOS check exposed a build-contract incompatibility: Cargo1.95's
versioned source sets DYLD_FALLBACK_LIBRARY_PATH on build-script hosts. The failed
job logged the key, not its value or provenance. A narrow correction is prepared
from `D:/Temp/arx-pr190-macos-guard-design/contract.md`, SHA256
`6039205c436fb9fcb325e4e8a0413830abfa28fa9accf68fc5f4ed2e0f218ded`.
Independent correction review and hosted evidence remain required before acceptance.

Reject inherited DYLD_*/LD_PRELOAD names and effective Cargo [env] loader entries
before qualified Cargo invocations; refuse config includes. Do not erase values.
Inside a macOS build-script host only, permit the exact raw fallback-library key
that Cargo uses. Other loader/compiler/linker/SQLite override refusals and all
static-link/source-ID/ABI/VFS checks remain. This is a trusted invocation contract,
not proof of each search entry's provenance or a loader sandbox. Cargo config CLI
overrides, alternate wrappers and unreviewed toolchain changes are not qualified.
Current-user execution's shared access and the separate Windows I/O hold remain.
