mod activity;
mod cancel;
pub(super) use cancel::dispatch as cancel;
mod tool;
pub(super) use tool::dispatch as tool;
mod session_write;
mod workspace;
pub(super) use workspace::dispatch as workspace;
mod security;
pub(super) use security::dispatch as security;
mod model;
pub(super) use model::dispatch as model;
mod agent;
mod git;
pub(super) use git::dispatch as git;
mod package;
pub(super) use package::dispatch as package;
mod browser_settings;
mod media;
mod memory;
mod ui_settings;
pub(super) fn is_management(method: &str) -> bool {
    aura_ipc::routes::core_route(method).is_some_and(|route| {
        route.auth == aura_ipc::routes::RouteAuth::AuthenticatedMain
            && matches!(
                route.owner,
                aura_ipc::routes::RouteOwner::Package | aura_ipc::routes::RouteOwner::Memory
            )
    })
}
pub(super) async fn management(
    state: &crate::RuntimeState,
    queue: &crate::AsyncPriorityQueue,
    request: &aura_ipc::JsonRpcRequest,
) -> anyhow::Result<aura_runtime_contract::IpcRequestTerminal> {
    if aura_ipc::routes::owned_by(&request.method, aura_ipc::routes::RouteOwner::Memory) {
        memory::dispatch(state, queue, request).await
    } else {
        package(state, queue, request).await
    }
}
mod shell;
pub(super) use agent::dispatch as agent;
