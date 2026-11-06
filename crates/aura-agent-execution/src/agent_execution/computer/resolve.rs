//! What a computer call operates, decided by Core before the permission engine sees it.
//!
//! A call names one program of the catalog and is frozen into the one host command it sends; its
//! refs (elements e12, screenshots p3) become what they stood for when they were handed out.
//! What the first resolution read, including whether the program was allowed, is kept by call,
//! so every later resolution of the call reads the same.
use super::super::*;
use super::Frozen;
use anyhow::{anyhow, bail, ensure};
use aura_config::ComputerControlSettings;
use aura_desktop_domain::*;
use aura_desktop_service::{DesktopControl, FoundApp, KnownElement, KnownShot, Pinned};
use aura_tool_registry::agent_release::AgentBuiltinHandler as H;
use serde_json::Value;

/// Core's refusal of a computer call, said to the model in Core's words.
pub(in crate::agent_execution) fn admission_feedback(error: &anyhow::Error) -> Option<String> {
    let text = error.to_string();
    text.starts_with("computer.").then_some(text)
}

pub(super) const OFF: &str =
    "computer.off: 电脑操控没有打开，这一步没有执行。需要时请用户在 设置 → 电脑操控 里打开它。";
/// How long opening a program waits for its new window.
const LAUNCH_MS: u32 = 15_000;
/// How long a read of a window may take.
const READ_MS: u32 = 10_000;
/// How long the steps of one act may take together.
const ACT_MS: u32 = 20_000;
/// The most steps of one computer.input call, and the longest text one step types.
const MAX_INPUT_STEPS: usize = 50;
const MAX_TEXT_CHARS: usize = 10_000;
/// The longest wait of one step.
const MAX_WAIT_MS: u32 = 10_000;
/// The most elements one screenshot numbers.
const MAX_MARKS: usize = 300;

/// The program a call names, from the catalog the step's listing read.
pub(super) fn program(desktop: &DesktopControl, app: &str) -> anyhow::Result<AppEntry> {
    if let Some(why) = desktop.catalog_issue() {
        bail!("computer.unavailable: {why}");
    }
    match desktop.find_app(app) {
        FoundApp::Found(entry) => Ok(entry),
        FoundApp::NotListed => bail!(
            "computer.not_listed: 读不到电脑上的程序列表，这一步没有执行；请重试，或先调用 computer.apps 看原因。"
        ),
        FoundApp::Unknown => {
            bail!(
                "computer.app: 没有 id 或名字是“{app}”的程序；用 computer.apps 查看程序和它们的 id。"
            )
        }
        FoundApp::Ambiguous(ids) => {
            bail!("computer.app: 有几个程序都叫“{app}”：{}；请用 id。", ids.join("、"))
        }
    }
}

fn known_element(
    desktop: &DesktopControl,
    session: SessionId,
    app: &AppEntry,
    reference: &str,
) -> anyhow::Result<KnownElement> {
    desktop.element(session, &app.id, reference).ok_or_else(|| {
        anyhow!(
            "computer.ref: “{reference}”不是本会话最近读取 {} 时给出的元素；先用 computer.snapshot 读取，再用结果里的 ref，例如 e12。",
            app.name
        )
    })
}

fn shot(
    desktop: &DesktopControl,
    session: SessionId,
    app: &AppEntry,
    reference: &str,
) -> anyhow::Result<KnownShot> {
    desktop.shot(session, reference).filter(|shot| shot.app_id == app.id).ok_or_else(|| {
        anyhow!(
            "computer.screenshot_ref: “{reference}”不是本会话给 {} 截的图；先用 computer.screenshot 截图，再用结果里的 ref，例如 p3。",
            app.name
        )
    })
}

pub(super) fn window(reference: &str) -> anyhow::Result<String> {
    ensure!(
        window_handle(reference.trim()).is_some(),
        "computer.window: “{reference}”不是窗口的 ref；用 computer.open 或 computer.snapshot 结果里的，例如 w1312。"
    );
    Ok(reference.trim().to_owned())
}

