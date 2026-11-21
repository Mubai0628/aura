use aura_domain_common::ArtifactId;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MediaProbeInput {
    /// A workspace file or a file already authorized for reading in this conversation.
    pub path: String,
}

fn default_max_side() -> u32 {
    1024
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MediaFramesInput {
    pub path: String,
    /// Frame times in seconds, from zero up to but excluding the duration. Use times or count.
    #[serde(default)]
    #[schemars(length(min = 1, max = 12))]
    pub times: Option<Vec<f64>>,
    /// Number of evenly spaced frames. Use count or times.
    #[serde(default)]
    #[schemars(range(min = 1, max = 12))]
    pub count: Option<u32>,
    #[serde(default)]
    pub sheet: bool,
    /// Longest side in pixels, including a contact sheet. Default 1024.
    #[serde(default = "default_max_side")]
    #[schemars(range(min = 64, max = 2048))]
    pub max_side: u32,
}

impl MediaFramesInput {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !(64..=2048).contains(&self.max_side) {
            return Err("media.max_side_invalid");
        }
        match (&self.times, self.count) {
            (Some(times), None)
                if !times.is_empty()
                    && times.len() <= super::MAX_MEDIA_FRAMES
                    && times.iter().all(|time| time.is_finite() && *time >= 0.0) =>
            {
                Ok(())
            }
            (None, Some(count)) if (1..=12).contains(&count) => Ok(()),
            _ => Err("media.frames_selection_invalid"),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MediaProbeResult {
    pub duration: f64,
    pub width: u32,
    pub height: u32,
    pub frame_rate: Option<f64>,
    pub has_audio: Option<bool>,
    pub container: Option<String>,
    pub codec: Option<String>,
    pub byte_length: u64,
    /// Why codec or frame_rate is null: what kept the file's header from naming them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MediaFrameResult {
    pub requested_time: f64,
    pub actual_time: f64,
    pub image_artifact_id: Option<ArtifactId>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MediaFramesResult {
    pub frames: Vec<MediaFrameResult>,
    pub sheet_artifact_id: Option<ArtifactId>,
    pub shown_to_model: bool,
    pub note: String,
}
