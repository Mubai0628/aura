//! The execution ledger and the store access Core's services share: the one V2 write authority
//! with its post-execution owner and dispatch port, the Agent store, the memory service the ledger
//! opens, and the credential facts of installed package tools that the store reads.
#![forbid(unsafe_code)]

pub mod agent_store;
pub mod broker;
mod memory_services;
pub mod package_credentials;
