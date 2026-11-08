use std::{collections::BTreeSet, error::Error, fs};

use poker_bank::{
    model::{Command, CreateRequest, Currency, GameState, JoinRequest, Phase},
    store::{Store, backup_database},
};
use rusqlite::{Connection, params};
use tempfile::tempdir;

type TestResult = Result<(), Box<dyn Error>>;
fn request() -> CreateRequest {
    CreateRequest {
        name: "Host".into(),
        title: "Persistence test".into(),
        currency: Currency::Inr,
        minor_per_chip: 50,
        default_buyin: 100,
    }
}

#[test]
fn accepted_commands_and_credentials_survive_reopening_and_duplicates_do_not_reapply() -> TestResult
{
    let dir = tempdir()?;
    let path = dir.path().join("poker.db");
    let mut store = Store::open(&path)?;
    let credential = store.create(&request())?;
    assert_eq!(credential.code.len(), 8);
    assert_eq!(credential.token.len(), 43);
    assert_eq!(
        store.authenticate(&credential.code, &credential.token)?,
        credential.player_id
    );
    let approved = store.execute(
        &credential.code,
        &credential.player_id,
        "approve-id",
        0,
        &Command::Approve {
            player_id: credential.player_id.clone(),
            amount: 100,
        },
    )?;
    assert_eq!(approved.revision, 1);
    assert!(!approved.duplicate);
    let before = store.get(&credential.code)?;
    drop(store);
    let mut store = Store::open(&path)?;
    let duplicate = store.execute(
        &credential.code,
        &credential.player_id,
        "approve-id",
        0,
        &Command::Approve {
            player_id: credential.player_id.clone(),
            amount: 100,
        },
    )?;
    assert!(duplicate.duplicate);
    assert_eq!(duplicate.revision, 1);
    assert_eq!(store.get(&credential.code)?, before);
    assert_eq!(
        store.authenticate(&credential.code, &credential.token)?,
        credential.player_id
    );
    let db = Connection::open(path)?;
    let stored: String = db.query_row("SELECT hash FROM credentials", [], |r| r.get(0))?;
    assert_eq!(stored.len(), 64);
    assert_ne!(stored, credential.token);
    Ok(())
}
#[test]
fn stale_and_invalid_commands_roll_back_without_recording_ids_or_changing_state() -> TestResult {
    let dir = tempdir()?;
    let path = dir.path().join("poker.db");
    let mut store = Store::open(&path)?;
    let c = store.create(&request())?;
    let before = store.get(&c.code)?;
    assert!(
        store
            .execute(&c.code, &c.player_id, "bad-call-id", 0, &Command::Start)
            .is_err()
    );
    assert!(
        store
            .execute(&c.code, &c.player_id, "stale-id-0", 20, &Command::Pause)
            .is_err()
    );
    assert_eq!(store.get(&c.code)?, before);
    let db = Connection::open(path)?;
    assert_eq!(
        db.query_row("SELECT count(*) FROM commands", [], |r| r.get::<_, i64>(0))?,
        0
    );
    assert!(
        store
            .execute(&c.code, &c.player_id, "bad", 0, &Command::Pause)
            .is_err()
    );
    assert!(
        store
            .execute(
                &c.code,
                &c.player_id,
                "revision-id",
                u64::MAX,
                &Command::Pause
            )
            .is_err()
    );
    Ok(())
}
#[test]
fn tokens_are_scoped_to_their_table_and_removed_seats_are_not_hidden_credentials() -> TestResult {
    let dir = tempdir()?;
    let mut store = Store::open(&dir.path().join("poker.db"))?;
    let a = store.create(&request())?;
    let b = store.create(&request())?;
    assert!(store.authenticate(&b.code, &a.token).is_err());
    assert!(store.authenticate(&a.code, "malformed").is_err());
    assert_eq!(store.authenticate(&a.code, &a.token)?, a.player_id);
    Ok(())
}
#[test]
fn joining_is_pending_and_bounded_and_ended_tables_leave_public_lobby() -> TestResult {
    let dir = tempdir()?;
    let mut store = Store::open(&dir.path().join("poker.db"))?;
    let host = store.create(&request())?;
    for i in 0..19 {
        store.join(&JoinRequest {
            code: host.code.to_lowercase(),
            name: format!("Pending {i}"),
        })?;
    }
    assert!(
        store
            .join(&JoinRequest {
                code: host.code.clone(),
                name: "Overflow".into()
            })
            .is_err()
    );
    assert_eq!(store.lobby()?.len(), 1);
    assert_eq!(store.lobby()?[0].players, 0);
    let state = store.get(&host.code)?;
    store.execute(
        &host.code,
        &host.player_id,
        "end-table-id",
        state.revision,
        &Command::End,
    )?;
    assert!(store.lobby()?.is_empty());
    assert!(
        store
            .join(&JoinRequest {
                code: host.code,
                name: "Late".into()
            })
            .is_err()
    );
    Ok(())
}
#[test]
fn expiry_removes_credentials_commands_and_tables_but_preserves_connected_games() -> TestResult {
    let dir = tempdir()?;
    let path = dir.path().join("poker.db");
    let mut store = Store::open(&path)?;
    let active = store.create(&request())?;
    let expired = store.create(&request())?;
    store.execute(
        &expired.code,
        &expired.player_id,
        "pause-table-id",
        0,
        &Command::Pause,
    )?;
    let db = Connection::open(path)?;
    db.execute("UPDATE tables SET updated=0", [])?;
    assert_eq!(
        store.clean_expired(&BTreeSet::from([active.code.clone()]))?,
        1
    );
    assert!(store.get(&expired.code).is_err());
    assert!(store.authenticate(&expired.code, &expired.token).is_err());
    assert!(store.get(&active.code).is_ok());
    assert_eq!(
        db.query_row("SELECT count(*) FROM commands", [], |r| r.get::<_, i64>(0))?,
        0
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM credentials", [], |r| r
            .get::<_, i64>(0))?,
        1
    );
    Ok(())
}
#[test]
fn existing_javascript_sqlite_state_and_online_backup_retain_all_fields() -> TestResult {
    let dir = tempdir()?;
    let path = dir.path().join("legacy.db");
    let backup_dir = dir.path().join("backups");
    let store = Store::open(&path)?;
    let fixture = include_str!("fixtures/javascript-showdown.json");
    let state: GameState = serde_json::from_str(fixture)?;
    let db = Connection::open(&path)?;
    db.execute(
        "INSERT INTO tables VALUES (?1,?2,?3)",
        params!["ABCDEFGH", fixture, chrono::Utc::now().timestamp_millis()],
    )?;
    assert_eq!(store.get("ABCDEFGH")?, state);
    let backup = backup_database(&path, &backup_dir)?;
    let mut restored = Store::open(&backup)?;
    assert_eq!(restored.get("ABCDEFGH")?, state);
    let winners = poker_bank::engine::pots(&state)
        .iter()
        .map(|pot| vec![pot.eligible[0].clone()])
        .collect();
    restored.execute(
        "ABCDEFGH",
        "p1",
        "award-restore-id",
        state.revision,
        &Command::Award { winners },
    )?;
    assert_eq!(restored.get("ABCDEFGH")?.phase, Phase::Between);
    assert_eq!(store.get("ABCDEFGH")?.phase, Phase::Showdown);
    store.health()?;
    Ok(())
}
#[test]
fn backups_retain_only_latest_fourteen_and_never_delete_unrelated_files() -> TestResult {
    let dir = tempdir()?;
    let source = dir.path().join("poker.db");
    let backup_dir = dir.path().join("backups");
    let _store = Store::open(&source)?;
    fs::create_dir(&backup_dir)?;
    for i in 0..15 {
        fs::write(backup_dir.join(format!("poker-2000-01-{i:02}.db")), [])?;
    }
    fs::write(backup_dir.join("unrelated.db"), b"keep")?;
    let latest = backup_database(&source, &backup_dir)?;
    assert!(latest.is_file());
    assert_eq!(fs::read_dir(&backup_dir)?.count(), 15);
    assert!(!backup_dir.join("poker-2000-01-00.db").exists());
    assert!(!backup_dir.join("poker-2000-01-01.db").exists());
    assert_eq!(fs::read(backup_dir.join("unrelated.db"))?, b"keep");
    let copy = Connection::open(latest)?;
    assert_eq!(
        copy.query_row("PRAGMA quick_check", [], |r| r.get::<_, String>(0))?,
        "ok"
    );
    Ok(())
}