pub(super) fn screenshot(
    app: &AppEntry,
    settings: &ComputerControlSettings,
    region: Option<Rect>,
    marks: Vec<Mark>,
) -> HostCommand {
    HostCommand::Screenshot {
        show: vec![app.clone()],
        paint: settings.paint_others,
        max_long_edge: settings.screenshot_long_edge,
        region,
        marks,
    }
}

/// A screenshot's numbered boxes: the elements of the program's latest read that can be acted
/// on, where the read found them.
fn numbered(
    desktop: &DesktopControl,
    session: SessionId,
    app: &AppEntry,
) -> anyhow::Result<Vec<Mark>> {
    let read = desktop.latest_read(session, &app.id);
    ensure!(
        !read.is_empty(),
        "computer.marks: 本会话还没有读取过 {}；先用 computer.snapshot 读取它的窗口，再截图标号。",
        app.name
    );
    let marks: Vec<Mark> = read
        .into_iter()
        .filter(|(_, known)| known.acts && !known.rect.is_empty())
        .take(MAX_MARKS)
        .map(|(number, known)| Mark { number, rect: known.rect })
        .collect();
    ensure!(
        !marks.is_empty(),
        "computer.marks: 最近一次读取 {} 时没有能操作的元素，没有可标号的。",
        app.name
    );
    Ok(marks)
}

/// The screen area a part of an earlier screenshot shows.
pub(super) fn region_on_screen(
    shot: &KnownShot,
    region: &ComputerRegionInput,
) -> anyhow::Result<Rect> {
    let map = shot.map;
    let fits = |start: u32, size: u32, side: u32| {
        size > 0 && start.checked_add(size).is_some_and(|end| end <= side)
    };
    ensure!(
        fits(region.x, region.width, map.width) && fits(region.y, region.height, map.height),
        "computer.region: 这块区域不在截图 {}（{}×{}）里",
        region.screenshot,
        map.width,
        map.height
    );
    let (across, down) = map.scale();
    let edge = |origin: i32, at: u32, scale: f64| f64::from(origin) + f64::from(at) * scale;
    let (left, top) = (edge(map.left, region.x, across), edge(map.top, region.y, down));
    let right = edge(map.left, region.x + region.width, across);
    let bottom = edge(map.top, region.y + region.height, down);
    Ok(Rect {
        x: left.floor() as i32,
        y: top.floor() as i32,
        width: ((right.ceil() - left.floor()) as i32).max(1),
        height: ((bottom.ceil() - top.floor()) as i32).max(1),
    })
}

fn act_step(step: &ComputerActStepV1, key: ElementKey) -> anyhow::Result<ActStep> {
    let element = key;
    Ok(match step {
        ComputerActStepV1::Click { .. } => ActStep::Click { element },
        ComputerActStepV1::SetValue { value, .. } => {
            ensure!(
                value.chars().count() <= MAX_TEXT_CHARS,
                "computer.act: 一次最多填 {MAX_TEXT_CHARS} 个字符"
            );
            ActStep::SetValue { element, value: value.clone() }
        }
        ComputerActStepV1::Check { .. } => ActStep::Check { element },
        ComputerActStepV1::Uncheck { .. } => ActStep::Uncheck { element },
        ComputerActStepV1::Expand { .. } => ActStep::Expand { element },
        ComputerActStepV1::Collapse { .. } => ActStep::Collapse { element },
        ComputerActStepV1::Select { .. } => ActStep::Select { element },
        ComputerActStepV1::Focus { .. } => ActStep::Focus { element },
        ComputerActStepV1::ScrollIntoView { .. } => ActStep::ScrollIntoView { element },
        ComputerActStepV1::Scroll { down, right, .. } => ActStep::Scroll {
            element,
            down: (*down).clamp(-MAX_SCROLL, MAX_SCROLL),
            right: (*right).clamp(-MAX_SCROLL, MAX_SCROLL),
        },
    })
}

