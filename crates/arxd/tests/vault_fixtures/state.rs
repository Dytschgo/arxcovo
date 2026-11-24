use super::*;
use serde_json::json;
use std::fs;
use std::sync::Mutex;

#[path = "private.rs"]
mod private;

const NOW: i64 = 1_800_000_000_000;
const OWNER: &str = "agent-one";
const SENTINEL: &str = "exit 7 # vault-script-sentinel-cadcf8bd";

#[derive(Default)]
struct Fixture(Mutex<Option<TestJob>>);

impl VaultTester for Fixture {
    fn enqueue(&self, job: TestJob) {
        assert!(self.0.lock().unwrap().replace(job).is_none());
    }
}

impl Fixture {
    fn take(&self) -> TestJob {
        self.0.lock().unwrap().take().unwrap()
    }
}

fn key(nonce: u64) -> RequestId {
    RequestId::parse(&format!("{NOW:016x}{nonce:016x}")).unwrap()
}

fn add(vault: &Arc<Vault>, nonce: u64, name: &str, body: &str) -> Reply {
    vault.mutate(
        OWNER,
        Action::Add,
        Ok(Write::Add {
            request_id: key(nonce),
            os: ScriptOs::Ubuntu,
            name: name.into(),
            script: body.into(),
        }),
        NOW,
        None,
    )
}

fn version_id(result: Value) -> Id {
    parse_id(result["version_id"].as_str().unwrap()).unwrap()
}

fn test(vault: &Arc<Vault>, nonce: u64, version_id: Id) -> Reply {
    vault.mutate(
        OWNER,
        Action::Test,
        Ok(Write::Test {
            request_id: key(nonce),
            version_id,
        }),
        NOW,
        None,
    )
}

fn activate(vault: &Arc<Vault>, nonce: u64, version_id: Id) -> Reply {
    vault.mutate(
        OWNER,
        Action::Activate,
        Ok(Write::Activate {
            request_id: key(nonce),
            version_id,
        }),
        NOW,
        None,
    )
}

