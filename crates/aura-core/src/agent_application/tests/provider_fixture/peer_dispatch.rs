use super::*;

pub(in crate::agent_application::tests::provider_fixture) fn latest_user(body: &Value) -> &str {
    body["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .rev()
        .filter(|message| message["role"] == "user")
        .filter_map(|message| message["content"].as_str())
        .find(|text| !text.contains("Current-Task-State"))
        .unwrap_or_default()
}

pub(in crate::agent_application::tests::provider_fixture) fn receiver_after_effect(
    body: &Value,
) -> bool {
    is_receiver(body)
        && body["messages"].as_array().into_iter().flatten().any(|m| m["role"] == "tool")
}

fn is_receiver(body: &Value) -> bool {
    latest_user(body).contains("\"peer_task\"")
}

pub(super) fn reply(
    body: &Value,
    call: &impl Fn(&str, Value, usize) -> Value,
    disconnect: bool,
) -> Value {
    let user = latest_user(body);
    let mut tools = body["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .rev()
        .take_while(|message| message["role"] != "user" || message["content"] != user)
        .filter(|message| message["role"] == "tool")
        .map(|m| {
            serde_json::from_str::<Value>(m["content"].as_str().unwrap_or("{}")).unwrap_or_default()
        });
    let invoke = |tool: &str, arguments: Value, n| {
        let available =
            aura_tool_registry::agent_release::release().expect("release").entry(tool).is_some();
        let invocation = if available && super::super::scenario::offers(body, tool) {
            call(tool, arguments, n)
        } else {
            call("tools.call", json!({"tool":tool,"arguments":arguments}), n)
        };
        json!({"role":"assistant","tool_calls":[invocation]})
    };
    if let Some(delivery) = user.strip_prefix("B5 discover ") {
        let results = body["messages"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|message| message["role"] == "tool")
            .filter_map(|message| {
                serde_json::from_str::<Value>(message["content"].as_str().unwrap_or("{}")).ok()
            })
            .collect::<Vec<_>>();
        if results.is_empty() {
            return invoke("peers.status", json!({"delivery_id":delivery,"tool_call_id":null}), 0);
        }
        let sources = results[0]["outcome"]["payload"]["value"]["deliveries"][0]["sources"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let event = sources
            .iter()
            .max_by_key(|source| source["source_sequence"].as_u64())
            .and_then(|source| source["event_id"].as_str())
            .expect("a delivered source");
        let max_chars = if sources.len() == 1 { 8000 } else { 200 };
        if results.len() == 1 {
            return invoke(
                "peers.read_source",
                json!({"delivery_id":delivery,"event_id":event,"offset":0,"max_chars":max_chars}),
                0,
            );
        }
        let page = &results.last().expect("read page")["outcome"]["payload"]["value"];
        if let Some(next) = page["next_offset"].as_u64() {
            return invoke(
                "peers.read_source",
                json!({"delivery_id":delivery,"event_id":event,"offset":next,"max_chars":max_chars}),
                0,
            );
        }
        return json!({"role":"assistant","content":page["text"].as_str().unwrap_or_default()});
    } else if let Some(args) = user.strip_prefix("B5 source ") {
        if tools.next().is_none() {
            return invoke(
                "peers.read_source",
                serde_json::from_str(args).expect("source fixture"),
                0,
            );
        }
    } else if let Some(args) = user.strip_prefix("B5 ask ") {
        if tools.next().is_none() {
            return invoke("peers.ask", serde_json::from_str(args).expect("question fixture"), 0);
        }
    } else if let Some(delivery) = user.strip_prefix("B4 reconcile ") {
        if tools.next().is_none() {
            let args = delivery
                .strip_prefix("call:")
                .map_or_else(|| json!({"delivery_id":delivery}), |id| json!({"tool_call_id":id}));
            return invoke("peers.result", args, 0);
        }
    } else if user == "B4 user authorizes a summary for B" {
        match tools.next() {
            None => return invoke("peers.list", json!({}), 0),
            Some(last) if tools.next().is_none() => {
                let directory = &last["outcome"]["payload"]["value"];
                if directory["peers"][0]["session_id"].is_string() {
                    return invoke(
                        "peers.dispatch",
                        json!({
                            "graph_id":directory["graph_id"], "graph_revision":directory["graph_revision"],
                            "targets":[directory["peers"][0]["session_id"]], "task":"B4 peer: summarize only"
                        }),
                        1,
                    );
                }
            }
            _ => {}
        }
    }
    if disconnect && is_receiver(body) && tools.next().is_none() {
        return invoke(
            "fs.write",
            json!({"path":"peer-effect.txt","text":"one real receiver write","expected":{"expected_kind":"absent"}}),
            0,
        );
    }
    let peer = body["messages"].as_array().into_iter().flatten().any(|message| {
        message["content"].as_str().is_some_and(|text| text.contains("B4 peer: summarize only"))
    });
    json!({"role":"assistant","content":if is_receiver(body) && user.contains("B5 example question") { "B5 once-only answer" } else if peer { "B4 summary result" } else { "B4 ordinary answer" }})
}
