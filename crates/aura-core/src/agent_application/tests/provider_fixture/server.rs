use super::*;

impl Provider {
    pub async fn scenario(scenario: Scenario) -> Self {
        Self::serve(scenario, false).await
    }

    /// The scenario, with every compaction answered in shape but past four times the summary
    /// length it was asked for: each pass falls short.
    pub async fn scenario_with_long_compactions(scenario: Scenario) -> Self {
        Self::serve(scenario, true).await
    }

    async fn serve(scenario: Scenario, long_compactions: bool) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("real Provider fixture request and response");
        let url = format!(
            "http://{}/v1",
            listener.local_addr().expect("real Provider fixture request and response")
        );
        let requests = Arc::new(Mutex::new(Vec::new()));
        let saved = requests.clone();
        let raw_requests = Arc::new(Mutex::new(Vec::new()));
        let saved_raw = raw_requests.clone();
        let release_window = Arc::new(Notify::new());
        let release = release_window.clone();
        let parked = Arc::new(Notify::new());
        let entered_park = parked.clone();
        let task = tokio::spawn(async move {
            let mut held = tokio::task::JoinSet::new();
            let mut normal_ordinal = 0;
            let mut compaction_ordinal = 0;
            // A branch and its source both continue, so that scenario answers more; so does a turn of eleven reads.
            let connections = if matches!(
                scenario,
                Scenario::Branching
                    | Scenario::BranchingCompaction
                    | Scenario::BranchingThenCompact
                    | Scenario::ClearOldResults
            ) {
                16
            } else {
                8
            };
            for ordinal in 0..connections {
                let (mut stream, _) =
                    listener.accept().await.expect("real Provider fixture request and response");
                let mut raw = Vec::new();
                let mut chunk = [0u8; 8192];
                let body = loop {
                    let n = stream
                        .read(&mut chunk)
                        .await
                        .expect("real Provider fixture request and response");
                    assert!(n > 0);
                    raw.extend_from_slice(&chunk[..n]);
                    assert!(raw.len() < 4 * 1024 * 1024);
                    if let Some(end) = raw.windows(4).position(|p| p == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&raw[..end]).to_ascii_lowercase();
                        assert!(
                            !headers.contains("authorization:") && !headers.contains("cookie:")
                        );
                        let length = headers
                            .lines()
                            .find_map(|l| l.strip_prefix("content-length:"))
                            .expect("real Provider fixture request and response")
                            .trim()
                            .parse::<usize>()
                            .expect("real Provider fixture request and response");
                        if raw.len() >= end + 4 + length {
                            saved_raw
                                .lock()
                                .expect("local Provider capture fixture")
                                .push(raw[end + 4..end + 4 + length].to_vec());
                            break serde_json::from_slice::<Value>(&raw[end + 4..end + 4 + length])
                                .expect("real Provider fixture request and response");
                        }
                    }
                };
                saved
                    .lock()
                    .expect("real Provider fixture request and response")
                    .push(body.clone());
                // DeepSeek's search, as `web.search` reaches it through the gateway.
                if String::from_utf8_lossy(&raw[..raw.len().min(512)])
                    .starts_with("POST /v1/tools/search ")
                {
                    let answer = json!({"results":[{"title":"Aura fixture result","url":"https://example.com/aura","snippet":"short","summary":"A longer summary the model reads.","site_name":"Example","date":"2026-10-01T00:00:00Z","rerank_score":2.0,"authority":3,"content":""}]}).to_string();
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}",
                        answer.len()
                    );
                    stream.write_all(response.as_bytes()).await.expect("search answer");
                    continue;
                }
                // A page DeepSeek's fetch cannot get.
                if String::from_utf8_lossy(&raw[..raw.len().min(512)])
                    .starts_with("POST /v1/tools/fetch ")
                {
                    let answer = json!({"error":{"message":"page not found"}}).to_string();
                    let response = format!(
                        "HTTP/1.1 404 Not Found\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}",
                        answer.len()
                    );
                    stream.write_all(response.as_bytes()).await.expect("fetch answer");
                    continue;
                }
                if let Scenario::Activity { format, retry } = scenario {
                    if retry && ordinal == 0 {
                        entered_park.notify_one();
                        release.notified().await;
                        let content = r#"{"error":{"message":"bounded retry fixture"}}"#;
                        stream.write_all(format!("HTTP/1.1 503 Service Unavailable\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{content}",content.len()).as_bytes()).await.expect("local Provider capture fixture");
                    } else {
                        super::activity_provider::respond(
                            &mut stream,
                            format,
                            ordinal - usize::from(retry),
                            &body,
                        )
                        .await;
                    }
                    continue;
                }
                if matches!(scenario, Scenario::ProviderUnavailable) {
                    // A real HTTP failure answer: the endpoint is reachable, the request was
                    // accepted and understood, and the Provider refused to serve it.
                    let body = json!({"error":{"type":"server_error","message":"the upstream provider is temporarily unavailable"}}).to_string();
                    let response = format!(
                        "HTTP/1.1 503 Service Unavailable\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    stream.write_all(response.as_bytes()).await.expect("provider failure answer");
                    continue;
                }
                if matches!(scenario, Scenario::BrokenConnection) {
                    // The connection really breaks: a complete response head, the beginning of an
                    // SSE frame, and then the socket is closed. Nothing downstream can turn this
                    // into a Provider error document.
                    let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n";
                    stream.write_all(head.as_bytes()).await.expect("broken connection head");
                    stream
                        .write_all(b"data: {\"choices\":[{\"delta\":{\"content\":\"trunc")
                        .await
                        .expect("broken connection partial frame");
                    stream.shutdown().await.expect("the connection really breaks");
                    continue;
                }
                if let Scenario::OverloadedThenText { failures } = scenario
                    && ordinal < failures
                {
                    let head = "HTTP/1.1 200 OK
Content-Type: text/event-stream
Cache-Control: no-cache
Connection: close

";
                    stream.write_all(head.as_bytes()).await.expect("overloaded stream head");
                    let delta = json!({"id":format!("fixture-response-{ordinal}"),"object":"chat.completion.chunk","model":"fixture","choices":[{"index":0,"delta":{"content":"partial "},"finish_reason":null}]});
                    let overloaded = json!({"error":{"type":"service_unavailable_error","code":"server_is_overloaded","message":"Our servers are currently overloaded."}});
                    let frames = format!(
                        "data: {delta}

data: {overloaded}

"
                    );
                    stream.write_all(frames.as_bytes()).await.expect("a delta, then the overload");
                    stream.shutdown().await.expect("the stream ends");
                    continue;
                }
                if matches!(scenario, Scenario::LateAnswer) {
                    // Parked until the test says the caller has already given up, and parked
                    // *beside* the accept loop so a later retry is still served. The answer is
                    // then written for real, into a socket whose reader is gone.
                    let parked_ordinal = normal_ordinal;
                    normal_ordinal += 1;
                    let release = release.clone();
                    held.spawn(async move {
                        let mut stream = stream;
                        release.notified().await;
                        send(
                            &mut stream,
                            parked_ordinal,
                            json!({"role":"assistant","content":"An answer that arrived after the caller gave up."}),
                        )
                        .await;
                    });
                    continue;
                }
                if matches!(scenario, Scenario::Window { failures } if ordinal < failures)
                    || matches!(scenario, Scenario::WindowHold) && ordinal == 0
                {
                    let body = json!({"error":{"code":"context_length_exceeded","message":"Maximum context length exceeded. Synthetic diagnostic C:\\private\\fixture.txt"}}).to_string();
                    let response = format!(
                        "HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    stream.write_all(response.as_bytes()).await.expect("context rejection");
                    continue;
                }
                if matches!(scenario, Scenario::WindowHold) && ordinal == 1 {
                    release.notified().await;
                }
                if matches!(scenario, Scenario::Hold)
                    || matches!(scenario, Scenario::RecoveredSession) && ordinal == 0
                {
                    held.spawn(async move {
                        let _ = stream.read(&mut [0u8; 1]).await;
                    });
                    continue;
                }
                // The Responses scenario has no chat-shaped request to inspect:
                // it is answered from the Responses body before any chat
                // assumption is made about the request.
                if matches!(
                    scenario,
                    Scenario::ResponsesWriteThenIllegalToolName
                        | Scenario::ResponsesUnknownThenWrite
                ) {
                    let step = normal_ordinal;
                    normal_ordinal += 1;
                    let step = if matches!(scenario, Scenario::ResponsesUnknownThenWrite) {
                        match step {
                            0 => 1,
                            1 => 0,
                            other => other,
                        }
                    } else {
                        step
                    };
                    responses_binding_step(&mut stream, step, &body).await;
                    continue;
                }
                let auxiliary = body["messages"]
                    .as_array()
                    .expect("real Provider fixture request and response")
                    .iter()
                    .any(|m| {
                        m["content"].as_str().is_some_and(|s| {
                            s.contains("Return exactly these headings in this order:")
                        })
                    });
                if auxiliary && matches!(scenario, Scenario::CompactHold) {
                    held.spawn(async move {
                        let _ = stream.read(&mut [0u8; 1]).await;
                    });
                    continue;
                }
                if auxiliary {
                    assert!(matches!(
                        scenario,
                        Scenario::Compact
                            | Scenario::CompactHold
                            | Scenario::CompactedAddition
                            | Scenario::CompactAfterTool
                            | Scenario::BranchingCompaction
                            | Scenario::BranchingThenCompact
                            | Scenario::Plain
                            | Scenario::PlainMeasured
                            | Scenario::PlainMeasuredRefusingSecond
                            | Scenario::CompactOnPrefix
                            | Scenario::CompactToolOnPrefix
                            | Scenario::ClearOldResults
                    ));
                    // A compaction asked on the step's prefix carries the step's tools,
                    // and its ask is its last instruction; one asked the isolated way has none.
                    let asked = compaction_ask(&body);
                    assert!(
                        body.get("tools").is_none_or(|v| v.as_array().is_some_and(Vec::is_empty))
                            || asked.contains("Compaction request:"),
                        "a compaction with Tools is asked on the step's prefix"
                    );
                    let anchors = json_in(&asked, "anchor_manifest")["anchor_manifest"]["anchors"]
                        .as_array()
                        .expect("real Provider fixture request and response")
                        .iter()
                        .map(|v| {
                            format!(
                                "- {}",
                                v["anchor"]
                                    .as_str()
                                    .expect("real Provider fixture request and response")
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    if matches!(scenario, Scenario::CompactToolOnPrefix) && compaction_ordinal == 0
                    {
                        compaction_ordinal += 1;
                        send(&mut stream, ordinal, compaction_tool_call(&body)).await;
                        continue;
                    }
                    let content = if long_compactions {
                        super::wire::long_compaction_answer(&asked, &anchors)
                    } else if compaction_ordinal == 0
                        && !matches!(
                            scenario,
                            Scenario::CompactedAddition
                                | Scenario::Plain
                                | Scenario::PlainMeasured
                                | Scenario::PlainMeasuredRefusingSecond
                                | Scenario::CompactOnPrefix
                                | Scenario::ClearOldResults
                        )
                    {
                        "Invalid output, to exercise the single bounded retry".to_string()
                    } else {
                        format!(
                            "# Historical-Snapshot-Summary\nThe prior response is retained by exact source anchors.\n# Active-Turn-Progress-Summary\nFinish the original user's request.\n# Structured-Compaction\n## Summary\nNo tools ran in the summarized response.\n## Full-Turn-Session-Line\nOne genuine user task.\n## Active-Turn-Segment-Line\nContinue from the retained response.\n## Truth-Index\n{anchors}"
                        )
                    };
                    compaction_ordinal += 1;
                    send(&mut stream, ordinal, json!({"role":"assistant","content":content})).await;
                    continue;
                }
                let ordinal = normal_ordinal;
                normal_ordinal += 1;
                if matches!(scenario, Scenario::ReasoningLoopThenText) {
                    let looping = "I should read the policy file once more before I decide.\n\n";
                    let delta = if ordinal < 2 {
                        json!({"role":"assistant","reasoning_content":looping.repeat(6)})
                    } else {
                        json!({"role":"assistant","content":"Answered after the loop."})
                    };
                    send(&mut stream, ordinal, delta).await;
                    continue;
                }
                if matches!(scenario, Scenario::CachedText) {
                    let delta = json!({"role":"assistant","content":"Cached."});
                    super::wire::send_cached(&mut stream, ordinal, delta, 1000, 600).await;
                    continue;
                }
                if matches!(
                    scenario,
                    Scenario::SearchThenText | Scenario::AdditionText | Scenario::CompactedAddition
                ) && ordinal == 0
                    || matches!(scenario, Scenario::ClearOldResults) && (9..=10).contains(&ordinal)
                {
                    entered_park.notify_one();
                    release.notified().await;
                }
                let write = matches!(
                    scenario,
                    Scenario::Write
                        | Scenario::LongWrite
                        | Scenario::Branching
                        | Scenario::BranchingThenCompact
                );
                let query = matches!(scenario, Scenario::Query);
                let search = matches!(scenario, Scenario::Search);
                let search_then_text = matches!(scenario, Scenario::SearchThenText);
                let external_read = matches!(
                    scenario,
                    Scenario::ExternalRead
                        | Scenario::ExternalReadRefused
                        | Scenario::ExternalReadMatrix
                );
                let external_list = matches!(scenario, Scenario::ExternalList);
                let external_list_outcome = matches!(scenario, Scenario::ExternalListOutcome);
                let external_find =
                    matches!(scenario, Scenario::ExternalFind | Scenario::ExternalFindScopeCapped);
                let external_search = matches!(scenario, Scenario::ExternalSearch);
                let external_write = matches!(scenario, Scenario::ExternalWrite);
                let read_missing = matches!(scenario, Scenario::ReadMissing);
                let discover = write
                    || query
                    || search
                    || search_then_text
                    || external_read
                    || external_list
                    || external_list_outcome
                    || external_find
                    || external_search
                    || external_write
                    || read_missing;
                let target = if query {
                    "query.user_question"
                } else if external_list {
                    "fs.list"
                } else if external_find {
                    "fs.find"
                } else if external_search {
                    "fs.search"
                } else if external_read || read_missing {
                    "fs.read"
                } else if search || search_then_text {
                    "fs.search"
                } else {
                    "fs.write"
                };
                let state = state(&body);
                let turn = &state["turn"];
                let call = |name: &str, args: Value, index: usize| {
                    let entry = aura_tool_registry::agent_release::release()
                        .expect("real Provider fixture request and response")
                        .entry(name)
                        .expect("real Provider fixture request and response");
                    let wire = provider_wire_tool_name(name, entry.definition.tool_id);
                    assert!(
                        body["tools"]
                            .as_array()
                            .expect("real Provider fixture request and response")
                            .iter()
                            .any(|t| t["function"]["name"] == wire),
                        "Provider cannot call an unexposed Tool: {name}"
                    );
                    json!({"index":index,"id":format!("fixture_{ordinal}_{index}"),"type":"function","function":{"name":wire,"arguments":super::flat_call("Verify the user-requested mode boundary", args)}})
                };
                if matches!(scenario, Scenario::RecoveredSession) {
                    let output = recovery_response(&state, &call);
                    send(&mut stream, ordinal, output).await;
                    continue;
                }
                if matches!(scenario, Scenario::FinishTurn) {
                    let revision = state["lineages"]
                        .as_array()
                        .expect("real Provider fixture request and response")
                        .iter()
                        .find(|lineage| lineage["lineage_id"] == turn["lineage_id"])
                        .expect("real Provider fixture request and response")["revision"]
                        .clone();
                    send(
                        &mut stream,
                        ordinal,
                        json!({"role":"assistant","tool_calls":[call("normal.complete",json!({
                            "lineage_id":turn["lineage_id"],
                            "expected_lineage_revision":revision,
                            "final_report":"Verified current mode behavior",
                            "disclosed_limitations":[]
                        }),0)]}),
                    )
                    .await;
                    continue;
                }
                if matches!(scenario, Scenario::ProtocolLoop) {
                    send(
                        &mut stream,
                        ordinal,
                        json!({"role":"assistant","content":"The previous step ended without a completion tool."}),
                    )
                    .await;
                    continue;
                }
                if matches!(scenario, Scenario::InvalidReconcile) {
                    send(
                        &mut stream,
                        ordinal,
                        json!({"role":"assistant","tool_calls":[call("task_lineage.reconcile",json!({
                            "decision":{"decision":"expire_prior","lineage_id":turn["lineage_id"]},
                            "reason":"Expire the current task"
                        }),0)]}),
                    )
                    .await;
                    continue;
                }
                let selected = state["lineages"]
                    .as_array()
                    .expect("real Provider fixture request and response")
                    .iter()
                    .find(|l| l["lineage_id"] == turn["lineage_id"])
                    .expect("real Provider fixture request and response");
                let recheck = query && ordinal == 3
                    || matches!(scenario, Scenario::Compact | Scenario::CompactHold)
                        && ordinal == 2;
                let delta = chat_reply::delta(
                    chat_reply::ChatReply {
                        scenario,
                        ordinal,
                        body: &body,
                        state: &state,
                        turn,
                        selected,
                        recheck,
                        query,
                        write,
                        search,
                        search_then_text,
                        external_read,
                        external_list,
                        external_list_outcome,
                        external_find,
                        external_search,
                        external_write,
                        read_missing,
                        discover,
                        target,
                    },
                    &call,
                );
                let truncated =
                    (matches!(scenario, Scenario::TruncatedText | Scenario::ThinkingCutOff)
                        && ordinal == 0)
                        || (matches!(scenario, Scenario::Compact | Scenario::CompactHold)
                            && ordinal < 2)
                        || (matches!(
                            scenario,
                            Scenario::CompactAfterTool | Scenario::BranchingCompaction
                        ) && ordinal == 1)
                        || (matches!(scenario, Scenario::BranchingThenCompact)
                            && (4..=5).contains(&ordinal))
                        || (matches!(
                            scenario,
                            Scenario::CompactOnPrefix | Scenario::CompactToolOnPrefix
                        ) && (2..=3).contains(&ordinal));
                if matches!(scenario, Scenario::PlainMeasuredRefusingSecond) && ordinal == 1 {
                    let body = json!({"error":{"type":"invalid_request_error","message":"refused by the fixture"}}).to_string();
                    let response = format!(
                        "HTTP/1.1 400 Bad Request
Content-Type: application/json
Content-Length: {}
Connection: close

{body}",
                        body.len()
                    );
                    stream.write_all(response.as_bytes()).await.expect("refusal");
                    continue;
                }
                if matches!(
                    scenario,
                    Scenario::PlainMeasured | Scenario::PlainMeasuredRefusingSecond
                ) {
                    let counted = serde_json::to_vec(&body).map_or(0, |bytes| bytes.len() / 4);
                    send_counted(&mut stream, ordinal, delta, None, counted as u64).await;
                    continue;
                }
                send_with(&mut stream, ordinal, delta, truncated.then_some("length")).await;
            }
        });
        Self { url, requests, raw_requests, release_window, parked, task }
    }
}
