//! A plugin's panel acting in the conversation it was opened in, through Core's own routes: its
//! public state, what happened since a cursor, a message sent for the user and a stop. A call is
//! refused, with the reason, unless the plugin is trusted, switched on and declared what the call
//! needs; a message goes the composer's way, is marked as the plugin's, and the same key never
//! starts a second turn. Only the Provider is a fixture.
use super::*;
use provider_fixture::Scenario;

const ALL: &[&str] = &["session.read", "session.events", "session.send", "session.control"];

/// A plugin in development with a panel and the capabilities named, added and trusted through
/// Settings → 插件's own routes.
async fn trusted_plugin(
    host: &crate::r03_tests::Fixture,
    runtime: &crate::RuntimeState,
    capabilities: &[&str],
) {
    let folder = host.root.path().join("plugin-sources").join("usage-panel");
    std::fs::create_dir_all(folder.join("panel")).expect("panel folder");
    let declared = capabilities.iter().map(|name| format!("\"{name}\"")).collect::<Vec<_>>();
    std::fs::write(
        folder.join("aura-plugin.toml"),
        format!(
            "schema_version = 2\nname = \"usage-panel\"\nversion = \"0.1.0\"\n\
             description = \"本会话用量\"\ncapabilities = [{}]\n\n[panel]\ntitle = \"本会话用量\"\n",
            declared.join(", ")
        ),
    )
    .expect("manifest");
    std::fs::write(folder.join("panel/index.html"), "<!doctype html><title>用量</title>")
        .expect("page");
    let listed = crate::r03_tests::success(runtime, "agent.plugins.list", json!({})).await;
    crate::r03_tests::success(
        runtime,
        "agent.plugins.add_development",
        json!({"path": folder, "expected_revision": listed["revision"]}),
    )
    .await;
    let preview =
        crate::r03_tests::success(runtime, "agent.plugins.preview", json!({"name": "usage-panel"}))
            .await;
    let listed = crate::r03_tests::success(runtime, "agent.plugins.list", json!({})).await;
    crate::r03_tests::success(
        runtime,
        "agent.plugins.trust",
        json!({"name": "usage-panel", "expected_fingerprint": preview["fingerprint"], "expected_revision": listed["revision"]}),
    )
    .await;
}

fn services(runtime: &crate::RuntimeState) -> &crate::RuntimeServices {
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    services
}

pub(super) fn call(plugin: &str, session: SessionId, more: Value) -> Value {
    let mut request = json!({"plugin": plugin, "session_id": session});
    request.as_object_mut().expect("object").extend(more.as_object().expect("object").clone());
    request
}

pub(super) async fn refused(runtime: &crate::RuntimeState, method: &str, request: Value) -> String {
    let answer = crate::r03_tests::request(runtime, method, request).await;
    answer["error"]["message"].as_str().unwrap_or_else(|| panic!("not refused: {answer}")).into()
}

pub(super) async fn history(runtime: &crate::RuntimeState, session: SessionId) -> Value {
    crate::r03_tests::success(
        runtime,
        "agent.history",
        json!({"session_id": session, "before_turn_task_id": null, "within_turn": null, "limit": 10}),
    )
    .await
}

/// The conversation's turns once the newest has ended.
pub(super) async fn ended_turns(runtime: &crate::RuntimeState, session: SessionId) -> Value {
    tokio::time::timeout(std::time::Duration::from_secs(45), async {
        loop {
            let shown = history(runtime, session).await;
            let state = &shown["turns"][0]["turn"]["state"]["state"];
            if ["completed", "failed", "cancelled"].iter().any(|ended| state == ended) {
                break shown["turns"].clone();
            }
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        }
    })
    .await
    .expect("the turn ended")
}

pub(super) fn kinds(events: &Value) -> Vec<String> {
    events["events"]
        .as_array()
        .expect("events")
        .iter()
        .map(|event| event["event"]["kind"].as_str().expect("kind").to_owned())
        .collect()
}

