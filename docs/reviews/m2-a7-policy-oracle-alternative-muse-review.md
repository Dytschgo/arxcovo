# M2 A7 policy-oracle alternative: independent architecture/security review

Independent reviewer: Muse Spark, 2026-11-15. Single-agent read-only review.
Decision: **NO-GO** — the candidate oracle cannot establish the exact A7
two-case requirement. No contract or gate is changed by this note.

## Reviewed target (pinned)

- Proposal branch: `docs/m2-sudo-policy-oracle`
- Proposal commit: `faf2f638196ec3eb608ab1259133fe052a8c5c26`
  (`docs(m2): propose bounded effective policy oracle alternative`)
- Parent: `e134af8944a5d12296f1905d33c63b7dc8fe2c08`
  (retained bounded public sudo policy provenance)
- Related public-research tip: `00e4b2aac8734190bb85b187083bd576613e5f61`
  (negative result: no complete reviewable closed public literal candidate)
- Scope read: `AGENTS.md`, `docs/KICKOFF.md`, the proposal doc above, and
  the retained provenance note
  (`docs/reviews/m2-a7-a11-public-policy-provenance.md`).
  Not read or run: `docs/PROGRESS.md`, source tree, ADRs, builds, tests,
  host/account/sudo queries. Evidence: local git metadata plus one pinned
  primary public source (upstream sudo manual at `fc9369cb`), nothing else.

## Standards

Repo rules applied: `AGENTS.md` plus the KICKOFF working style (small
one-concern commits, every behavior tested, no secrets, honest
unverified-on-hardware labels, no Lab Manager/owner-admin contact).

1. Form: docs-only proposal, explicitly not accepted, no product or gate
   change — one concern, no scope leak into product behavior, parser,
   upload, or audit infrastructure. Pass.
2. No secrets, credentials, account names, UIDs, or policy text in the
   proposal; it mandates releasing only fixed results. Pass.
3. Honesty labeling: the proposal marks A7/A11/M2/full F/G/PA6 open and
   installed/dedicated paths **unverified on real hardware**, and marks its
   own manual citation a lead rather than proof. Pass.
4. Smell baseline (judgement calls, docs-only diff): no Mysterious Name,
   Duplicated Code, or Speculative Generality beyond what the brief asked
   the reviewer to decide; the control/sequence wording is concrete enough
   to falsify, which is what this review does below. No finding.

Standards findings: 0. Worst: none.

## Spec

Spec applied: the proposal's own decision task — name either a documented
fixed public output/exit discriminator or calibrated control sequence that
establishes the exact A7 two-case requirement (owned NOPASSWD rule present
for one case, absent for the other) without policy text, plus the minimum
binding amendment; otherwise explain the concrete ambiguity and retain the
current exact-byte requirement.

1. Exit-1 ambiguity is real and fatal, and the proposal correctly refuses
   to assume it away — but it has no fix for it. Verified manual text
   (upstream `sudo.man.in` at pinned `fc9369cb`, EXIT VALUE): list-with-
   command exits 0 iff the command is permitted, else 1 — **and** the same
   paragraph states sudo also exits 1 on authentication failure,
   configuration/permission problems, or when the command cannot be
   executed. So `exit 1` on `sudo -n -l -U <owned> -- /usr/bin/true`
   means {policy-deny, auth/config/permission/exec error}. A control pair
   (one known-positive, one known-negative, rechecked adjacently) proves
   only that the oracle returned 0/1 in two known states; it does not
   transfer the *cause* of a later 1 in the two actual Runner cases.
   Interpreting both actual `exit_nonzero` results as "denied" is therefore
   an inference, not an established discriminator. Spec fail, load-bearing.
2. NOPASSWD is not what list-mode exit 0 attests. Verified: `-l` without a
   command exits 0 iff allowed *and authenticated successfully as required*;
   `-l` with a command exits 0 iff *permitted*. No verified sentence states
   that root-issued `-n -l -U <owned> -- <cmd>` returning 0 implies the
   target account could execute without a password. Whether `-n` turns a
   password-requiring rule into list-mode failure is unstated in the
   verified excerpts, so the positive leg (exact narrow owned NOPASSWD
   rule present) is also unproven. Spec fail, load-bearing.
