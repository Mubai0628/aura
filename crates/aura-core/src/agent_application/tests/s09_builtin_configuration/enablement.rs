use super::*;

fn has_http(request: &Value) -> bool {
    let entry = aura_tool_registry::agent_release::release()
        .expect("release")
        .entry("http.request")
        .expect("HTTP");
    let wire = aura_model_domain::provider_wire_tool_name("http.request", entry.definition.tool_id);
    request["tools"].as_array().into_iter().flatten().any(|t| t["function"]["name"] == wire)
}

#[tokio::test]
async fn s09_disabled_builtin_is_absent_from_the_next_actual_model_request() {
    let host = crate::r03_tests::Fixture::new();
    let runtime = host.start().await;
    let crate::StartupMode::Ready(s) = &runtime.startup_mode else { panic!("Core ready") };
    expose_http(s).await;
    let provider = Provider::new(false).await;
    let model = profile(&runtime, &provider.url).await;
    let enabled = run_text(&host, &runtime, model, &provider, "enabled-http").await;
    assert!(enabled.iter().any(has_http), "the positive control really exposed HTTP");
    set_instance(s, false, None);
    let disabled = run_text(&host, &runtime, model, &provider, "disabled-http").await;
    assert!(
        disabled.iter().all(|r| !has_http(r)),
        "saved disabled instances cannot reach the next compiled request"
    );
}

#[tokio::test]
async fn s09_upgrade_preserves_disabled_builtin_in_the_next_model_request() {
    let host = crate::r03_tests::Fixture::new();
    let first = host.start().await;
    let crate::StartupMode::Ready(s) = &first.startup_mode else { panic!("Core ready") };
    expose_http(s).await;
    seed_disabled_previous(s);
    drop(first);
    let runtime = host.start().await;
    let crate::StartupMode::Ready(s) = &runtime.startup_mode else {
        let crate::StartupMode::RecoveryRequired { reason } = &runtime.startup_mode else {
            unreachable!()
        };
        panic!("Core ready after real startup upgrade: {reason}")
    };
    let current = aura_tool_registry::agent_release::release()
        .expect("release")
        .entry("http.request")
        .expect("HTTP")
        .instance
        .tool_instance_id;
    assert!(
        !s.persistence
            .tool_authority_snapshot(None, None, None)
            .expect("upgraded authority")
            .instances
            .iter()
            .find(|i| i.tool_instance_id == current)
            .expect("new instance")
            .enabled,
        "upgrade preserves the disabled configuration"
    );
    let provider = Provider::new(false).await;
    let model = profile(&runtime, &provider.url).await;
    let requests = run_text(&host, &runtime, model, &provider, "upgraded-disabled-http").await;
    assert!(requests.iter().all(|r| !has_http(r)), "carried disabled state affects real requests");
}
