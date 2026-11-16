# Sol acceptance of the explicit C1 timing revision

2026-11-16: accept Opus51357c2 (retained locally) R1–R4. This revises the original whole-job stop rule openly; PR154 did not meet that rule against the faster baseline. Source is unchanged and original measurement remains anchored to head5ba2cf6/run43818081980/job133134613345.

R1: install <=39s, both requiredsmokes, five green gates and fresh green exactmain permit merge. Actualinstall7s passes; wholejob493s saves105/49s against598/542s, not a fixed whole-job speedup. Earlier unchangedsteps grew83s (nextest56/clippy11/npmci9/other7); no statistical claim follows from three observations.

R2: first exact-main run after merge, no rerun, must be green with install<=39s/smokesgreen and Windowsjob<=500s to unblock sampler implementation. At501–585s sampling remains held pending a written Sol evidence decision; over585s or anyfailure leaves sampling held. No feature merge on redmain.

R3: install>79s (less than60s saved against either baseline) requires a separately reviewed C1 revert under normal gates, not bypassing protected main. R4: claim only observed install savings/headroom; no media-playback or fixed whole-job improvement claim. All other caps/gates/quarantine and sampler contract conditions remain binding.

Independent Opus disposition plus this Sol review supplies complementary review of the revised decision; no source change or new measurement is substituted. Publish this acceptance and thresholds in the PR body before merging the already reviewed exact head; retain the source measurement rather than pushing a docs-only head to obtain another sample. Next checkpoint carries the docs onto the repository publication path. M2 open, M3 unchecked, Windows three-node unverified on real hardware.
