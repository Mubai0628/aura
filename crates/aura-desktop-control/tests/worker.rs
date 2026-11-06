//! UI Automation requests on threads of their own: the host answers in time whatever a program
//! does, and stops leaving threads behind once enough are stuck.
use aura_desktop_control::desktop::worker::{MAX_LEFT_WAITING, Ran, on_own_thread};
use aura_desktop_domain::{Delivery, HostErrorCode};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const STUCK: Duration = Duration::from_millis(1_500);

/// The count of stuck requests is the process's own, so these tests take turns.
static TURN: Mutex<()> = Mutex::new(());

#[test]
fn the_host_answers_in_time_and_refuses_more_once_enough_requests_are_stuck() {
    let _turn = TURN.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    assert!(matches!(on_own_thread(Duration::from_secs(5), || Ok(7)), Ran::Done(Ok(7))));

    let started = Instant::now();
    for _ in 0..MAX_LEFT_WAITING {
        let ran = on_own_thread(Duration::from_millis(100), || {
            std::thread::sleep(STUCK);
            Ok(())
        });
        assert!(matches!(ran, Ran::TimedOut));
    }
    assert!(started.elapsed() < STUCK, "each waited only its own time: {:?}", started.elapsed());

    let refused = on_own_thread(Duration::from_secs(5), || Ok(()));
    let Ran::Done(Err(error)) = refused else { panic!("not refused") };
    assert_eq!((error.code, error.delivery), (HostErrorCode::Timeout, Delivery::NotDelivered));

    std::thread::sleep(STUCK + Duration::from_millis(500));
    assert!(matches!(on_own_thread(Duration::from_secs(5), || Ok(8)), Ran::Done(Ok(8))));
}

#[test]
fn requests_that_are_answered_in_time_are_never_refused_however_many_run_at_once() {
    let _turn = TURN.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let callers: Vec<_> = (0..MAX_LEFT_WAITING as u32 * 2)
        .map(|index| {
            std::thread::spawn(move || {
                on_own_thread(Duration::from_secs(10), move || {
                    std::thread::sleep(Duration::from_millis(300));
                    Ok(index)
                })
            })
        })
        .collect();
    for (index, caller) in callers.into_iter().enumerate() {
        let ran = caller.join().expect("caller");
        assert!(matches!(ran, Ran::Done(Ok(answer)) if answer == index as u32), "request {index}");
    }
}
