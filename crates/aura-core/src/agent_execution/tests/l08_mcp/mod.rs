//! MCP tools through the real Agent admission, permission, execution and result path.
//! The server is the repository's test MCP server running as the user in a Job, configured and
//! trusted through the real settings routes; only the Provider output is a fixture.
use super::*;
mod approvals;
mod calls;
mod changes;
mod failures;
mod http;
mod support;
