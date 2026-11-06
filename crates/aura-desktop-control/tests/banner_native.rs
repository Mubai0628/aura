//! The banner on the real desktop: it goes back on top at once when another window covers it, it
//! steps out of the way of Aura's own click so that the click lands, and it can be drawn as a
//! picture for a person to look at (no capture shows the banner itself). These take the
//! foreground and click; they run one at a time only in the owner's idle, unlocked desktop window.
mod support;
use aura_desktop_control::host::Desktop;
use aura_desktop_control::native::guard;
use aura_desktop_domain::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use support::*;

fn held(command: HostCommand, hold: &str) -> HostCommand {
    HostCommand::Guarded {
        command: Box::new(command),
        hold: Some(hold.into()),
        stop_keys: Some(pip::PIP_STOP_KEYS.into()),
    }
}

fn steps(app: &AppEntry, steps: Vec<InputStep>) -> HostCommand {
    HostCommand::Input {
        app: app.clone(),
        steps,
        window: None,
        system_keys: false,
        clipboard: false,
    }
}

fn wait_until(what: &str, within: Duration, done: impl Fn() -> bool) {
    let until = Instant::now() + within;
    while !done() {
        assert!(Instant::now() < until, "{what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The longest another window may stand above the banner.
const BACK_ON_TOP: Duration = Duration::from_millis(60);

/// How long each time the banner was covered lasted, watched every millisecond from another
/// thread until `watching` turns false.
fn covers(watching: Arc<AtomicBool>) -> std::thread::JoinHandle<Vec<Duration>> {
    std::thread::spawn(move || {
        let (mut covers, mut since) = (Vec::new(), None);
        while watching.load(Ordering::SeqCst) {
            match (guard::banner_on_top(), since) {
                (false, None) => since = Some(Instant::now()),
                (true, Some(at)) => {
                    covers.push(Instant::now() - at);
                    since = None;
                }
                _ => {}
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        covers
    })
}

#[test]
#[ignore = "takes the foreground and shows the banner; owner idle for 10 minutes"]
fn the_banner_goes_back_on_top_at_once_when_a_window_covers_it() {
    let mut fixture = OwnFixture::new("banner-top");
    fixture.open((80, 80));
    let mut desktop = desktop();
    done(desktop.run(held(steps(&fixture.app, vec![InputStep::Wait { ms: 100 }]), "b1")));
    let banner = guard::banner_place().expect("the banner shows");
    let watching = Arc::new(AtomicBool::new(true));
    let watcher = covers(Arc::clone(&watching));
    let mut above = Vec::new();
    for n in 0..4 {
        let mut window = OwnFixture::new(&format!("banner-above-{n}"));
        window.open_with((banner.x + 20 + 40 * n, banner.y - 10), &["--topmost"]);
        wait_until("the banner went back on top", Duration::from_secs(2), guard::banner_on_top);
        above.push(window);
    }
    watching.store(false, Ordering::SeqCst);
    let covers = watcher.join().expect("watched");
    assert!(covers.iter().all(|cover| *cover <= BACK_ON_TOP), "{covers:?}");
    assert_eq!(done(desktop.run(HostCommand::Release { hold: "b1".into() })), HostResult::Released);
}

#[test]
#[ignore = "clicks a button under the banner; owner idle for 10 minutes"]
fn the_banner_steps_out_of_the_way_of_auras_click_and_the_click_lands() {
    let mut first = OwnFixture::new("banner-way");
    let window = first.open((80, 80));
    let mut desktop = desktop();
    done(desktop.run(held(steps(&first.app, vec![InputStep::Wait { ms: 100 }]), "b2")));
    let banner = guard::banner_place().expect("the banner shows");
    let ok = center(first.place("ok"));
    let middle = Point { x: banner.x + banner.width / 2, y: banner.y + banner.height / 2 };
    let mut under = OwnFixture::new("banner-under");
    under.open((middle.x - (ok.x - window.rect.x), middle.y - (ok.y - window.rect.y)));
    let ok = center(under.place("ok"));
    assert!(banner.contains(ok.x, ok.y), "the button is under the banner: {ok:?} {banner:?}");
    let click = InputStep::Click { at: ok, button: MouseButton::Left, count: 1 };
    assert_eq!(
        done(desktop.run(held(steps(&under.app, vec![click]), "b2"))),
        HostResult::Input { steps_done: 1 }
    );
    assert!(under.events().iter().any(|e| e == "click ok 1"), "{:?}", under.events());
    let moved = guard::banner_place().expect("the banner still shows");
    assert!(!moved.contains(ok.x, ok.y) && moved.y > banner.y, "{moved:?}");
    assert_eq!(done(desktop.run(HostCommand::Release { hold: "b2".into() })), HostResult::Released);
}

#[test]
#[ignore = "draws the banner into AURA_BANNER_PICTURE (a folder) for a person to look at"]
fn the_banner_as_a_picture() {
    let Some(folder) = std::env::var_os("AURA_BANNER_PICTURE") else { return };
    let _desktop = desktop();
    let paint = AppEntry {
        id: "Microsoft.Paint_8wekyb3d8bbwe!App".into(),
        name: "画图".into(),
        kind: AppKind::Packaged,
        path: None,
    };
    for (scale, keys) in [(1.0, Some(pip::PIP_STOP_KEYS)), (1.5, None), (2.0, Some("Ctrl+Alt+Q"))] {
        let png = guard::banner_picture(&paint, keys, scale).expect("drawn");
        let name = format!("banner-{}.png", (scale * 100.0) as u32);
        std::fs::write(std::path::Path::new(&folder).join(name), png).expect("written");
    }
}
