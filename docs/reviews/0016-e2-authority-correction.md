# E2 authority correction

Local correction by OpenAI / gpt-6.1-sol, medium, coordinator. This small mechanical follow-up uses the coordinator's actual effort; Windows install implementation normally routes to Sol high. Independent Opus recheck remains required.

Base: `42718df0005e89c184c75368b4ec8bf2087ad1f1`, containing the full security review of `d28cbd7`.

## Disposition

- SP-B1: `OwnerGuard` fields are private again. Installer access is restricted to a borrowed layout, a borrowed binary-directory pin, and consuming the guard into its pins. The positional binary-directory lookup stays inside the defining module. No new constructor, clone or mutable reference is exposed. Provisioning policy and operation order are unchanged.
- ST-1: `Local` owns a private pointer. Siblings can borrow its pointer through `as_ptr()` but cannot construct the owning wrapper in safe code.
- S1 checks exact private field declarations and scans repository Rust source, without following symlinks, for `OwnerGuard {` outside its defining module. The lexer ignores literals/comments and now handles UTF-8 character literals and punctuation. This scan is a structural fence, not native authority acceptance.
- Other review advisories remain open. No privileged behavior or new native verification is claimed.

## Actual verification

- Before the fix, the new test failed on public guard fields: run `d09eae2a-d192-4a7d-b0a3-4fb120de715f`.
- First focused run: 58 passed, one source-scan failure from the existing lexer's UTF-8 limitation (`21c6c5a5-2fdd-4452-ab35-a4839f5030ef`). The lexer was corrected. A subsequent encoding error in the test edit was corrected before building; it is not red-behavior evidence.
- Formatting and locked workspace/all-target Clippy with warnings denied passed.
- Full normal nextest: **246 passed, 2 skipped**, run `c3364ae6-ec34-4e60-8b82-e7c437c2bf40`, 10.421 seconds. The two subprocess-only fixtures were exercised through parents, never directly.
- Re-exposing guard fields compiled and failed the new assertion: `a7e1c215-7b2f-4a1a-a331-3a26e5434313`.
- Re-exposing the Local pointer compiled and failed the new assertion: `0f6b20b1-1335-4e15-ba0c-1973484f7d73`.
- Both mutations were restored. The focused fence passed afterwards (`8db0631a-ef79-4156-95b8-34669ae3ee40`). Diff whitespace checks passed.

No production guard/session/installer, privilege enabling, real SCM/LSA, service, account, owner administration, VM or Lab Manager operation ran. No push, PR or remote write. Installed, native and dedicated paths remain **unverified on real hardware**. M2 remains incomplete.
