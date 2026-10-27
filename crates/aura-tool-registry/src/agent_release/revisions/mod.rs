//! Immutable release identities and compatibility checks.
mod integrity;
mod published;
mod published_browser;
mod published_later;
mod published_media;
pub use integrity::current;
pub(super) use integrity::{identity, verify};
pub use published::{PUBLISHED, PublishedRevision};
pub use published_browser::PUBLISHED_BROWSER;
pub use published_later::PUBLISHED_LATER;
pub use published_media::PUBLISHED_MEDIA;
#[cfg(test)]
mod tests;
