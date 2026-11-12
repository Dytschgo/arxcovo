# State permission exit-code correction

OpenAI / gpt-6.1-sol / medium coordinator, local correction from base `7fefe4d`. ADR0009 assigns code 4 to config/state ownership or protection failures and code 5 to store failures. The earlier Application review identified that `Native::store` discarded this distinction.

Source verification: `Activity::open_service` propagates the native state guard's typed `layout::Error` through anyhow. `Native::store` previously mapped every error to Store. It now uses one private classifier: typed Permissions or OwnerRequired, including under context, maps to Permissions; every other error remains Store. No text matching, store/protection policy change, path, response or schema change. This restores the settled contract and introduces no architectural deviation.

Pure classification coverage includes both typed protection errors with an anyhow context, all remaining layout variants, an unrelated synthetic store error and a raw I/O PermissionDenied error (still a store I/O failure). The production mapping call was checked by source inspection; no production Native/store path was invoked by the test.

Actual verification:

- Initial classifier test scaffolding passed in run `46f47059-efa8-465b-81a3-9e94a7ca6479`; at that point it was not yet used by Native. This is not pre-fix runtime evidence. The original discrepancy was established from the source path above.
- Classifier collapsed back to Store compiled and failed the assertion: `08d72fcb-690b-481f-b618-ebfb39bfa976`.
- Every typed layout error mapped to Permissions compiled and failed the assertion: `94e50d6e-263e-4c34-bc93-abff56d542cb`.
- Mutations restored. Formatting and locked workspace/all-target Clippy passed.
- Full normal nextest **254 passed, 2 skipped**, run `c032f585-06a4-443b-9fcf-79216ccbcb1e`, 11.318 seconds. Subprocess-only fixtures ran only through parents.
- Whitespace checks passed. No native startup, system state, constructor, privilege, SCM/LSA, pipe, account, owner admin, service, VM or Lab Manager operation ran. No push or new PR.

Independent review remains required. Installed/native/dedicated paths remain **unverified on real hardware**; M2 remains incomplete.
