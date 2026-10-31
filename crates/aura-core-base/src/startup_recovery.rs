//! The marker that names a process's one-time startup work.
/// The one-time work a process does at startup, named so it cannot be repeated by accident.
///
/// An empty marker type on purpose: it carries no data, so it cannot be used to smuggle
/// startup state into a live rebuild. Its sole job is to make the distinction explicit at
/// the call site — a function that takes `StartupRecovery` is one that may only run while
/// the process is starting.
#[derive(Clone, Copy, Debug)]
pub struct StartupRecovery(());

impl StartupRecovery {
    /// The single permission to run startup-only work.
    ///
    /// Called from the startup path and nowhere else. It is not a capability check; it is a
    /// statement in the type system that the caller is the startup path.
    pub const fn for_process_startup() -> Self {
        Self(())
    }
}
