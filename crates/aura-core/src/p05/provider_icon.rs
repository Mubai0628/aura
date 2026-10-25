use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use aura_artifact_contract::{
    ArtifactAccess, ArtifactOwnerRef, ArtifactScope, ArtifactSystemOwner,
};
use aura_domain_common::{
    ArtifactId, ContentHash, CorrelationId, NetworkRequestId, PrincipalId, UtcTimestamp,
};
use aura_model_config::ModelConfigService;
use aura_model_domain::ProviderIconRef;
use aura_network_control::{
    CoreHttpClientGateway, GatewayRequest, NetworkControlPlane, StandardNetworkRequestGrant,
};
use aura_permission_domain::{
    CoreRequestGrant, NetworkByteLimits, NetworkDestinationPolicy, NetworkProtocolClass,
    NetworkRequestPlan, RedirectPolicy,
};
use aura_persistence::{
    ArtifactMetadataRequest, ArtifactReadRequest, ArtifactWriteRequest, ModelProjectionDraft,
    PersistenceCoordinator, PersistenceError,
};
use aura_security_contract::{
    InvocationActor, InvocationIdentity, NetworkCallerKind, NetworkDataClass,
    NetworkDestinationClass,
};
use regex::Regex;
use thiserror::Error;

use super::catalog::{PersistentProviderCatalog, project_catalog};

const MODELS_DEV_ORIGIN: &str = "https://models.dev";
const ICON_MAX_BYTES: u64 = 256 * 1024;
const ICON_REQUEST_TIMEOUT_MS: u64 = 15_000;
const ICON_GRANT_TTL_SECONDS: i64 = 30;
const ICON_FETCH_CONCURRENCY: usize = 8;
const ICON_PUBLISH_BATCH_SIZE: usize = 8;
const AURA_NEUTRAL_FOREGROUND: &str = "#5f6b66";

