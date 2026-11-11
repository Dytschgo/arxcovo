# Review: Ubuntu signed-mirror mitigation candidate

- **Target:** unpublished local branch `ci/ubuntu-signed-mirror`
- **Base:** `b475bf1f8747d7f7b8e7cb10e1e063760d299ae6`, the published PR32 diagnostics
- **Head:** `a97fd7469cd49da549a5a0b40ae8ad0b83095964`, containing `79463df` and `a97fd74`
- **Diff:** five files, 380 insertions: `ci.yml`, `PROGRESS.md`, `tools/ci/ubuntu_mirror.py`, `tools/ci/test_ubuntu_mirror.py` and `tools/ci/ubuntu-mirror.md`
- **Builder:** Sol (Codex)
- **Reviewer:** Claude Opus 5.5 at high effort, working as a single agent with no subagents
- **Inputs:** `AGENTS.md` and `docs/KICKOFF.md` (lines 62–64 and 136) as the standards, the Fowler smell baseline, and the coordinator brief as the spec
- **Scope:** this review commit adds only this file. No source file was edited, and nothing was pushed or posted to GitHub.

## Verdict: CHANGES REQUIRED

The candidate is fail-closed, and I found no gate weakening and no secrets. On the observed runner image, however, it changes nothing and then refuses. This turns the job red at the new step, so it cannot unblock CI. See finding B1.

## Evidence gathered

- I read completed job `130635247058` (run `43053098205`, head `b475bf1`) through read-only API GET requests. It ran on image `ubuntu-26.04` / `20261101.186.1`. The DNS observation at log line 424 confirms three Ubuntu timeouts, while the Kernel Archives, GitHub and Microsoft controls resolved.
- In the same log, the Playwright `apt-get update` output shows `Get:1 file:/etc/apt/apt-mirrors.txt Mirrorlist [144 B]`. It also shows `W: Failed to fetch mirror+file:/etc/apt/apt-mirrors.txt/dists/{resolute,resolute-updates,resolute-backports,resolute-security}/InRelease`.
  - All four Ubuntu suites, including `-security`, use the URI `mirror+file:/etc/apt/apt-mirrors.txt`, not a direct Ubuntu host.
  - The 144 bytes match the three runner-images lines exactly (`http://azure…/ubuntu/\tpriority:1`, `https://archive…\tpriority:2`, `https://security…\tpriority:3`).
- I fetched these files with a public read-only GET: `https://mirrors.edge.kernel.org/ubuntu/dists/{resolute,resolute-security}/Release`.
  - Both have `Origin: Ubuntu`, with `Architectures` including `amd64v3` and 72 `binary-amd64v3/Packages` entries. The runner fetches `amd64v3` indexes.
  - The `resolute-security` file is dated 2026-11-11 07:34 UTC.
  - The mirror is therefore a viable target. I did not verify the signatures locally.

## Standards (AGENTS.md / KICKOFF.md / smell baseline)

| # | Sev | Finding | Ref | Root cause → fix |
| --- | --- | --- | --- | --- |
| S1 | Medium | The plan, recheck and publication loop in `apply()` (`ubuntu_mirror.py:150–186`) has no behavioral test. Seven mutants survived the unchanged suite: M1 (empty-plan refusal), M2 (fresh bytes), M3 (fresh identity), M4 (mode preservation), M5 (32-file cap), M6 (parent-directory guard) and M12 (rewritten-size bound). | AGENTS "Every behavior needs a meaningful test" | The roots are hardcoded, and only the context refusal is tested. → Inject the roots and `read_source` into a private `_apply(roots, reader)` function, then test it against an owned temp tree with synthetic facts. The scratch harness `probe.py` shows this works portably. |
| S2 | Low | deb822 tests cover `Trusted`, `Allow-Insecure` and `Check-Valid-Until`, but not `Allow-Weak` or `Allow-Downgrade-To-Insecure`. Mutant M7 survived. | Same | The test table is incomplete. → Add both fields. |
| S3 | Info | Root runs `/usr/bin/python3 tools/ci/ubuntu_mirror.py` without `-I`/`-B`, so the script directory comes first on `sys.path`. Sudo already resets `PYTHON*`, and the code is repository-controlled. | AGENTS validation/safety posture | → Use `/usr/bin/python3 -I -B`. |
| S4 | Info | `read_source` opens the file before `fstat`. A FIFO named `*.list` would block until the 12-minute job timeout. Only root can create one. | Same | → Add `O_NONBLOCK` to the open flags. This sits inside the stated "root writers outside guarantee" boundary. |
| S5 | Info | In `PROGRESS.md`, the second bullet says "unactivated", while the fourth bullet adds the activation step. | AGENTS "Keep PROGRESS current" | → Reword the second bullet as "not yet executed". |

Smell baseline: I found nothing material. `rewrite_list`/`rewrite_sources` share a trust shape across the two formats, which is intentional per format.

## Spec (coordinator brief)

