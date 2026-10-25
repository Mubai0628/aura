//! Git contracts are owned by the workspace domain, collected here for generation.
#[path = "git_contract.rs"]
mod reads;
#[path = "git_remote_contract.rs"]
mod remotes;
#[path = "git_review_contract.rs"]
mod review;
#[path = "git_write_contract.rs"]
mod writes;
pub(super) use reads::{PATH as READ_PATH, bytes as read_bytes};
pub(super) use remotes::{PATH as REMOTE_PATH, bytes as remote_bytes};
pub(super) use review::PATH as REVIEW_PATH;
pub(super) use writes::{PATH as WRITE_PATH, bytes as write_bytes};
pub(super) fn register(generator: &mut schemars::SchemaGenerator) {
    reads::register(generator);
    writes::register(generator);
    remotes::register(generator);
}
pub(super) fn outputs() -> anyhow::Result<Vec<(&'static str, Vec<u8>)>> {
    Ok(vec![
        (READ_PATH, read_bytes()?),
        (WRITE_PATH, write_bytes()?),
        (REMOTE_PATH, remote_bytes()?),
        (REVIEW_PATH, review::bytes()?),
    ])
}