3. Query mode is not the execution path. List mode never executes the
   target; it cannot attest the Runner's actual exec conditions (binary
   resolution of `/usr/bin/true` at exec time, argv/env/cwd, PAM and
   timestamp state, plugin close/audit behavior). The proposal keeps
   descriptor binding and NNP Runner probes, but the *binding between the
   list answer and the exec outcome* is assumed, not documented. Checklist
   item unresolved.
4. Trusted-plugin risk is unclosed. Verified manual text: sudo has a plugin
   architecture for security policies; the default is sudoers via file or
   LDAP and third parties may supply plugins. The oracle trusts "installed
   sudo," but without the separate A11 plugin/route attestation (open and
   unchanged), a non-sudoers policy plugin or LDAP/SSSD route could answer
   the list query under semantics the cited manual lines do not govern.
   The retained provenance note already records that plugin arguments can
   override the sudoers file and that routing/include bytes are unverified.
   Checklist item unresolved.

Spec findings: 2 load-bearing (exit-1 cause ambiguity; NOPASSWD unattested
by list-mode 0), 2 unresolved checklist items (query-vs-exec binding;
answering-plugin trust). Worst: exit 1 cannot be claimed as "denied."

## Verified vs inference (explicit)

- Verified from the pinned primary source: the four list/exit/`-n`/`-U`/
  plugin statements quoted or precisely paraphrased above.
- Inference (not relied on for acceptance): that adjacent controls,
  bounded env/cwd/output, or fixed `/usr/bin/sudo` + `/usr/bin/true`
  paths remove the ambiguity. They narrow flakiness; they do not supply
  the missing cause discriminator.
- Not established anywhere in scope: distinct public stderr/exit literals
  separating deny from auth/config errors, or a documented `-U` NOPASSWD
  reporting semantic. The proposal rightly proposes none; inventing them
  here would reintroduce the version-fragile literal problem the negative
  provenance result already closed.

## Decision: NO-GO

The candidate `sudo -n -l -U <owned-account> -- /usr/bin/true` oracle plus
positive/negative controls plus adjacent NNP probes does not satisfy the
exact A7 brief, in either leg: the negative leg cannot distinguish
"without rule" from auth/config/exec failure on exit 1, and the positive
leg cannot establish "with NOPASSWD rule" from list-mode exit 0 issued by
root for another user. No binding amendment is named because none is
warranted: retain the current exact-byte qualification requirement
(complete consulted literal set, whole-directory includes, route/plugin
review, unknown-refusal latches) unchanged. No waiver, no new parser, no
audit pipeline, no product change. A7/A11/M2/full F/G/PA6 stay open;
installed/dedicated paths stay **unverified on real hardware**.

## Smallest practical alternative and exact minimal next slice

- Keep the existing automated NNP exec probes exactly as they are:
  functional fences, never cited as exact-policy proof.
- Do not build query-mode discriminators, literal matchers, or host audit
  collection.
- Minimal next slice (one concern, normal review gates, no owner admin
  step; authorized disposable-host operation only): pin the single fixed
  qualification image revision plus the exact generated account names,
  sole primary groups, and true-rule literal, then submit the delivery
  contract's separate bounded root-read qualification grant for one
  independently reviewed private read of the complete consulted literal
  set (sudoers file, all include names including ignored ones, sudo.conf,
  nsswitch passwd/group routing, resolved plugin paths, final
  account/group membership). Manual two-case review against the fixed
  names; publish only digests and pass/fail. If that read cannot be
  granted through normal gates, A7 stays blocked rather than weakening to
  the oracle.

Validation of this note: documentation only; UTF-8/LF; no tests, builds,
host reads, provisioning, push, or PR. Commit contains only this file.
