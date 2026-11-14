//! Where an installation's data is. This crate is the one place that turns a namespace, the user's
//! folders and the location record into paths; every process is handed the result and never
//! builds an Aura data path of its own.
//!
//! Without a record the paths are the ones Aura has always used. A record that cannot be read,
//! that the installation folder's copy contradicts, or that names a folder which is not there,
//! does not hold this installation's marker or is not on a fixed local disk, means the data is
//! not found: working the locations out only reads, so nothing empty is ever created in its place.
mod command;
mod handoff;
mod process_log;
mod record;
mod resolve;
mod roots;

pub use command::{COMMAND, command};
pub use handoff::{
    ConfirmError, DATA_ROOT, DESKTOP_CHROME_ROOT, DESKTOP_DATA_ROOT, DESKTOP_UI_PROFILE, confirm,
};
pub use process_log::{core_log_path, route_core_log};
pub use record::{
    DataMarker, LocationRecord, MARKER_FILE, RECORD_FILE, current_record, store_record,
    write_marker, write_record,
};
pub use resolve::{
    InstallCopy, LocationError, Locations, installation, point_at, same_place, verify_location,
};
/// The window profile's folder name inside a data folder of its own.
pub const UI_PROFILE_FOLDER: &str = resolve::UI_PROFILE;
pub use roots::Roots;
