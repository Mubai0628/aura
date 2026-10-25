//! Decoding is an explicit model choice, after Core has authorized one file for reading.
use super::definitions::entry;
use super::*;
use aura_agent_domain::{MediaFramesInput, MediaFramesResult, MediaProbeInput, MediaProbeResult};
use aura_security_contract::ResolvedCapability;

pub(super) fn entries(
    at: UtcTimestamp,
) -> Result<Vec<AgentBuiltinEntry>, BuiltinToolDefinitionSetError> {
    let read = &[ResolvedCapability::WorkspaceFileRead];
    [
        entry::<MediaProbeInput, MediaProbeResult>(
            84,
            AgentBuiltinHandler::MediaProbe,
            "Inspect a video file",
            "Read a video's metadata with Chromium's decoder: duration in seconds, dimensions, frame rate when available, audio-track presence, container and codec when available, and byte length. path names a workspace file or an external file with an active single-file read grant for this conversation. ReadOnly is allowed. Inspect metadata when it helps choose frame times or a contact sheet; attaching a video never starts decoding automatically. Unknown metadata is null. This tool does not listen to audio or transcribe it.",
            read,
            false,
            at,
        ),
        entry::<MediaFramesInput, MediaFramesResult>(
            85,
            AgentBuiltinHandler::MediaFrames,
            "Read frames from a video",
            "Read video frames from a workspace file or an external file with an active single-file read grant for this conversation; ReadOnly is allowed. Supply exactly one of times (1 to 12 requested times in seconds, from zero up to but excluding the duration) or count (1 to 12 uniformly spaced frames). sheet=true returns one contact sheet with time labels; otherwise each frame is an image. max_side bounds the longest side of each final image, including the whole sheet: default 1024, range 64 to 2048. Each result reports requested_time and actual_time. For an image-capable model, the newest media.frames result's images accompany the next request with shown_to_model=true, alongside the latest 3 browser screenshots. A model without image input receives times and an explicit explanation that it cannot see the images. You cannot hear the audio; this tool performs no audio transcription. Unsupported codecs and decoding timeouts return an explicit error.",
            read,
            false,
            at,
        ),
    ]
    .into_iter()
    .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn media_definitions_match_their_immutable_published_fingerprints() {
        let at = "2026-09-09T00:00:00Z".parse().expect("timestamp");
        let entries = super::entries(at).expect("valid media schemas");
        let actual = entries
            .iter()
            .map(|entry| {
                (entry.handler.name(), entry.definition.definition_fingerprint.hex().to_owned())
            })
            .collect::<Vec<_>>();
        let published = entries
            .iter()
            .map(|entry| {
                let row = crate::agent_release::revisions::current(entry.handler.name())
                    .expect("published media Tool");
                (row.name, row.fingerprint.to_owned())
            })
            .collect::<Vec<_>>();
        assert_eq!(actual, published, "publish fingerprints from the owning Rust definitions");
    }
}
