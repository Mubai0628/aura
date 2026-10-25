//! Dedicated network-tool contracts. URLs and headers are data, never caller authority.
mod input;
mod output;
pub use input::*;
pub use output::*;

pub const HTTP_BODY_LIMIT: usize = 1024 * 1024;
pub const HTTP_RESPONSE_LIMIT: u64 = 8 * 1024 * 1024;
pub const HTTP_INLINE_LIMIT: usize = 256 * 1024;
pub const HTTP_DOWNLOAD_LIMIT: u64 = 256 * 1024 * 1024;