#[tokio::test]
async fn l17_a_panel_reads_its_conversation_and_sends_a_message_marked_as_the_plugins() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Text).await;
    let (runtime, session, model) = prepare(&host, &provider).await;
    crate::r03_tests::success(
        &runtime,
        "models.session.set_binding",
        json!({"expected_revision": "0", "session_id": session, "model_profile_id": model}),
    )
    .await;
    trusted_plugin(&host, &runtime, ALL).await;

    let state = crate::r03_tests::success(
        &runtime,
        "agent.plugin_session.state",
        call("usage-panel", session, json!({})),
    )
    .await;
    assert_eq!(state["title"], "Agent");
    assert_eq!(state["activity"], "idle");
    assert_eq!(state["model_name"], "Fixture");
    assert_eq!(state["usage"]["requests"], 0);
    // The first read of the events says only where now is.
    let first = crate::r03_tests::success(
        &runtime,
        "agent.plugin_session.events",
        call("usage-panel", session, json!({})),
    )
    .await;
    assert_eq!(first["events"], json!([]));
    let cursor = first["cursor"].clone();

    let send =
        |text: &str| call("usage-panel", session, json!({"text": text, "dedupe_key": "c2c-1"}));
    let sent =
        crate::r03_tests::success(&runtime, "agent.plugin_session.send", send("本会话用了多少？"))
            .await;
    assert_eq!(sent["repeated"], false);
    assert!(sent["message_id"].is_string(), "{sent}");
    let turns = ended_turns(&runtime, session).await;
    assert_eq!(turns[0]["turn"]["state"]["state"], "completed");
    let user = &turns[0]["user_messages"][0];
    assert_eq!(user["text"], "本会话用了多少？");
    assert_eq!(user["sender"], json!({"plugin": "usage-panel"}), "{user}");

    // The same key again is the same message: no second turn. Another text under it is refused.
    let again =
        crate::r03_tests::success(&runtime, "agent.plugin_session.send", send("本会话用了多少？"))
            .await;
    assert_eq!(
        (again["repeated"].clone(), again["message_id"].clone()),
        (json!(true), json!(null))
    );
    let reused = refused(&runtime, "agent.plugin_session.send", send("另一句")).await;
    assert!(reused.starts_with("plugin_session.dedupe_key_reused"), "{reused}");
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert_eq!(history(&runtime, session).await["turns"].as_array().expect("turns").len(), 1);

    let events = crate::r03_tests::success(
        &runtime,
        "agent.plugin_session.events",
        call("usage-panel", session, json!({"after": cursor})),
    )
    .await;
    assert_eq!(
        kinds(&events),
        ["turn_started", "step_started", "step_ended", "turn_ended"],
        "{events}"
    );
    // The step's request and its answer, with what the Provider said it used.
    let (step, answered) = (&events["events"][1]["event"], &events["events"][2]["event"]);
    assert_eq!(step["turn_task_id"], events["events"][0]["event"]["turn_task_id"], "{events}");
    assert_eq!(answered["model_step_id"], step["model_step_id"], "{events}");
    assert_eq!(answered["usage"]["output_tokens"], 20, "{events}");
    assert!(answered["usage"]["input_tokens"].as_u64().expect("input") > 0, "{events}");
    let ended = &events["events"][3]["event"];
    assert_eq!(ended["outcome"], "completed");
    assert!(!ended["final_reply"].as_str().expect("final reply").is_empty(), "{ended}");
    let state = crate::r03_tests::success(
        &runtime,
        "agent.plugin_session.state",
        call("usage-panel", session, json!({})),
    )
    .await;
    assert_eq!(state["activity"], "idle");
    assert!(state["usage"]["requests"].as_u64().expect("requests") >= 1, "{state}");
    assert!(state["cursor"].as_u64() >= events["cursor"].as_u64(), "{state}");
    // Nothing credential-like or from the workspace is in the state.
    assert!(!state.to_string().contains("waiting-workspace"), "{state}");
}

