//! What a sub-agent's commands run under: one group Job per sub-agent with its own CPU and
//! memory caps, every sub-agent's processes together under the CPU share the user set, and a
//! temporary folder of its own. The user's own conversations keep running their commands
//! without a group.
use aura_backend_windows_user::compat::ProcessPolicy;
use aura_domain_common::SessionId;
use aura_windows_platform::GroupLimits;
use std::path::PathBuf;

/// One sub-agent's share of all processors' time, in hundredths of a percent.
const SUB_AGENT_CPU_RATE: u32 = 5_000;
/// A quarter of the machine's memory, and never less than this.
const SUB_AGENT_MEMORY_FLOOR: u64 = 2 * 1024 * 1024 * 1024;

/// The policy of `session`'s commands, every sub-agent's together held to `all_cpu_percent` of
/// all processors.
pub fn sub_agent(session: SessionId, all_cpu_percent: u32) -> ProcessPolicy {
    let memory = aura_windows_platform::physical_memory()
        .map_or(SUB_AGENT_MEMORY_FLOOR, |memory| (memory.total / 4).max(SUB_AGENT_MEMORY_FLOOR));
    ProcessPolicy {
        group: session.to_string(),
        limits: GroupLimits { cpu_rate: SUB_AGENT_CPU_RATE, memory_bytes: Some(memory) },
        all_cpu_rate: all_cpu_percent.clamp(1, 100) * 100,
        temp_directory: temp_directory(session),
    }
}

/// The sub-agent's TEMP and TMP.
pub fn temp_directory(session: SessionId) -> PathBuf {
    std::env::temp_dir().join("aura-agents").join(session.to_string())
}

/// Removes the sub-agent's temporary folder once nothing of it runs any more. A folder still
/// held by a process is left for the next time.
pub fn remove_temp_directory(session: SessionId) {
    let folder = temp_directory(session);
    if folder.exists() {
        let _ = std::fs::remove_dir_all(folder);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_sub_agents_share_is_the_users_in_hundredths_of_a_percent() {
        let session = SessionId::new();
        assert_eq!(sub_agent(session, 40).all_cpu_rate, 4_000);
        assert_eq!(sub_agent(session, 100).all_cpu_rate, 10_000);
        assert_eq!(sub_agent(session, 0).all_cpu_rate, 100, "never an empty share");
        assert_eq!(sub_agent(session, 40).limits.cpu_rate, SUB_AGENT_CPU_RATE);
    }
}
