//! A
//! listing asked for while the start's first reading of the Skill folders runs waits for it
//! instead of reading the same files beside it.
use super::support::*;
use std::time::{Duration, Instant};

#[test]
fn a_listing_waits_for_the_first_reading_at_start() {
    let fixture = Fixture::new();
    skill(&fixture.global(), "notes");
    let runtime = std::sync::Arc::clone(&fixture.runtime);
    let (reading, started_reading) = std::sync::mpsc::channel();
    let first = std::thread::spawn(move || {
        let _held = runtime.warming.lock().expect("warming");
        reading.send(()).expect("reading");
        std::thread::sleep(Duration::from_millis(300));
    });
    started_reading.recv().expect("the first reading started");
    let asked = Instant::now();
    let (_, _, skills) = fixture.runtime.list(None).expect("list");
    assert!(asked.elapsed() >= Duration::from_millis(250), "{:?}", asked.elapsed());
    assert_eq!(skills.len(), 1);
    first.join().expect("first reading");
}
