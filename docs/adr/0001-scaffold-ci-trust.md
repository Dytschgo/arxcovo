# 0001: Workspace and CI trust

Status: proposed for independent Opus review.

Use one locked Cargo workspace and one locked npm workspace. Binaries identify themselves and exit. Electron loads local UI in a sandboxed renderer with navigation and new windows denied. An independent reviewer exercised these software behaviors on Windows at 835d06c. The follow-up Electron test exercises rendering, isolation preferences, a real navigation attempt and popup denial; it clears only the child process's ELECTRON_RUN_AS_NODE variable so the harness does not force Node mode. It passes locally on Windows; hosted and other-OS results must be recorded separately. VM/hypervisor paths are unverified on real hardware.

Budget evaluation uses trusted default-branch code with workflow_run, an unprivileged label-change signal and periodic retry. It never installs or executes candidate code. It posts a status on the current candidate head using API measurements. History recording is separate from merge decisions. See ADR 0002 for activation, trust and failure handling.

No workflow manufactures reviewer approval. A separate reviewer integration must post agent-review for the exact reviewed SHA. Bootstrap has no main CI baseline; policy fails closed. Coordinator must resolve activation before milestone acceptance.
