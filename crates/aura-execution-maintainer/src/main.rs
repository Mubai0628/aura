//! No CLI actions. Only the fixed SCM service invokes the resource worker.
fn main() -> std::io::Result<()> {
    if std::env::args_os().len() != 1 {
        return Err(std::io::Error::other("maintenance.no_command_interface"));
    }
    aura_windows_platform::execution_maintenance::run_maintenance_service(
        aura_execution_maintainer::run,
    )
}
