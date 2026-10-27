//! The relay Chrome starts for Aura's extension: Chrome's native-messaging frames on stdio in, the
//! same frames on Core's named pipe out. It checks that the pipe's server is the Core installed
//! beside it, tells Core which extension Chrome named, and otherwise passes frames unread; it keeps
//! and writes nothing of their content.
pub mod frame;
pub mod relay;