#[tokio::test]
async fn l17_a_panel_is_refused_what_its_plugin_did_not_declare_or_cannot_use_now() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Text).await;
    let (runtime, session, _) = prepare(&host, &provider).await;
    trusted_plugin(&host, &runtime, &["session.read"]).await;
    crate::r03_tests::success(
        &runtime,
        "agent.plugin_session.state",
        call("usage-panel", session, json!({})),
    )
    .await;
    for (method, extra, capability) in [
        ("agent.plugin_session.events", json!({}), "session.events"),
        ("agent.plugin_session.send", json!({"text": "hi"}), "session.send"),
        ("agent.plugin_session.stop", json!({}), "session.control"),
    ] {
        let message = refused(&runtime, method, call("usage-panel", session, extra)).await;
        assert!(message.starts_with("plugin_session.capability_missing"), "{method}: {message}");
        assert!(message.contains(capability), "{method}: {message}");
    }
    let ghost =
        refused(&runtime, "agent.plugin_session.state", call("ghost", session, json!({}))).await;
    assert!(ghost.starts_with("plugin_session.plugin_missing"), "{ghost}");
    let elsewhere = refused(
        &runtime,
        "agent.plugin_session.state",
        call("usage-panel", SessionId::new(), json!({})),
    )
    .await;
    assert!(elsewhere.starts_with("plugin_session.session_missing"), "{elsewhere}");
    let listed = crate::r03_tests::success(&runtime, "agent.plugins.list", json!({})).await;
    crate::r03_tests::success(
        &runtime,
        "agent.plugins.untrust",
        json!({"name": "usage-panel", "expected_revision": listed["revision"]}),
    )
    .await;
    let untrusted =
        refused(&runtime, "agent.plugin_session.state", call("usage-panel", session, json!({})))
            .await;
    assert!(untrusted.starts_with("plugin_session.not_ready"), "{untrusted}");
    assert!(untrusted.contains("信任"), "{untrusted}");
    assert!(provider.requests.lock().expect("requests").is_empty(), "nothing was sent");
}

#[tokio::test]
async fn l17_main_is_given_the_panel_to_show_and_keeps_its_problems_in_the_plugins_log() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Text).await;
    let (runtime, session, _) = prepare(&host, &provider).await;
    trusted_plugin(&host, &runtime, &["session.read"]).await;
    let folder = host.root.path().join("plugin-sources").join("usage-panel");
    let listed_panel = |listed: &Value| {
        let plugins = listed["plugins"].as_array().expect("plugins");
        plugins.iter().find(|p| p["name"] == "usage-panel").expect("listed")["panel"].clone()
    };

    // The sidebar's "+" offers the panel by its title.
    let listed = crate::r03_tests::success(&runtime, "agent.plugins.list", json!({})).await;
    assert_eq!(listed_panel(&listed), "本会话用量");
    let panel = crate::r03_tests::success(
        &runtime,
        "agent.plugin_session.panel",
        call("usage-panel", session, json!({})),
    )
    .await;
    assert_eq!(panel["title"], "本会话用量");
    assert_eq!(panel["entry"], "panel/index.html");
    assert_eq!(panel["network"], json!([]));
    assert_eq!(panel["capabilities"], json!(["session.read"]));
    let given = std::fs::canonicalize(panel["folder"].as_str().expect("folder")).expect("folder");
    assert_eq!(given, std::fs::canonicalize(&folder).expect("folder"));

    // A problem Main saw is kept in the plugin's log, under the panel's page.
    crate::r03_tests::success(
        &runtime,
        "agent.plugin_session.log",
        call("usage-panel", session, json!({"text": "入口页没有打开：ERR_FILE_NOT_FOUND"})),
    )
    .await;
    let (entries, _) = services(&runtime).plugins.recent_logs("usage-panel", 10).expect("logs");
    let kept = entries.iter().find(|e| e.text.contains("ERR_FILE_NOT_FOUND")).expect("kept");
    let panel_source = aura_agent_domain::PluginLogSource::Panel;
    assert_eq!((kept.source, kept.part.as_str()), (panel_source, "panel/index.html"));

    // The page it names is gone: the panel is no longer offered, and Main is told why.
    std::fs::remove_file(folder.join("panel/index.html")).expect("page removed");
    // The usable plugins are read again after two seconds.
    tokio::time::sleep(std::time::Duration::from_millis(2_200)).await;
    let listed = crate::r03_tests::success(&runtime, "agent.plugins.list", json!({})).await;
    assert_eq!(listed_panel(&listed), Value::Null);
    let unusable =
        refused(&runtime, "agent.plugin_session.panel", call("usage-panel", session, json!({})))
            .await;
    assert!(unusable.starts_with("plugin_session.panel_unusable"), "{unusable}");
    assert!(unusable.contains("panel/index.html"), "{unusable}");

    let listed = crate::r03_tests::success(&runtime, "agent.plugins.list", json!({})).await;
    crate::r03_tests::success(
        &runtime,
        "agent.plugins.untrust",
        json!({"name": "usage-panel", "expected_revision": listed["revision"]}),
    )
    .await;
    let untrusted = refused(
        &runtime,
        "agent.plugin_session.log",
        call("usage-panel", session, json!({"text": "after"})),
    )
    .await;
    assert!(untrusted.starts_with("plugin_session.not_ready"), "{untrusted}");
}

