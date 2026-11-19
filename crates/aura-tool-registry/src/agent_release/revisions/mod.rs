//! Immutable release identities and compatibility checks.
mod integrity;
mod published;
mod published_browser;
mod published_handover;
mod published_later;
mod published_media;
mod published_shared;
mod published_team;
pub use integrity::current;
pub(super) use integrity::{identity, verify};
pub use published::{PUBLISHED, PublishedRevision};
pub use published_browser::PUBLISHED_BROWSER;
pub use published_handover::PUBLISHED_HANDOVER;
pub use published_later::PUBLISHED_LATER;
pub use published_media::PUBLISHED_MEDIA;
pub use published_shared::PUBLISHED_SHARED;
pub use published_team::PUBLISHED_TEAM;

/// Every published revision in release order, including superseded tool instances.
pub fn all() -> impl Iterator<Item = &'static PublishedRevision> {
    PUBLISHED
        .iter()
        .chain(PUBLISHED_BROWSER)
        .chain(PUBLISHED_MEDIA)
        .chain(PUBLISHED_LATER)
        .chain(PUBLISHED_TEAM)
        .chain(PUBLISHED_HANDOVER)
        .chain(PUBLISHED_SHARED)
        .chain(published_browser::PUBLISHED_BROWSER_RECEIPTS)
}
#[cfg(test)]
mod tests;
