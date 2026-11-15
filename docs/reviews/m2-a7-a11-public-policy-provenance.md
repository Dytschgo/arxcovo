# A7/A11 public policy provenance: bounded research

Sol / gpt-6.1-sol / high; 2026-11-15. Public-source research only.
Read AGENTS/KICKOFF, delivery-contract qualification/binding corrections, and
linux-a7-a11-qualification-source. No host or policy attestation or selection grant.

## Result

**No complete reviewable closed public literal candidate was established.**
The pins below identify useful public inputs, not installed-image equality.
This is a bounded negative result, not proof that public reconstruction is impossible.
No literal allowlist, runtime semantic parser, collection or execution is authorized.
A7/A11 remain blocked and **unverified on real hardware**.

## Fixed public facts and exact references

- Ubuntu's [Noble source index](https://packages.ubuntu.com/en/source/noble/sudo)
  lists source `1.9.15p5-3ubuntu5.24.04.4`, producing sudo, sudo-ldap and libnss-sudo.
  Its versioned [source descriptor](https://archive.ubuntu.com/ubuntu/pool/main/s/sudo/sudo_1.9.15p5-3ubuntu5.24.04.4.dsc),
  [Debian delta](https://archive.ubuntu.com/ubuntu/pool/main/s/sudo/sudo_1.9.15p5-3ubuntu5.24.04.4.debian.tar.xz)
  and [upstream archive](https://archive.ubuntu.com/ubuntu/pool/main/s/sudo/sudo_1.9.15p5.orig.tar.gz)
  are linked there. Archive bytes/signatures/checksums were NOT retrieved/verified.
  A package version is an input pin, not a reviewed literal or binary digest.
- The [Noble sudoers manual](https://manpages.ubuntu.com/manpages/noble/man5/sudoers.5.html)
  identifies that package version; policy may use local files or LDAP, and plugin
  arguments can override the sudoers file. Includes can themselves include files;
  directory inclusion ignores certain names. Hence the contract correctly requires
  the whole directory set, including ignored names, and rejects unknown includes.
- Upstream tag SUDO_1_9_15p5 resolves to commit
  `fc9369cb442c8c5f6612d1831294569f1db75028` ([tag API](https://api.github.com/repos/sudo-project/sudo/git/ref/tags/SUDO_1_9_15p5)).
  Its [sudo.conf template](https://github.com/sudo-project/sudo/blob/fc9369cb442c8c5f6612d1831294569f1db75028/examples/sudo.conf.in)
  has commented default plugin directives and configure substitutions, not final
  Ubuntu file bytes. Its [NSS implementation](https://github.com/sudo-project/sudo/blob/fc9369cb442c8c5f6612d1831294569f1db75028/plugins/sudoers/sudo_nss.c)
  supports files and conditional LDAP/SSSD routes; the nsswitch branch falls back
  to files when no source is selected. Compile options and distro patches matter.
- The [Noble sudo.conf manual](https://manpages.ubuntu.com/manpages/noble/man5/sudo.conf.5.html)
  documents default policy/audit/I/O use of sudoers when Plugin directives are
  absent, configurable plugin paths, and static/dynamic/adaptive group lookup.
  This describes routing semantics; it does not pin the installed sudoers.so bytes.

## Runner-image provenance (one public revision, not a selected execution image)

All runner-images links below pin `95e6f51a068507e8f43d061d2166a39d0ef36ce8`
([commit](https://github.com/actions/runner-images/commit/95e6f51a068507e8f43d061d2166a39d0ef36ce8), 2026-11-13).

- [Ubuntu24 locals](https://github.com/actions/runner-images/blob/95e6f51a068507e8f43d061d2166a39d0ef36ce8/images/ubuntu/templates/locals.ubuntu.pkr.hcl)
  select Canonical's ubuntu-24_04-lts server SKU for x64.
  [Source](https://github.com/actions/runner-images/blob/95e6f51a068507e8f43d061d2166a39d0ef36ce8/images/ubuntu/templates/source.ubuntu.pkr.hcl)
  uses a source-image version variable; [defaults](https://github.com/actions/runner-images/blob/95e6f51a068507e8f43d061d2166a39d0ef36ce8/images/ubuntu/templates/variable.ubuntu.pkr.hcl)
  set that to latest, winrm_username to packer, install_user to installer.
  These are build defaults, not proof of final runner account names/groups/grants.
- [24.04 build recipe](https://github.com/actions/runner-images/blob/95e6f51a068507e8f43d061d2166a39d0ef36ce8/images/ubuntu/templates/build.ubuntu-24_04.pkr.hcl)
  runs many privileged provisioners and ends with waagent deprovision+user.
  [APT configuration](https://github.com/actions/runner-images/blob/95e6f51a068507e8f43d061d2166a39d0ef36ce8/images/ubuntu/scripts/build/configure-apt.sh)
  updates indexes and upgrades packages without pinning sudo's version.
  [Software report](https://github.com/actions/runner-images/blob/95e6f51a068507e8f43d061d2166a39d0ef36ce8/images/ubuntu/Ubuntu2404-Readme.md)
  lists sudo `1.9.15p5-3ubuntu5.24.04.4`; it contains no complete policy snapshot.
- [Post-generation linger script](https://github.com/actions/runner-images/blob/95e6f51a068507e8f43d061d2166a39d0ef36ce8/images/ubuntu/assets/post-gen/systemd-linger.sh)
  derives a UID from the last passwd entry. It supplies neither a fixed final
  account/group manifest nor sudo grants. Final provisioning literals were not
  established here; no claim that they are necessarily nonpublic is made.

## Missing bytes, inference and proportionate next step

Ubuntu/Launchpad Git, Debian Sources/Salsa file fetches failed as inaccessible;
the descriptor fetch reported unsupported content-type. Therefore Ubuntu's exact
sudoers/sudo.conf, packaging substitutions/patches and default root/admin/sudo
grant literals were NOT verified. Do not substitute a familiar sample file.
Also missing: exact nsswitch bytes (including passwd/group/netgroup routing),
complete include names+bytes after image/job provisioning, resolved plugin paths
and trusted binary bytes, and final build/runner account membership and grants.
Mutable marketplace/APT inputs mean the recipe SHA cannot fill those gaps.

Inference: a manual independent review of a complete *fixed* local literal set
could prove case1 has no applicable grant without a runtime general parser.
It must enumerate every user/group/UID/GID selector, alias, include and default;
exclude external policy/group/netgroup routing; and bind generated collision-free
accounts to their sole primary groups. Sole-group membership alone proves nothing
about ALL/direct-user/numeric grants. Case0 additionally needs its exact generated
true rule; the existing driver literal alone cannot prove case1 absence.

Smallest next step: finish public package-byte retrieval/review if feasible, with
the missing final-image set explicitly unresolved. If public literals cannot close
it, use the delivery contract's separate independently reviewed bounded root-read
qualification grant and explicitly approved private review channel for exact bytes;
release only fixed results/digests. That channel is not settled by this note.
No new upload/storage system or host collection is proposed. Plugin trust and the
read adapter still need narrow review; the current collector cannot attest plugins.
Only later independently reviewed complete literals permit in-memory equality;
any unknown file/route/plugin or byte drift refuses. Preserve all current latches.

Validation: documentation only; UTF-8/LF and staged diff checks recorded at commit.
No tests/builds, host reads, account/sudo/admin operations, dispatch, push or PR.
