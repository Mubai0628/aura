//! The fs.write model input accepts the same hash object fs.patch accepts.
use crate::FsExpectedState;
use aura_domain_common::ContentHash;
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer};

#[derive(Deserialize, JsonSchema)]
#[serde(untagged)]
pub(crate) enum AgentFileWriteExpectation {
    Tagged(FsExpectedState),
    Hash(ContentHash),
}

pub(crate) fn deserialize_expected<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<FsExpectedState, D::Error> {
    Ok(match AgentFileWriteExpectation::deserialize(deserializer)? {
        AgentFileWriteExpectation::Tagged(expected) => expected,
        AgentFileWriteExpectation::Hash(hash) => FsExpectedState::FileHash { hash },
    })
}
