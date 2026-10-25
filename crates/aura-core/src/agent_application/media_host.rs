//! The authenticated host's bounded read of one active command's retained file handle.
use super::*;
impl AgentApplication {
    pub(crate) async fn media_read(
        &self,
        input: MediaHostReadRequestV1,
    ) -> anyhow::Result<MediaHostReadResultV1> {
        let bridge = self.media();
        self.store
            .run_critical(StoreStep::operation("media.host.read"), StoreAccess::Read, move || {
                bridge.read(&input)
            })
            .await
    }
}
