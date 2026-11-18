# PR180 Windows executed-job timeout evidence

Sol6.1/high coordinator,2026-11-18. Hostedheadb1cf9d50abc87b7ac3858dad3a1daba3e9a9613b,
CI44279691035 attempt1 concludedcancelled. Windowsjob134716782048 ran09:50:34–
10:02:38Z724s. GitHubcheckannotation explicitly states the job exceeded12m0s;
.github/workflows/ci.yml timeout-minutes12 unchanged. This is an executedjob
budget timeout, not runner0/steps[] prestart infrastructure and not a passinggate.

Windows fmt/ClippyPASS, nextest662/662PASS36skipped385.336stest-only. Nexteststep
start->summary487.41s vs310.29s on greenmain6bf8f46 (samecounts,test-only230.046s).
npmci began10:01:31.187Z and was cancelled10:02:34.517Z. Node/UI/build/smoke and
optionaldiagnostic sample unexecuted. No founder failure is established here.
The timing does not establish npm, a particular test, runner health or source as
cause. Ubuntu job134716782235SUCCESS291s; macOS134716781990SUCCESS431s.
Source outside docs is byte-identical to greenmain6bf8f46. Hosted cancelledhead
is distinct from localcheckpoint0c354674. No same-head rerun/dispatch/cancel byroot.

Boundedlogs and job/annotation JSON retained onlyD:/Temp/arx-pr180-b1cf9d50-hosted.
Eachlog<8MiB. Independent Astra6.2/high read-only disposition verified Windows
and main log hashes and classified timeout/unknowncause without a runtimefix.
Allfive gates/freshgreenmain still mandatory; PR180 staysheld. No exemption,
settings/timeouts/quarantine tweak or docs-only trigger push authorized/performed.
Sol3/3exhausted/Opus0/2 unchanged; M2OPEN/M3UNCHECKED, hardware unverified.

Separately authorized genuine purecodec source work can proceed locally under
acceptedADR0026/brief; this is not timeout repair or new diagnostic. Its substantive
newhead needs independent actualsource review and normalfirstattemptCI. Preserve
this cancellation as failedmergeeligibility even if futurework passes. Existing
babysitter/ownerPCfences unchanged; no native/admin/VM/cache/userdata action.

## Local artifact SHA256
f35bedb9d18a19ab8a7c8f04af85455cfeba4e91ec2c959971016ff52829dc79  macos.log (230082 bytes)
64dfdf64339ee23f29b7bc3b2f2cb64f480686e5fed1949c7126bb269a2f4359  ubuntu.log (249946 bytes)
9285a9a5ed19b2043481f617c65b8e7a5a12c9618442e988cdb78cd2c4e225b8  windows-annotations.json (1208 bytes)
796ebc67ec771c0d8d921ca004f14588ccc70d19d2c1b8226ab25b453cd7c0f5  windows-job.json (6691 bytes)
1bb010e75dfe6507c770e6345a87e004cda3f5d0c7984ad4888e1f4400c5c9c2  windows.log (222755 bytes)
