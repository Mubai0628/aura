//! A screenshot a browser call of the conversation took, for the conversation to show. Only
//! an image the conversation itself may read (its own browser calls grant it), and only an image.
use super::*;
use aura_browser_domain::{BrowserScreenshotImageV1, BrowserScreenshotReadRequest};
use base64::Engine as _;

/// The longest screenshot shown (a full page is at most 1280 by 6000 pixels as a JPEG).
const LONGEST: usize = 16 * 1024 * 1024;

/// What an image's first bytes say it is: the browser's screenshots are JPEG, or PNG.
pub(super) fn image_media_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some("image/png")
    } else {
        None
    }
}

impl AgentApplication {
    pub async fn screenshot(
        &self,
        input: BrowserScreenshotReadRequest,
    ) -> anyhow::Result<BrowserScreenshotImageV1> {
        let journal = Arc::clone(&self.execution.journal);
        let bytes = self
            .store
            .run_critical(
                StoreStep::operation("browser.screenshot.read"),
                StoreAccess::Read,
                move || {
                    let journal = journal.lock()?;
                    Ok(aura_persistence_core::artifact::read(
                        journal.database.connection(),
                        input.artifact_id,
                        &input.session_id.to_string(),
                    )
                    .ok())
                },
            )
            .await?
            .ok_or_else(|| anyhow::anyhow!("browser.screenshot_unavailable: 这张截图已不可用。"))?;
        let media_type = image_media_type(&bytes)
            .filter(|_| bytes.len() <= LONGEST)
            .ok_or_else(|| anyhow::anyhow!("browser.screenshot_unavailable: 这不是一张截图。"))?;
        Ok(BrowserScreenshotImageV1 {
            media_type: media_type.into(),
            data_base64: base64::engine::general_purpose::STANDARD.encode(&bytes),
        })
    }
}
