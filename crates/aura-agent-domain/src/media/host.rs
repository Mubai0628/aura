use super::MediaProbeResult;
use aura_domain_common::{RuntimeInstanceId, SessionId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MediaHostActionV1 {
    Probe,
    Frames { times: Option<Vec<f64>>, count: Option<u32>, sheet: bool, max_side: u32 },
}

/// Main receives no path, only a bounded command whose bytes remain owned by Core.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MediaHostCommandV1 {
    pub command_id: String,
    pub runtime_instance_id: RuntimeInstanceId,
    pub session_id: SessionId,
    pub file_size: u64,
    pub media_type: String,
    pub timeout_ms: u32,
    pub action: MediaHostActionV1,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MediaHostCancelV1 {
    pub command_id: String,
    pub runtime_instance_id: RuntimeInstanceId,
    pub session_id: SessionId,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MediaHostReadRequestV1 {
    pub command_id: String,
    pub runtime_instance_id: RuntimeInstanceId,
    pub session_id: SessionId,
    pub offset: u64,
    pub length: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MediaHostReadResultV1 {
    pub data_base64: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MediaImageV1 {
    pub media_type: String,
    pub data_base64: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MediaHostFrameV1 {
    pub requested_time: f64,
    pub actual_time: f64,
    pub image: Option<MediaImageV1>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MediaFramesHostResultV1 {
    pub frames: Vec<MediaHostFrameV1>,
    pub sheet: Option<MediaImageV1>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MediaHostCompletedV1 {
    Probe(MediaProbeResult),
    Frames(MediaFramesHostResultV1),
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum MediaHostOutcomeV1 {
    Completed { result: MediaHostCompletedV1 },
    Refused { message: String },
    Failed { message: String },
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MediaHostResultV1 {
    pub command_id: String,
    pub runtime_instance_id: RuntimeInstanceId,
    pub session_id: SessionId,
    pub outcome: MediaHostOutcomeV1,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MediaHostResultAck {
    pub accepted: bool,
}