#[tokio::test]
async fn l17_a_panel_sees_an_approval_waiting_and_stops_the_turn() {
    let host = crate::r03_tests::Fixture::new();
    // ReadOnly asks before the write, so the turn waits for an answer.
    let provider = Provider::new(true).await;
    let (runtime, session, model) = prepare(&host, &provider).await;
    trusted_plugin(&host, &runtime, ALL).await;
    let first = crate::r03_tests::success(
        &runtime,
        "agent.plugin_session.events",
        call("usage-panel", session, json!({})),
    )
    .await;
    let started = start(&runtime, session, model, AgentTaskMode::Normal).await;
    let request = AgentRunRequest { session_id: session, agent_run_id: started.run.agent_run_id };
    let waiting =
        wait(&runtime, &request, |v| matches!(v.turn.state, TurnTaskState::AwaitingApproval)).await;
    let events = crate::r03_tests::success(
        &runtime,
        "agent.plugin_session.events",
        call("usage-panel", session, json!({"after": first["cursor"]})),
    )
    .await;
    let step = ["step_started", "step_ended", "tool_call"];
    let mut expected = vec!["turn_started"];
    expected.extend(step.iter().cycle().take(9));
    expected.push("approval_waiting");
    assert_eq!(kinds(&events), expected, "{events}");
    // Each answer's call, named as the tool it is: the write is found and loaded, then asked for.
    let listed = events["events"].as_array().expect("events");
    let calls = listed.iter().filter(|event| event["event"]["kind"] == "tool_call");
    let named = calls.map(|event| event["event"]["tool"].clone()).collect::<Vec<_>>();
    assert_eq!(named, [json!("tools.search"), json!("tools.load"), json!("fs.write")], "{events}");
    let tool = &listed[9]["event"];
    assert_eq!(tool["turn_task_id"], json!(started.turn.turn_task_id), "{events}");
    assert_eq!(tool["model_step_id"], listed[8]["event"]["model_step_id"], "{events}");
    assert!(tool["tool_call_id"].is_string(), "{events}");
    assert_eq!(listed[10]["event"]["approval_id"], json!(waiting.pending_approval_ids[0]));
    assert_eq!(listed[10]["event"]["turn_task_id"], json!(started.turn.turn_task_id));
    let state = crate::r03_tests::success(
        &runtime,
        "agent.plugin_session.state",
        call("usage-panel", session, json!({})),
    )
    .await;
    assert_eq!(
        (state["activity"].clone(), state["waiting_for"].clone()),
        (json!("waiting"), json!("approval"))
    );

    let stopped = crate::r03_tests::success(
        &runtime,
        "agent.plugin_session.stop",
        call("usage-panel", session, json!({})),
    )
    .await;
    assert_eq!(stopped["stopped"], true);
    wait(&runtime, &request, |v| v.turn.state.terminal()).await;
    let after = crate::r03_tests::success(
        &runtime,
        "agent.plugin_session.events",
        call("usage-panel", session, json!({"after": events["cursor"]})),
    )
    .await;
    assert_eq!(kinds(&after), ["turn_ended"], "{after}");
    assert_eq!(after["events"][0]["event"]["outcome"], "cancelled");
    let idle = crate::r03_tests::success(
        &runtime,
        "agent.plugin_session.stop",
        call("usage-panel", session, json!({})),
    )
    .await;
    assert_eq!(idle["stopped"], false, "nothing under way to stop");
}

