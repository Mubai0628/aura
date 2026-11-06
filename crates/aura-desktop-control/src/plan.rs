//! Input steps read and checked before anything is sent: every chord is parsed, the limits are
//! held, and chords the system answers are refused unless the user allowed them.
use aura_desktop_domain::{Chord, Delivery, HostError, HostErrorCode, InputStep, parse_chord};

pub const MAX_WAIT_MS: u32 = 10_000;
pub const MAX_REPEAT: u8 = 50;

/// What the user's settings let simulated input do beyond the program's own keys and clicks.
#[derive(Clone, Copy, Debug, Default)]
pub struct Allowed {
    pub system_keys: bool,
    pub clipboard: bool,
}

/// A step after its keys were read.
#[derive(Debug, PartialEq)]
pub enum Planned<'a> {
    Step(&'a InputStep),
    Chord(Chord, u8),
}

pub fn invalid(message: String) -> HostError {
    HostError::new(HostErrorCode::InvalidRequest, message, Delivery::NotDelivered)
}

pub fn plan(steps: &[InputStep], allowed: Allowed) -> Result<Vec<Planned<'_>>, HostError> {
    if steps.is_empty() {
        return Err(invalid("没有要做的步骤".into()));
    }
    steps
        .iter()
        .map(|step| match step {
            InputStep::Press { keys, repeat } => {
                let chord = parse_chord(keys).map_err(invalid)?;
                if chord.is_system() && !allowed.system_keys {
                    return Err(HostError::new(
                        HostErrorCode::Refused,
                        format!("{keys} 是系统组合键，设置里没有允许"),
                        Delivery::NotDelivered,
                    ));
                }
                Ok(Planned::Chord(chord, (*repeat).clamp(1, MAX_REPEAT)))
            }
            InputStep::Drag { path, .. } if path.len() < 2 => {
                Err(invalid("拖动至少要两个点".into()))
            }
            InputStep::Type { text } if text.is_empty() => Err(invalid("没有要输入的文字".into())),
            InputStep::Wait { ms } if *ms > MAX_WAIT_MS => {
                Err(invalid(format!("一次最多等 {MAX_WAIT_MS} 毫秒")))
            }
            InputStep::Click { count, .. } if !(1..=3).contains(count) => {
                Err(invalid("点击次数是 1 到 3".into()))
            }
            step => Ok(Planned::Step(step)),
        })
        .collect()
}
