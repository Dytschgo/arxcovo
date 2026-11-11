# CI budget acceptance probe

This documentation-only probe has no `ci-speed` exception. At its original head
`690bf5d`, the three platform checks and independent review passed, but the budget
status failed against main run 42952698370 (1016 seconds). Enabling auto-merge at
08:05:19 UTC on 2026-11-11 left the PR open and blocked. The budget gate was the
sole failing requirement.

Recovery PR #6 removed the temporary CI delay and merged through the measured
`ci-speed` exception at 08:05:25 UTC. This probe now includes recovered main, so
its CI no longer sleeps. A fresh independent review, green required checks, and
normal merging on this updated head are still required to prove recovery. No
complete setup acceptance is claimed here yet.
