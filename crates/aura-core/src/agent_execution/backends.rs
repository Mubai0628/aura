//! Execution backend composition; domain adapters retain their actual invocation.
use super::*;
impl AgentExecution {
    pub fn broker_backend(
        self: &Arc<Self>,
        call: &CallContext,
    ) -> anyhow::Result<Arc<dyn aura_backend_api::ExecutionBackend>> {
        if call.operation.content.action_kind.starts_with("memory.") {
            return memory::backend(self, call);
        }
        if call.operation.content.action_kind.starts_with("git.") {
            return git::backend(self, call);
        }
        if call.operation.content.action_kind.starts_with("browser.") {
            return browser::backend(Arc::clone(self), call);
        }
        let content = &call.operation.content;
        if mcp_tools::is_mcp(content) {
            return Ok(Arc::new(
                aura_backend_core::CoreBroker::new(
                    mcp_tools::descriptor(self.runtime),
                    broker::Port { service: self.clone(), call: call.clone(), commands: None },
                )
                .map_err(|e| anyhow::anyhow!("{e:?}"))?,
            ));
        }
        if matches!(
            content.action_kind.as_str(),
            "web.search" | "web.fetch" | "web.read" | "http.request" | "http.download"
        ) || aura_security_contract::package_http(
            &content.source,
            &content.action_kind,
            &content.capabilities,
            &content.resources,
        ) {
            Ok(Arc::new(web::WebBackend { service: self.clone(), call: call.clone() }))
        } else {
            Ok(Arc::new(
                aura_backend_core::CoreBroker::new(
                    broker::descriptor(self.runtime),
                    broker::Port { service: self.clone(), call: call.clone(), commands: None },
                )
                .map_err(|e| anyhow::anyhow!("{e:?}"))?,
            ))
        }
    }
    pub(super) fn shell_control_backend(
        self: &Arc<Self>,
        call: &CallContext,
        commands: Arc<crate::command_execution::CommandExecution>,
    ) -> anyhow::Result<Arc<dyn aura_backend_api::ExecutionBackend>> {
        Ok(Arc::new(
            aura_backend_core::CoreBroker::new(
                broker::descriptor(self.runtime),
                broker::Port {
                    service: self.clone(),
                    call: call.clone(),
                    commands: Some(commands),
                },
            )
            .map_err(|e| anyhow::anyhow!("{e:?}"))?,
        ))
    }
}