#[test]
fn vault_failed_replacement_keeps_predecessor_active_and_redacts_audit() {
    let dir = private::private_dir();
    let tester = Arc::new(Fixture::default());
    let vault = Vault::open(dir.path(), Some(tester.clone()), NOW).unwrap();
    let first = version_id(add(&vault, 1, "build", "exit 0").unwrap());
    assert_eq!(activate(&vault, 2, first), Err(Refusal::PassRequired));
    let test_receipt = test(&vault, 3, first).unwrap();
    assert_eq!(vault.get(OWNER, first, false).unwrap()["state"], "testing");
    assert_eq!(activate(&vault, 4, first), Err(Refusal::PassRequired));
    assert_eq!(test(&vault, 3, first).unwrap(), test_receipt);
    let job = tester.take();
    assert_eq!(job.os().as_str(), "ubuntu");
    assert_eq!(job.content(), "exit 0");
    assert_eq!(job.sha256(), <[u8; 32]>::from(Sha256::digest(b"exit 0")));
    job.complete(ScriptOutcome::Exited { exit_code: 0 }, true)
        .unwrap();
    let activation = activate(&vault, 5, first).unwrap();
    assert_eq!(activate(&vault, 5, first).unwrap(), activation);

    let broken = version_id(add(&vault, 6, "build", SENTINEL).unwrap());
    test(&vault, 7, broken).unwrap();
    tester
        .take()
        .complete(ScriptOutcome::Exited { exit_code: 7 }, true)
        .unwrap();
    let rejected = vault.get(OWNER, broken, true).unwrap();
    assert_eq!(rejected["state"], "rejected");
    assert_eq!(rejected["content"], SENTINEL);
    assert_eq!(rejected["test"]["exit_code"], 7);
    assert_eq!(activate(&vault, 8, broken), Err(Refusal::PassRequired));
    assert_eq!(vault.get(OWNER, first, false).unwrap()["state"], "active");

    let unclean = version_id(add(&vault, 9, "build", "exit 0").unwrap());
    test(&vault, 10, unclean).unwrap();
    tester
        .take()
        .complete(ScriptOutcome::Exited { exit_code: 0 }, false)
        .unwrap();
    assert_eq!(
        vault.get(OWNER, unclean, false).unwrap()["state"],
        "rejected"
    );
    assert_eq!(activate(&vault, 11, unclean), Err(Refusal::PassRequired));
    let replacement = version_id(add(&vault, 12, "build", "exit 0").unwrap());
    test(&vault, 13, replacement).unwrap();
    tester
        .take()
        .complete(ScriptOutcome::Exited { exit_code: 0 }, true)
        .unwrap();
    activate(&vault, 14, replacement).unwrap();
    assert_eq!(
        vault.get(OWNER, first, false).unwrap()["state"],
        "superseded"
    );
    assert_eq!(
        vault.get(OWNER, replacement, false).unwrap()["state"],
        "active"
    );
    assert_eq!(vault.get("another", broken, true), Err(Refusal::NotFound));

    let page = vault
        .page(
            OWNER,
            read::List::Audit,
            serde_json::from_value(json!({})).unwrap(),
        )
        .unwrap();
    assert!(
        page["activities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["version_id"] == hex::encode(broken) && row["outcome"] == "failed")
    );
    assert!(!page.to_string().contains(SENTINEL));
    vault
        .store
        .read(|db| {
            let audit = db
                .prepare("SELECT * FROM audit")?
                .query_map([], |row| {
                    let mut bytes = Vec::new();
                    for col in 0..row.as_ref().column_count() {
                        match row.get_ref(col)? {
                            types::ValueRef::Blob(data) | types::ValueRef::Text(data) => {
                                bytes.extend_from_slice(data)
                            }
                            _ => {}
                        }
                    }
                    Ok(bytes)
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            assert!(
                !audit
                    .concat()
                    .windows(SENTINEL.len())
                    .any(|bytes| bytes == SENTINEL.as_bytes())
            );
            Ok(())
        })
        .unwrap();
}

#[test]
fn vault_completion_refuses_a_changed_digest() {
    let dir = private::private_dir();
    let tester = Arc::new(Fixture::default());
    let vault = Vault::open(dir.path(), Some(tester.clone()), NOW).unwrap();
    let version = version_id(add(&vault, 1, "build", "exit 0").unwrap());
    test(&vault, 2, version).unwrap();
    let job = tester.take();
    let changed_body = b"exit 7";
    let changed_digest: [u8; 32] = Sha256::digest(changed_body).into();
    assert_ne!(job.sha256(), changed_digest);
    vault
        .store
        .write(|db| {
            assert_eq!(
                db.execute(
                    "UPDATE versions SET body=?1,hash=?2 WHERE id=?3",
                    params![changed_body.as_slice(), changed_digest, version],
                )?,
                1
            );
            Ok(())
        })
        .unwrap();

    job.complete(ScriptOutcome::Exited { exit_code: 0 }, true)
        .unwrap();
    let row = vault.get(OWNER, version, false).unwrap();
    assert_eq!(row["state"], "rejected");
    assert_eq!(row["test"]["cleanup_confirmed"], false);
    assert_eq!(activate(&vault, 3, version), Err(Refusal::PassRequired));
}

#[test]
fn vault_replay_lock_recovery_and_clock_rollback() {
    let dir = private::private_dir();
    let vault = Vault::open(dir.path(), None, NOW).unwrap();
    assert!(Vault::open(dir.path(), None, NOW).is_err());
    let receipt = add(&vault, 1, "one", "exit 0").unwrap();
    let version = version_id(receipt.clone());
    assert_eq!(add(&vault, 1, "one", "exit 0").unwrap(), receipt);
    assert_eq!(add(&vault, 1, "one", "exit 1"), Err(Refusal::KeyCollision));
    assert_eq!(test(&vault, 2, version), Err(Refusal::TesterUnavailable));
    assert_eq!(vault.get(OWNER, version, false).unwrap()["state"], "draft");
    let visible = vault.get(OWNER, version, false).unwrap();
    let subject = parse_id(visible["script_id"].as_str().unwrap()).unwrap();
    assert_eq!(
        vault.page(
            "other",
            read::List::Versions(subject),
            serde_json::from_value(json!({})).unwrap()
        ),
        Err(Refusal::NotFound)
    );
    // Leave a committed intent without delivering it, simulating process loss.
    vault
        .store
        .write(|db| {
            let scope = store::WriteScope {
                owner: OWNER,
                action: Action::Test,
                at_ms: NOW,
            };
            let (_, job) = scope.execute(
                db,
                Write::Test {
                    request_id: key(2),
                    version_id: version,
                },
                true,
                None,
            )?;
            assert!(job.is_some());
            Ok(())
        })
        .unwrap();
    drop(vault);
    let vault = Vault::open(dir.path(), None, NOW).unwrap();
    let recovered = vault.get(OWNER, version, false).unwrap();
    assert_eq!(recovered["state"], "test_inconclusive");
    assert_eq!(
        recovered["test"],
        json!({"status":"test_inconclusive","cleanup_confirmed":false})
    );
    vault
        .store
        .read(|db| {
            let count: i64 = db.query_row(
                "SELECT count(*) FROM audit WHERE version=?1 AND action='vault_test_version'
             AND outcome='failed' AND reason='test_inconclusive'",
                [version],
                |row| row.get(0),
            )?;
            assert_eq!(count, 1);
            Ok(())
        })
        .unwrap();
    assert_eq!(add(&vault, 1, "one", "exit 0").unwrap(), receipt);
    assert!(test(&vault, 2, version).is_ok());
    assert_eq!(
        vault.mutate(
            OWNER,
            Action::Activate,
            Ok(Write::Activate {
                request_id: key(3),
                version_id: version,
            }),
            NOW + crate::vm::RETRY_WINDOW_MS + 1,
            None,
        ),
        Err(Refusal::StaleKey)
    );
    assert_eq!(add(&vault, 1, "one", "exit 0"), Err(Refusal::StaleKey));
}

#[test]
fn vault_script_version_and_byte_caps_refuse_without_partial_rows() {
    let dir = private::private_dir();
    let vault = Vault::open(dir.path(), None, NOW).unwrap();
    vault.store.write(|db| {
        db.execute_batch("
            WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i+1 FROM n WHERE i<128)
            INSERT INTO scripts(seq,id,owner,os,name)
                SELECT i,randomblob(16),'agent-one','ubuntu','script-'||i FROM n;
            WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i+1 FROM n WHERE i<256)
            INSERT INTO versions(id,script,body,hash,created_ms)
                SELECT randomblob(16),1+(i-1)/64,CAST(printf('%04096d',i) AS BLOB),randomblob(32),1 FROM n;
        ")?;
        Ok(())
    }).unwrap();
    assert_eq!(add(&vault, 1, "new", "x"), Err(Refusal::ScriptLimit));
    assert_eq!(add(&vault, 2, "script-1", "x"), Err(Refusal::VersionLimit));
    assert_eq!(add(&vault, 3, "script-5", "x"), Err(Refusal::ByteLimit));
    vault
        .store
        .read(|db| {
            assert_eq!(
                db.query_row("SELECT count(*) FROM versions", [], |r| r.get::<_, i64>(0))?,
                256
            );
            assert_eq!(
                db.query_row("SELECT count(*) FROM replay", [], |r| r.get::<_, i64>(0))?,
                0
            );
            Ok(())
        })
        .unwrap();
}

#[test]
fn vault_owner_caps_keep_capacity_available_to_other_principals() {
    let dir = private::private_dir();
    let vault = Vault::open(dir.path(), None, NOW).unwrap();
    vault
        .store
        .write(|db| {
            db.execute_batch(
                "
            INSERT INTO scripts(seq,id,owner,os,name) VALUES
                (1,randomblob(16),'agent-one','ubuntu','count'),
                (2,randomblob(16),'agent-one','windows','count'),
                (3,randomblob(16),'byte-owner','ubuntu','bytes');
            WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i+1 FROM n WHERE i<95)
            INSERT INTO versions(id,script,body,hash,created_ms)
                SELECT randomblob(16),1+(i-1)/48,x'78',randomblob(32),1 FROM n;
            WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i+1 FROM n WHERE i<63)
            INSERT INTO versions(id,script,body,hash,created_ms)
                SELECT randomblob(16),3,CAST(printf('%04096d',i) AS BLOB),randomblob(32),1 FROM n;
        ",
            )?;
            Ok(())
        })
        .unwrap();

    assert!(add(&vault, 1, "count", "x").is_ok());
    assert_eq!(
        add(&vault, 2, "refused-script", "x"),
        Err(Refusal::PrincipalVersionLimit)
    );
    let add_as = |principal: &str, nonce: u64, name: &str, content: &str| {
        vault.mutate(
            principal,
            Action::Add,
            Ok(Write::Add {
                request_id: key(nonce),
                os: ScriptOs::Ubuntu,
                name: name.into(),
                script: content.into(),
            }),
            NOW,
            None,
        )
    };
    assert!(add_as("byte-owner", 1, "bytes", &"x".repeat(MAX_SCRIPT)).is_ok());
    assert_eq!(
        add_as("byte-owner", 2, "refused-script", "x"),
        Err(Refusal::PrincipalByteLimit)
    );
    assert!(add_as("another", 2, "refused-script", "x").is_ok());
    vault
        .store
        .read(|db| {
            assert_eq!(
                db.query_row(
                    "SELECT count(*) FROM scripts WHERE name='refused-script'",
                    [],
                    |r| r.get::<_, i64>(0),
                )?,
                1
            );
            assert_eq!(
                db.query_row("SELECT count(*) FROM replay", [], |r| r.get::<_, i64>(0))?,
                3
            );
            Ok(())
        })
        .unwrap();
}

