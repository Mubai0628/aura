//! B2 Native write/remote process lifecycle. Read jobs deliberately cannot create children.
use super::{tests::fixture, *};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[test]
fn timeout_and_cancellation_confirm_the_whole_git_child_job_has_stopped() {
    let (root, runner) = fixture();
    for cancel_requested in [false, true] {
        let cancelled = Arc::new(AtomicBool::new(false));
        let signal = cancelled.clone();
        let trigger = cancel_requested.then(|| {
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(250));
                signal.store(true, Ordering::Release);
            })
        });
        let began = std::time::Instant::now();
        let result = runner
            .run_native(
                root.path(),
                &["-c".into(), "alias.wait=!echo ready; sleep 30".into(), "wait".into()],
                &cancelled,
                if cancel_requested { Duration::from_secs(5) } else { Duration::from_millis(300) },
                65536,
            )
            .expect("Native bounded Job");
        if let Some(trigger) = trigger {
            trigger.join().expect("trigger");
        }
        assert_eq!(result.reason, if cancel_requested { "cancelled" } else { "timed_out" });
        assert!(result.stopped && result.exit_code.is_some(), "{result:?}");
        assert!(
            String::from_utf8_lossy(&result.stdout).contains("ready"),
            "child started: {result:?}"
        );
        assert!(began.elapsed() < Duration::from_secs(5));
    }
}

#[test]
fn continuous_output_is_capped_and_its_job_is_stopped() {
    let (root, runner) = fixture();
    let result = runner
        .run_native(
            root.path(),
            &[
                "-c".into(),
                "alias.flood=!while :; do echo bounded-output; done".into(),
                "flood".into(),
            ],
            &AtomicBool::new(false),
            Duration::from_secs(5),
            1024,
        )
        .expect("Native output cap");
    assert_eq!(result.reason, "output_limit");
    assert!(result.stopped && result.exit_code.is_some());
    assert_eq!(result.stdout.len() + result.stderr.len(), 1024);
}

#[test]
fn native_start_notification_is_absent_for_prestart_cancellation_and_present_for_a_real_process() {
    let (root, runner) = fixture();
    for cancelled in [true, false] {
        let mut started = false;
        let result = runner
            .run_native_at_start(
                root.path(),
                &["--version".into()],
                &AtomicBool::new(cancelled),
                (Duration::from_secs(5), 65536),
                &mut || started = true,
            )
            .expect("bounded Native start");
        assert_eq!(started, !cancelled);
        assert_eq!(result.reason, if cancelled { "cancelled" } else { "exited" });
        assert!(result.stopped);
    }
}

#[test]
fn remote_start_retains_selected_user_proxy_settings_and_never_starts_a_cancelled_call() {
    let (root, mut runner) = fixture();
    environment::put(&mut runner.environment, "HTTP_PROXY", "http://127.0.0.1:9");
    environment::put(&mut runner.environment, "NO_PROXY", "fixture.invalid");
    for cancelled in [true, false] {
        let mut started = false;
        let result = runner
            .run_remote_at_start(
                root.path(),
                &[
                    "-c".into(),
                    "alias.fixture-env=!printf '%s\\n' \"$HTTP_PROXY\" \"$NO_PROXY\"".into(),
                    "fixture-env".into(),
                ],
                &AtomicBool::new(cancelled),
                (Duration::from_secs(5), 65536),
                &mut || started = true,
            )
            .expect("remote Native Job; fixture only prints environment, no HTTP request");
        assert_eq!(started, !cancelled);
        assert!(result.stopped);
        if cancelled {
            assert_eq!(result.reason, "cancelled");
            assert!(result.stdout.is_empty());
        } else {
            assert_eq!(result.exit_code, Some(0), "{result:?}");
            assert_eq!(result.stdout, b"http://127.0.0.1:9\nfixture.invalid\n");
        }
    }
}
