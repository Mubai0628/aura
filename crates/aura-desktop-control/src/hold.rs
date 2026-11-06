//! How long the host keeps the desktop, independent of Windows: for a turn, from its first
//! action to its end, between its commands too and while its model thinks (Core keeps it), so
//! the banner, the pointer's ring and the watch on the user's input stay; for a command of no
//! hold, while it runs. One is kept at a time.
use aura_desktop_domain::{HOLD_IDLE_MS, HoldEnd};
use std::collections::VecDeque;

/// The most turns whose hold was taken over that are refused the desktop afterwards.
pub const TAKEN_REMEMBERED: usize = 32;

/// What a command that begins finds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Begin {
    /// Nothing was kept: its hold starts now.
    Started,
    /// Its hold was kept between its commands.
    Continued,
    /// Another hold keeps the desktop.
    Busy,
    /// The user took its hold over.
    TakenOver,
}

/// A hold that ended by itself, for the host to say.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Ended {
    /// `None`: a command's own hold, which nothing waits to hear of.
    pub key: Option<String>,
    pub reason: HoldEnd,
}

struct Kept {
    key: Option<String>,
    running: bool,
    /// When its last command ended.
    since: u64,
}

pub struct Holds {
    kept: Option<Kept>,
    taken: VecDeque<String>,
}

impl Default for Holds {
    fn default() -> Self {
        Self::new()
    }
}

impl Holds {
    #[must_use]
    pub const fn new() -> Self {
        Self { kept: None, taken: VecDeque::new() }
    }

    /// A command of `key` (`None`: of no hold) begins at `now`. `taken_over`: the user took the
    /// hold kept now over, as its watch saw it. A kept hold the user took over, or that waited
    /// [`HOLD_IDLE_MS`], ends first.
    pub fn begin(
        &mut self,
        key: Option<&str>,
        taken_over: bool,
        now: u64,
    ) -> (Begin, Option<Ended>) {
        let mut ended = None;
        if let Some(kept) = &self.kept
            && !kept.running
        {
            let same = key.is_some() && kept.key.as_deref() == key;
            if taken_over {
                ended = self.end(HoldEnd::TakenOver);
                if same {
                    return (Begin::TakenOver, ended);
                }
            } else if !same && now.saturating_sub(kept.since) >= HOLD_IDLE_MS {
                ended = self.end(HoldEnd::Idle);
            }
        }
        let begin = match &mut self.kept {
            Some(kept) if !kept.running && key.is_some() && kept.key.as_deref() == key => {
                kept.running = true;
                Begin::Continued
            }
            Some(_) => Begin::Busy,
            None if key.is_some_and(|key| self.taken.iter().any(|taken| taken == key)) => {
                Begin::TakenOver
            }
            None => {
                self.kept = Some(Kept { key: key.map(str::to_owned), running: true, since: now });
                Begin::Started
            }
        };
        (begin, ended)
    }

    /// The running command ended at `now`. Returns whether the desktop is given back: a
    /// command's own hold ends with it.
    pub fn finish(&mut self, now: u64) -> bool {
        match &mut self.kept {
            Some(kept) if kept.key.is_none() => {
                self.kept = None;
                true
            }
            Some(kept) => {
                kept.running = false;
                kept.since = now;
                false
            }
            None => false,
        }
    }

    /// Core says the turn of hold `key` goes on at `now`: a kept hold of it waits
    /// [`HOLD_IDLE_MS`] from then. Returns whether it is kept.
    pub fn keep(&mut self, key: &str, now: u64) -> bool {
        match &mut self.kept {
            Some(kept) if kept.key.as_deref() == Some(key) => {
                if !kept.running {
                    kept.since = now;
                }
                true
            }
            _ => false,
        }
    }

    /// Core gives hold `key` back. Returns whether it was kept.
    pub fn release(&mut self, key: &str) -> bool {
        if self.kept.as_ref().is_some_and(|kept| !kept.running && kept.key.as_deref() == Some(key))
        {
            self.kept = None;
            return true;
        }
        false
    }

    /// Looked at every few milliseconds: a kept hold between commands ends when the user took
    /// it over or it waited [`HOLD_IDLE_MS`] since its last command or keep. A running command is
    /// stopped by its own checks.
    pub fn tick(&mut self, taken_over: bool, now: u64) -> Option<Ended> {
        let kept = self.kept.as_ref().filter(|kept| !kept.running)?;
        if taken_over {
            self.end(HoldEnd::TakenOver)
        } else if now.saturating_sub(kept.since) >= HOLD_IDLE_MS {
            self.end(HoldEnd::Idle)
        } else {
            None
        }
    }

    fn end(&mut self, reason: HoldEnd) -> Option<Ended> {
        let kept = self.kept.take()?;
        if reason == HoldEnd::TakenOver
            && let Some(key) = &kept.key
        {
            if self.taken.len() == TAKEN_REMEMBERED {
                self.taken.pop_front();
            }
            self.taken.push_back(key.clone());
        }
        Some(Ended { key: kept.key, reason })
    }
}
