use super::target::{read_error, target_window};
use crate::elements;
use crate::native::uia::Automation;
use aura_desktop_domain::*;
use std::time::Duration;

pub const PASSWORD: &str = "密码要用户自己输入，这一步没有执行";
pub const PAYMENT: &str =
    "付款、转账、交易和购买须由用户自己操作，任何批准都不能放开，这一步没有执行";

pub fn refuse(message: &str, done: u32) -> HostError {
    HostError {
        code: HostErrorCode::Refused,
        message: message.into(),
        delivery: if done == 0 { Delivery::NotDelivered } else { Delivery::Uncertain },
        steps_done: Some(done),
    }
}

pub fn check_focus(automation: &Automation, done: u32) -> Result<(), HostError> {
    let (element, node) = automation
        .focused()
        .map_err(|_| refuse("读不到焦点元素，无法安全输入，这一步没有执行", done))?;
    if node.password {
        return Err(refuse(PASSWORD, done));
    }
    check_payment(automation, &element, &node, done)?;
    Ok(())
}

pub fn check_payment(
    automation: &Automation,
    element: &crate::native::uia::UiElement,
    node: &crate::elements::Node,
    done: u32,
) -> Result<(), HostError> {
    if automation
        .payment(element, node)
        .map_err(|_| refuse("无法检查付款边界，这一步没有执行", done))?
    {
        return Err(refuse(PAYMENT, done));
    }
    Ok(())
}

pub fn check_point(automation: &Automation, point: Point, done: u32) -> Result<(), HostError> {
    let (element, node) = automation
        .at_point(point.x, point.y)
        .map_err(|_| refuse("读不到落点元素，无法安全点击，这一步没有执行", done))?;
    check_payment(automation, &element, &node, done)
}

pub fn inspect(command: &HostCommand) -> Result<HostResult, HostError> {
    super::user_desktop()?;
    let automation = Automation::new(Duration::from_secs(2))
        .map_err(|_| refuse("读不到目标元素，无法检查安全边界，这一步没有执行", 0))?;
    let mut safety = "clear";
    match command {
        HostCommand::Act { app, window, steps, .. } => {
            let target = target_window(app, Some(window))?;
            let (root, _) = automation
                .window(window_handle(&target.window).unwrap_or_default())
                .map_err(|error| read_error(&app.name, &error))?;
            for step in steps {
                let (element, node) = automation
                    .find(&root, &step.element().runtime_id)
                    .map_err(|error| read_error(&app.name, &error))?
                    .filter(|(_, node)| elements::same(step.element(), node))
                    .ok_or_else(|| refuse("元素编号已过期，这一步没有执行", 0))?;
                if node.password && matches!(step, ActStep::SetValue { .. }) {
                    safety = "password";
                }
                if !matches!(step, ActStep::Scroll { .. } | ActStep::ScrollIntoView { .. })
                    && automation
                        .payment(&element, &node)
                        .map_err(|_| refuse("无法检查付款边界，这一步没有执行", 0))?
                {
                    safety = "payment";
                }
            }
        }
        HostCommand::Input { steps, .. } => {
            for step in steps {
                let points: &[Point] = match step {
                    InputStep::Click { at, .. } => std::slice::from_ref(at),
                    InputStep::Drag { path, .. } => path,
                    _ => &[],
                };
                for at in points {
                    let (element, node) = automation
                        .at_point(at.x, at.y)
                        .map_err(|_| refuse("读不到落点元素，这一步没有执行", 0))?;
                    if automation
                        .payment(&element, &node)
                        .map_err(|_| refuse("无法检查付款边界，这一步没有执行", 0))?
                    {
                        safety = "payment";
                    }
                }
            }
            if steps
                .iter()
                .any(|step| matches!(step, InputStep::Type { .. } | InputStep::Press { .. }))
            {
                let (element, node) = automation
                    .focused()
                    .map_err(|_| refuse("读不到焦点元素，这一步没有执行", 0))?;
                if node.password {
                    safety = "password";
                }
                if automation
                    .payment(&element, &node)
                    .map_err(|_| refuse("无法检查付款边界，这一步没有执行", 0))?
                {
                    safety = "payment";
                }
            }
        }
        _ => return Err(refuse("这个命令不能作为输入目标检查", 0)),
    }
    Ok(HostResult::Inspected { safety: safety.into() })
}
