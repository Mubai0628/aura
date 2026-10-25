use std::fmt;

/// Severity ordering is shared by parsing and preview filters.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Debug,
    Info,
    Warn,
    Error,
}

impl Level {
    pub fn parse(word: &str) -> Option<Self> {
        match word {
            "DEBUG" => Some(Self::Debug),
            "INFO" => Some(Self::Info),
            "WARN" => Some(Self::Warn),
            "ERROR" => Some(Self::Error),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Debug => "DEBUG",
            Self::Info => "INFO",
            Self::Warn => "WARN",
            Self::Error => "ERROR",
        }
    }
}

impl fmt::Display for Level {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.label())
    }
}

/// `truncated` describes the message, never the number of omitted entries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub timestamp: u64,
    pub level: Level,
    pub text: String,
    pub truncated: bool,
}

impl Entry {
    pub fn empty(timestamp: u64, level: Level) -> Self {
        Self { timestamp, level, text: String::new(), truncated: false }
    }

    pub fn display(&self) -> String {
        let suffix = if self.truncated { " [truncated]" } else { "" };
        format!("{} {} {}{suffix}", self.timestamp, self.level, self.text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_round_trip() {
        for level in [Level::Debug, Level::Info, Level::Warn, Level::Error] {
            assert_eq!(Level::parse(level.label()), Some(level));
        }
        assert_eq!(Level::parse("TRACE"), None);
    }

    #[test]
    fn display_marks_truncation() {
        let mut entry = Entry::empty(7, Level::Error);
        entry.text = "failed".into();
        entry.truncated = true;
        assert_eq!(entry.display(), "7 ERROR failed [truncated]");
    }
}
