use super::*;

#[derive(Clone)]
pub(super) struct ExternalProcessService {
    pub(super) sandbox: SidecarClient,
    pub(super) host_capacity: std::sync::Arc<Semaphore>,
}

#[derive(Clone)]
pub(crate) struct ExternalProcessExecutionService {
    pub(super) requests: mpsc::Sender<ExternalProcessCommand>,
}

impl ExternalProcessExecutionService {
    #[allow(
        dead_code,
        reason = "P04 owns the live typed service entry; P09 adds only its Tool caller"
    )]
    pub(crate) async fn execute_external_process(
        &self,
        plan: ExternalProcessLaunchPlan,
    ) -> std::result::Result<ExternalProcessExecutionOutcome, ExternalProcessExecutionError> {
        let (reply, response) = oneshot::channel();
        self.requests.send(ExternalProcessCommand { plan, reply }).await.map_err(|_| {
            ExternalProcessExecutionError::Unavailable(anyhow::anyhow!(
                "Core external-process service is unavailable"
            ))
        })?;
        response.await.map_err(|_| {
            ExternalProcessExecutionError::Unavailable(anyhow::anyhow!(
                "Core external-process service abandoned the request"
            ))
        })?
    }
}

impl ExternalProcessService {
    pub(super) async fn execute_external_process(
        &self,
        plan: ExternalProcessLaunchPlan,
    ) -> std::result::Result<ExternalProcessExecutionOutcome, ExternalProcessExecutionError> {
        let _host_permit = self.host_capacity.acquire().await.map_err(|_| {
            ExternalProcessExecutionError::Unavailable(anyhow::anyhow!(
                "Sandbox Host capacity was closed"
            ))
        })?;
        self.sandbox.request(method::SANDBOX_EXECUTE_EXTERNAL, &plan).await.map_err(|error| {
            match error.downcast::<SidecarRemoteError>() {
                Ok(remote) => ExternalProcessExecutionError::Rejected(remote.envelope),
                Err(error) => ExternalProcessExecutionError::Unavailable(error),
            }
        })
    }
}

pub(super) struct ExternalProcessCommand {
    pub(super) plan: ExternalProcessLaunchPlan,
    pub(super) reply: oneshot::Sender<
        std::result::Result<ExternalProcessExecutionOutcome, ExternalProcessExecutionError>,
    >,
}

pub(super) async fn run_external_process_service(
    service: ExternalProcessService,
    mut requests: mpsc::Receiver<ExternalProcessCommand>,
) {
    while let Some(ExternalProcessCommand { plan, reply }) = requests.recv().await {
        let _ = reply.send(service.execute_external_process(plan).await);
    }
}