/// The middle of an element on the screen.
pub(super) fn middle(rect: Rect) -> Option<Point> {
    (!rect.is_empty()).then(|| Point {
        x: rect.x.saturating_add(rect.width / 2),
        y: rect.y.saturating_add(rect.height / 2),
    })
}

/// Turns the points of an input's steps into screen points.
pub(super) struct Points<'a> {
    pub desktop: &'a DesktopControl,
    pub session: SessionId,
    pub app: &'a AppEntry,
    pub shot: Option<&'a KnownShot>,
    /// The program's window under the first point of the screenshot, where it was then.
    pub window: Option<WindowCheck>,
}

impl Points<'_> {
    fn image(&mut self, x: u32, y: u32) -> anyhow::Result<Point> {
        let shot = self.shot.ok_or_else(|| {
            anyhow!("computer.input: 给出 x、y 时要用 image 指明是哪张截图上的点（例如 p3）")
        })?;
        let (screen_x, screen_y) =
            shot.map.to_screen(f64::from(x), f64::from(y)).ok_or_else(|| {
                anyhow!(
                    "computer.point: ({x}, {y}) 不在这张截图（{}×{}）里",
                    shot.map.width,
                    shot.map.height
                )
            })?;
        if self.window.is_none() {
            let under = shot
                .windows
                .iter()
                .find(|window| !window.minimized && window.rect.contains(screen_x, screen_y))
                .ok_or_else(|| {
                    anyhow!(
                        "computer.point: 截图上的 ({x}, {y}) 不在 {} 的窗口上；只能操作这个程序的窗口",
                        self.app.name
                    )
                })?;
            self.window = Some(WindowCheck { window: under.window.clone(), expected: under.rect });
        }
        Ok(Point { x: screen_x, y: screen_y })
    }

    fn target(
        &mut self,
        x: Option<u32>,
        y: Option<u32>,
        element: Option<&str>,
    ) -> anyhow::Result<Point> {
        match (x, y, element) {
            (Some(x), Some(y), None) => self.image(x, y),
            (None, None, Some(reference)) => {
                let known = known_element(self.desktop, self.session, self.app, reference)?;
                middle(known.rect).ok_or_else(|| {
                    anyhow!("computer.ref: {reference} 没有在屏幕上的位置；请重新读取")
                })
            }
            _ => bail!("computer.input: 每个指针步骤给出 x 与 y（截图上的点），或 ref（元素）"),
        }
    }

    pub(super) fn step(&mut self, step: &ComputerInputStepV1) -> anyhow::Result<InputStep> {
        Ok(match step {
            ComputerInputStepV1::Click { x, y, element, button, count } => InputStep::Click {
                at: self.target(*x, *y, element.as_deref())?,
                button: *button,
                count: count.unwrap_or(1).clamp(1, 3),
            },
            ComputerInputStepV1::Move { x, y, element } => {
                InputStep::Move { at: self.target(*x, *y, element.as_deref())? }
            }
            ComputerInputStepV1::Drag { path, button } => {
                ensure!(
                    (2..=100).contains(&path.len()),
                    "computer.input: 拖动的路径要有 2 到 100 个点"
                );
                let path = path
                    .iter()
                    .map(|point| self.image(point.x, point.y))
                    .collect::<anyhow::Result<Vec<_>>>()?;
                InputStep::Drag { path, button: *button }
            }
            ComputerInputStepV1::Scroll { x, y, element, down, right } => InputStep::Scroll {
                at: self.target(*x, *y, element.as_deref())?,
                down: (*down).clamp(-MAX_SCROLL, MAX_SCROLL),
                right: (*right).clamp(-MAX_SCROLL, MAX_SCROLL),
            },
            ComputerInputStepV1::Type { text } => {
                ensure!(!text.is_empty(), "computer.input: type 要给出文字");
                ensure!(
                    text.chars().count() <= MAX_TEXT_CHARS,
                    "computer.input: 一次最多输入 {MAX_TEXT_CHARS} 个字符"
                );
                InputStep::Type { text: text.clone() }
            }
            ComputerInputStepV1::Press { keys, repeat } => {
                parse_chord(keys).map_err(|why| anyhow!("computer.keys: {why}"))?;
                InputStep::Press { keys: keys.clone(), repeat: repeat.unwrap_or(1).clamp(1, 50) }
            }
            ComputerInputStepV1::Wait { ms } => InputStep::Wait { ms: (*ms).min(MAX_WAIT_MS) },
        })
    }
}

