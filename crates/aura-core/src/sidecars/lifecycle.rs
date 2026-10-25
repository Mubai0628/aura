use super::*;
use crate::readiness::ReadinessStore;
use aura_projection_contract::BackendLifecycle;

/// One task and one Job per backend. Only the first request starts the process;
/// a failed backend stays failed for this Core lifetime, with no silent replay.
pub(super) fn deferred(
    options: CoreOptions,
    release: CoreReleaseContext,
    component: SidecarReleaseIdentity,
    id: &'static str,
    readiness: ReadinessStore,
) -> (SidecarClient, crate::RuntimeBackgroundTask) {
    let (requests, mut receiver) = mpsc::channel::<SidecarRequest>(64);
    let (events, _) = broadcast::channel(256);
    let public_events = events.clone();
    let task = tokio::spawn(async move {
        let Some(first) = receiver.recv().await else {
            return;
        };
        readiness.update(id, BackendLifecycle::Starting, "backend.starting");
        let (failures, mut failed) = broadcast::channel(8);
        let started = async {
            let job = ComponentJob::create()?;
            let client = start_sidecar(&job, &options, &release, &component, failures).await?;
            let ready: Value = client.request(method::SIDECAR_PING, &json!({})).await?;
            let endpoint = if id == "egress_proxy" {
                let address: std::net::SocketAddr = serde_json::from_value(
                    ready.get("proxy_endpoint").cloned().context("missing proxy endpoint")?,
                )?;
                anyhow::ensure!(address.ip().is_loopback(), "proxy endpoint must be loopback");
                Some(aura_network_control::ordinary_tools::register_control_endpoint(
                    options.runtime_instance_id,
                    address,
                )?)
            } else {
                None
            };
            Ok::<_, anyhow::Error>((job, client, endpoint))
        }
        .await;
        let (_job, client, _endpoint) = match started {
            Ok(value) => value,
            Err(error) => {
                readiness.update(id, BackendLifecycle::Degraded, "backend.start_failed");
                append_core_log(
                    "backend_start_failed",
                    json!({"backend":id,"reason":error.to_string()}),
                );
                drop(first); // closes this request without dispatching or replaying it
                return;
            }
        };
        readiness.update(id, BackendLifecycle::Ready, "backend.ready");
        let mut incoming = client._events.subscribe();
        if client.requests.send(first).await.is_err() {
            readiness.update(id, BackendLifecycle::Degraded, "backend.transport_lost");
            return;
        }
        loop {
            tokio::select! {
                request=receiver.recv()=>{
                    let Some(request)=request else {break;};
                    if client.requests.send(request).await.is_err(){break;}
                }
                event=incoming.recv()=>match event {
                    Ok(event)=>{let _=public_events.send(event);},
                    Err(broadcast::error::RecvError::Lagged(_))=>{break;},
                    Err(broadcast::error::RecvError::Closed)=>{break;},
                },
                _=failed.recv()=>{break;}
            }
        }
        readiness.update(id, BackendLifecycle::Degraded, "backend.transport_lost");
        // Dropping only this Job kills its children. Other backends and Core survive.
    });
    (SidecarClient { requests, _events: events }, crate::RuntimeBackgroundTask(task))
}

pub(super) fn validate_image(
    options: &CoreOptions,
    component: &SidecarReleaseIdentity,
) -> Result<()> {
    if component.protocol_version.is_some_and(|v| {
        v.major != ContractVersion::CURRENT.major || v.minor > ContractVersion::CURRENT.minor
    }) {
        bail!("backend protocol incompatible");
    }
    let bytes = std::fs::read(&component.executable).context("backend executable unavailable")?;
    let actual = match component.content_hash.algorithm {
        aura_domain_common::ContentHashAlgorithm::Sha256 => {
            use sha2::Digest;
            format!("{:x}", sha2::Sha256::digest(&bytes))
        }
        aura_domain_common::ContentHashAlgorithm::Blake3 => {
            blake3::hash(&bytes).to_hex().to_string()
        }
    };
    if actual != component.content_hash.hex && !options.allow_dev_component_mismatch {
        bail!("backend executable does not match release manifest");
    }
    Ok(())
}
