use crate::*;

/// Main is quitting. The execution store records the normal exit first, since that decides how
/// the next start checks it; then `aura.db` writes its log back and marks a run owed before the
/// next start when one is. Whether the normal exit was recorded.
pub(crate) fn prepare_exit(services: &RuntimeServices) -> bool {
    let marked = services.broker.prepare_exit().unwrap_or(false);
    let exit = match services.persistence.prepare_exit() {
        Ok(exit) => json!(exit),
        Err(error) => json!({ "error": error.to_string() }),
    };
    append_core_log("aura_db_exit", exit);
    marked
}
