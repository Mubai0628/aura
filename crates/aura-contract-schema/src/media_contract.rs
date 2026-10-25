//! The owning Rust media tools and Main-only bounded decoder channel. A message's
//! attachments read again for the 文件 tab. An entry of an attached zip. A file outside
//! the workspace a message names.
pub(super) fn register(generator: &mut schemars::SchemaGenerator) {
    use aura_agent_domain::*;
    macro_rules! register { ($($ty:ty),* $(,)?) => { $(generator.subschema_for::<$ty>();)* } }
    register!(
        MediaProbeInput,
        MediaProbeResult,
        MediaFramesInput,
        MediaFramesResult,
        MediaFrameResult,
        MediaHostCommandV1,
        MediaHostCancelV1,
        MediaHostReadRequestV1,
        MediaHostReadResultV1,
        MediaHostResultV1,
        MediaHostResultAck,
        AttachmentTextRequest,
        AttachmentText,
        AttachmentLocateRequest,
        AttachmentLocation,
        AttachmentEntryRequest,
        MentionedFileRequest,
    );
}
