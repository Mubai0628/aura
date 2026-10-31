//! The security authority in Core: permission modes and approvals, file, network, session and
//! memory consents, external grants, host preparation with the elevated helper, the workspace
//! policy, and the P04 publications with their activity records.
#![forbid(unsafe_code)]

mod agent_activity;
pub mod external_grants;
pub mod host_preparation;
pub mod native_session_grants;
pub mod p04;
pub mod p04_broker;
pub mod workspace_policy;

// p04.rs names the settlement codes `crate::command_settlement` and external_grants.rs the journal
// `crate::broker::BrokerJournal`: both files are at their size ceilings, and the direct paths would
// lengthen them.
use aura_core_base::command_settlement;
mod broker {
    pub(crate) use aura_core_broker::broker::BrokerJournal;
}
use p04::P04Services;
