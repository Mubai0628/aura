//! A conversation's MCP resources. The "@" list names the resources of the servers that are ready
//! and said they offer resources, each through the conversation's own instance of the server;
//! a server that offers none is not started for it. A resource is read as text when a message
//! that refers to it is sent.
use super::McpRuntime;
use aura_domain_common::SessionId;
use aura_mcp::{
    McpResourceList, McpResourceProblem, McpResourceView, McpServerConfig, McpServerState,
    parse_read_result, parse_resources_page, resource_text,
};
use serde_json::json;
use std::{sync::Arc, time::Duration};

/// The most pages and resources of one server the list reads.
const MAX_PAGES: usize = 10;
const MAX_RESOURCES: usize = 200;
/// The most of a resource's text a message keeps: as much as a capture holds.
const MAX_TEXT_BYTES: usize = 1024 * 1024;

/// One resource read for a message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadResource {
    /// The server as Settings → MCP names it.
    pub server_name: String,
    pub text: String,
    /// The resource has more text than was kept.
    pub truncated: bool,
}

impl McpRuntime {
    /// Whether `server` is ready and offers resources: as its last discovery said, or as the
    /// conversation's running instance of it says.
    fn offers_resources(&self, session: SessionId, server: &McpServerConfig) -> bool {
        if self.view(server).state != McpServerState::Ready {
            return false;
        }
        self.catalog(&server.id).is_some_and(|catalog| catalog.server.resources)
            || self
                .alive(&(session, server.id.clone()))
                .ok()
                .flatten()
                .is_some_and(|connection| connection.initialized.resources_supported)
    }

    /// The resources of every ready server that offers them, in the servers' order.
    pub fn resources(self: &Arc<Self>, session: SessionId) -> McpResourceList {
        let mut list = McpResourceList { resources: Vec::new(), problems: Vec::new() };
        for server in self.configured().servers {
            if !self.offers_resources(session, &server) {
                continue;
            }
            match self.list_resources(session, &server) {
                Ok(found) => list.resources.extend(found),
                Err(reason) => list.problems.push(McpResourceProblem {
                    server_id: server.id.clone(),
                    server_name: server.display_name.clone(),
                    reason,
                }),
            }
        }
        list
    }

    fn list_resources(
        self: &Arc<Self>,
        session: SessionId,
        server: &McpServerConfig,
    ) -> Result<Vec<McpResourceView>, String> {
        let connection = self.connection(session, server)?;
        let timeout = Duration::from_millis(server.call_timeout_ms);
        let mut found = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..MAX_PAGES {
            let params = cursor.as_ref().map(|cursor| json!({"cursor": cursor}));
            let page = connection
                .request("resources/list", params, timeout, None)
                .map_err(|failure| format!("列出资源失败：{}", failure.describe()))?;
            let page = parse_resources_page(&page).map_err(|error| error.to_string())?;
            found.extend(page.resources.into_iter().map(|resource| McpResourceView {
                server_id: server.id.clone(),
                server_name: server.display_name.clone(),
                uri: resource.uri,
                name: resource.name,
                title: resource.title,
                mime_type: resource.mime_type,
            }));
            if found.len() >= MAX_RESOURCES {
                found.truncate(MAX_RESOURCES);
                break;
            }
            match page.next_cursor {
                Some(next) if Some(&next) != cursor.as_ref() => cursor = Some(next),
                _ => break,
            }
        }
        Ok(found)
    }

    /// One resource of a ready server that offers resources, read as text through the
    /// conversation's own instance of the server.
    pub fn read_resource(
        self: &Arc<Self>,
        session: SessionId,
        server_id: &str,
        uri: &str,
    ) -> anyhow::Result<ReadResource> {
        let server = self
            .configured()
            .servers
            .into_iter()
            .find(|server| server.id == server_id)
            .ok_or_else(|| anyhow::anyhow!("mcp.not_found: 这个服务器不在列表里，请刷新"))?;
        anyhow::ensure!(
            self.offers_resources(session, &server),
            "mcp.resource_unavailable: 服务器「{}」现在不能读资源，请在 设置 → MCP 里查看",
            server.display_name
        );
        let connection = self.connection(session, &server).map_err(|reason| {
            anyhow::anyhow!("mcp.resource_unavailable: 服务器「{}」：{reason}", server.display_name)
        })?;
        let read = connection
            .request(
                "resources/read",
                Some(json!({"uri": uri})),
                Duration::from_millis(server.call_timeout_ms),
                None,
            )
            .map_err(|failure| {
                anyhow::anyhow!(
                    "mcp.resource_failed: 服务器「{}」没有读出这个资源：{}",
                    server.display_name,
                    failure.describe()
                )
            })?;
        let parts = parse_read_result(&read).map_err(|error| {
            anyhow::anyhow!("mcp.resource_failed: 服务器「{}」：{error}", server.display_name)
        })?;
        let (text, truncated) = resource_text(&parts, MAX_TEXT_BYTES);
        Ok(ReadResource { server_name: server.display_name, text, truncated })
    }
}
