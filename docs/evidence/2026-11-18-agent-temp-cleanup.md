# Attributable agent Temp cleanup

Coordinator, 2026-11-18. The owner-relayed disk correction stopped new builds and
tests while C: agent outputs were inventoried and removed. No build/test ran as
part of cleanup. All cleanup scratch and manifests are on D:.

| UTC observation | C: free bytes | Decimal GB |
| --- | ---: | ---: |
| 11:07:41 before | 4,711,038,976 | 4.71 |
| 12:12:15 after three targets | 18,877,157,376 | 18.88 |
| 12:21:30 after remaining outputs | 69,358,583,808 | 69.36 |

These are volume measurements. The full increase cannot be attributed to this
cleanup; concurrent disk activity occurred. Large builds remained paused at the
18.88 GB observation. The final observation clears the owner's 20 GB threshold.

Three task-attributed `target` subtrees under `%LOCALAPPDATA%/Temp` were deleted:

| Task root | Logical bytes | Files |
| --- | ---: | ---: |
| arxcovo-founder-bbe9-observer | 5,809,664,065 | 4,110 |
| arxcovo-review22-12b3fd92f9c14736b99e7db119b6deed | 3,461,953,634 | 4,000 |
| arxcovo-pr22-recheck-uypikbwi | 3,461,803,766 | 4,000 |

Total target output: 12,733,421,465 logical bytes. Attribution used Arxcovo source
metadata/task evidence; founder provenance is also recorded in
[the earlier observation](m3-ubuntu-founder-branch-observation.md). Resolved paths
and parents were checked, recursive inventories found no reparse points or cache
paths, and no process using those three roots was observed before removal.

Another 88 top-level task entries (26,320,108 logical bytes) and eight founder
logs were copied to `D:/Temp/arx-c-temp-cleanup/preserved-final`, SHA256-checked
file by file, then removed from C:. Two inventoried entries disappeared before
our archive pass: the pure-mutation directory and the remaining review22 source
copy. They are recorded as `already_absent`, with zero deletion credit. An earlier
archive pass stopped on a missing entry before its deletion loop; partial D:
copies remain in `preserved`.

The two remaining `arx*` Temp entries are the founder root containing only its
preserved `cargo` dependency cache and `arx-cache-metadata.json`. No cache, other
Temp entry, user file, Lab Manager state or VM data was deleted. Inventory and
archive checks rejected reparse points, caches, backups and VM disk extensions.

Local manifests under `D:/Temp/arx-c-temp-cleanup`:

| File | SHA256 |
| --- | --- |
| before.json | db98801211ac3a5fc9e057d5c70a26be99ebebac19b88a4bc4b8b27d88201515 |
| approved-targets.json | e49289a23fe157227a2b785568a1288ba04dec38729045c1c2c5a89ec1c92db8 |
| other-results.json | 7dd38c4d16fb9721fe7abae5a6bed8d61d7799d235a080fcb07881ccb9a67268 |
| after-final.json | 3b4538f7026c6089477c2c9ee19ca29fc4e0651b684128e2be8582d5025ad524 |

Prevention: [AGENTS.md](../../AGENTS.md) requires explicit D: environment in every
command and delegate brief; [the scratch guide](../dev-scratch.md) gives a
checkable template. No global environment/admin changes were made.