#[test]
fn vault_state_binding_rejects_substituted_main_and_linked_files() {
    let dir = private::private_dir();
    drop(Vault::open(dir.path(), None, NOW).unwrap());
    let original = dir.path().join("vault.sqlite3");
    let replacement = dir.path().join("replacement.sqlite3");
    let parked = dir.path().join("parked.sqlite3");
    fs::copy(&original, &replacement).unwrap();
    {
        let alternate = rusqlite::Connection::open(&replacement).unwrap();
        alternate.pragma_update(None, "user_version", 7).unwrap();
    }
    let before = [&original, &replacement].map(|path| fs::read(path).unwrap());
    let result = files::open_bound(dir.path(), |path| {
        #[cfg(unix)]
        {
            // Restore both names before returning. Path-only checks would miss the swap.
            fs::rename(path, &parked)?;
            fs::rename(&replacement, path)?;
            let opened = rusqlite::Connection::open(path)?;
            fs::rename(path, &replacement)?;
            fs::rename(&parked, path)?;
            Ok(opened)
        }
        #[cfg(windows)]
        {
            let error = fs::rename(path, &parked).unwrap_err();
            assert_eq!(
                error.raw_os_error(),
                Some(windows_sys::Win32::Foundation::ERROR_SHARING_VIOLATION as i32)
            );
            Ok(rusqlite::Connection::open(&replacement)?)
        }
    });
    assert_eq!(result.err().unwrap().to_string(), "vault database changed");

    let other_link = dir.path().join("alias.sqlite3");
    fs::hard_link(&original, &other_link).unwrap();
    let error = Vault::open(dir.path(), None, NOW).err().unwrap();
    assert_eq!(error.to_string(), "single-link vault file required");
    fs::remove_file(other_link).unwrap();
    let journal_alias = dir.path().join("vault.sqlite3-journal");
    fs::hard_link(&replacement, &journal_alias).unwrap();
    let error = Vault::open(dir.path(), None, NOW).err().unwrap();
    assert_eq!(error.to_string(), "single-link vault file required");
    fs::remove_file(journal_alias).unwrap();
    for (path, expected) in [&original, &replacement].into_iter().zip(before) {
        assert_eq!(fs::read(path).unwrap(), expected);
    }
    assert!(Vault::open(dir.path(), None, NOW).is_ok());
}

