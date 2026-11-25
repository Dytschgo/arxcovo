use super::*;

#[test]
fn owner_stop_interrupts_boot_and_requests_graceful_stop_after_readiness() {
    for ready in [false, true] {
        let mut instance = Running {
            control: RunControl::default(),
            stop_worker: None,
            queue: mpsc::channel(1).0,
            epoch: [2; 16],
            witness: ready.then_some(Witness {
                generation: [1; 16],
                operation: [2; 16],
                id: [3; 16],
            }),
        };
        instance.request_stop();
        assert!(instance.control.stop.is_cancelled());
        assert_eq!(instance.control.cancel.is_cancelled(), !ready);
        assert!(instance.witness.is_none());
    }
}

#[tokio::test(start_paused = true)]
async fn missing_release_fences_before_a_woken_owned_retry() {
    use std::cell::Cell;
    let wake = tokio::sync::Notify::new();
    let fenced = Cell::new(false);
    let retried = Cell::new(false);
    let start = tokio::time::Instant::now();
    reconcile_release(
        false,
        &wake,
        |proof| Ok(*proof),
        || {
            fenced.set(true);
            wake.notify_one();
            Ok(())
        },
        async || {
            assert!(fenced.get());
            retried.set(true);
            true
        },
    )
    .await
    .unwrap();
    assert!(retried.get());
    assert_eq!(start.elapsed(), Duration::ZERO);
}

#[test]
fn issuer_open_refuses_fixture_and_stale_dossiers() {
    use std::cell::Cell;
    let root = tempfile::tempdir().unwrap();
    crate::dashboard::tests::set_owner_private(root.path());
    let controller = Controller::open(
        root.path(),
        VmPolicy::default(),
        ForegroundClock { utc: None },
    )
    .unwrap();
    for stale in [false, true] {
        crate::vm::qualification::with_refused_issuer_dossier(stale, |dossier, selection, read| {
            let checked = Cell::new(false);
            let config = TestQualificationSelection {
                enrollment: crate::vm::paths::LocalPath::parse(r"D:\test-enrollment").unwrap(),
                dossier,
                selection,
            };
            let result = SelectedTest::open(&controller, config, |_, _, dossier, pin| {
                checked.set(true);
                read(dossier, pin)?;
                panic!("untrusted dossier passed native selection")
            });
            assert!(checked.get());
            assert!(matches!(result, Err(NativeRefusal::QualificationMissing)));
        });
    }
}

#[test]
fn catalog_identifies_only_the_two_selected_recipes() {
    let recipes = TestRecipes {
        ubuntu: RecipeIdentity {
            recipe_id: [1; 16],
            recipe_sha256: [1; 32],
        },
        windows: RecipeIdentity {
            recipe_id: [2; 16],
            recipe_sha256: [3; 32],
        },
    };
    assert_eq!(
        catalog([9; 32], recipes),
        json!({
            "available": true, "selected_test_qualification": {
                "sha256": "09".repeat(32),
                "recipes": [
                    {"guest":"ubuntu", "recipe_id":"01".repeat(16), "recipe_sha256":"01".repeat(32)},
                    {"guest":"windows", "recipe_id":"02".repeat(16), "recipe_sha256":"03".repeat(32)}
                ]
            }
        })
    );
}

#[test]
fn epoch_refuses_restart_rollback_and_mismatched_sampling() {
    let start = Instant2 {
        utc: 100,
        mono_ms: 20_000,
    };
    let boot = [2; 32];
    let monotonic = Instant::now();
    let epoch = Epoch {
        id: [3; 16],
        boot_marker: boot,
        start,
        monotonic,
    };
    let reading = ClockReading {
        utc_ms: 105_000,
        monotonic: monotonic + Duration::from_secs(5),
    };
    let now = Instant2 {
        utc: 105,
        mono_ms: 25_000,
    };
    assert!(epoch.valid(now, boot, reading));
    assert!(!epoch.valid(now, [4; 32], reading));
    assert!(!epoch.valid(Instant2 { utc: 99, ..now }, boot, reading));
    assert!(!epoch.valid(
        now,
        boot,
        ClockReading {
            utc_ms: 110_000,
            ..reading
        }
    ));
}

