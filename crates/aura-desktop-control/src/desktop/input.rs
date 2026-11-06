//! Simulated input for one program, step by step. Every step is checked before anything is
//! sent, and before each step the program's window must be in front and the point must be on
//! it; a step that fails says whether anything already reached the program. After a click or a
//! key, the next step waits for a menu or dialog the program opens to finish opening.
use super::apps::windows_of;
use crate::native::{focus, input, screen, window_list};
use crate::plan::{Allowed, Planned, invalid, plan};
use aura_desktop_domain::{
    AppEntry, Delivery, HostError, HostErrorCode, HostResult, InputStep, Point, WindowCheck,
    parse_chord, window_handle,
};
use std::collections::BTreeSet;
use std::time::{Duration, Instant};

/// Text longer than this is pasted when the clipboard may be used.
const PASTE_FROM_CHARS: usize = 400;
/// Lets the program take in one step before the next.
const BETWEEN_STEPS: Duration = Duration::from_millis(40);

/// Takes the pointer to `to` along [`crate::glide::path`], looking before each move whether the
/// user took the desktop over; with a button `held`, along [`crate::glide::drag_path`], leaving
/// the drag's trail.
fn glide(
    screen: aura_desktop_domain::Rect,
    to: Point,
    held: bool,
    guard: Option<&crate::native::guard::Control>,
    done: u32,
) -> Result<bool, HostError> {
    let from = input::cursor().map_or(to, |(x, y)| Point { x, y });
    let points =
        if held { crate::glide::drag_path(from, to) } else { crate::glide::path(from, to, true) };
    for point in points {
        if let Some(guard) = guard {
            guard.checkpoint(done)?;
        }
        if !input::move_to(screen, point.x, point.y) {
            return Ok(false);
        }
        if held {
            crate::native::guard::dragged(point);
        }
        std::thread::sleep(Duration::from_millis(crate::glide::STEP_MS));
    }
    Ok(true)
}

/// Waits `ms`, looking meanwhile whether the user took the desktop over.
fn pause(
    guard: Option<&crate::native::guard::Control>,
    ms: u64,
    done: u32,
) -> Result<(), HostError> {
    let duration = Duration::from_millis(ms);
    match guard {
        Some(guard) => guard.sleep(duration, done),
        None => {
            std::thread::sleep(duration);
            Ok(())
        }
    }
}

/// One click at `at`, the button held as long as a hand holds it. Nothing stops it between press
/// and release, so the button is never left down.
fn click(
    screen: aura_desktop_domain::Rect,
    at: Point,
    button: aura_desktop_domain::MouseButton,
) -> bool {
    if !input::press_at(screen, at.x, at.y, button) {
        return false;
    }
    std::thread::sleep(Duration::from_millis(crate::glide::PRESS_MS));
    input::release_at(screen, at.x, at.y, button)
}

struct Drag<'a> {
    screen: aura_desktop_domain::Rect,
    button: aura_desktop_domain::MouseButton,
    guard: Option<&'a crate::native::guard::Control>,
    automation: &'a crate::native::uia::Automation,
    owned: &'a Owned,
    done: u32,
    held: bool,
    /// Where the pointer was taken last: the press and the release go there.
    at: Point,
}

impl crate::drag::Driver for Drag<'_> {
    fn move_to(&mut self, point: Point) -> Result<bool, HostError> {
        let moved = glide(self.screen, point, self.held, self.guard, self.done)?;
        self.at = point;
        Ok(moved)
    }
    fn press(&mut self) -> bool {
        std::thread::sleep(Duration::from_millis(crate::glide::HOVER_MS));
        self.held = true;
        input::press_at(self.screen, self.at.x, self.at.y, self.button)
    }
    fn release(&mut self) -> bool {
        let sent = input::release_at(self.screen, self.at.x, self.at.y, self.button);
        self.held = false;
        sent
    }
    fn checkpoint(&mut self) -> Result<(), HostError> {
        if let Some(guard) = self.guard {
            guard.checkpoint(self.done)?;
        }
        super::user_desktop()
    }
    fn check_point(&mut self, point: Point) -> Result<(), HostError> {
        if !self.owned.holds(focus::foreground())
            || !self.owned.holds(focus::root_at(point.x, point.y))
        {
            return Err(super::safety::refuse(
                "拖动位置或前台已不属于目标程序，没有继续",
                self.done,
            ));
        }
        super::safety::check_point(self.automation, point, self.done)
    }
}

impl Drop for Drag<'_> {
    fn drop(&mut self) {
        if self.held {
            let _ = input::release_button(self.button);
        }
    }
}

/// The program's windows and processes: a window belongs to it when it is one of them or is
/// any other top-level window of the same processes (a menu, a dialog).
struct Owned {
    windows: BTreeSet<isize>,
    processes: BTreeSet<u32>,
}