#[derive(Debug, Error)]
pub(crate) enum ProviderIconError {
    #[error("provider catalog icon request could not be authorized or completed")]
    Network(#[from] aura_network_control::NetworkControlError),
    #[error("provider catalog icon cache operation failed")]
    Persistence(#[from] PersistenceError),
    #[error("provider catalog icon URL is invalid")]
    InvalidUrl,
    #[error("provider catalog icon response is invalid")]
    InvalidResponse,
    #[error("provider catalog icon SVG is unsafe or invalid")]
    InvalidSvg,
    #[error("provider catalog icon PNG encoding failed")]
    InvalidPng,
}

struct ProviderIconServiceInner {
    gateway: Arc<CoreHttpClientGateway>,
    catalog: Arc<PersistentProviderCatalog>,
    model_config: Arc<ModelConfigService>,
    persistence: Arc<PersistenceCoordinator>,
    refresh_in_progress: AtomicBool,
}

#[derive(Clone)]
pub(crate) struct ProviderIconService {
    inner: Arc<ProviderIconServiceInner>,
}

impl ProviderIconService {
    #[must_use]
    pub(crate) fn new(
        gateway: Arc<CoreHttpClientGateway>,
        catalog: Arc<PersistentProviderCatalog>,
        model_config: Arc<ModelConfigService>,
        persistence: Arc<PersistenceCoordinator>,
    ) -> Self {
        Self {
            inner: Arc::new(ProviderIconServiceInner {
                gateway,
                catalog,
                model_config,
                persistence,
                refresh_in_progress: AtomicBool::new(false),
            }),
        }
    }

    pub(crate) fn schedule_missing(&self) {
        if tokio::runtime::Handle::try_current().is_err()
            || self.inner.refresh_in_progress.swap(true, Ordering::AcqRel)
        {
            return;
        }
        let service = self.clone();
        drop(tokio::spawn(async move {
            let _ = service.refresh_missing().await;
            service.inner.refresh_in_progress.store(false, Ordering::Release);
        }));
    }

    async fn refresh_missing(&self) -> Result<(), ProviderIconError> {
        let status = self.inner.catalog.status().map_err(|_| ProviderIconError::InvalidResponse)?;
        let Some(snapshot) = status.snapshot.as_ref() else {
            return Ok(());
        };
        let existing = self.inner.persistence.provider_catalog_icons()?;
        let provider_ids = snapshot
            .provider_order
            .iter()
            .filter(|provider_id| {
                snapshot.providers.contains_key(*provider_id)
                    && !existing.contains_key(*provider_id)
            })
            .cloned()
            .collect::<Vec<_>>();
        let mut pending = provider_ids.into_iter();
        let mut tasks = tokio::task::JoinSet::new();
        for _ in 0..ICON_FETCH_CONCURRENCY {
            let Some(provider_id) = pending.next() else {
                break;
            };
            let service = self.clone();
            tasks.spawn(async move {
                let result = service.fetch_and_store(&provider_id).await;
                (provider_id, result)
            });
        }
        let mut changed_since_publish = 0_usize;
        while let Some(joined) = tasks.join_next().await {
            if let Ok((provider_id, Ok(icon))) = joined {
                self.inner.persistence.store_provider_catalog_icon(provider_id, icon)?;
                changed_since_publish += 1;
                if changed_since_publish >= ICON_PUBLISH_BATCH_SIZE {
                    self.publish_projection()?;
                    changed_since_publish = 0;
                }
            }
            if let Some(provider_id) = pending.next() {
                let service = self.clone();
                tasks.spawn(async move {
                    let result = service.fetch_and_store(&provider_id).await;
                    (provider_id, result)
                });
            }
        }
        if changed_since_publish > 0 {
            self.publish_projection()?;
        }
        Ok(())
    }

    async fn fetch_and_store(
        &self,
        catalog_provider_id: &str,
    ) -> Result<ProviderIconRef, ProviderIconError> {
        let url = provider_icon_url(catalog_provider_id)?;
        let (grant, request) = authorize_icon_request(Arc::clone(&self.inner.gateway), url).await?;
        let response = self.inner.gateway.execute(grant, request).await?;
        if !(200..300).contains(&response.status)
            || !is_svg_media_type(header(&response.headers, "content-type").as_deref())
        {
            return Err(ProviderIconError::InvalidResponse);
        }
        let png = rasterize_provider_svg(&response.body)?;
        let artifact = self.inner.persistence.write_artifact(ArtifactWriteRequest {
            scope: ArtifactScope::SystemCache,
            media_type: "image/png".to_owned(),
            access: ArtifactAccess::USER_READ,
            owner: ArtifactOwnerRef::System { owner: ArtifactSystemOwner::ProviderCatalogCache },
            bytes: png,
        })?;
        Ok(ProviderIconRef {
            artifact_id: artifact.artifact_id,
            content_hash: artifact.content_hash,
            byte_length: artifact.byte_length,
            pixel_size: ProviderIconRef::PIXEL_SIZE,
        })
    }

    fn publish_projection(&self) -> Result<(), ProviderIconError> {
        let status = self.inner.catalog.status().map_err(|_| ProviderIconError::InvalidResponse)?;
        let snapshot =
            self.inner.model_config.snapshot().map_err(|_| ProviderIconError::InvalidResponse)?;
        let projection = project_catalog(&status, &snapshot, &self.inner.persistence)
            .map_err(|_| ProviderIconError::InvalidResponse)?;
        let changes = self.inner.persistence.commit_model_projection_snapshots(vec![
            ModelProjectionDraft::ProviderCatalog(projection),
        ])?;
        self.inner.persistence.publish_model_projection_changes(&changes)?;
        Ok(())
    }

    pub(crate) fn read_png(
        &self,
        artifact_id: ArtifactId,
        content_hash: &ContentHash,
    ) -> Result<Vec<u8>, ProviderIconError> {
        let mapping =
            self.inner.persistence.provider_catalog_icon_provider(artifact_id, content_hash)?;
        if mapping.is_none() {
            self.schedule_missing();
            return Err(ProviderIconError::InvalidResponse);
        }
        let metadata = self.inner.persistence.artifact_record(ArtifactMetadataRequest {
            artifact_id,
            session_id: None,
            required_access: ArtifactAccess::USER_READ,
            allow_quarantine: false,
        });
        let metadata = match metadata {
            Ok(metadata) => metadata,
            Err(error) => {
                self.schedule_missing();
                return Err(error.into());
            }
        };
        if metadata.scope != ArtifactScope::SystemCache
            || metadata.media_type != "image/png"
            || metadata.content_hash != *content_hash
            || metadata.owner
                != (ArtifactOwnerRef::System { owner: ArtifactSystemOwner::ProviderCatalogCache })
            || metadata.byte_length == 0
        {
            return Err(ProviderIconError::InvalidResponse);
        }
        let mut bytes = Vec::with_capacity(
            usize::try_from(metadata.byte_length)
                .map_err(|_| ProviderIconError::InvalidResponse)?,
        );
        let mut offset = 0_u64;
        while offset < metadata.byte_length {
            let length = usize::try_from((metadata.byte_length - offset).min(64 * 1024))
                .map_err(|_| ProviderIconError::InvalidResponse)?;
            let chunk = self.inner.persistence.read_artifact_range(ArtifactReadRequest {
                artifact_id,
                session_id: None,
                required_access: ArtifactAccess::USER_READ,
                offset,
                length,
                allow_quarantine: false,
            })?;
            bytes.extend_from_slice(&chunk);
            offset = offset.saturating_add(u64::try_from(chunk.len()).unwrap_or(u64::MAX));
        }
        if u64::try_from(bytes.len()).ok() != Some(metadata.byte_length)
            || ContentHash::blake3(&bytes) != metadata.content_hash
            || !bytes.starts_with(b"\x89PNG\r\n\x1a\n")
        {
            return Err(ProviderIconError::InvalidResponse);
        }
        Ok(bytes)
    }
}

async fn authorize_icon_request(
    gateway: Arc<CoreHttpClientGateway>,
    requested_url: String,
) -> Result<(CoreRequestGrant<StandardNetworkRequestGrant>, GatewayRequest), ProviderIconError> {
    let request_id = NetworkRequestId::new();
    let plan = NetworkRequestPlan {
        request_id,
        identity: InvocationIdentity {
            principal_id: PrincipalId::new(),
            actor: InvocationActor::SystemRuntime,
            workspace_id: None,
            session_id: None,
            turn_task_id: None,
            agent_run_id: None,
            parent_agent_run_id: None,
            correlation_id: CorrelationId::new(),
            permission_snapshot_id: None,
        },
        caller: NetworkCallerKind::ModelsCatalog,
        method: "GET".to_owned(),
        requested_url,
        destination: None,
        request_data: Vec::new(),
        data_classes: vec![NetworkDataClass::PublicQuery],
        redirect_policy: RedirectPolicy::SameOrigin { max_hops: 3 },
        byte_limits: NetworkByteLimits {
            max_upload_bytes: 1,
            max_download_bytes: ICON_MAX_BYTES,
            max_decompressed_bytes: ICON_MAX_BYTES,
        },
        timeout_ms: ICON_REQUEST_TIMEOUT_MS,
        credential_refs: Vec::new(),
        network_grant_ids: Vec::new(),
    };
    let authorized = gateway.authorize(plan).await?;
    let destination = authorized.destination;
    let grant = CoreRequestGrant {
        logical_request_id: request_id,
        purpose: StandardNetworkRequestGrant,
        caller: NetworkCallerKind::ModelsCatalog,
        destination_policy: NetworkDestinationPolicy {
            protocol: NetworkProtocolClass::Http,
            host: destination.host,
            port: destination.port,
            allowed_destination_classes: vec![NetworkDestinationClass::Public],
        },
        methods: vec!["GET".to_owned()],
        redirect_policy: authorized.plan.redirect_policy,
        data_classes: authorized.plan.data_classes.clone(),
        byte_limits: authorized.plan.byte_limits.clone(),
        expires_at: UtcTimestamp::from_value(
            UtcTimestamp::now().value() + time::Duration::seconds(ICON_GRANT_TTL_SECONDS),
        ),
    };
    Ok((
        grant,
        GatewayRequest {
            plan: authorized.plan,
            headers: vec![
                ("accept".to_owned(), "image/svg+xml".to_owned()),
                ("accept-encoding".to_owned(), "gzip, br, zstd".to_owned()),
            ],
            body: Vec::new(),
        },
    ))
}

fn provider_icon_url(catalog_provider_id: &str) -> Result<String, ProviderIconError> {
    if catalog_provider_id.is_empty() || catalog_provider_id.len() > 512 {
        return Err(ProviderIconError::InvalidUrl);
    }
    let mut url = url::Url::parse(MODELS_DEV_ORIGIN).map_err(|_| ProviderIconError::InvalidUrl)?;
    url.path_segments_mut()
        .map_err(|_| ProviderIconError::InvalidUrl)?
        .extend(["logos", &format!("{catalog_provider_id}.svg")]);
    Ok(url.into())
}

fn header(headers: &[(String, String)], name: &str) -> Option<String> {
    headers
        .iter()
        .find(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.clone())
}

fn is_svg_media_type(value: Option<&str>) -> bool {
    value
        .and_then(|value| value.split(';').next())
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("image/svg+xml"))
}

fn rasterize_provider_svg(source: &[u8]) -> Result<Vec<u8>, ProviderIconError> {
    if source.is_empty() || u64::try_from(source.len()).unwrap_or(u64::MAX) > ICON_MAX_BYTES {
        return Err(ProviderIconError::InvalidSvg);
    }
    let source = std::str::from_utf8(source).map_err(|_| ProviderIconError::InvalidSvg)?;
    validate_svg(source)?;
    let current_color =
        Regex::new("(?i)currentcolor").map_err(|_| ProviderIconError::InvalidSvg)?;
    let normalized = current_color.replace_all(source, AURA_NEUTRAL_FOREGROUND);
    let options = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_data(normalized.as_bytes(), &options)
        .map_err(|_| ProviderIconError::InvalidSvg)?;
    let size = tree.size();
    let width = size.width();
    let height = size.height();
    if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
        return Err(ProviderIconError::InvalidSvg);
    }
    let target = f32::from(ProviderIconRef::PIXEL_SIZE);
    let scale = (target / width).min(target / height);
    let transform = resvg::tiny_skia::Transform::from_row(
        scale,
        0.0,
        0.0,
        scale,
        (target - width * scale) / 2.0,
        (target - height * scale) / 2.0,
    );
    let mut pixmap = resvg::tiny_skia::Pixmap::new(
        u32::from(ProviderIconRef::PIXEL_SIZE),
        u32::from(ProviderIconRef::PIXEL_SIZE),
    )
    .ok_or(ProviderIconError::InvalidPng)?;
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    pixmap.encode_png().map_err(|_| ProviderIconError::InvalidPng)
}

