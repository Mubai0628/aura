//! What an installer does to the user's Windows session: its entry in Settings > Apps, shortcuts,
//! the Credential Manager entries an installation owns, the programs running from a folder, and
//! its windows. All of it is per user and needs no administrator rights.
mod credentials;
mod folder_picker;
mod native_messaging;
mod notification_identity;
mod processes;
mod shortcut;
mod task_dialog;
mod uninstall_entry;

pub use credentials::{delete_installation_credentials, installation_credentials};
pub use folder_picker::choose_folder;
pub use native_messaging::{
    read_chrome_native_host, remove_chrome_native_host, write_chrome_native_host,
};
pub use notification_identity::{notification_identity, remove_notification_identity};
pub use processes::{RunningProgram, programs_running_in, stop_program};
pub use shortcut::{Shortcut, create_shortcut, desktop_folder, start_menu_programs};
pub use task_dialog::{Answer, LinkHandler, Progress, Question, ask, run_with_progress};
pub use uninstall_entry::{
    UninstallEntry, read_uninstall_location, remove_uninstall_entry, write_uninstall_entry,
};

pub(crate) fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
