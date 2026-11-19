//! Timing of an interaction displayed in one or more desktop windows.
use crate::UtcTimestamp;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, time::Duration};

pub const IDLE_SECONDS: u64 = 30;
pub const COUNTDOWN_SECONDS: u64 = 30;
const FOCUS_LEASE: Duration = Duration::from_secs(12);
const MAX_FOCUSED_WINDOWS: usize = 32;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[schemars(rename = "AgentWaitActivity")]
#[serde(rename_all = "snake_case")]
pub enum Activity {
    Present,
    Touch,
    Focus,
    Blur,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct HumanWait {
    pub deadline: UtcTimestamp,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    focus_leases: BTreeMap<String, UtcTimestamp>,
}

#[must_use]
pub fn idle_deadline(from: UtcTimestamp) -> UtcTimestamp {
    UtcTimestamp::from_value(from.value() + Duration::from_secs(IDLE_SECONDS + COUNTDOWN_SECONDS))
}

#[must_use]
pub fn valid_viewer(viewer: &str) -> bool {
    (1..=64).contains(&viewer.len())
        && viewer.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
}

impl HumanWait {
    #[must_use]
    pub fn focused_until(&self, at: UtcTimestamp) -> Option<UtcTimestamp> {
        self.focus_leases.values().copied().filter(|until| *until > at).max()
    }

    #[must_use]
    pub fn advance(
        previous: Option<&Self>,
        viewer: &str,
        activity: Activity,
        at: UtcTimestamp,
    ) -> Option<Self> {
        if activity == Activity::Blur && previous.is_none() {
            return None;
        }
        let mut next = previous
            .cloned()
            .unwrap_or_else(|| Self { deadline: idle_deadline(at), focus_leases: BTreeMap::new() });
        next.focus_leases.retain(|_, until| *until > at);
        let reset = match activity {
            Activity::Focus
                if next.focus_leases.contains_key(viewer)
                    || next.focus_leases.len() < MAX_FOCUSED_WINDOWS =>
            {
                next.focus_leases
                    .insert(viewer.to_owned(), UtcTimestamp::from_value(at.value() + FOCUS_LEASE));
                true
            }
            Activity::Blur => next.focus_leases.remove(viewer).is_some(),
            Activity::Touch => next.focus_leases.is_empty(),
            _ => false,
        };
        if reset {
            next.deadline = idle_deadline(next.focused_until(at).unwrap_or(at));
        }
        Some(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_blur_releases_only_its_own_window() {
        let now = UtcTimestamp::now();
        let a = HumanWait::advance(None, "a", Activity::Focus, now).unwrap();
        let later = UtcTimestamp::from_value(now.value() + Duration::from_secs(4));
        let both = HumanWait::advance(Some(&a), "b", Activity::Focus, later).unwrap();
        let b = HumanWait::advance(Some(&both), "a", Activity::Blur, later).unwrap();
        assert_eq!(b.focused_until(later), both.focused_until(later));
        assert_eq!(b.deadline, both.deadline);
        let idle = HumanWait::advance(Some(&b), "b", Activity::Blur, later).unwrap();
        assert_eq!(idle.focused_until(later), None);
        assert_eq!(idle.deadline, idle_deadline(later));
    }

    #[test]
    fn a_crashed_window_releases_its_lease_without_gaining_time_on_reopen() {
        let now = UtcTimestamp::now();
        let wait = HumanWait::advance(None, "a", Activity::Focus, now).unwrap();
        let recovered: HumanWait =
            serde_json::from_str(&serde_json::to_string(&wait).unwrap()).unwrap();
        let reopened = UtcTimestamp::from_value(now.value() + Duration::from_secs(13));
        let seen = HumanWait::advance(Some(&recovered), "b", Activity::Present, reopened).unwrap();
        assert_eq!(seen.focused_until(reopened), None);
        assert_eq!((seen.deadline.value() - reopened.value()).whole_seconds(), 59);
        assert!(HumanWait::advance(None, "b", Activity::Blur, reopened).is_none());
    }
}
