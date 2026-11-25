use super::*;

fn identity(id: u8) -> FileIdentity {
    FileIdentity {
        volume: 1,
        file: [id; 16],
    }
}

fn workspace() -> WorkspaceBinding {
    WorkspaceBinding {
        generation: [1; 16],
        production: identity(1),
        workspace: identity(2),
        lease: identity(3),
    }
}

fn pair(generation: Id) -> PairSlots {
    PairSlots {
        disk: ArtifactSlot {
            generation,
            role: "disk",
            key: [1; 16],
        },
        vars: ArtifactSlot {
            generation,
            role: "vars",
            key: [2; 16],
        },
    }
}

#[test]
fn consumed_permit_requires_the_exact_seal_generation_and_workspace() {
    let workspace = workspace();
    let permit = LaunchPermitBinding {
        workspace,
        seal_sha256: [7; 32],
    };
    assert_eq!(check_permit(permit, permit), Ok(()));
    for changed in [
        WorkspaceBinding {
            generation: [4; 16],
            ..workspace
        },
        WorkspaceBinding {
            workspace: identity(4),
            ..workspace
        },
        WorkspaceBinding {
            production: identity(4),
            ..workspace
        },
        WorkspaceBinding {
            lease: identity(4),
            ..workspace
        },
    ] {
        assert_eq!(
            check_permit(
                permit,
                LaunchPermitBinding {
                    workspace: changed,
                    ..permit
                }
            ),
            Err(NativeRefusal::QualificationMissing)
        );
    }
    assert_eq!(
        check_permit(
            permit,
            LaunchPermitBinding {
                seal_sha256: [8; 32],
                ..permit
            }
        ),
        Err(NativeRefusal::QualificationMissing)
    );
}

#[test]
fn both_reserved_slots_must_match_the_workspace_generation() {
    let workspace = workspace();
    assert_eq!(check_pair(workspace, pair(workspace.generation)), Ok(()));
    assert_eq!(
        check_pair(workspace, pair([4; 16])),
        Err(NativeRefusal::PathCustodyUnverified)
    );
    let mut mixed = pair(workspace.generation);
    mixed.vars.generation = [4; 16];
    assert_eq!(
        check_pair(workspace, mixed),
        Err(NativeRefusal::PathCustodyUnverified)
    );
}

#[tokio::test]
async fn released_proof_binds_generation_and_keeps_the_writer_exclusion() {
    let temp = tempfile::tempdir().unwrap();
    let root = local(temp.path().to_str().unwrap()).unwrap();
    let workspace = workspace();
    let slots = pair(workspace.generation);
    let paths = [slots.disk, slots.vars].map(|slot| {
        root.join(&format!("{}-{}", slot.role, hex::encode(slot.key)))
            .unwrap()
    });
    for path in &paths {
        std::fs::write(path.as_str(), b"synthetic").unwrap();
    }
    let pins = paths
        .each_ref()
        .map(|path| PathPin::open(path, false, true).unwrap());
    let ids = pins.each_ref().map(|pin| pin.identity);
    let root = PathPin::open(&root, true, false).unwrap();
    let lease = std::fs::File::open(paths[0].as_str()).unwrap();
    let workspace_custody = WorkspaceCustody::production(&root, &lease).unwrap();
    let (disposition, held) = finish_release_held(
        StopEvidence {
            process_exited: true,
            job_empty: true,
            workers_joined: true,
            ..StopEvidence::default()
        },
        tokio::time::Instant::now() + Duration::from_secs(2),
        move || {
            let guards = pins
                .iter()
                .map(PathPin::reopen_without_writer)
                .collect::<io::Result<Vec<_>>>()?;
            Ok((std::sync::Arc::from(pins), guards))
        },
        || true,
    )
    .await;
    assert_eq!(disposition, StopDisposition::DirtyRelease);
    let proof = ReleasedGeneration {
        binding: workspace,
        pair: slots,
        ids,
        _workspace: workspace_custody,
        _held: held.unwrap(),
    };
    assert!(proof.matches(workspace, slots, ids));
    assert!(!proof.matches(
        WorkspaceBinding {
            generation: [4; 16],
            ..workspace
        },
        slots,
        ids
    ));
    assert!(!proof.matches(workspace, pair([4; 16]), ids));
    assert!(!proof.matches(workspace, slots, [ids[1], ids[0]]));
    for path in &paths {
        assert!(
            std::fs::OpenOptions::new()
                .write(true)
                .open(path.as_str())
                .is_err()
        );
    }
    drop(proof);
    for path in &paths {
        assert!(
            std::fs::OpenOptions::new()
                .write(true)
                .open(path.as_str())
                .is_ok()
        );
    }
}