fn validate_svg(source: &str) -> Result<(), ProviderIconError> {
    let document = resvg::usvg::roxmltree::Document::parse(source)
        .map_err(|_| ProviderIconError::InvalidSvg)?;
    let url_pattern =
        Regex::new(r"(?i)url\s*\(\s*([^)]*)\s*\)").map_err(|_| ProviderIconError::InvalidSvg)?;
    for node in document.descendants().filter(resvg::usvg::roxmltree::Node::is_element) {
        let tag = node.tag_name().name();
        if ["script", "foreignobject", "image"]
            .iter()
            .any(|blocked| tag.eq_ignore_ascii_case(blocked))
        {
            return Err(ProviderIconError::InvalidSvg);
        }
        for attribute in node.attributes() {
            let name = attribute.name();
            let value = attribute.value();
            if name.len() > 2 && name[..2].eq_ignore_ascii_case("on") {
                return Err(ProviderIconError::InvalidSvg);
            }
            if name.eq_ignore_ascii_case("href") && !value.trim().starts_with('#') {
                return Err(ProviderIconError::InvalidSvg);
            }
            if value.to_ascii_lowercase().contains("@import")
                || url_pattern.captures_iter(value).any(|capture| {
                    !capture
                        .get(1)
                        .map(|target| target.as_str().trim_matches([' ', '\t', '\'', '"']))
                        .is_some_and(|target| target.starts_with('#'))
                })
            {
                return Err(ProviderIconError::InvalidSvg);
            }
        }
        if tag.eq_ignore_ascii_case("style") {
            let style = node.text().unwrap_or_default();
            if style.to_ascii_lowercase().contains("@import")
                || url_pattern.captures_iter(style).any(|capture| {
                    !capture
                        .get(1)
                        .map(|target| target.as_str().trim_matches([' ', '\t', '\'', '"']))
                        .is_some_and(|target| target.starts_with('#'))
                })
            {
                return Err(ProviderIconError::InvalidSvg);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{ProviderIconError, rasterize_provider_svg};

    #[test]
    fn rasterizes_safe_svg_to_fixed_transparent_png() {
        let png = rasterize_provider_svg(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="12"><rect width="24" height="12" fill="currentColor"/></svg>"#,
        )
        .expect("safe provider icon");
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
    }

    #[test]
    fn rejects_active_and_external_svg_content() {
        for svg in [
            br#"<svg xmlns="http://www.w3.org/2000/svg"><script>alert(1)</script></svg>"#.as_slice(),
            br#"<svg xmlns="http://www.w3.org/2000/svg"><image href="https://example.test/a.png"/></svg>"#.as_slice(),
            br#"<svg xmlns="http://www.w3.org/2000/svg"><rect onclick="x()"/></svg>"#.as_slice(),
        ] {
            assert!(matches!(
                rasterize_provider_svg(svg),
                Err(ProviderIconError::InvalidSvg)
            ));
        }
    }
}