/// The host command a call is, the screenshot it takes afterwards, and the program it is for.
pub(super) fn command(
    desktop: &DesktopControl,
    handler: H,
    session: SessionId,
    input: Value,
    settings: &ComputerControlSettings,
) -> anyhow::Result<(Frozen, AppEntry)> {
    let frozen = |command, after| Frozen {
        mode: Default::default(),
        command,
        after,
        refs: vec![],
        safety: None,
    };
    Ok(match handler {
        H::ComputerOpen => {
            let input: ComputerOpenInput = serde_json::from_value(input)?;
            let app = program(desktop, &input.app)?;
            let command = HostCommand::Launch { app: app.clone(), timeout_ms: LAUNCH_MS };
            (frozen(command, None), app)
        }
        H::ComputerSnapshot => {
            let input: ComputerSnapshotInput = serde_json::from_value(input)?;
            let app = program(desktop, &input.app)?;
            let (window, root) = match input.root.as_deref() {
                Some(reference) => {
                    let known = known_element(desktop, session, &app, reference)?;
                    if let Some(named) = &input.window {
                        ensure!(
                            window(named)? == known.window,
                            "computer.snapshot: {reference} 不在窗口 {named} 里"
                        );
                    }
                    (Some(known.window), Some(known.key))
                }
                None => (input.window.as_deref().map(window).transpose()?, None),
            };
            let command = HostCommand::Elements {
                app: app.clone(),
                window,
                root,
                max_elements: input.max_elements.unwrap_or(400).clamp(1, MAX_ELEMENTS),
                max_depth: MAX_DEPTH,
                timeout_ms: READ_MS,
            };
            (frozen(command, None), app)
        }
        H::ComputerScreenshot => {
            let input: ComputerScreenshotInput = serde_json::from_value(input)?;
            let app = program(desktop, &input.app)?;
            let region = match &input.region {
                Some(region) => Some(region_on_screen(
                    &shot(desktop, session, &app, &region.screenshot)?,
                    region,
                )?),
                None => None,
            };
            let marks = if input.marks { numbered(desktop, session, &app)? } else { Vec::new() };
            (frozen(screenshot(&app, settings, region, marks), None), app)
        }
        H::ComputerAct => {
            let input: ComputerActInput = serde_json::from_value(input)?;
            let app = program(desktop, &input.app)?;
            ensure!(
                (1..=MAX_ACT_STEPS).contains(&input.steps.len()),
                "computer.act: 一次给 1 到 {MAX_ACT_STEPS} 步"
            );
            let mut window = None;
            let mut steps = Vec::with_capacity(input.steps.len());
            for step in &input.steps {
                let known = known_element(desktop, session, &app, step.element())?;
                match &window {
                    None => window = Some(known.window.clone()),
                    Some(first) => ensure!(
                        *first == known.window,
                        "computer.act: 一次的各步要是同一个窗口里的元素；{} 在另一个窗口",
                        step.element()
                    ),
                }
                steps.push(act_step(step, known.key)?);
            }
            let window = window.ok_or_else(|| anyhow!("computer.act: 一次给 1 到 50 步"))?;
            let after = input.screenshot.then(|| screenshot(&app, settings, None, Vec::new()));
            let command = HostCommand::Act { app: app.clone(), window, steps, timeout_ms: ACT_MS };
            let refs = input.steps.iter().map(|step| step.element().trim().to_owned()).collect();
            (Frozen { mode: Default::default(), command, after, refs, safety: None }, app)
        }
        H::ComputerInput => {
            let input: ComputerInputInput = serde_json::from_value(input)?;
            let app = program(desktop, &input.app)?;
            ensure!(
                (1..=MAX_INPUT_STEPS).contains(&input.steps.len()),
                "computer.input: 一次给 1 到 {MAX_INPUT_STEPS} 步"
            );
            let known = input
                .image
                .as_deref()
                .map(|reference| shot(desktop, session, &app, reference))
                .transpose()?;
            let mut points =
                Points { desktop, session, app: &app, shot: known.as_ref(), window: None };
            let steps =
                input.steps.iter().map(|step| points.step(step)).collect::<anyhow::Result<_>>()?;
            let after = input.screenshot.then(|| screenshot(&app, settings, None, Vec::new()));
            let command = HostCommand::Input {
                app: app.clone(),
                steps,
                window: points.window,
                system_keys: settings.system_keys,
                clipboard: settings.clipboard,
            };
            (frozen(command, after), app)
        }
        H::ComputerWindow => {
            let input: ComputerWindowInput = serde_json::from_value(input)?;
            let app = program(desktop, &input.app)?;
            let command = HostCommand::Window {
                app: app.clone(),
                window: input.window.as_deref().map(window).transpose()?,
                action: input.action,
            };
            (frozen(command, None), app)
        }
        _ => bail!("computer.unsupported_tool"),
    })
}

