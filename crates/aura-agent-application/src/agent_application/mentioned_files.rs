//! A file outside the workspace that the conversation names by its full path, in one of its
//! messages or in the row of one of its calls, opened for the 文件 tab. Core reads that message or
//! that row and finds the path in it first, so only what the conversation shows can be opened;
//! Aura's own data and the user's credential stores stay closed.
use super::*;
use aura_agent_domain::{
    AgentMessageTextRequest, AttachmentLocation, MentionedFileRequest, MentionedFileSource,
};
use aura_agent_execution::agent_execution;
use aura_tool_projection::{AgentToolActivityPage, AgentToolActivityRequest};
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

/// `%XX` escapes read as the UTF-8 bytes they spell, as in a file address; text whose escapes do
/// not spell UTF-8 stays as written.
fn percent_decoded(text: &str) -> String {
    let hex = |byte: u8| match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    };
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let (Some(high), Some(low)) = (hex(bytes[index + 1]), hex(bytes[index + 2]))
        {
            out.push(high * 16 + low);
            index += 3;
            continue;
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| text.to_owned())
}

/// What a call's row shows: the file or page it names and the lines it found, and the same again
/// with a file address's escapes read (`file:///D:/%E6%B8%B8%E6%88%8F.html` names `D:\游戏.html`).
/// Nothing for a call that is not this conversation's.
fn row_text(page: AgentToolActivityPage) -> String {
    let Some(row) = page.calls.into_iter().next() else {
        return String::new();
    };
    let mut text = row.display.target.unwrap_or_default();
    for line in row.display.details {
        text.push('\n');
        text.push_str(&line);
    }
    let decoded = percent_decoded(&text);
    if decoded != text {
        text.push('\n');
        text.push_str(&decoded);
    }
    text
}

impl AgentApplication {
    /// Where a file a message or a call's row of this conversation names is, once that message or
    /// row is found to name it.
    pub async fn mentioned_file(
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
        let named_in = input.mentioned_in;
        let unnamed = || match named_in {
            MentionedFileSource::ToolCall { .. } => {
                refused("not_mentioned", "这次调用里没有这个路径。")
            }
            _ => refused("not_mentioned", "这条消息里没有这个路径。"),
        };
        let text = self
            .store
            .read(StoreStep::operation("agent.files.mentioned"), move |db| {
                if let MentionedFileSource::ToolCall { tool_call_id } = named_in {
                    // Read only within this conversation: another's call is no row of it.
                    let request = AgentToolActivityRequest {
                        session_id: session,
                        before_call_id: None,
                        limit: 1,
                        call_ids: Some(vec![tool_call_id]),
                    };
                    return aura_persistence_v2::tool::activity::page(db, &request).map(row_text);
                }
                let Some(source) = named_in.message() else {
                    return Ok(String::new());
                };
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
            .map_err(|_| unnamed())?;
        anyhow::ensure!(folded(&text).contains(&folded(&path)), unnamed());
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
                path: agent_execution::protected_paths::strip_verbatim_prefix(
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
    use super::{folded, percent_decoded};

    #[test]
    fn a_path_is_found_however_its_slashes_and_case_are_written() {
        let message = folded("已保存到 `C:/Users/Me/Desktop/Report.PDF`，以及 C:\\\\Temp\\\\a.txt");
        assert!(message.contains(&folded("C:\\Users\\me\\Desktop\\report.pdf")));
        assert!(message.contains(&folded("C:\\Temp\\a.txt")));
        assert!(!message.contains(&folded("C:\\Temp\\b.txt")));
    }

    #[test]
    fn a_file_address_names_its_file_once_its_escapes_are_read() {
        let page = percent_decoded("file:///D:/%E6%B8%B8%E6%88%8F/Game%20One.html");
        assert_eq!(page, "file:///D:/游戏/Game One.html");
        assert!(folded(&page).contains(&folded("D:\\游戏\\Game One.html")));
        // Escapes that spell no UTF-8, a lone `%` and a cut-off escape stay as written.
        assert_eq!(percent_decoded("C:\\100%\\%FF.txt"), "C:\\100%\\%FF.txt");
        assert_eq!(percent_decoded("50% %4"), "50% %4");
    }
}
