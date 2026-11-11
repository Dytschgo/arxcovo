# CI budget acceptance probe

This documentation-only PR preserves the temporary CI delay from main. It has no
`ci-speed` exception. Once the delayed main run finishes, the required
`ci-time-budget` status must prevent merging even after platform checks and the
independent review pass.

The coordinator enables auto-merge only after observing the slow-main failure,
then records the blocked outcome. A separate measured `ci-speed` PR removes the
delay. After recovery this probe is updated against main and reviewed again to
verify that normal merging resumes. This file describes the test; it does not
claim any acceptance result.
