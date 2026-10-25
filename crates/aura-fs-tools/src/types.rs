use std::time::Duration;

use aura_workspace_domain::LargeFileReadMode;

#[derive(Clone, Debug)]
pub struct FsToolPolicy {
    pub large_file_read_mode: LargeFileReadMode,
    pub full_text_read_limit_bytes: u64,
    pub small_range_line_limit: u64,
    pub small_range_character_limit: u64,
    pub default_binary_range_bytes: u32,
    pub max_binary_range_bytes: u32,
    pub max_changeset_operations: usize,
    pub max_search_results: usize,
    /// How long one file mutation waits for a live holder before it defers.
    ///
    /// This is the caller's bounded wait budget, not a conflict decision: a resource held by another
    /// execution is *waited for* inside this budget and re-judged from scratch on every attempt, and
    /// only when the budget expires is the caller told to re-read. The confirmed behaviour is a
    /// bounded, cancellable wait, so a synchronous refusal is not a substitute for this value.
    pub mutation_wait: Duration,
}

impl Default for FsToolPolicy {
    fn default() -> Self {
        Self {
            large_file_read_mode: LargeFileReadMode::SmallRangeOnly,
            full_text_read_limit_bytes: 2 * 1024 * 1024,
            small_range_line_limit: 500,
            small_range_character_limit: 65_536,
            default_binary_range_bytes: 4 * 1024,
            max_binary_range_bytes: 64 * 1024,
            max_changeset_operations: 64,
            max_search_results: 100,
            // Comfortably inside any realistic tool-call budget, and long enough that an ordinary
            // editor save or a second tool's short commit is waited for rather than reported as a
            // conflict. A caller that wants a different budget states it; nothing here extends an
            // execution's own deadline.
            mutation_wait: Duration::from_secs(5),
        }
    }
}

impl FsToolPolicy {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.full_text_read_limit_bytes == 0 {
            return Err("full_text_read_limit_bytes must be positive");
        }
        if self.small_range_character_limit == 0 {
            return Err("small_range_character_limit must be positive");
        }
        if self.small_range_line_limit == 0 {
            return Err("small_range_line_limit must be positive");
        }
        if self.default_binary_range_bytes == 0
            || self.default_binary_range_bytes > self.max_binary_range_bytes
        {
            return Err("default binary range must be positive and not exceed the maximum");
        }
        if self.max_binary_range_bytes > 64 * 1024 {
            return Err("max binary range must not exceed 64 KiB");
        }
        if self.max_changeset_operations == 0 || self.max_changeset_operations > 1_000 {
            return Err("max changeset operations must be between 1 and 1000");
        }
        if self.max_search_results == 0 || self.max_search_results > 100_000 {
            return Err("max search results must be between 1 and 100000");
        }
        Ok(())
    }
}
