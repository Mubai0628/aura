use super::*;

#[derive(Clone)]
pub(super) struct SidecarClient {
    pub(super) requests: mpsc::Sender<SidecarRequest>,
    pub(super) _events: broadcast::Sender<Value>,
}

impl SidecarClient {
    pub(super) async fn request<T: Serialize, R: DeserializeOwned>(
        &self,
        method_name: &'static str,
        input: &T,
    ) -> Result<R> {
        let params = serde_json::to_value(input)?;
        let (reply_tx, reply_rx) = oneshot::channel();
        self.requests
            .send(SidecarRequest { method_name, params, reply: reply_tx })
            .await
            .context("security Sidecar request channel closed")?;
        let value = reply_rx
            .await
            .context("security Sidecar request was abandoned")?
            .map_err(|envelope| anyhow::Error::new(SidecarRemoteError { envelope }))?;
        serde_json::from_value(value).context("security Sidecar response contract mismatch")
    }

    pub(super) async fn ping(&self) -> Result<()> {
        let _: Value = self.request(method::SIDECAR_PING, &json!({})).await?;
        Ok(())
    }
}

pub(super) struct SidecarRequest {
    pub(super) method_name: &'static str,
    pub(super) params: Value,
    pub(super) reply: oneshot::Sender<Result<Value, IpcErrorEnvelope>>,
}

pub(super) enum PendingRequest {
    Caller(oneshot::Sender<Result<Value, IpcErrorEnvelope>>),
    Heartbeat,
}

pub(super) async fn start_sidecar(
    component_job: &ComponentJob,
    options: &CoreOptions,
    release: &CoreReleaseContext,
    component: &SidecarReleaseIdentity,
    failure_tx: broadcast::Sender<String>,
) -> Result<SidecarClient> {
    lifecycle::validate_image(options, component)?;
    let runtime_instance_id = RuntimeInstanceId::new();
    let pipe_name = random_sidecar_pipe_name(
        &release.release_descriptor.bundle_namespace,
        options.runtime_instance_id,
        runtime_instance_id,
    )?;
    let mut server = create_secure_byte_pipe_server(&pipe_name, true)
        .context("failed to create Logon SID-only security Sidecar pipe")?;
    let mut secret_bytes = [0_u8; 32];
    getrandom::fill(&mut secret_bytes).context("failed to create Sidecar bootstrap secret")?;
    let secret = Zeroizing::new(secret_bytes);
    let working_directory = component
        .executable
        .parent()
        .context("security Sidecar executable has no parent directory")?;
    let arguments = sidecar_arguments(options, runtime_instance_id, &pipe_name);
    let process = component_job
        .spawn_sidecar(&component.executable, &arguments, working_directory, &secret)
        .with_context(|| {
            format!("failed to start {:?} inside Component Job", component.process_kind)
        })?;
    let expected_process =
        process.identity().context("failed to snapshot retained Sidecar process handle")?;
    time::timeout(STARTUP_TIMEOUT, server.connect())
        .await
        .context("security Sidecar pipe connect timed out")??;
    validate_named_pipe_client(&server, &expected_process)
        .context("security Sidecar pipe client identity mismatch")?;
    authenticate_sidecar(
        &mut server,
        options,
        release,
        component,
        runtime_instance_id,
        &pipe_name,
        &expected_process,
        &secret,
    )
    .await?;
    let (request_tx, request_rx) = mpsc::channel(64);
    let (events, _) = broadcast::channel(256);
    tokio::spawn(run_sidecar_actor(
        component.process_kind,
        runtime_instance_id,
        server,
        process,
        request_rx,
        events.clone(),
        failure_tx,
    ));
    Ok(SidecarClient { requests: request_tx, _events: events })
}

pub(super) fn sidecar_arguments(
    options: &CoreOptions,
    runtime_instance_id: RuntimeInstanceId,
    pipe_name: &str,
) -> Vec<String> {
    let mut arguments = vec![
        format!("--pipe-name={pipe_name}"),
        format!("--parent-pid={}", std::process::id()),
        format!("--parent-runtime-instance-id={}", options.runtime_instance_id),
        format!("--runtime-instance-id={runtime_instance_id}"),
        format!("--component-manifest={}", options.component_manifest_path.display()),
    ];
    if options.allow_dev_component_mismatch {
        arguments.push("--allow-dev-component-mismatch".to_owned());
    }
    arguments
}

pub(super) fn random_sidecar_pipe_name(
    namespace: &aura_runtime_contract::BundleNamespace,
    parent: RuntimeInstanceId,
    child: RuntimeInstanceId,
) -> Result<String> {
    let mut random = [0_u8; 16];
    getrandom::fill(&mut random).context("failed to generate Sidecar pipe randomness")?;
    let suffix = random.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
    Ok(format!(r"\\.\pipe\Aura\{}\{parent}\sidecar\{child}\{suffix}", namespace.object_name_hash()))
}

