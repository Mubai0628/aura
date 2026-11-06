//! An MCP resource a message refers to, read through the conversation's own instance of its
//! server when the message is first accepted and kept as a capture under the name the composer
//! showed.
use super::*;

/// The kept file's name: what the composer showed, as text.
pub(super) fn file_name(name: &str) -> String {
    format!("{name}.txt")
}

/// What the model reads of the resource: where it came from, then its text.
pub(super) fn read(
    execution: &aura_agent_execution::agent_execution::AgentExecution,
    session: SessionId,
    reference: (&str, &str, &str),
) -> anyhow::Result<String> {
    let (server_id, uri, name) = reference;
    let runtime = execution
        .mcp
        .get()
        .ok_or_else(|| anyhow::anyhow!("mcp.unavailable: 这个进程里没有 MCP 服务"))?;
    let read = runtime.read_resource(session, server_id, uri)?;
    let more = if read.truncated {
        "\n[The resource's text goes on; only its first 1 MB was kept.]\n"
    } else {
        ""
    };
    Ok(format!(
        "MCP resource \"{name}\" ({uri}) of server \"{}\" as it was when this message was sent:\n\n{}\n{more}",
        read.server_name, read.text
    ))
}
