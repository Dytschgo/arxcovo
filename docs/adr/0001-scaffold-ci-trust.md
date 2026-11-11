# 0001: Workspace and CI trust

Status: proposed for independent Opus review.

Use one locked Cargo workspace and one locked npm workspace. Binaries identify themselves and exit. Electron loads local UI in a sandboxed renderer with navigation and new windows denied. Electron behavior is unverified on real hardware.

Budget evaluation uses trusted default-branch code with pull_request_target and workflow_run. It never installs or executes candidate code. It posts a status on the current candidate head, using API measurements and bounded candidate history JSON as data. CI completion re-evaluates ci-speed exceptions.

No workflow manufactures reviewer approval. A separate reviewer integration must post agent-review for the exact reviewed SHA. Bootstrap has no main CI baseline; policy fails closed. Coordinator must resolve activation before milestone acceptance.
