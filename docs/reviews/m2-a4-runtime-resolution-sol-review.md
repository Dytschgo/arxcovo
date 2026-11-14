# A4 standalone-helper amendment: independent Sol review

2026-11-15. OpenAI gpt-6.1-sol, medium coordinator. Independent review of
Muse amendment `3f3fe1145acae52a90c185755c0d0bc2d11a14b1` (retained as
`0828a21`), against main `9d5be079adc92c9426ed0f5054e2e0e964cfb9dd`.
Read the actual ADR delta, resolution, blocker evidence, brief A4 and the
existing `unix_shim_rights_linux.rs::execute` implementation.

## Standards

PASS, no hard findings. The amendment changes one test-fixture architecture
binding and records its reason. No executable, workspace target, workflow,
permission, product approval flow or production behavior changes. No Fowler
smell applies to this documentation delta.

Three precision corrections were applied before publication: Cargo.lock pins
libc 0.2.191; stdio redirected to /dev/null is distinct from the two handshake
FDs; and future build/hash/symbol evidence is required, not already present.
The brief delegates helper provenance to D9a and needs no extra helper-shape
amendment. These corrections grant no source or runtime evidence.

## Spec

PASS, no findings after those corrections. One fixed prebuilt helper replaces
the post-fork waiting lifetime that blocked implementation. The source slice
must still supply a concrete compiler invocation, source/image hashes, size,
symbol inventory, strict argv parser and validated descriptor before use.
Normal discovery cannot select it. Single-task placement and all numeric,
identity/reap, S7 attribution, no-adoption and owned-cleanup requirements stay.
Bindings 2-5 are unchanged, including A6's joined nonnative evidence.

The existing E4 execute path actually opens /dev/null and a CLOEXEC error pipe,
forks, duplicates null to 0-2, then execveat with AT_EMPTY_PATH against an owned
executable descriptor. Its parent tracks its child and polls exit with a bound.
It therefore provides the cited spawn shape. It is not a reusable helper API
as written: settings argv/environment and lifetime must be prepared for A4 in
its later test-only source slice. The amendment does not authorize caller
commands or a second production execution route.

## Security

PASS, no blocker. The amendment explicitly accepts the same parent-image
spawn-window basis as E4 rather than claiming to prove an unknown atfork set.
After exec, the waiting helper no longer carries the parent test runtime.
This is a documented, narrow engineering tradeoff, not a claim that arbitrary
post-fork Rust is safe. Future helper source/build review must establish the
single-task image and limited operations; a source file alone is not binary
proof. SHA/size mismatch refuses before spawn; descriptor-bound execution
prevents a later pathname selection from replacing the validated image.

No test/build/native operation was run for this docs review. Validation is
actual source comparison, local-link/UTF-8/LF checks and git diff --check.
This is a two-way door: revert restores the prior blocked contract. Source
preparation follows normal review/check/merge gates; native execution needs a
separate disposition. A4, M2, full F/G and real-Runner PA6 remain open.
Hardware paths are **unverified on real hardware**.

Standards: 0 hard findings. Spec: 0 findings. Security: 0 blockers.