#[test]
fn vault_retry_and_receipt_survive_v1_migration_and_v2_reopen() {
    let dir = private::private_dir();
    let tester = Arc::new(Fixture::default());
    let vault = Vault::open(dir.path(), Some(tester.clone()), NOW).unwrap();
    let added = add(&vault, 1, "build", "exit 0").unwrap();
    let version = version_id(added.clone());
    vault
        .store
        .write(|db| {
            db.execute_batch(
                "ALTER TABLE versions DROP COLUMN inconclusive; PRAGMA user_version=1",
            )?;
            Ok(())
        })
        .unwrap();
    drop(vault);
    let vault = Vault::open(dir.path(), Some(tester.clone()), NOW).unwrap();
    assert_eq!(add(&vault, 1, "build", "exit 0").unwrap(), added);
    let receipt = test(&vault, 2, version).unwrap();
    tester.take().inconclusive().unwrap();
    drop(vault);
    let vault = Vault::open(dir.path(), Some(tester.clone()), NOW).unwrap();
    assert_eq!(
        vault.get(OWNER, version, false).unwrap()["state"],
        "test_inconclusive"
    );
    assert_eq!(test(&vault, 2, version).unwrap(), receipt);
    assert_ne!(
        test(&vault, 3, version).unwrap()["operation_id"],
        receipt["operation_id"]
    );
    tester
        .take()
        .complete(ScriptOutcome::Exited { exit_code: 0 }, true)
        .unwrap();
    activate(&vault, 4, version).unwrap();
}
