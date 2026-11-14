//! The progress window of work that runs before any other window: it waits before it shows,
//! follows the work, and goes when the work ends.
use std::{
    thread,
    time::{Duration, Instant},
};

use super::{launcher_progress_page, one_window_at_a_time};
use crate::{
    page::{Button, Kind},
    progress::{Progress, SHOW_AFTER, STOP, with_progress_window},
    testing::{invoke, windows},
};

#[test]
fn quick_work_returns_its_result_without_a_window() {
    let _one = one_window_at_a_time();
    let started = Instant::now();
    let result = with_progress_window(
        |_| launcher_progress_page(),
        |progress| {
            progress.set_total(10);
            progress.advance(10);
            42
        },
    );
    assert_eq!(result, 42);
    assert!(started.elapsed() < SHOW_AFTER * 4);
    assert!(windows().is_empty());
}

#[test]
fn slow_work_shows_a_window_that_goes_when_the_work_ends() {
    let _one = one_window_at_a_time();
    let mut seen = false;
    let result = with_progress_window(
        |snapshot| {
            let mut page = launcher_progress_page();
            if let Some(meter) = page.meter_mut() {
                meter.fraction = snapshot.fraction();
            }
            page
        },
        |progress| {
            progress.set_total(10);
            let started = Instant::now();
            while started.elapsed() < SHOW_AFTER * 3 {
                seen |= !windows().is_empty();
                progress.advance(1);
                thread::sleep(Duration::from_millis(50));
            }
            "done"
        },
    );
    assert_eq!(result, "done");
    assert!(seen, "the window showed while the work ran");
    assert!(windows().is_empty(), "and went when it ended");
}

#[test]
fn progress_is_a_fraction_capped_at_one_or_none_while_unmeasured() {
    let progress = Progress::default();
    assert_eq!(progress.snapshot().fraction(), Some(0.0));
    progress.set_total(4);
    progress.advance(1);
    assert_eq!(progress.snapshot().fraction(), Some(0.25));
    progress.advance(10);
    assert_eq!(progress.snapshot().fraction(), Some(1.0));
    progress.set_stage("整理数据库文件", true);
    let snapshot = progress.snapshot();
    assert_eq!((snapshot.fraction(), snapshot.stage.as_deref()), (None, Some("整理数据库文件")));
}

#[test]
fn the_stop_button_asks_the_work_to_stop_and_the_window_goes_when_it_does() {
    let _one = one_window_at_a_time();
    let mut pressed = false;
    let result = with_progress_window(
        |snapshot| {
            let mut page = launcher_progress_page();
            page.footer.end = vec![Button {
                id: STOP,
                label: "取消".to_owned(),
                kind: Kind::Normal,
                enabled: !snapshot.stop_asked,
            }];
            page
        },
        |progress| {
            let started = Instant::now();
            while started.elapsed() < Duration::from_secs(20) {
                if progress.stop_asked() {
                    return "stopped";
                }
                if !pressed && let Some(window) = windows().first() {
                    pressed = invoke(*window, "button-stop").is_ok();
                }
                thread::sleep(Duration::from_millis(50));
            }
            "ran to the end"
        },
    );
    assert!(pressed, "the window showed its stop button");
    assert_eq!(result, "stopped");
    assert!(windows().is_empty());
}
