# M2 A7/A11 next effective step: bounded disposition (b)

Scope: parent `26d2758` atop main `18b8fef` (PR130 all-5-green,
independent PASS, reviewed tree `291a08a` = `215d75d`). Read AGENTS/KICKOFF,
ADR0019 D7i (consumed), ADR0020 D9b, retained run `43715436515` record and
disposition, path-refusal diagnosis, Muse path-attribution review, package/D9b
and delivery-contract sources. Public image/source facts only; no host, root,
proc, sys, policy, account, sudo, dispatch, or variable queries. A7/A11, M2,
full F/G, real-Runner PA6 remain **unverified on real hardware**.

## Observed host fact (closed)

Run `43715436515` attempt1/job`132815838689` failed in 18s with first = final
= `E_PATH`, no role, mutation `none`, cleanup `not_reached`, internal null.
D7i is consumed; the host is excluded; no retry or redispatch is authorized.
PR130 merged only the four closed pre-mutation role labels
(`a7_c2_parent`, `a7_alias_root`, `a7_alias_target`, `a7_c2_image`); all fences
unchanged. No source bug is proved; qualifying mock metadata still admits, so
controls admit valid mock metadata and the historical cause stays unknown.

## Public-source finding: qualifying fresh default is impossible

Pinned runner-images `95e6f51a068507e8f43d061d2166a39d0ef36ce8` builds from a
`latest` source-image variable with unpinned APT upgrade and unestablished
final provisioning literals; its software report lists sudo
`1.9.15p5-3ubuntu5.24.04.4` but no complete policy snapshot. Exact conflict: a
recipe SHA plus a package version does not pin installed policy bytes, include
names, nsswitch routing, plugin bytes, or account grants, so case1 no-grant
closure is unprovable on defaults. No path-fence relaxation or host mutation is
assumed. Smallest fixture-local alternative preserving actual production NNP
oracles: ADR0020 D9b controlled fixture (B) — four `BindReadOnlyPaths`
literals over installed real sudo/plugin, real-descriptor C1 alias `O_PATH` +
`execveat`, NNP, 64MiB/swap0/pids32 — for a later execution grant, not this one.

## Decision: (b), finite read-only admission-only leg

A full fresh combined attempt (a) needs D9b wiring + qualification + execution
grants and still cannot qualify on defaults per the conflict above. (b) answers
the useful question — which call class refuses on current image lineage —
with stat-only admission (C2/alias ancestry, `/bin` alias, C1/C2 presence)
under merged role attribution, no provisioning, accounts, sudo execution, or
policy reconstruction. If the fresh host admits, the historical metadata
differed (cause still unknown); if it refuses with a role, that fence class is
systematic. Either outcome earns no acceptance and proves no historical cause.

## Exact binding text for the minimal ADR disposition candidate (D7j)

New explicit one-shot read-only grant on one fresh disposable host, pins:
source SHA == workflow SHA, runner-images revision above re-verified (never
assumed), sudo `1.9.15p5-3ubuntu5.24.04.4` amd64 member digests (`sudo`
`6ae6665c…a31ff`, `sudoers.so` `151d681d…5d4c`, `libsudo_util.so.0.0.0`
`aae167b9…e26d`), one dispatch, admission-only driver path, bounded capture of
codes/roles/digests/flags only. Refuse on any drift, unknown include/route/
plugin, or extra mapping; exclude the host after; D7i stays consumed and this
grant is not an implicit retry. Requires different-provider exact-head review
and green required gates before any execution; coordinator merges then wakes.
Follow-on only: D9b fixture execution grant with both-case oracles.

## Hypotheses vs facts

Hypothesis: one protected ancestor or alias open refused historically. Public
fact: recipe + version cannot close policy. Observed host fact: closed `E_PATH`
record above. Source cannot prove the historical cause; this leg only labels
the fresh-host fence class. D7a-i consumed; no optional audit infrastructure.
