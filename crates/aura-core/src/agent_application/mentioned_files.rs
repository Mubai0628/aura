//! A file outside the workspace that a message of the conversation names by its full path,
//! opened for the 文件 tab. Core reads that message and finds the path in it first, so only what
//! the conversation shows can be opened; Aura's own data and the user's credential stores stay
//! closed.
use super::*;
use aura_agent_domain::{AgentMessageTextRequest, AttachmentLocation, MentionedFileRequest};
use aura_windows_platform::{NativeFileAccess, open_verified_existing};

/// How much of the message is read to find the path.
const READ_AT_MOST: usize = 4 * 1024 * 1024;
const LONGEST_PATH: usize = 1024;

fn refused(code: &str, why: &str) -> anyhow::Error {
    anyhow::anyhow!("agent.mentioned_file_{code}: {why}")
}

/// The spelling paths are compared in: backslashes, one at a time, in any case.
fn folded(text: &str) -> String {
    let mut out = text.replace('/', "\\");
    while out.contains("\\\\") {
        out = out.replace("\\\\", "\\");
    }
    out.to_lowercase()
}

impl AgentApplication {
    /// Where a file a message of this conversation names is, once that message is found to name
    /// it.
    pub(crate) async fn mentioned_file(
        &self,
        input: MentionedFileRequest,
    ) -> anyhow::Result<AttachmentLocation> {
        let path = input.path.trim().to_owned();
        anyhow::ensure!(
            !path.is_empty()
                && path.len() <= LONGEST_PATH
                && !path.contains('\0')
                && std::path::Path::new(&path).is_absolute(),
            refused("invalid", "这不是一个完整的文件路径。")
        );
        let session = input.session_id;
        let source = input.mentioned_in;
        let text = self
            .store
            .read(StoreStep::operation("agent.files.mentioned"), move |db| {
                let mut text = String::new();
                let mut offset = 0_u64;
                loop {
                    let page = aura_persistence_v2::agent::desktop_messages::text(
                        db,
                        &AgentMessageTextRequest {
                            session_id: session,
                            source,
                            offset,
                            limit: 65_536,
                        },
                    )?;
                    text.push_str(&page.text);
                    match page.next_offset {
                        Some(next) if next > offset && text.len() < READ_AT_MOST => offset = next,
                        _ => return Ok(text),
                    }
                }
            })
            .await
            .map_err(|_| refused("not_mentioned", "这条消息里没有这个路径。"))?;
        anyhow::ensure!(
            folded(&text).contains(&folded(&path)),
            refused("not_mentioned", "这条消息里没有这个路径。")
        );
        let resources = Arc::clone(&self.execution.control_resources);
        tokio::task::spawn_blocking(move || {
            let file = open_verified_existing(std::path::Path::new(&path), NativeFileAccess::Read)
                .map_err(|_| refused("missing", "找不到这个文件，它可能已被移动或删除。"))?;
            anyhow::ensure!(!file.info().is_directory, refused("folder", "这是文件夹。"));
            anyhow::ensure!(
                resources.viewable_outside(&file.info().final_path),
                refused("protected", "这里是 Aura 的数据或你的凭据所在的位置，不在这里打开。")
            );
            let byte_length = file.file().metadata()?.len();
            Ok(AttachmentLocation {
                path: crate::agent_execution::protected_paths::strip_verbatim_prefix(
                    &file.info().final_path.display().to_string(),
                ),
                byte_length,
            })
        })
        .await?
    }
}

#[cfg(test)]
mod tests {
    use super::folded;

    #[test]
    fn a_path_is_found_however_its_slashes_and_case_are_written() {
        let message = folded("已保存到 `C:/Users/Me/Desktop/Report.PDF`，以及 C:\\\\Temp\\\\a.txt");
        assert!(message.contains(&folded("C:\\Users\\me\\Desktop\\report.pdf")));
        assert!(message.contains(&folded("C:\\Temp\\a.txt")));
        assert!(!message.contains(&folded("C:\\Temp\\b.txt")));
    }
}
