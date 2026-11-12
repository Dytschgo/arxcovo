# S1 source-review follow-up for publication

Coordinator: OpenAI / gpt-6.1-sol / medium, 2026-11-12.
Actual main: d70e80818569491238656255ee415669f04a731d (PR47).
Reviewed source: 813dd1e575e1f1cbfe50237c43d115a6134b39bd.
The [full Opus review](../reviews/0018-s1-independent-review.md) is retained
unchanged from b4b8154. Standards passes; Spec passes conditional on actual
hosted Linux/macOS compilation and tests. Those conditions remain mandatory.

Two small advisory corrections change tests only:

- ST-2: the existing Unix previous-read fixture now changes its private
  config directory to0750 and expects Permissions, then restores0700 before
  subsequent file-mode/size/symlink cases. No production statement changes.
  This case is uncompiled/unexecuted locally because the Unix cross compiler
  is missing; no mutation kill is claimed.
- ST-3: rename the fake-only configure-session test to
  configure_session_fake_can_refuse_without_native_calls. It exercises only
  the fake refusal/signature seam, not real WindowsOwnerSession behavior.

On the final source, coordinator fmt and locked all-target Clippy passed.
The exact renamed test passed1/1,271 not run,0.024s; run
2f72e6af-ac46-4d74-b9b6-c4f41e9f91f1. No full suite repeat is claimed: the
builder's full271/2 run belongs to813dd1e's Windows behavior. The added Unix
case is cfg-excluded on Windows; the other change is a test name only.
All production source remains byte-identical to the full reviewed source.

Residual ST-1/ST-4/SP-1 wiring gaps remain disclosed: inventory-only parser
selection in the real native read_config caller, service identity before
state_members, and the real owner_start_preflight callback selection are
source-reviewed rather than exercised through fixed system paths. S3 must
target the real read_config selection in its mutation evidence. Same-file
negative coverage and duplicate resolver-fake advisories remain as stated.
No native constructor or owner path is invoked to fill those gaps.

Main after PR47 passed in564s, run43204089423,10:51:49–11:01:13UTC.
Fresh published-head review and all hosted gates are required before merge;
no failed/missing Unix check is waived. S2's local dependency does not clear
S1's gate. Installed/native/dedicated paths remain **unverified on real
hardware**; M2 incomplete and ExecutionFailed remains unresolved.
