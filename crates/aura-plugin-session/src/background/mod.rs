//! A plugin's background service in Core: which services should run, the host that keeps them
//! running, what each one's requests do, the commands its chats send, the menus its channels
//! show, the replies owed to its channels' chats, and the approvals their turns wait for.
mod approvals;
mod bridge;
mod command_lists;
mod commands;
mod driver;
mod menus;
mod messages;
mod pump;
mod sessions;
mod wanted;

pub use bridge::{SERVICE_NOTICE_WINDOW, SERVICE_NOTICES, ServiceBridge, UI_NOTIFICATION};
pub use driver::PluginServices;
pub use messages::NO_SCOPE_REPLY;
pub use sessions::{AgentAccess, LISTED_SESSIONS};
pub use wanted::service_identity;
