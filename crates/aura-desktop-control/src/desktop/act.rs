//! Acting on a program's elements through UI Automation, in the background. Each step finds its
//! element again and checks it is still the one that was read; a step that fails says how many
//! were done and whether it may have reached the program.
use super::target::{STALE, not_responding, read_error, target_window, window_error};
use super::worker::{Ran, on_own_thread};
use crate::elements::{self, Expand, Node, UiError};
use crate::native::uia::{Automation, UiElement};
use crate::plan::invalid;
use aura_desktop_domain::{
    ActStep, AppEntry, Checked, Delivery, HostError, HostErrorCode, HostResult, window_handle,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

/// Lets the program show the step's result before the element is read again.
const SETTLE: Duration = Duration::from_millis(80);

/// How far an act got, for the answer when its thread does not finish in time.
#[derive(Default)]
struct Progress {
    begun: AtomicU32,
    finished: AtomicU32,
}

pub fn act(
    app: &AppEntry,
    window: &str,
    steps: Vec<ActStep>,
    timeout_ms: u32,
) -> Result<HostResult, HostError> {
    elements::check_steps(&steps, timeout_ms).map_err(invalid)?;
    let target = target_window(app, Some(window))?;
    let handle = window_handle(&target.window).unwrap_or_default();
    let timeout = Duration::from_millis(u64::from(timeout_ms));
    let progress = Arc::new(Progress::default());
    let guard = crate::native::guard::current();
    let ran = on_own_thread(timeout, {
        let (name, progress) = (app.name.clone(), Arc::clone(&progress));
        move || run(&name, handle, &steps, timeout, &progress, guard.as_deref())
    });
    match ran {
        Ran::Done(result) => result,
        Ran::TimedOut => Err(not_responding(
            &app.name,
            Some(timeout_ms),
            if progress.begun.load(Ordering::SeqCst) == 0 {
                Delivery::NotDelivered
            } else {
                Delivery::Uncertain
            },
            Some(progress.finished.load(Ordering::SeqCst)),
        )),
    }
}

fn run(
    name: &str,
    window: isize,
    steps: &[ActStep],
    timeout: Duration,
    progress: &Progress,
    guard: Option<&crate::native::guard::Control>,
) -> Result<HostResult, HostError> {
    let automation = Automation::new(timeout / 2).map_err(|error| read_error(name, &error))?;
    let (root, _) = automation.window(window).map_err(|error| window_error(name, &error))?;
    let mut after = Vec::with_capacity(steps.len());
    for (done, step) in (0_u32..).zip(steps) {
        if let Some(guard) = guard {
            guard.checkpoint(done)?;
        }
        super::user_desktop()?;
        let number = done + 1;
        let fail = |code: HostErrorCode, message: String, reached: bool| HostError {
            code,
            message,
            delivery: if done == 0 && !reached {
                Delivery::NotDelivered
            } else {
                Delivery::Uncertain
            },
            steps_done: Some(done),
        };
        let key = step.element();
        let (element, node) = match automation.find(&root, &key.runtime_id) {
            Ok(Some((element, node))) if elements::same(key, &node) => (element, node),
            Ok(_) | Err(UiError::Gone) => {
                return Err(fail(HostErrorCode::Stale, format!("第 {number} 步：{STALE}"), false));
            }
            Err(UiError::Timeout) => {
                return Err(fail(HostErrorCode::Timeout, format!("{name} 没有响应"), false));
            }
            Err(error) => return Err(fail(HostErrorCode::Internal, error.to_string(), false)),
        };
        if !node.enabled {
            return Err(fail(
                HostErrorCode::Unsupported,
                format!("第 {number} 步的元素现在不可用（灰着）"),
                false,
            ));
        }
        if node.password && matches!(step, ActStep::SetValue { .. }) {
            return Err(super::safety::refuse(super::safety::PASSWORD, done));
        }
        if !matches!(step, ActStep::Scroll { .. } | ActStep::ScrollIntoView { .. }) {
            super::safety::check_payment(&automation, &element, &node, done)?;
        }
        if let Some(guard) = guard {
            guard.checkpoint(done)?;
        }
        progress.begun.store(number, Ordering::SeqCst);
        perform(&automation, &element, &node, step).map_err(|error| match error {
            UiError::NotSupported => fail(
                HostErrorCode::Unsupported,
                format!("第 {number} 步：这个元素不能{}，改用截图和模拟输入", verb(step)),
                false,
            ),
            UiError::NotEnabled => {
                fail(HostErrorCode::Unsupported, format!("第 {number} 步的元素现在不可用"), false)
            }
            UiError::Gone => fail(
                HostErrorCode::Stale,
                format!("第 {number} 步的元素在操作时不见了，这一步可能已经做了：请重新读取"),
                true,
            ),
            UiError::Timeout => fail(
                HostErrorCode::Timeout,
                format!("{name} 没有响应，第 {number} 步可能已经做了：请重新读取"),
                true,
            ),
            UiError::Failed(message) if message.starts_with("密码要用户自己输入") => {
                fail(HostErrorCode::Refused, message, false)
            }
            UiError::Failed(message) => fail(HostErrorCode::Internal, message, true),
        })?;
        progress.finished.store(number, Ordering::SeqCst);
        if let Some(guard) = guard {
            guard.completed(number);
        }
        if let Some(guard) = guard {
            guard.checkpoint(number)?;
        }
        std::thread::sleep(SETTLE);
        after.push(automation.refresh(&element).ok().map(|node| elements::element(&node, 0)));
    }
    Ok(HostResult::Acted { after })
}

fn perform(
    automation: &Automation,
    element: &UiElement,
    node: &Node,
    step: &ActStep,
) -> Result<(), UiError> {
    let patterns = node.patterns;
    match step {
        ActStep::Click { .. } if patterns.invoke => automation.invoke(element),
        ActStep::Click { .. } if patterns.toggle => automation.toggle(element),
        ActStep::Click { .. } if patterns.selection_item => automation.select(element),
        ActStep::Click { .. } if patterns.expand_collapse => match node.expand {
            Some(Expand::Collapsed) => automation.expand(element),
            _ => automation.collapse(element),
        },
        ActStep::Click { .. } => Err(UiError::NotSupported),
        ActStep::SetValue { value, .. } => automation.set_value(element, value),
        ActStep::Check { .. } => toggle_to(automation, element, node, Checked::On),
        ActStep::Uncheck { .. } => toggle_to(automation, element, node, Checked::Off),
        ActStep::Expand { .. } => automation.expand(element),
        ActStep::Collapse { .. } => automation.collapse(element),
        ActStep::Select { .. } => automation.select(element),
        ActStep::Focus { .. } => automation.focus(element),
        ActStep::ScrollIntoView { .. } => automation.scroll_into_view(element),
        ActStep::Scroll { down, right, .. } => automation.scroll(element, *down, *right),
    }
}

/// Toggles until the element is in the state asked for; a check box with a third state needs
/// up to two toggles.
fn toggle_to(
    automation: &Automation,
    element: &UiElement,
    node: &Node,
    wanted: Checked,
) -> Result<(), UiError> {
    if !node.patterns.toggle {
        return Err(UiError::NotSupported);
    }
    let mut state = node.toggle;
    for _ in 0..3 {
        if state == Some(wanted) {
            return Ok(());
        }
        automation.toggle(element)?;
        state = automation.checked(element)?;
    }
    if state == Some(wanted) {
        Ok(())
    } else {
        Err(UiError::Failed("切了三次也没到要的勾选状态".into()))
    }
}

fn verb(step: &ActStep) -> &'static str {
    match step {
        ActStep::Click { .. } => "点",
        ActStep::SetValue { .. } => "填值",
        ActStep::Check { .. } => "勾选",
        ActStep::Uncheck { .. } => "取消勾选",
        ActStep::Expand { .. } => "展开",
        ActStep::Collapse { .. } => "收起",
        ActStep::Select { .. } => "选中",
        ActStep::Focus { .. } => "聚焦",
        ActStep::ScrollIntoView { .. } => "滚到看得见",
        ActStep::Scroll { .. } => "滚动",
    }
}
