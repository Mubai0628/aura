//! The real desktop for the tests, and Aura's test window run from a folder of each test's own,
//! with the log of what happened to it.
#![allow(dead_code)]
use aura_desktop_control::desktop::{RealDesktop, windows_of};
use aura_desktop_control::host::Desktop;
use aura_desktop_control::native::dpi;
use aura_desktop_domain::*;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

pub const FIXTURE: &str = env!("CARGO_BIN_EXE_aura-desktop-fixture");
pub const HOST: &str = env!("CARGO_BIN_EXE_aura-desktop-host");

pub fn desktop() -> RealDesktop {
    dpi::per_monitor_aware();
    RealDesktop::new().expect("COM")
}

pub fn done(outcome: HostOutcome) -> HostResult {
    match outcome {
        HostOutcome::Done(result) => result,
        HostOutcome::Failed(error) => panic!("{error:?}"),
    }
}

pub fn failed(outcome: HostOutcome) -> HostError {
    match outcome {
        HostOutcome::Failed(error) => error,
        HostOutcome::Done(result) => panic!("{result:?}"),
    }
}

pub fn catalog(desktop: &mut RealDesktop, manual: Vec<ManualApp>) -> Vec<AppEntry> {
    match done(desktop.run(HostCommand::Catalog { manual })) {
        HostResult::Catalog { apps } => apps,
        other => panic!("{other:?}"),
    }
}

#[must_use]
pub fn center(rect: Rect) -> Point {
    Point { x: rect.x + rect.width / 2, y: rect.y + rect.height / 2 }
}

pub fn read(
    desktop: &mut RealDesktop,
    app: &AppEntry,
    root: Option<ElementKey>,
    max_elements: u32,
    max_depth: u32,
    timeout_ms: u32,
) -> HostOutcome {
    desktop.run(HostCommand::Elements {
        app: app.clone(),
        window: None,
        root,
        max_elements,
        max_depth,
        timeout_ms,
    })
}

/// The window, the elements and whether the read was complete.
pub fn elements_of(outcome: HostOutcome) -> (WindowInfo, Vec<Element>, bool) {
    match done(outcome) {
        HostResult::Elements { window, elements, complete } => (window, elements, complete),
        other => panic!("{other:?}"),
    }
}

/// A whole read of the program's front window.
pub fn read_all(desktop: &mut RealDesktop, app: &AppEntry) -> Vec<Element> {
    let (_, elements, complete) = elements_of(read(desktop, app, None, 500, 32, 8_000));
    assert!(complete, "{}", outline(&elements));
    elements
}

pub fn act(
    desktop: &mut RealDesktop,
    app: &AppEntry,
    window: &str,
    steps: Vec<ActStep>,
) -> HostOutcome {
    desktop.run(HostCommand::Act {
        app: app.clone(),
        window: window.into(),
        steps,
        timeout_ms: 8_000,
    })
}

pub fn acted(outcome: HostOutcome) -> Vec<Option<Element>> {
    match done(outcome) {
        HostResult::Acted { after } => after,
        other => panic!("{other:?}"),
    }
}

