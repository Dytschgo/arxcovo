# Controlled A7 policy fixture: independent Sol review

2026-11-15. Sol coordinator reviews Muse architecture `dec7f9b` against
ADR0020 D8/R5 and brief0020 A7. Standards PASS; Spec PASS with the case-name
correction below; Security PASS subject to the binding native prerequisites.
No tests, host reads, native execution or grant. Documentation only.

A7 explicitly establishes an in-fence exit-status observation with and without
NOPASSWD, not a host sudoers policy proof. A source-controlled policy visible to
the real installed sudo through the unchanged NNP Runner preserves that intent.
Both generated account cases, descriptor execution, identity/limits, exact result
parsing and owned cleanup remain. The rejected list-mode shortcut stays rejected.

Independent primary [systemd.exec documentation](https://www.freedesktop.org/software/systemd/man/latest/systemd.exec.html)
confirms BindReadOnlyPaths creates unit-specific read-only bind mounts that are
not visible in the host mount table and implies a private mount namespace;
ProtectSystem=full makes /etc read-only. Bind definitions append, so exact-set
validation must reject inherited/extra mappings. These semantics support the
proposal on paper; actual view, read-only status and literal bytes must be
verified from inside the disposable unit before either Runner case. This is
not evidence that any hosted mount or policy has worked yet.

One exact transcription error is corrected: binding3 must exclude a grant for
case1-name-or-group, not case0, because the sole intended NOPASSWD rule grants
case0. No security intent or acceptance scope is reduced by this correction.

The amendment must retain all four masks with no optional/missing-source form,
source literals and exact include set, local plugin and files-only NSS routing,
root-only ownership, installed binary/plugin digest+identity and compiled-default
path checks, floor checks and unchanged packaged hardening. Unknown defaults,
plugin drift or broken account lookup refuse; hashes alone are not semantic
policy proof. No host policy modification, raw-byte capture or production flow.

GO for docs-only binding amendment through normal independent exact-head review,
five green gates and coordinator merge. Implementation only after that merge;
new native grant and run/source/oracle assessment still required for acceptance.
No current gate is opened. A7/A11/M2/full F/G/PA6 remain open; installed/dedicated
paths are **unverified on real hardware**.