#[test]
fn witness_requires_the_current_generation_and_start_operation() {
    let witness = Witness {
        generation: [4; 16],
        operation: [5; 16],
        id: [6; 16],
    };
    assert!(witness.matches(Some(([4; 16], [5; 16]))));
    assert!(!witness.matches(None));
    assert!(!witness.matches(Some(([4; 16], [1; 16]))));
    assert!(!witness.matches(Some(([1; 16], [5; 16]))));
    assert!(
        !Witness {
            id: witness.operation,
            ..witness
        }
        .matches(Some(([4; 16], [5; 16])))
    );
}

#[test]
fn only_an_unready_run_without_a_stop_takes_the_epoch_bound_stop() {
    let run = |ready: bool, stopping: bool| Running {
        control: RunControl::default(),
        stop_worker: stopping.then(|| PendingOperation {
            activity_id: 7,
            deadline: Instant::now(),
            binding: Arc::new(()),
        }),
        queue: mpsc::channel(1).0,
        epoch: [2; 16],
        witness: ready.then_some(Witness {
            generation: [1; 16],
            operation: [2; 16],
            id: [3; 16],
        }),
    };
    for (ready, stopping, joinable) in [
        (false, false, true),
        (true, false, false),
        (false, true, false),
    ] {
        let running = HashMap::from([([5; 16], run(ready, stopping))]);
        assert_eq!(
            joinable_stop_epoch(&running, [5; 16]),
            joinable.then_some([2; 16])
        );
        assert_eq!(joinable_stop_epoch(&running, [6; 16]), None);
    }
}

#[test]
fn guest_loss_withdraws_only_its_own_witness() {
    let ready = || Running {
        control: RunControl::default(),
        stop_worker: None,
        queue: mpsc::channel(1).0,
        epoch: [2; 16],
        witness: Some(Witness {
            generation: [1; 16],
            operation: [2; 16],
            id: [3; 16],
        }),
    };
    let mut running = HashMap::from([([5; 16], ready()), ([6; 16], ready())]);
    withdraw_witness(&mut running, [5; 16]);
    assert!(running[&[5; 16]].witness.is_none());
    assert!(running[&[6; 16]].witness.is_some());
    // The epoch-bound stop can then join the run in recovery.
    assert_eq!(joinable_stop_epoch(&running, [5; 16]), Some([2; 16]));
    withdraw_witness(&mut running, [7; 16]);
}

#[test]
fn unlaunched_release_is_clean_only_without_a_spawn_or_byte_change() {
    use crate::vm::paths::ArtifactPin;
    let file = |byte| ArtifactEvidence {
        identity: FileIdentity {
            volume: 1,
            file: [byte; 16],
        },
        directory: [9; 16],
        pin: ArtifactPin {
            size: 1,
            sha256: [byte; 32],
        },
    };
    let stopped = [file(1), file(2)];
    assert_eq!(
        unlaunched_disposition(false, &stopped, &stopped),
        StopDisposition::CleanRelease
    );
    assert_eq!(
        unlaunched_disposition(true, &stopped, &stopped),
        StopDisposition::DirtyRelease
    );
    assert_eq!(
        unlaunched_disposition(false, &[file(1), file(3)], &stopped),
        StopDisposition::DirtyRelease
    );
}

#[test]
fn reaper_accepts_build_seed_and_verification_roles_only() {
    for role in ["disk", "vars", "seed", "verification_overlay"] {
        assert_eq!(reapable_role(role), Some(role));
    }
    for role in ["static_disk", "static_vars", "golden", ""] {
        assert_eq!(reapable_role(role), None);
    }
}

#[test]
fn a_live_clone_or_build_job_is_registered_until_its_worker_returns() {
    let jobs = Mutex::new(std::collections::HashSet::from([[4; 16]]));
    let job = LiveJob {
        jobs: &jobs,
        subject: [4; 16],
    };
    let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _job = job;
        panic!("worker failed");
    }));
    assert!(unwound.is_err());
    assert!(jobs.lock().unwrap().is_empty());
}