| # | Sev | Finding | Spec line | Root cause → fix |
| --- | --- | --- | --- | --- |
| **B1** | **Blocking** | On the observed image, the helper rewrites nothing, raises `Refusal` on the empty plan and exits 1 with `ubuntu-mirror-refused`. The job fails at the new step, and the Ubuntu indexes would still go through `apt-mirrors.txt` to the failing hosts. The scratch proof is P1/P4: a fixture reconstructed from the log goes through the real `main()`/`apply()` with synthetic root facts and gives exit 1. Control P5, with direct Ubuntu URIs, gives exit 0 and the expected bytes. | "Purpose unblock CI without weakening gates"; "changes only allowlisted http/https Ubuntu URI values" | `replacement()` treats `mirror+file:` (`hostname=None`) as foreign. The design assumed direct host URIs. → Minimal fix: treat only the exact value `mirror+file:/etc/apt/apt-mirrors.txt` as a target. Before planning, read `/etc/apt/apt-mirrors.txt` with the same `read_source` guards, and require every non-comment line to be `<allowlisted http/https Ubuntu URI>[\tpriority:N]`. Refuse otherwise. Then replace the URI token in the sources file with `MIRROR`, leaving the mirror file untouched. Add a fixture regression test of the observed configuration (P1/P4), and update `ubuntu-mirror.md` and `PROGRESS` to describe the indirection. |
| S6 | Low | The legacy `trust()` function matches only `=yes`/`=no`. `[trusted=true]`, `[trusted=1]`, `[allow-insecure=true]` and `[check-valid-until=false]` are accepted and moved to the new mirror (probe P2). APT boolean parsing also accepts `true`/`1`/`on`; I did not exercise this against real APT. deb822 is already strict (any value other than `no`/`yes` is refused). | "Unsafe trust options … refuse" (`ubuntu-mirror.md`) | → For legacy files, refuse any trust key with a value other than `no`, or `yes` for `check-valid-until`, matching deb822. This only preserves existing options, so it does not add new trust. |
| S7 | Info | A lowercase `uris:` stanza is left unchanged (P3), because the URI regex is case-sensitive while field parsing is not. This fails open to no change, not to a trust change. | Same | → Optionally use `(?im)`. |

What is correct against the spec:
- The fixed HTTPS Kernel mirror, and exact-host URI allowlisting that excludes userinfo, ports, other paths, queries and fragments.
- Suites, components, options and comments stay byte-identical. Third-party sources are preserved.
- No keys, insecure flags, DNS or IP changes, retries, timeout changes or skipped checks are introduced.
- An absent Signed-By retains global trust, and the documentation says so honestly.
- Bounds of 64 KiB and 32 files, no-follow open, root ownership, `nlink==1`, and no group/other write.
- The whole plan is built before the first write. Bytes and identity are rechecked fresh, and staging happens in the same directory.
- Errors print fixed tokens only, with no paths or private output.
- The step is Linux-only, sits before the unchanged Playwright step and preserves only four environment variables. All original steps are unchanged.
- The documentation correctly calls the intent guard spoofable and discloses partial non-rollback.

## Checks actually run

- The candidate's 11 portable tests, on scratch copies with local Windows Python 3.12.10 and `-B`: **OK**.
- Probes P0–P5 through `tools/probe.py`, run in the session scratchpad on an owned temp tree with synthetic root facts and patched `fchmod`. No APT file, root process or sudo was involved.
- 12 mutants on scratch copies through `tools/mutate.py`.
  - Killed (4): M8, M9, M10 and M11, matching the builder's own four.
  - Survived (8): M1–M7 and M12.
- `git diff --quiet a97fd74 -- tools/ci` confirmed the source was unchanged after the runs.
- **Not run:** native `apply()` as root, the real helper CLI, publication, sudo, real APT, or any hosted run. Root publication and package installation through the mirror remain **unverified on a real hosted runner**.

## Before/after and reversibility

- **Before:** the Playwright `--with-deps` step fails on Ubuntu DNS.
- **After, as reviewed:** the new step fails first with the same red result, and it changes no files.
- **After the B1 fix (expected, unverified):** the Ubuntu indexes come from `mirrors.edge.kernel.org` and are verified with the explicit Ubuntu keyring.
- **Reversibility:** this is a two-way door. It is CI-only, mutates only a disposable runner, and can be reverted with a single revert of the commit.

## Required hosted acceptance (after fix, on exact published head)

1. The `ubuntu-26.04` job prints `ubuntu-mirror-updated:N` with N ≥ 1.
2. In the Playwright `apt-get update`, there are `Get:` lines from `https://mirrors.edge.kernel.org/ubuntu` for all four suites. There is no `NO_PUBKEY`, no `insecure`/`not signed` warning and no `W: Failed to fetch` for Ubuntu suites.
3. Chromium dependencies install, and the smoke and Linux Electron smoke tests pass. Windows and macOS are unaffected.
4. The job stays within its 12-minute timeout, and `ci-time-budget` and `agent-review` are green, with a different-provider recheck of the exact published head.