impl AgentExecution {
    pub(in crate::agent_execution) fn resolve_computer(
        &self,
        handler: H,
        tool_call_id: aura_domain_common::ToolCallId,
        action: &mut Value,
        snapshot: &PermissionSnapshot,
        turn: aura_domain_common::TurnTaskId,
    ) -> anyhow::Result<Vec<ResourceDescriptor>> {
        let desktop =
            super::routing::for_turn(self, self.journal.lock()?.database.connection(), turn)?;
        let settings = desktop.settings();
        ensure!(settings.settings.enabled && settings.issue.is_none(), OFF);
        let session = snapshot.session_id;
        let pinned = match desktop.pinned(tool_call_id) {
            Some(pinned) => pinned,
            None => {
                let (mut frozen, app) =
                    command(&desktop, handler, session, action.clone(), &settings.settings)?;
                frozen.mode = desktop.mode();
                super::safety::bind(&desktop, tool_call_id, &mut frozen)?;
                let answered = aura_persistence_v2::desktop_apps::authorized(
                    self.journal.lock()?.database.connection(),
                    session,
                )?;
                let pinned = Pinned {
                    action: serde_json::to_value(frozen)?,
                    authorized: answered.contains(&app.id)
                        || desktop.always_allowed(session, &app.id),
                    app_id: app.id,
                };
                desktop.pin(tool_call_id, pinned.clone());
                pinned
            }
        };
        let reads = matches!(handler, H::ComputerSnapshot | H::ComputerScreenshot);
        ensure!(
            !desktop.turn_stopped(session, turn, &pinned.app_id) || reads,
            "computer.stopped: 用户已停止这个程序的电脑操控，这一回合不能再操作；没有执行"
        );
        ensure!(
            !desktop.taken_over(session, turn) || reads,
            "computer.taken_over: {}",
            aura_desktop_service::taken_over_refusal(desktop.takeover_cause(turn).as_deref())
        );
        *action = pinned.action;
        Ok(vec![ResourceDescriptor::DesktopApp {
            app_id: pinned.app_id,
            authorized: pinned.authorized,
        }])
    }
}
