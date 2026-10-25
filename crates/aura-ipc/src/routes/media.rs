//! Main-only media bytes and completion. No renderer or preload capability is created.
//! A message's attachments read again for the conversation's 文件 tab.
use super::*;
pub(super) const ROUTES: &[RouteDescriptor] = &[
    route!(
        "media.host.read",
        Agent,
        Core,
        AuthenticatedMain,
        "MediaHostReadRequestV1",
        "MediaHostReadResultV1",
        ReadOnly,
        None
    ),
    route!(
        "media.host.result",
        Agent,
        Core,
        AuthenticatedMain,
        "MediaHostResultV1",
        "MediaHostResultAck",
        TransientInput,
        None
    ),
    // A text file or a zip's listing one of the conversation's messages carries.
    route!(
        "agent.attachments.text",
        Agent,
        Main,
        TrustedRenderer,
        "AttachmentTextRequest",
        "AttachmentText",
        ReadOnly,
        None
    ),
    route!(
        "agent.attachments.text",
        Agent,
        Core,
        AuthenticatedMain,
        "AttachmentTextRequest",
        "AttachmentText",
        ReadOnly,
        None
    ),
    // Where an attached file outside the workspace is now, for Main to stream or open it.
    // The renderer names the attachment; only Main learns the place.
    route!(
        "agent.attachments.locate",
        Agent,
        Core,
        AuthenticatedMain,
        "AttachmentLocateRequest",
        "AttachmentLocation",
        ReadOnly,
        None
    ),
    // An entry of an attached zip, copied into Aura's folder for the conversation; only
    // Main learns where.
    route!(
        "agent.attachments.entry",
        Agent,
        Core,
        AuthenticatedMain,
        "AttachmentEntryRequest",
        "AttachmentLocation",
        ReadOnly,
        None
    ),
    // A file outside the workspace a message names, found in that message first; only Main
    // learns where.
    route!(
        "agent.files.mentioned",
        Agent,
        Core,
        AuthenticatedMain,
        "MentionedFileRequest",
        "AttachmentLocation",
        ReadOnly,
        None
    ),
];

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn media_host_bytes_and_results_are_main_only_and_cannot_replay_commands() {
        for method in [
            "media.host.read",
            "media.host.result",
            "agent.attachments.locate",
            "agent.attachments.entry",
            "agent.files.mentioned",
        ] {
            let route = core_route(method).expect("Main-only media route");
            assert_eq!(route.auth, RouteAuth::AuthenticatedMain);
            assert!(
                registry()
                    .all(|entry| entry.method != method || entry.surface == RouteSurface::Core)
            );
            assert!(!records_command_outcome(method));
        }
    }
}
