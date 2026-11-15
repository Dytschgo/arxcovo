# A7 policy qualification: smaller alternative for independent review

Coordinator Sol proposal, 2026-11-15. Not accepted; no contract or gate changes.
Public provenance research `00e4b2a` did not establish complete runtime literals.
The current exact-byte qualification requirement therefore remains unresolved.
User priority is working M2 paths with proportionate automated fences.

Consider replacing full policy-byte reconstruction with a fixed effective-policy
oracle on only the two newly owned disposable accounts. Retain trusted installed
sudo, source/image provenance, account/group ownership, NNP Runner, descriptor
binding, deadlines, private bounded capture and owned cleanup. Do not change
product behavior or add a policy parser, upload system, or host audit framework.

Candidate root-side query before service start: trusted fixed `/usr/bin/sudo`
with list-only `-n -l -U <owned-account> -- /usr/bin/true`, empty controlled env,
fixed cwd, bounded lifetime/output, no target execution. Require one known
positive control for the exact narrow owned NOPASSWD rule and one known negative
control without that rule. Recheck controls on the same host adjacent to actual
NNP probes, then both actual Runner cases must produce sudo exit_nonzero.
Never release output, usernames, IDs or policy text; preserve only fixed results.
No query or provisioning is authorized by this proposal.

Primary upstream [sudo manual at pinned source](https://github.com/sudo-project/sudo/blob/fc9369cb442c8c5f6612d1831294569f1db75028/docs/sudo.man.in)
describes list-with-command as a policy check and documents failure status1
for a forbidden command. This is a source lead, not a proof that every status1
means denied: authentication, plugin, configuration and account errors must not
be silently converted into the negative-policy control. Root query also does
not automatically prove NOPASSWD behavior for the target account. These are
explicit unresolved review questions, not assumptions used for acceptance.

Independent architecture/security review must decide whether a documented,
fixed public output/exit discriminator or calibrated control sequence can
establish the exact A7 two-case requirement without policy text. If yes, name
the minimum binding contract amendment and tests; if no, explain the concrete
ambiguity and retain current qualification requirements. No spec waiver or
native authority is implied. A11 installed ELF/merged-usr/descriptor evidence
is separate and unchanged. A7/A11/M2/full F/G/PA6 remain open and installed/
dedicated paths are **unverified on real hardware**.
