//! The desktop as Aura keeps it: for a hold (see [`crate::hold`]) from its first command to its
//! end, with the banner, the pointer's marks and the watch on the user's input between the
//! hold's commands too. Low-level hooks stay on their own message thread; their callbacks only
//! update small local state.
mod banner;
mod hooks;
mod marker;
pub use banner::{banner_on_top, banner_picture, banner_place, banner_status, make_room};
pub use marker::marker_status;

use crate::guard::{Gate, State, Takeover};
use crate::hold::{Begin, Ended, Holds};
use crate::host::{Output, say, write};
use crate::pointer_marks::Effects;
use aura_desktop_domain::{
    AppEntry, Chord, Delivery, HoldEnd, HostError, HostErrorCode, HostMessage, Point,
    taken_over_message,
};
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

struct Held {
    holds: Holds,
    /// The kept hold's latest command: what the hooks watch and the banner names.
    control: Option<Arc<Control>>,
}

static HELD: Mutex<Held> = Mutex::new(Held { holds: Holds::new(), control: None });
static EFFECTS: Mutex<Effects> = Mutex::new(Effects::new());
static STARTED: OnceLock<Result<(), String>> = OnceLock::new();
static CLOCK: OnceLock<Instant> = OnceLock::new();
static SLOT: Mutex<()> = Mutex::new(());
pub(super) static ALIVE: AtomicBool = AtomicBool::new(false);

fn taken_over(takeover: Option<Takeover>) -> String {
    taken_over_message(takeover.map(Takeover::words).as_deref())
}

