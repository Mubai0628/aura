use anyhow::Result;
use aura_domain_common::{
    ContentHash, ToolCallId, ToolExecutionAttemptId, jcs::canonical_json_jcs,
};
use aura_tool_contract::{StdioArtifactReadyPayloadV1, StdioProtocolFrameV1};
use aura_tool_runtime::{
    StdioAttemptProtocolV1, StdioRuntimeFrameDecoderV1, StdioRuntimeFrameDispositionV1,
    encode_stdio_frame_bytes,
};

pub(super) struct Protocol {
    decoder: StdioRuntimeFrameDecoderV1,
    protocol: StdioAttemptProtocolV1,
    arguments: serde_json::Value,
    arguments_hash: ContentHash,
    pub(super) artifacts: Vec<StdioArtifactReadyPayloadV1>,
}
impl Protocol {
    pub(super) fn new(
        call: ToolCallId,
        attempt: ToolExecutionAttemptId,
        arguments: serde_json::Value,
    ) -> Result<Self> {
        let arguments_hash = ContentHash::sha256(&canonical_json_jcs(&arguments)?);
        Ok(Self {
            decoder: StdioRuntimeFrameDecoderV1::new(),
            protocol: StdioAttemptProtocolV1::new(call, attempt),
            arguments,
            arguments_hash,
            artifacts: vec![],
        })
    }
    pub(super) fn accept(&mut self, bytes: &[u8]) -> Result<Vec<Vec<u8>>> {
        let mut responses = Vec::new();
        for frame in self.decoder.push_chunk(bytes)? {
            let artifact = if let StdioProtocolFrameV1::ArtifactReady { payload, .. } = &frame {
                Some(payload.clone())
            } else {
                None
            };
            let disposition = self.protocol.accept_runtime_frame(frame)?;
            if disposition == StdioRuntimeFrameDispositionV1::HandshakeAccepted {
                responses.push(encode_stdio_frame_bytes(
                    &self
                        .protocol
                        .prepare_execute(self.arguments.clone(), self.arguments_hash.clone())?,
                )?);
            }
            if let Some(artifact) = artifact {
                self.artifacts.push(artifact);
            }
        }
        Ok(responses)
    }
    pub(super) fn result(&self) -> Result<Vec<u8>> {
        self.decoder.finish_on_eof()?;
        self.protocol.finish_on_eof()?;
        match self.protocol.terminal_frame() {
            Some(StdioProtocolFrameV1::FinalResult { payload, .. }) => {
                Ok(canonical_json_jcs(&payload.result)?)
            }
            _ => anyhow::bail!("package.stdio_failure"),
        }
    }
}
