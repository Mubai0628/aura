//! Typed identities order overlapping work without granting any authority.
use super::{FileTargetLaneKey, Lane, publication_scope, terminal_input};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LaneKey {
    Connection(Lane),
    Session(SessionLaneKey),
    Terminal(terminal_input::TerminalLaneKey),
    FileTarget(FileTargetLaneKey),
    Publication(publication_scope::PublicationScope),
}

impl LaneKey {
    pub fn lane(&self) -> Lane {
        match self {
            Self::Connection(lane) => *lane,
            _ => Lane::Session,
        }
    }

    pub(super) fn holds(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Connection(mine), Self::Connection(theirs)) => mine == theirs,
            (Self::Session(mine), Self::Session(theirs)) => mine == theirs,
            (Self::Terminal(mine), Self::Terminal(theirs)) => mine == theirs,
            // Resource ordering continues to depend on actual overlap, not
            // merely unequal DTO spellings of two file operations.
            (Self::FileTarget(mine), Self::FileTarget(theirs)) => {
                mine.inner.overlaps(&theirs.inner)
            }
            (Self::Publication(mine), Self::Publication(theirs)) => mine.overlaps(theirs),
            (Self::Publication(mine), Self::FileTarget(theirs))
            | (Self::FileTarget(theirs), Self::Publication(mine)) => mine.overlaps_file(theirs),
            _ => false,
        }
    }
}

/// Session strings are ordering identities only. Domain handlers still
/// validate whether the authenticated caller may use that session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionLaneKey {
    session_id: String,
}
impl SessionLaneKey {
    pub(super) fn of(session_id: &str) -> Self {
        Self { session_id: session_id.to_owned() }
    }
}
