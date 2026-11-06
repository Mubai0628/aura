//! What a read or an act may ask, checked before anything reaches the program.
use aura_desktop_domain::{ActStep, MAX_ACT_STEPS, MAX_DEPTH, MAX_ELEMENTS, MAX_SCROLL};

/// The longest text one step may put in an element.
pub const MAX_SET_VALUE_CHARS: usize = 100_000;
pub const MIN_TIMEOUT_MS: u32 = 500;
pub const MAX_TIMEOUT_MS: u32 = 60_000;

pub fn check_read(max_elements: u32, max_depth: u32, timeout_ms: u32) -> Result<(), String> {
    if !(1..=MAX_ELEMENTS).contains(&max_elements) {
        return Err(format!("一次读 1 到 {MAX_ELEMENTS} 个元素"));
    }
    if !(1..=MAX_DEPTH).contains(&max_depth) {
        return Err(format!("层数要在 1 到 {MAX_DEPTH} 之间"));
    }
    check_timeout(timeout_ms)
}

pub fn check_steps(steps: &[ActStep], timeout_ms: u32) -> Result<(), String> {
    if steps.is_empty() {
        return Err("没有要做的步骤".into());
    }
    if steps.len() > MAX_ACT_STEPS {
        return Err(format!("一次最多 {MAX_ACT_STEPS} 步"));
    }
    let most = MAX_SCROLL.unsigned_abs();
    for (index, step) in steps.iter().enumerate() {
        let number = index + 1;
        if step.element().runtime_id.is_empty() {
            return Err(format!("第 {number} 步没有说是哪个元素"));
        }
        match step {
            ActStep::Scroll { down: 0, right: 0, .. } => {
                return Err(format!("第 {number} 步没有说往哪边滚"));
            }
            ActStep::Scroll { down, right, .. }
                if down.unsigned_abs() > most || right.unsigned_abs() > most =>
            {
                return Err(format!("第 {number} 步一次最多滚 {MAX_SCROLL} 格"));
            }
            ActStep::SetValue { value, .. } if value.chars().count() > MAX_SET_VALUE_CHARS => {
                return Err(format!("第 {number} 步的文字超过 {MAX_SET_VALUE_CHARS} 个字符"));
            }
            _ => {}
        }
    }
    check_timeout(timeout_ms)
}

fn check_timeout(timeout_ms: u32) -> Result<(), String> {
    if (MIN_TIMEOUT_MS..=MAX_TIMEOUT_MS).contains(&timeout_ms) {
        Ok(())
    } else {
        Err(format!("时限要在 {MIN_TIMEOUT_MS} 到 {MAX_TIMEOUT_MS} 毫秒之间"))
    }
}
