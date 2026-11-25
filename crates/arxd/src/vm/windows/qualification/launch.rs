//! Private construction boundary for the deferred TEST recipe collectors.
#![allow(dead_code, reason = "recipe collectors wire this entry in PR 2")]

use super::*;

/// Run- and generation-bound ownership of a disposable workspace and its
/// exclusive slot file. Replaces bare mutable paths on the native launch edge.
pub(in crate::vm::windows) struct WorkspaceCustody<'a> {
    run: [u8; 16],
    generation: u64,
    root: &'a PathPin,
    slot: &'a File,
}

impl<'a> WorkspaceCustody<'a> {
    fn new(
        run: [u8; 16],
        generation: u64,
        root: &'a PathPin,
        slot: &'a File,
    ) -> Result<Self, NativeRefusal> {
        let custody = NativeRefusal::PathCustodyUnverified;
        if run == [0; 16] || generation == 0 {
            return Err(custody);
        }
        qualification::inventory::binding(root)?;
        let (slot_id, _) = file_identity(slot, false).map_err(|_| custody)?;
        if slot_id.volume != root.identity.volume {
            return Err(custody);
        }
        Ok(Self {
            run,
            generation,
            root,
            slot,
        })
    }

    pub(in crate::vm::windows) fn verify(&self) -> Result<(), NativeRefusal> {
        let custody = NativeRefusal::PathCustodyUnverified;
        qualification::inventory::binding(self.root)?;
        let (root_id, _) = file_identity(&self.root.leaf, true).map_err(|_| custody)?;
        let (slot_id, _) = file_identity(self.slot, false).map_err(|_| custody)?;
        if root_id != self.root.identity || slot_id.volume != self.root.identity.volume {
            return Err(custody);
        }
        Ok(())
    }

    pub(in crate::vm::windows) fn volume(&self) -> u64 {
        self.root.identity.volume
    }

    pub(in crate::vm::windows) fn path(&self) -> &LocalPath {
        &self.root.path
    }
}

struct QualifierPlanPermit {
    run: [u8; 16],
    generation: u64,
}

impl QualifierPlanPermit {
    fn check(
        &self,
        run: [u8; 16],
        generation: u64,
        kind: PlanKind,
        has_offline_lease: bool,
    ) -> Result<(), NativeRefusal> {
        if run != self.run
            || generation != self.generation
            || has_offline_lease
            || !matches!(
                kind,
                PlanKind::Boot | PlanKind::InstallerBoot(_) | PlanKind::Inspect
            )
        {
            return Err(NativeRefusal::QualificationMissing);
        }
        Ok(())
    }
}

struct QualifierLaunch<'a> {
    core: CheckedLaunchCore<'a>,
}

impl<'a> QualifierLaunch<'a> {
    fn hold(
        permit: QualifierPlanPermit,
        baseline: &'a Baseline,
        inputs: LaunchInputs<'a>,
        refresh: impl FnOnce() -> Result<std::time::Instant, NativeRefusal>,
    ) -> Result<(Self, tokio::io::DuplexStream, tokio::io::DuplexStream), NativeRefusal> {
        permit.check(
            inputs.workspace.run,
            inputs.workspace.generation,
            inputs.plan.kind(),
            inputs.plan.offline_lease().is_some(),
        )?;
        let held = &baseline._held;
        if !held.full_test
            || !held
                .bundle
                .as_ref()
                .is_some_and(|b| std::ptr::eq(b, inputs.bundle))
            || !held.trial.path.contains(inputs.workspace.path())
        {
            return Err(NativeRefusal::QualificationMissing);
        }
        let (core, read, write) = CheckedLaunchCore::hold_checked(&held.token, inputs, refresh)?;
        Ok((Self { core }, read, write))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inspect_requires_held_workspace_and_both_verified_inputs() {
        use crate::vm::{paths::Artifact, qemu_args::golden_info, qemu_img::CLUSTER};
        use sha2::{Digest, Sha256};
        let fixture = inventory::tests::Fixture::with_payload("qemu-img.exe", &vec![0; 2_520_350]);
        let bundle = fixture
            .inspect(Instant::now() + Duration::from_secs(5))
            .unwrap();
        let root = tempfile::tempdir().unwrap();
        let path = local(root.path().to_str().unwrap()).unwrap();
        let slot = File::create(root.path().join("slot")).unwrap();
        let workspace_root = PathPin::open(&path, true, false).unwrap();
        let custody = WorkspaceCustody::new([1; 16], 1, &workspace_root, &slot).unwrap();
        assert!(WorkspaceCustody::new([0; 16], 1, &workspace_root, &slot).is_err());
        assert!(WorkspaceCustody::new([1; 16], 0, &workspace_root, &slot).is_err());

        let image_path = bundle.root.path.join("qemu-img.exe").unwrap();
        let image = Artifact::new(ArtifactRole::QemuImg, image_path.clone()).unwrap();
        let mut header = vec![0; CLUSTER];
        header[..4].copy_from_slice(b"QFI\xfb");
        for (at, val) in [(4, 3u32), (20, 16), (96, 4), (100, 112)] {
            header[at..at + 4].copy_from_slice(&val.to_be_bytes());
        }
        header[24..32].copy_from_slice(&1_048_576u64.to_be_bytes());
        std::fs::write(root.path().join("golden"), &header).unwrap();
        let disk_path = path.join("golden").unwrap();
        let disk = PathPin::open(&disk_path, false, false).unwrap();
        let digest = ArtifactPin {
            size: header.len() as u64,
            sha256: Sha256::digest(&header).into(),
        };
        let golden = Artifact::new(ArtifactRole::Golden, disk_path)
            .unwrap()
            .with_identity(disk.identity)
            .with_pin(digest);
        let plan = golden_info(&image, &golden, &header).unwrap();
        let mut pins = [PathPin::open(&image_path, false, false).unwrap(), disk];
        let cwd = path.join("cwd").unwrap();
        let check =
            |cwd, pins: &[PathPin]| StockLaunch::verify_inputs(&bundle, cwd, &plan, pins, &custody);
        assert!(check(&cwd, &pins).is_err());
        pins[1].verify_content(digest, digest.size).unwrap();
        assert_eq!(check(&cwd, &pins), Ok(vec![]));
        assert!(check(&path, &pins).is_err());
        assert!(check(&cwd, &pins[..1]).is_err());
    }

    #[test]
    fn qualifier_plan_kind_is_restricted_even_without_a_lease() {
        let permit = QualifierPlanPermit {
            run: [1; 16],
            generation: 1,
        };
        for kind in [
            PlanKind::InspectWritable,
            PlanKind::Mutate,
            PlanKind::StaticWebBoot,
            PlanKind::PublicEgressBoot,
        ] {
            assert_eq!(
                permit.check(permit.run, 1, kind, false),
                Err(NativeRefusal::QualificationMissing)
            );
        }
        assert_eq!(
            permit.check(permit.run, 1, PlanKind::Inspect, false),
            Ok(())
        );
    }

    #[test]
    fn qualifier_refuses_an_offline_lease_even_for_allowed_plan_kinds() {
        let permit = QualifierPlanPermit {
            run: [1; 16],
            generation: 1,
        };
        assert_eq!(
            permit.check(permit.run, 1, PlanKind::Boot, true),
            Err(NativeRefusal::QualificationMissing)
        );
        assert_eq!(
            permit.check(permit.run, 2, PlanKind::Boot, false),
            Err(NativeRefusal::QualificationMissing)
        );
        assert_eq!(
            permit.check([2; 16], 1, PlanKind::Boot, false),
            Err(NativeRefusal::QualificationMissing)
        );
    }
}
