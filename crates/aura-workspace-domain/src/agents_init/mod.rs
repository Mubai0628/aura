//! `/init`: the root AGENTS.md as five fixed chapters, its rendering and the rules
//! that need no file system. Evidence, link targets and secrets are checked by Core.
mod input;
mod markdown;
mod render;
mod rules;
pub use input::*;
pub use render::{
    AGENTS_PATH, CHAPTERS, DOCUMENT_TITLE, GUIDES_DIRECTORY, RenderedFile, guide_path, render,
};
pub use rules::*;
