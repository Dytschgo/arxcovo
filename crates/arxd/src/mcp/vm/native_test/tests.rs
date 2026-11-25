use super::*;

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