pub(super) async fn run_sidecar_actor(
    process_kind: RuntimeProcessKind,
    runtime_instance_id: RuntimeInstanceId,
    mut pipe: NamedPipeServer,
    process: ManagedProcess,
    mut requests: mpsc::Receiver<SidecarRequest>,
    events: broadcast::Sender<Value>,
    failures: broadcast::Sender<String>,
) {
    let label = format!("{process_kind:?}/{runtime_instance_id}");
    let outcome = run_actor_loop(&mut pipe, &process, &mut requests, &events).await;
    let reason = match outcome {
        Ok(()) => format!("{label} command channel ended unexpectedly"),
        Err(error) => format!("{label}: {error:#}"),
    };
    append_core_log("security_sidecar_failed", json!({ "reason": reason }));
    let _ = process.terminate(1);
    let _ = failures.send(reason);
}

pub(super) async fn run_actor_loop(
    pipe: &mut NamedPipeServer,
    process: &ManagedProcess,
    requests: &mut mpsc::Receiver<SidecarRequest>,
    events: &broadcast::Sender<Value>,
) -> Result<()> {
    let mut pending = BTreeMap::<CorrelationId, PendingRequest>::new();
    let mut heartbeat_pending = None;
    let mut last_seen = time::Instant::now();
    let mut heartbeat = time::interval(HEARTBEAT_INTERVAL);
    heartbeat.set_missed_tick_behavior(time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            request = requests.recv() => {
                let request = request.context("security Sidecar request channel closed")?;
                let id = CorrelationId::new();
                write_sidecar_request(pipe, id, request.method_name, request.params).await?;
                pending.insert(id, PendingRequest::Caller(request.reply));
            }
            frame = read_frame(pipe) => {
                let frame = frame?;
                last_seen = time::Instant::now();
                handle_inbound(frame, &mut pending, &mut heartbeat_pending, events)?;
            }
            _ = heartbeat.tick() => {
                if let Some(exit_code) = process.try_exit_code()? {
                    bail!("security Sidecar exited with code {exit_code}");
                }
                if last_seen.elapsed() >= HEARTBEAT_FAILURE_WINDOW {
                    bail!("security Sidecar missed three 2-second liveness windows");
                }
                if heartbeat_pending.is_none() {
                    let id = CorrelationId::new();
                    write_sidecar_request(pipe, id, method::SIDECAR_PING, json!({})).await?;
                    pending.insert(id, PendingRequest::Heartbeat);
                    heartbeat_pending = Some(id);
                }
            }
        }
    }
}

pub(super) async fn write_sidecar_request(
    pipe: &mut NamedPipeServer,
    id: CorrelationId,
    method_name: &str,
    params: Value,
) -> Result<()> {
    let request = JsonRpcRequest {
        jsonrpc: JSON_RPC_VERSION.to_owned(),
        id,
        command_invocation_id: None,
        method: method_name.to_owned(),
        params,
    };
    write_frame(pipe, &WireFrame::Control(serde_json::to_vec(&request)?)).await?;
    Ok(())
}

pub(super) fn handle_inbound(
    frame: WireFrame,
    pending: &mut BTreeMap<CorrelationId, PendingRequest>,
    heartbeat_pending: &mut Option<CorrelationId>,
    events: &broadcast::Sender<Value>,
) -> Result<()> {
    let WireFrame::Control(payload) = frame else {
        bail!("security Sidecar sent an unexpected binary frame");
    };
    match serde_json::from_slice::<JsonRpcMessage>(&payload)? {
        JsonRpcMessage::Response(response) => {
            if !response.has_exactly_one_terminal() {
                bail!("security Sidecar response had no unique terminal");
            }
            let pending_request = pending
                .remove(&response.id)
                .context("security Sidecar responded to an unknown request")?;
            if *heartbeat_pending == Some(response.id) {
                *heartbeat_pending = None;
            }
            let terminal = match response.result {
                Some(result) => Ok(result.value),
                None => {
                    Err(response.error.context("security Sidecar returned malformed failure")?.data)
                }
            };
            if let PendingRequest::Caller(reply) = pending_request {
                let _ = reply.send(terminal);
            } else if let Err(error) = terminal {
                bail!("security Sidecar heartbeat failed: {}", error.code);
            }
        }
        JsonRpcMessage::Notification(JsonRpcNotification { method: name, params, .. })
            if name == method::SANDBOX_EVENT =>
        {
            let _ = events.send(params);
        }
        _ => bail!("security Sidecar sent an unsolicited or unsupported message"),
    }
    Ok(())
}
