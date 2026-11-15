# A7/A11 GitHub-hosted achievability decision

2026-11-15, Anthropic claude-opus-5-5, high. Parent `1702c1a` (atop main
`b379837`). Static source/docs reading only: no run, dispatch, pin, bind, host,
account, mount, sudo or native operation. Sol partial `b0599dee` was not read,
adopted or discarded.

## Decision: GO for exactly ONE Opus-owned hosted attempt, then NO-GO on any miss

Source feasibility is established. Host acceptance is NOT. Nothing found in the
kernel, systemd, Runner, settings or product contract blocks two-account NNP
A7/A11 on a GitHub-hosted Ubuntu 24.04 VM with the D9c fixture. The only blocker
any run has observed is image state that the D9c view targets. No production,
host-permission or fence relaxation is needed.

## Evidence (actual source, not the mask proposal)

1. **Floor and Runner work on hosted VMs.** A1/A2, A3 Standard/None, A5 and
   D9a A4 earned hosted credit through the same disposable unit,
   delegated-cgroup Runner, generated accounts, Standard drop-in
   (`NoNewPrivileges=yes`, CapBnd `c1`, `ProtectSystem=full`) and owned
   cleanup. A7 adds only the probe, fixture views and E4 reentry.
2. **C1 identity is reachable.** D7h `43693642693`: installed sudo, plugin and
   util payloads, links and ancestors matched the reviewed Noble
   `1.9.15p5-3ubuntu5.24.04.4` facts (FILE leg only).
3. **The observed refusal is host C2.** `sudo_nnp.rs::runtime_package` (host
   side, before effects) calls `delivery_open("/usr/local/bin", true)`, which
   requires no group/other write. `driver.py::a7_alias_c2` opens
   `a7_c2_parent` first. D7j `43720496074` saw `/`, `usr` and `usr/local`
   pass and `usr/local/bin` fail the no-022 check. That is consistent with
   D7i's pre-mutation E_PATH. It is not proof of the D7i cause.
4. **The probe reaches `exit_nonzero` inside the D9c view.** In
   `unix_shim_rights_linux.rs`, `directory()` opens `O_RDONLY|O_DIRECTORY`,
   and `trusted()` checks root ownership and no 022. A root-owned 0755 empty
   view at `/usr/local/bin` passes, and `openat(sudo, O_PATH)` then returns
   ENOENT, which `fd()` maps to `"absent"`. The C3 `/bin -> usr/bin` alias
   is skipped, and C1 runs through `execveat(O_PATH fd)`. A setuid image
   under NNP keeps the caller's euid (kernel `no_new_privs` doc, ADR0020 D8).
   So sudo cannot become root, and both cases exit nonzero whatever the
   policy says. 0700 would give EACCES, which maps to `"unknown"` and
   refuses. 0755 is right.
5. **The view is mechanically possible.** systemd applies
   `BindReadOnlyPaths=` after `ProtectSystem=full`, on an existing mount
   point inside the unit namespace. The Runner's children share that
   namespace. A bind keeps the source's st_dev/st_ino and is marked `ro` in
   mountinfo and `ST_RDONLY` in statvfs. `controlled_mounts` only rejects
   `/etc/*` submounts, and `ProtectSystem` adds `/etc` and `/usr` lines,
   which it does not reject. The fx parent can stay 0700 because accounts
   reach the view through the bind root.

## Bounded route (Opus owns; Sol does not resume)

1. Opus/high builds a fresh D9c source slice from current main, not
   `b0599dee`. It touches only `sudo_nnp.rs` and the driver/test/mutation
   files, plus evidence:
   - an exact five-mapping set and fifth mount line;
   - a `c2-empty` root 0755 empty source, with held dev/ino and collision,
     reverse owned-cleanup and quarantine coverage;
   - an in-unit five-set/mountinfo/statvfs/dev+ino/empty/C2-ENOENT proof
     before Runner construction and before each call;
   - a host split: drop only the C2 parent from the controlled-fixture host
     checks, keeping C1, plugin, util, `/usr/bin`, alias and floor checks.

   Ordinary strict host gate, production rights, NNP and descriptor oracles
   stay unchanged.
2. Sol does the independent review. Then all five gates, coordinator merge,
   and a separately reviewed one-dispatch native disposition (D7k) on one
   new host. This decision grants no run.
3. Register these predictions before dispatch. Both accounts:
   `no_new_privs=true`, C1 chosen, C3 alias skipped, C2 absent through the
   view, `sudo=exit_nonzero`, and the identity/descriptor/cleanup oracles
   pass.

## Critical falsifiable limits (any one ends hosted A7/A11: NO-GO)

- Package or digest drift, an unsafe `/usr/bin` or alias, a floor mismatch,
  or any other pre-mutation refusal. A new image is not a fix.
- The in-unit proof fails: extra `/etc/*` or `/usr/local/bin` submounts, the
  view not RO, a dev/ino mismatch, a non-empty view or the wrong mapping set.
- An envelope, budget, cancel-join or cleanup is uncertain. No timeout or
  fence relaxation is allowed.
- Any unknown or unattributed result. There are no diagnostic sub-legs,
  read-only follow-ups or new grant names.
- `exit_zero` with NNP true is a product security finding, not a hosted
  issue. Stop and escalate.

The native budget is ONE attempt. Source-review corrections follow the
normal three-fix stuck rule, but they create no extra native attempt.

## If the attempt misses (pre-recorded NO-GO path)

A7/A11 stays **unverified on real hardware**, pending a dedicated Linux host
(systemd >= 254, kernel >= 5.14, cgroup v2, image without writable
`/usr/local/bin`). That needs the owner's host and is not an agent admin step.
Next steps from PROGRESS/brief, in order:

1. Remaining M2 work that does not depend on A7/A11: crash/A9/U9, A10, A12,
   full F/G and real-Runner PA6, each under its own bounded disposition.
2. Then independent M3 from KICKOFF: join codes (15-minute expiry,
   reuse/expiry rejection), rcgen cluster CA plus rustls mTLS, openraft
   state, and a host-A-to-host-B build. Acceptance: a 3-node CI cluster
   survives one node offline without lost writes.

M2 is not marked complete under either branch.
