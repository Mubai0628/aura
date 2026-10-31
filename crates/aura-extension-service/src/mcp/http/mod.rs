//! A Streamable HTTP server: JSON-RPC over POST, answered as JSON or as server-sent events.
//! Every request goes through Core's network gateway under a grant for exactly the trusted
//! address (`aura_network_control::McpHttpRequestGrant`): no redirect, no other host, and a
//! domain name only on the public internet.
mod failure;
mod session;
mod wire;

pub(crate) use failure::describe as describe_network;
pub(crate) use session::{HttpPlan, HttpSession, SIGN_IN_REQUIRED};
