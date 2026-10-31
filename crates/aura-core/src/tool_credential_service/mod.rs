//! The tool credential service is aura-tool-service's. Core keeps the routes that take a tool
//! secret on the bound secret input (`commands`) and the scope tests that need a started Core;
//! both read the service's names from here.
pub(crate) use aura_tool_service::tool_credential_service::*;

#[cfg(test)]
use aura_domain_common::CommandInvocationId;
#[cfg(test)]
use aura_persistence::ToolCredentialMutationIntentV1;
#[cfg(test)]
use aura_tool_domain::ToolCredentialManagementTargetV1;
#[cfg(test)]
use aura_windows_platform::SecretBytes;
#[cfg(test)]
use aura_windows_platform::WindowsCredentialStore;
#[cfg(test)]
use std::sync::Arc;

pub(crate) mod commands;
#[cfg(test)]
mod scope_tests;