/// The elements as lines of depth, role and name, for messages.
#[must_use]
pub fn outline(elements: &[Element]) -> String {
    elements
        .iter()
        .map(|element| {
            format!(
                "{}{:?} {:?}{}",
                "  ".repeat(element.depth as usize),
                element.role,
                element.key.name,
                if element.more { " (more)" } else { "" }
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn named<'a>(elements: &'a [Element], role: Role, name: &str) -> &'a Element {
    elements
        .iter()
        .find(|element| element.role == role && element.key.name == name)
        .unwrap_or_else(|| panic!("no {role:?} {name:?} in\n{}", outline(elements)))
}

/// The element whose place on the screen is the logged place of a control of the test window.
pub fn at_place(elements: &[Element], place: Rect) -> &Element {
    elements
        .iter()
        .find(|element| element.rect == place)
        .unwrap_or_else(|| panic!("nothing at {place:?} in\n{}", outline(elements)))
}

/// A copy of the test window program of this test's own, so that no other test's window counts
/// as one of its windows; removed with the windows it opened.
pub struct OwnFixture {
    pub folder: PathBuf,
    pub app: AppEntry,
    /// Processes the host opened, ended by id.
    pub processes: Vec<u32>,
    pub log: PathBuf,
    started: Vec<Child>,
}

impl OwnFixture {
    pub fn new(name: &str) -> Self {
        let folder =
            std::env::temp_dir().join(format!("aura-desktop-test-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&folder).expect("folder");
        let path = folder.join(format!("aura-fixture-{name}.exe"));
        std::fs::copy(FIXTURE, &path).expect("copy");
        let path = path.display().to_string();
        let app = AppEntry {
            id: path.clone(),
            name: format!("测试窗口 {name}"),
            kind: AppKind::Manual,
            path: Some(path),
        };
        let log = folder.join("events.log");
        Self { folder, app, processes: Vec::new(), log, started: Vec::new() }
    }

    /// Starts the window with its top left at a screen point and waits until it shows and has
    /// logged where its controls are.
    pub fn open(&mut self, at: (i32, i32)) -> WindowInfo {
        self.open_with(at, &[])
    }

    /// `open` with more of the window's arguments, such as `--hang-at`.
    pub fn open_with(&mut self, at: (i32, i32), arguments: &[&str]) -> WindowInfo {
        dpi::per_monitor_aware();
        let child = Command::new(&self.app.id)
            .arg("--log")
            .arg(&self.log)
            .args(["--at", &format!("{},{}", at.0, at.1)])
            .args(arguments)
            .spawn()
            .expect("the test window");
        let process = child.id();
        self.started.push(child);
        assert!(self.wait_for(
            |events| events.iter().any(|event| event == "ready"),
            Duration::from_secs(20)
        ));
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(window) =
                windows_of(&self.app).into_iter().find(|window| window.process_id == process)
            {
                return window;
            }
            assert!(Instant::now() < deadline, "the test window did not show");
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    pub fn events(&self) -> Vec<String> {
        std::fs::read_to_string(&self.log).unwrap_or_default().lines().map(str::to_owned).collect()
    }

    /// The events after the window said it was ready.
    pub fn since_ready(&self) -> Vec<String> {
        let events = self.events();
        let start = events.iter().position(|event| event == "ready").map_or(0, |ready| ready + 1);
        events[start..].to_vec()
    }

    pub fn wait_for(&self, done: impl Fn(&[String]) -> bool, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            if done(&self.events()) {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Where every control and `client`, the client area, are on the screen, from the log.
    pub fn places(&self) -> Vec<(String, Rect)> {
        self.events()
            .iter()
            .filter_map(|event| match event.split(' ').collect::<Vec<_>>().as_slice() {
                ["place", name, x, y, width, height] => Some((
                    (*name).to_owned(),
                    Rect {
                        x: x.parse().ok()?,
                        y: y.parse().ok()?,
                        width: width.parse().ok()?,
                        height: height.parse().ok()?,
                    },
                )),
                _ => None,
            })
            .collect()
    }

    /// Where a control (or `client`, the client area) is on the screen, from the log.
    pub fn place(&self, name: &str) -> Rect {
        self.places()
            .into_iter()
            .find_map(|(found, place)| (found == name).then_some(place))
            .unwrap_or_else(|| panic!("the log has no place for {name}: {:?}", self.events()))
    }

    /// The controls whose place holds a screen point, hidden ones included.
    pub fn controls_at(&self, at: Point) -> Vec<String> {
        self.places()
            .into_iter()
            .filter(|(name, place)| name != "client" && place.contains(at.x, at.y))
            .map(|(name, _)| name)
            .collect()
    }
}

impl Drop for OwnFixture {
    fn drop(&mut self) {
        for child in &mut self.started {
            let _ = child.kill();
            let _ = child.wait();
        }
        for process in &self.processes {
            let _ = Command::new("taskkill").args(["/PID", &process.to_string(), "/F"]).output();
        }
        for _ in 0..20 {
            if std::fs::remove_dir_all(&self.folder).is_ok() {
                break;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }
}