pub(super) fn now() -> u64 {
    CLOCK.get_or_init(Instant::now).elapsed().as_millis() as u64
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub struct Control {
    /// Shared by the hold's commands: a takeover between them stops the next.
    pub(super) state: Arc<Mutex<State>>,
    pub(super) processes: BTreeSet<u32>,
    pub(super) windows: BTreeSet<isize>,
    pub(super) name: String,
    /// The stop shortcut the hooks watch for besides Escape, and as the banner names it.
    pub(super) stop_keys: Option<(Chord, String)>,
    /// The program's window in front when the command began: the banner stands on its screen.
    front: Option<isize>,
    icon: Option<Arc<banner::Icon>>,
    app_id: String,
    output: Option<(Output, u64)>,
    done: AtomicU32,
    reported: AtomicBool,
}

/// One command's share of the kept desktop; dropping it ends the command.
pub struct Lease {
    control: Arc<Control>,
    _slot: MutexGuard<'static, ()>,
}

impl Lease {
    /// The command for `app` of hold `hold` (`None`: of no hold) begins; `stop_keys`, as
    /// [`aura_desktop_domain::pip::stop_chord`] reads them, stop it besides Escape. Unreadable
    /// ones are left out.
    pub fn begin(
        app: &AppEntry,
        hold: Option<&str>,
        output: Option<(Output, u64)>,
        stop_keys: Option<&str>,
    ) -> Result<Self, HostError> {
        let slot = lock(&SLOT);
        STARTED.get_or_init(hooks::start).clone().map_err(|why| {
            HostError::new(
                HostErrorCode::Refused,
                format!("看护用户输入的线程不可用：{why}，没有执行"),
                Delivery::NotDelivered,
            )
        })?;
        if !ALIVE.load(Ordering::SeqCst) {
            return Err(HostError::new(
                HostErrorCode::Refused,
                "看护用户输入的线程已退出，没有执行",
                Delivery::NotDelivered,
            ));
        }
        let windows = crate::desktop::windows_of(app);
        let front = windows.first().and_then(|w| aura_desktop_domain::window_handle(&w.window));
        // Read before the hooks' lock: the Shell can take a while the first time.
        let icon = banner::icon_for(app, banner::scale_of(front));
        let mut held = lock(&HELD);
        let takeover = held.control.as_ref().and_then(|control| control.takeover());
        let (begin, ended) = held.holds.begin(hold, takeover.is_some(), now());
        if let Some(ended) = ended {
            tell(&mut held, ended);
        }
        let state = match begin {
            Begin::Busy => {
                return Err(HostError::new(
                    HostErrorCode::Busy,
                    "另一段对话正在操控电脑，等它结束再操作",
                    Delivery::NotDelivered,
                ));
            }
            Begin::TakenOver => {
                return Err(HostError::new(
                    HostErrorCode::TakenOver,
                    taken_over(takeover),
                    Delivery::NotDelivered,
                ));
            }
            Begin::Continued => held.control.as_ref().map(|control| Arc::clone(&control.state)),
            Begin::Started => {
                banner::hold_started();
                None
            }
        }
        .unwrap_or_else(|| Arc::new(Mutex::new(State::new())));
        let control = Arc::new(Control {
            state,
            processes: windows
                .iter()
                .filter(|w| !w.process_name.eq_ignore_ascii_case("ApplicationFrameHost.exe"))
                .map(|w| w.process_id)
                .collect(),
            windows: windows
                .iter()
                .filter_map(|w| aura_desktop_domain::window_handle(&w.window))
                .collect(),
            name: app.name.chars().take(160).collect(),
            stop_keys: stop_keys.and_then(|keys| {
                aura_desktop_domain::pip::stop_chord(keys).ok().map(|chord| (chord, keys.into()))
            }),
            front,
            icon,
            app_id: app.id.clone(),
            output,
            done: AtomicU32::new(0),
            reported: AtomicBool::new(false),
        });
        held.control = Some(Arc::clone(&control));
        Ok(Self { control, _slot: slot })
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        self.control.report(self.control.done.load(Ordering::SeqCst));
        banner::command_ended();
        let mut held = lock(&HELD);
        if held.holds.finish(now()) {
            held.control = None;
        }
    }
}

/// Core says the turn of hold `hold` goes on; whether the desktop is still kept for it. A hold
/// the user took over, or that waited too long, ends here as at the next look.
pub fn keep(hold: &str) -> bool {
    let mut held = lock(&HELD);
    let taken_over = held.control.as_ref().is_some_and(|control| control.taken_over());
    if let Some(ended) = held.holds.tick(taken_over, now()) {
        tell(&mut held, ended);
    }
    held.holds.keep(hold, now())
}

/// Core gives hold `hold` back; nothing when it is not kept.
pub fn release(hold: &str) {
    let mut held = lock(&HELD);
    if held.holds.release(hold) {
        held.control = None;
    }
}

/// The kept hold's command, between commands too.
#[must_use]
pub fn current() -> Option<Arc<Control>> {
    lock(&HELD).control.clone()
}

/// Looked at every few milliseconds by the hooks' thread: ends a hold between commands that the
/// user took over or that waited too long, and says so.
pub(super) fn tick() {
    let mut held = lock(&HELD);
    let taken_over = held.control.as_ref().is_some_and(|control| control.taken_over());
    if let Some(ended) = held.holds.tick(taken_over, now()) {
        tell(&mut held, ended);
    }
}

/// The kept desktop is given back: the banner and the ring go, and Core hears of a hold's end and
/// what of the user's took it over.
fn tell(held: &mut Held, ended: Ended) {
    let control = held.control.take();
    if let (Some(hold), Some(control)) = (ended.key, control.as_ref())
        && let Some((output, _)) = control.output.as_ref()
    {
        let cause = (ended.reason == HoldEnd::TakenOver)
            .then(|| control.takeover())
            .flatten()
            .map(Takeover::words);
        say(output, HostMessage::HoldEnded { hold, reason: ended.reason, cause });
    }
}

/// Aura clicked at `at`: a ripple shows there.
pub fn clicked(at: Point) {
    lock(&EFFECTS).click(at, now());
}

/// The pointer is at `at` with a button held: the drag leaves its trail there.
pub fn dragged(at: Point) {
    lock(&EFFECTS).drag(at, now());
}

impl Control {
    pub(super) fn taken_over(&self) -> bool {
        lock(&self.state).gate() == Gate::TakenOver
    }

    fn takeover(&self) -> Option<Takeover> {
        lock(&self.state).takeover()
    }

    pub fn take_over(&self) {
        lock(&self.state).take_over();
    }

    fn report(&self, done: u32) {
        let first = !self.reported.swap(true, Ordering::SeqCst);
        let progressed = self.done.swap(done, Ordering::SeqCst) != done;
        if !first && !progressed {
            return;
        }
        if let Some((output, id)) = &self.output {
            let _ = write(
                output,
                &HostMessage::Control { id: *id, app_id: self.app_id.clone(), steps_done: done },
            );
        }
    }

    pub fn completed(&self, done: u32) {
        self.report(done);
    }

    /// Whether the command may go on to its next event: not once the user took the desktop
    /// over, or nothing watches the user's input any more.
    pub fn checkpoint(&self, done: u32) -> Result<(), HostError> {
        let delivery = if done == 0 { Delivery::NotDelivered } else { Delivery::Uncertain };
        if !ALIVE.load(Ordering::SeqCst) {
            return Err(HostError {
                code: HostErrorCode::Refused,
                message: "看护用户输入的线程已退出，已停止".into(),
                delivery,
                steps_done: Some(done),
            });
        }
        if let Some(takeover) = self.takeover() {
            return Err(HostError {
                code: HostErrorCode::TakenOver,
                message: taken_over(Some(takeover)),
                delivery,
                steps_done: Some(done),
            });
        }
        self.report(done);
        Ok(())
    }

    pub fn sleep(&self, duration: Duration, done: u32) -> Result<(), HostError> {
        let until = Instant::now() + duration;
        loop {
            self.checkpoint(done)?;
            let remaining = until.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Ok(());
            }
            std::thread::sleep(remaining.min(Duration::from_millis(20)));
        }
    }
}
