//! A branch carries its source conversation up to one finished turn.
//!
//!The carried turns are not copied table by table. Each is kept as what the model was sent of it
//! and what the conversation showed of it, read once through the same readers, and owned by the
//! branch. The branch's requests begin with that history; its compactions, tool groups and user
//! messages are the branch's own rows, so its own compaction and the model's reads find them. No
//! operation, approval, grant, queued message, terminal or process is carried.
mod capture;
mod copying;
mod display;
mod import;
mod model;
mod record;
mod timing;

pub use copying::{CopyStep, copying_from, fail, finish, progress, step, unfinished};
pub use display::origin;
pub(crate) use display::{
    carried_activity, carried_result, carried_text, carried_turn_ids, carried_view,
};
pub use import::{BranchImport, FirstBatch, SourceSnapshot, begin, import};
pub use model::{ModelPrefix, carried_user_text, history_sequences, model_prefix};
pub(crate) use model::{carried_compaction, carried_roots, carried_rows, carried_tool_result};
pub(crate) use record::CarriedToolResult;
pub use timing::BranchTiming;
