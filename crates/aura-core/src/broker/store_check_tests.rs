use super::*;
use std::io::{Seek, SeekFrom, Write};
use std::sync::mpsc;
use std::time::Instant;

fn settled(check: &StoreCheck, from: u8) -> u8 {
    let deadline = Instant::now() + Duration::from_secs(30);
    while check.state.load(Ordering::Acquire) == from {
        assert!(Instant::now() < deadline, "the check ends");
        std::thread::sleep(Duration::from_millis(10));
    }
    check.state.load(Ordering::Acquire)
}

fn with_state(state: u8) -> StoreCheck {
    StoreCheck { state: Arc::new(AtomicU8::new(state)), stop: Arc::new(Stop::default()) }
}

/// A Store whose quick check fails withholds its next normal exit and reports the damage.
#[test]
fn a_store_whose_quick_check_fails_withholds_its_normal_exit() {
    let dir = tempfile::tempdir().expect("temp");
    let root = dir.path().join("v2");
    let store = aura_persistence_v2::V2Store::create(&root).expect("store");
    store.mark_clean_exit().expect("checkpoint");
    drop(store);
    let database = root.join("aura-v2.db");
    let mut file = std::fs::OpenOptions::new().write(true).open(&database).expect("open");
    file.seek(SeekFrom::Start(4096)).expect("seek");
    file.write_all(&[0xFF; 64]).expect("damage");
    drop(file);

    let check = StoreCheck::start_after(PendingCheck::Quick, database, Duration::ZERO);
    assert_eq!(settled(&check, CHECKING), FAILED);
    assert!(check.damaged());
}

/// The full check an unannounced end leaves owed runs in the background and finds what
/// the structure check at open cannot see.
#[test]
fn the_owed_full_check_finds_a_broken_key_in_the_background() {
    let dir = tempfile::tempdir().expect("temp");
    let root = dir.path().join("v2");
    drop(aura_persistence_v2::V2Store::create(&root).expect("store"));
    let database = root.join("aura-v2.db");
    let raw = rusqlite::Connection::open(&database).expect("raw");
    raw.execute_batch("PRAGMA foreign_keys=OFF; INSERT INTO artifact_gc_queue VALUES('art_gone');")
        .expect("broken key");
    drop(raw);

    let check = StoreCheck::start_after(PendingCheck::Full, database, Duration::ZERO);
    assert_eq!(settled(&check, OWED), FAILED);
    assert!(check.damaged());
}

/// The check waits until the start is over, so it cannot slow it.
#[test]
fn the_quick_check_waits_until_the_start_is_over() {
    let dir = tempfile::tempdir().expect("temp");
    let root = dir.path().join("v2");
    drop(aura_persistence_v2::V2Store::create(&root).expect("store"));
    let check = StoreCheck::start(PendingCheck::Quick, root.join("aura-v2.db"));
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(check.state.load(Ordering::Acquire), CHECKING);
}

/// A normal exit is recorded unless a check found damage or the full check is still owed: a
/// quick check still running at quit runs again at the next start, after this normal exit.
#[test]
fn an_exit_is_recorded_unless_the_store_check_failed_or_is_owed() {
    let dir = tempfile::tempdir().expect("temp");
    let root = dir.path().join("v2");
    let store = aura_persistence_v2::V2Store::create(&root).expect("store");
    let marker = root.join("clean-exit.json");
    for state in [FAILED, OWED] {
        assert!(!with_state(state).record_exit(&store).expect("record"));
        assert!(!marker.exists());
    }
    for state in [CHECKING, SOUND] {
        assert!(with_state(state).record_exit(&store).expect("record"));
        assert!(marker.exists());
        std::fs::remove_file(&marker).expect("spent");
    }
}

/// A quit while the check reads the Store stops it first, so the exit is recorded at once
/// instead of after Main has given up on Core; a stopped check is not damage.
#[test]
fn a_check_running_at_quit_is_stopped_and_the_exit_recorded_at_once() {
    for (pending, recorded) in [(PendingCheck::Quick, true), (PendingCheck::Full, false)] {
        let dir = tempfile::tempdir().expect("temp");
        let root = dir.path().join("v2");
        let store = aura_persistence_v2::V2Store::create(&root).expect("store");
        let (running, started) = mpsc::channel();
        let check = StoreCheck::start_with(pending, Duration::ZERO, move |stop| {
            running.send(()).expect("test waits");
            while !stop.requested() {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err("interrupted".to_owned())
        });
        started.recv_timeout(Duration::from_secs(10)).expect("the check runs");

        let quit = Instant::now();
        assert_eq!(check.record_exit(&store).expect("record"), recorded, "{pending:?}");
        assert!(quit.elapsed() < Duration::from_secs(1), "{:?}", quit.elapsed());
        assert_eq!(root.join("clean-exit.json").exists(), recorded);
        std::thread::sleep(Duration::from_millis(50));
        assert!(!check.damaged(), "stopping is not damage");
    }
}

/// The owed full check, once passed, lets the exit be recorded again.
#[test]
fn a_passed_full_check_lets_the_normal_exit_be_recorded() {
    let dir = tempfile::tempdir().expect("temp");
    let root = dir.path().join("v2");
    drop(aura_persistence_v2::V2Store::create(&root).expect("store"));
    let store = aura_persistence_v2::V2Store::open(&root).expect("open");
    assert_eq!(store.pending_check(), PendingCheck::Full);
    let check = StoreCheck::start_after(PendingCheck::Full, store.database_path(), Duration::ZERO);
    assert_eq!(settled(&check, OWED), SOUND);
    assert!(check.record_exit(&store).expect("record"));
    assert!(root.join("clean-exit.json").exists());
}
