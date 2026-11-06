//! What a service writes to its log, as it is kept: the secret values it was given taken out,
//! and at most so many lines a minute.
use std::time::{Duration, Instant};

pub const HIDDEN: &str = "••••";
pub const LOG_LINES_PER_MINUTE: u32 = 120;
const WINDOW: Duration = Duration::from_secs(60);
/// A shorter value would hide ordinary words.
const SHORTEST_HIDDEN: usize = 4;

/// Takes the values of a service's secrets out of text.
pub struct Redactor {
    secrets: Vec<String>,
}

impl Redactor {
    #[must_use]
    pub fn new(values: impl IntoIterator<Item = String>) -> Self {
        let mut secrets: Vec<String> =
            values.into_iter().filter(|value| value.chars().count() >= SHORTEST_HIDDEN).collect();
        // A longer value that contains a shorter one is taken out whole.
        secrets.sort_by_key(|value| std::cmp::Reverse(value.len()));
        Self { secrets }
    }

    #[must_use]
    pub fn hide(&self, text: &str) -> String {
        self.secrets
            .iter()
            .fold(text.to_owned(), |text, secret| text.replace(secret.as_str(), HIDDEN))
    }
}

impl Drop for Redactor {
    fn drop(&mut self) {
        for secret in &mut self.secrets {
            zeroize::Zeroize::zeroize(secret);
        }
    }
}

/// Whether a line is kept, and how many were dropped before it.
#[derive(Debug, Eq, PartialEq)]
pub enum Admit {
    Keep,
    /// Kept, after `dropped` lines the limit left out.
    KeepAfter {
        dropped: u32,
    },
    Drop,
}

#[derive(Debug, Default)]
pub struct LogGate {
    since: Option<Instant>,
    kept: u32,
    dropped: u32,
}

impl LogGate {
    pub fn admit(&mut self, now: Instant) -> Admit {
        let fresh = self.since.is_none_or(|since| now.duration_since(since) >= WINDOW);
        if fresh {
            self.since = Some(now);
            self.kept = 1;
            let dropped = std::mem::take(&mut self.dropped);
            return if dropped == 0 { Admit::Keep } else { Admit::KeepAfter { dropped } };
        }
        if self.kept < LOG_LINES_PER_MINUTE {
            self.kept += 1;
            return Admit::Keep;
        }
        self.dropped = self.dropped.saturating_add(1);
        Admit::Drop
    }
}