impl Owned {
    fn holds(&self, found: Option<(isize, u32)>) -> bool {
        found.is_some_and(|(window, process)| {
            self.windows.contains(&window) || self.processes.contains(&process)
        })
    }
}

pub fn run(
    app: &AppEntry,
    steps: &[InputStep],
    window: Option<&WindowCheck>,
    allowed: Allowed,
) -> Result<HostResult, HostError> {
    super::user_desktop()?;
    let planned = plan(steps, allowed)?;
    let windows = windows_of(app);
    let Some(front) = windows.first() else {
        return Err(HostError::new(
            HostErrorCode::WindowNotFound,
            format!("{} 没有打开的窗口", app.name),
            Delivery::NotDelivered,
        ));
    };
    let owned = Owned {
        windows: windows.iter().filter_map(|w| window_handle(&w.window)).collect(),
        processes: windows.iter().map(|w| w.process_id).collect(),
    };
    if owned.processes.iter().any(|process| focus::above_us(*process)) {
        return Err(HostError::new(
            HostErrorCode::Refused,
            format!("{} 以管理员身份运行，Windows 不让 Aura 给它模拟输入", app.name),
            Delivery::NotDelivered,
        ));
    }
    let target = match window {
        Some(check) => {
            let handle = window_handle(&check.window)
                .filter(|handle| owned.windows.contains(handle))
                .ok_or_else(|| {
                    HostError::new(
                        HostErrorCode::Moved,
                        format!("窗口 {} 已经关了，或不是 {} 的", check.window, app.name),
                        Delivery::NotDelivered,
                    )
                })?;
            if focus::bounds(handle) != check.expected {
                return Err(HostError::new(
                    HostErrorCode::Moved,
                    "截图之后窗口挪了或改了大小，按旧截图的坐标操作会点错地方：请重新截图",
                    Delivery::NotDelivered,
                ));
            }
            handle
        }
        None => window_handle(&front.window).unwrap_or_default(),
    };
    let screen = screen::virtual_screen();
    let guard = crate::native::guard::current();
    // Before any point is checked: the banner would otherwise be the window under it.
    crate::native::guard::make_room(&pointer_points(steps));
    let automation = crate::native::uia::Automation::new(Duration::from_secs(2))
        .map_err(|_| super::safety::refuse("无法检查输入目标，这一步没有执行", 0))?;
    for step in steps {
        match step {
            InputStep::Click { at, .. } => super::safety::check_point(&automation, *at, 0)?,
            InputStep::Drag { path, .. } => {
                for at in path {
                    super::safety::check_point(&automation, *at, 0)?;
                }
            }
            _ => {}
        }
    }
    for (done, step) in planned.iter().enumerate() {
        if let Some(guard) = &guard {
            guard.checkpoint(done as u32)?;
        }
        super::user_desktop()?;
        let failed = |code: HostErrorCode, message: String| HostError {
            code,
            message,
            delivery: if done == 0 { Delivery::NotDelivered } else { Delivery::Uncertain },
            steps_done: Some(done as u32),
        };
        if !owned.holds(focus::foreground())
            && (!focus::bring_to_front(target) || !owned.holds(focus::foreground()))
        {
            return Err(failed(
                HostErrorCode::NotForeground,
                format!("没能把 {} 的窗口拿到前面", app.name),
            ));
        }
        let on_app = |at: &Point| -> Result<(), HostError> {
            if owned.holds(focus::root_at(at.x, at.y)) {
                Ok(())
            } else {
                Err(failed(
                    HostErrorCode::Moved,
                    format!("({}, {}) 上不是 {} 的窗口，没有操作", at.x, at.y, app.name),
                ))
            }
        };
        let opens = crate::popup::appear_ms(step);
        let before: BTreeSet<isize> = match opens {
            Some(_) => window_list::of_processes(&owned.processes)
                .into_iter()
                .map(|(handle, _)| handle)
                .collect(),
            None => BTreeSet::new(),
        };
        let sent = match step {
            Planned::Chord(chord, repeat) => {
                let mut sent = true;
                for _ in 0..*repeat {
                    if let Some(guard) = &guard {
                        guard.checkpoint(done as u32)?;
                    }
                    super::safety::check_focus(&automation, done as u32)?;
                    sent = sent && input::chord(&chord.modifiers, chord.key);
                }
                sent
            }
            Planned::Step(InputStep::Click { at, button, count }) => {
                on_app(at)?;
                let mut sent = glide(screen, *at, false, guard.as_deref(), done as u32)?;
                pause(guard.as_deref(), crate::glide::HOVER_MS, done as u32)?;
                for clicked in 0..*count {
                    let partly = |error: HostError| {
                        if clicked == 0 {
                            error
                        } else {
                            HostError { delivery: Delivery::Uncertain, ..error }
                        }
                    };
                    if let Some(guard) = &guard {
                        guard.checkpoint(done as u32).map_err(partly)?;
                    }
                    super::safety::check_point(&automation, *at, done as u32).map_err(partly)?;
                    if clicked > 0 {
                        std::thread::sleep(Duration::from_millis(crate::glide::BETWEEN_CLICKS_MS));
                    }
                    sent = sent && click(screen, *at, *button);
                }
                // Drawn after the clicks, so that nothing of it is under the point they check.
                crate::native::guard::clicked(*at);
                sent
            }
            Planned::Step(InputStep::Move { at }) => {
                on_app(at)?;
                glide(screen, *at, false, guard.as_deref(), done as u32)?
            }
            Planned::Step(InputStep::Drag { path, button }) => {
                on_app(&path[0])?;
                crate::drag::run(
                    &mut Drag {
                        screen,
                        button: *button,
                        guard: guard.as_deref(),
                        automation: &automation,
                        done: done as u32,
                        owned: &owned,
                        held: false,
                        at: path[0],
                    },
                    path,
                    done as u32,
                )?
            }
            Planned::Step(InputStep::Scroll { at, down, right }) => {
                on_app(at)?;
                let mut sent = glide(screen, *at, false, guard.as_deref(), done as u32)?;
                pause(guard.as_deref(), crate::glide::HOVER_MS, done as u32)?;
                // A notch at a time, as a hand turns the wheel.
                let most = aura_desktop_domain::MAX_SCROLL;
                let (down, right) = ((*down).clamp(-most, most), (*right).clamp(-most, most));
                let notches = down.unsigned_abs().max(right.unsigned_abs());
                for notch in 0..notches {
                    if notch > 0 {
                        pause(guard.as_deref(), crate::glide::NOTCH_MS, done as u32).map_err(
                            |error| HostError { delivery: Delivery::Uncertain, ..error },
                        )?;
                    }
                    let turn = |amount: i32| {
                        if notch < amount.unsigned_abs() { amount.signum() } else { 0 }
                    };
                    sent = sent && input::wheel_at(screen, at.x, at.y, turn(down), turn(right));
                }
                sent
            }
            Planned::Step(InputStep::Type { text }) => {
                super::safety::check_focus(&automation, done as u32)?;
                if allowed.clipboard && text.chars().count() >= PASTE_FROM_CHARS {
                    input::set_clipboard_text(text).map_err(|error| {
                        failed(HostErrorCode::Internal, format!("写不了剪贴板：{error}"))
                    })?;
                    let paste = parse_chord("Control+v").map_err(invalid)?;
                    input::chord(&paste.modifiers, paste.key)
                } else {
                    input::type_text(text)
                }
            }
            Planned::Step(InputStep::Wait { ms }) => {
                let duration = Duration::from_millis(u64::from(*ms));
                if let Some(guard) = &guard {
                    guard.sleep(duration, done as u32)?;
                } else {
                    std::thread::sleep(duration);
                }
                true
            }
            Planned::Step(InputStep::Press { .. }) => true,
        };
        if !sent {
            return Err(failed(HostErrorCode::Refused, "Windows 没有接受这次模拟输入".into()));
        }
        if let Some(guard) = &guard {
            guard.completed(done as u32 + 1);
        }
        match opens {
            Some(appear_ms) => settle(&owned.processes, &before, appear_ms),
            None => std::thread::sleep(BETWEEN_STEPS),
        }
    }
    if let Some(guard) = &guard {
        guard.checkpoint(planned.len() as u32)?;
    }
    Ok(HostResult::Input { steps_done: planned.len() as u32 })
}

/// Where the steps take Aura's pointer.
fn pointer_points(steps: &[InputStep]) -> Vec<Point> {
    steps
        .iter()
        .flat_map(|step| match step {
            InputStep::Click { at, .. } | InputStep::Move { at } | InputStep::Scroll { at, .. } => {
                vec![*at]
            }
            InputStep::Drag { path, .. } => path.clone(),
            _ => Vec::new(),
        })
        .collect()
}

/// Lets a menu or dialog the step opened finish opening (see [`crate::popup`]).
fn settle(processes: &BTreeSet<u32>, before: &BTreeSet<isize>, appear_ms: u64) {
    let start = Instant::now();
    crate::popup::settle(
        before,
        appear_ms,
        || {
            std::thread::sleep(Duration::from_millis(crate::popup::POLL_MS));
            start.elapsed().as_millis() as u64
        },
        || window_list::of_processes(processes),
    );
}