// A plugin's notice and its status entry are checked before Main shows them: the capability for
// each, one line of text short enough for where it shows, and a plugin still usable. Neither
// belongs to a conversation; a status entry without text is cleared.
#[tokio::test]
async fn l17_a_plugins_notice_and_status_entry_are_checked_before_main_shows_them() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Text).await;
    let (runtime, _session, _model) = prepare(&host, &provider).await;
    trusted_plugin(&host, &runtime, &["status"]).await;

    let shown = crate::r03_tests::success(
        &runtime,
        "agent.plugin_session.status",
        json!({"plugin": "usage-panel", "text": " 已连接 ", "tone": "success"}),
    )
    .await;
    assert_eq!(shown, json!({"plugin": "usage-panel", "text": "已连接", "tone": "success"}));
    let cleared = crate::r03_tests::success(
        &runtime,
        "agent.plugin_session.status",
        json!({"plugin": "usage-panel"}),
    )
    .await;
    assert_eq!(cleared, json!({"plugin": "usage-panel", "tone": "info"}));
    let long = refused(
        &runtime,
        "agent.plugin_session.status",
        json!({"plugin": "usage-panel", "text": "字".repeat(41)}),
    )
    .await;
    assert!(long.starts_with("plugin_session.status_invalid"), "{long}");
    let undeclared = refused(
        &runtime,
        "agent.plugin_session.notify",
        json!({"plugin": "usage-panel", "text": "你好"}),
    )
    .await;
    assert!(undeclared.starts_with("plugin_session.capability_missing"), "{undeclared}");
    assert!(undeclared.contains("notify"), "{undeclared}");

    // Declaring `notify` changes the manifest: the plugin is trusted again at what it is now.
    let folder = host.root.path().join("plugin-sources").join("usage-panel");
    let manifest = std::fs::read_to_string(folder.join("aura-plugin.toml")).expect("manifest");
    std::fs::write(
        folder.join("aura-plugin.toml"),
        manifest.replace("[\"status\"]", "[\"notify\", \"status\"]"),
    )
    .expect("manifest");
    tokio::time::sleep(std::time::Duration::from_millis(2_200)).await;
    trusted_plugin(&host, &runtime, &["notify", "status"]).await;
    let notice = crate::r03_tests::success(
        &runtime,
        "agent.plugin_session.notify",
        json!({"plugin": "usage-panel", "text": "已同步 3 条消息"}),
    )
    .await;
    assert_eq!(notice, json!({"plugin": "usage-panel", "text": "已同步 3 条消息", "tone": "info"}));
    for text in ["", "第一行\n第二行"] {
        let wrong = refused(
            &runtime,
            "agent.plugin_session.notify",
            json!({"plugin": "usage-panel", "text": text}),
        )
        .await;
        assert!(wrong.starts_with("plugin_session.notice_invalid"), "{wrong}");
    }
    let loud = crate::r03_tests::request(
        &runtime,
        "agent.plugin_session.notify",
        json!({"plugin": "usage-panel", "text": "你好", "tone": "loud"}),
    )
    .await;
    assert!(loud["error"].is_object(), "a tone Aura does not know is refused: {loud}");

    let listed = crate::r03_tests::success(&runtime, "agent.plugins.list", json!({})).await;
    crate::r03_tests::success(
        &runtime,
        "agent.plugins.untrust",
        json!({"name": "usage-panel", "expected_revision": listed["revision"]}),
    )
    .await;
    let untrusted = refused(
        &runtime,
        "agent.plugin_session.notify",
        json!({"plugin": "usage-panel", "text": "你好"}),
    )
    .await;
    assert!(untrusted.starts_with("plugin_session.not_ready"), "{untrusted}");
}
