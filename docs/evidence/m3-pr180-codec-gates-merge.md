# PR180 codec gates and merge

Coordinator Sol6.1/high,2026-11-18. Exactreviewedhead
548a22ff326f2f27427e1b793de9f7a8a530db3e, CI44284448547 attempt1SUCCESS.
528s wall (10:20:20–10:29:08Z); Windows512s<=feature660s, Ubuntu243s/macOS331s.
Allfive requiredchecksSUCCESS; strictprotectionunchanged. IndependentSol source
review covers exactrestoredcodec and priorrevieweddocs; actualmoduleblob9322c5c6.
Verified each of8namedcodec testsPASS exactlyonce on EVERYOS. Node48/48eachOS.
FullRust Windows670PASS/36skip, Ubuntu662PASS/22skip, macOS630PASS/8skip.
No localpostrestore runtime rerun; hosted compiled reviewedbaseline.

Perjoblogs bounded<8MiB, retainedD:/Temp/arx-pr180-548a22ff-hosted:
windows249040bytes SHA2569c3225ebea11deba837c4ae130350c1c7ec5f0cd75a9776e541055e242cb0b61
ubuntu251581bytes SHA25698a07cd4fa7eef8e7c936a3c2f80d1675736be1c3560ccb68d02e756099c45fd
macos231642bytes SHA256e919af14a1361d9f8b9b6454909e04ae024a0d9b2aee3d9127d874229c7968df
OptionalWindows8cases all_pass, nextest0,wall28765ms,orphans0,e_s492. No stability/
re-enable/fixcredit; earlierdocshead12m cancellation and PR179negative sample
remainretained, underlyingcausesundetermined. No retry/rerun or timeout/quarantine/
settings/capacity changes. This is purecodec, not transport or timeoutrepair.

Immediatelybeforemerge, live main6bf8f46bc87a05bed01e342c1a37f68427e4c16b,
CI44276313116 attempt1GREEN. PR exact548a22ffCLEAN/allfiveSUCCESS rechecked.
ExpectedSHA guarded normal squash mergedPR180 at10:31:29Z as
8b36405e932ef2921a3c8d71dbaf7ed001314de7. Live mainmatches, no bypass/admin.
Newmain ownfirstCI pending; no nextfeaturemerge until exactmainGREEN.

OwnerPCchecklist/ADR0023/containerplan/ADR0026 and purecodec now merged.
NativePC acceptance stillwaits verifiedStandardaccount/caps/reserves. Future
nonblockingregistrycandidate remains D:/Temp/arx-peer-registry-design.md, notbuilt
oracceptedsource. LiveTLS ownership/actualtransport/join/custody/clock/container
proofs remain. M2OPEN/M3UNCHECKED, Sol3/3exhausted/Opus0/2unchanged. Hardware
**unverified on real hardware**. Existingbabysitter and allfences unchanged.
