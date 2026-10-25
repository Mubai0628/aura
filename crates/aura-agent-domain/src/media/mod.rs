//! Bounded read-only video metadata and frames, decoded by Main from Core-held bytes.
mod host;
mod tools;
pub use host::*;
pub use tools::*;

pub const MAX_MEDIA_FRAMES: usize = 12;
pub const MAX_MEDIA_IMAGE_BYTES: usize = 256 * 1024;
pub const MAX_MEDIA_IMAGES_BYTES: usize = 3 * 1024 * 1024;
pub const MAX_MEDIA_CONTROL_BYTES: usize = 6 * 1024 * 1024;
pub const MAX_MEDIA_READ_BYTES: u32 = 128 * 1024;
pub const MEDIA_TIMEOUT_MS: u32 = 30_000;
