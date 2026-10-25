use super::*;

#[tokio::test]
async fn probe_and_all_three_frame_modes_reach_main_and_frames_reach_the_next_model_request() {
    for (tool, arguments, expected_images) in [
        ("media.probe", json!({"path":"fixture.webm"}), 0),
        ("media.frames", json!({"path":"fixture.webm","times":[0.2,1.2]}), 2),
        ("media.frames", json!({"path":"fixture.webm","count":3}), 3),
        ("media.frames", json!({"path":"fixture.webm","count":3,"sheet":true}), 1),
    ] {
        let host = crate::r03_tests::Fixture::new();
        let provider = Provider::scenario(scenario(tool, arguments)).await;
        let (runtime, session, model) = prepare(&host, &provider).await;
        images(&runtime, model, true).await;
        std::fs::write(host.root.path().join("waiting-workspace/fixture.webm"), SOURCE).unwrap();
        let (seen, task) = play_host(&runtime);
        let started = start(&runtime, session, model, AgentTaskMode::Normal).await;
        finish(&runtime, &started).await;
        assert_eq!(seen.lock().unwrap().len(), 1, "{tool}: {}", tool_text(&provider));
        let requests = provider.requests.lock().unwrap();
        let last = requests.last().unwrap();
        let serialized = last.to_string();
        assert_eq!(
            serialized.matches("data:image/jpeg;base64,").count(),
            expected_images,
            "{tool}: {last}"
        );
        if expected_images > 0 {
            assert!(serialized.contains(JPEG), "{last}");
        }
        drop(requests);
        if expected_images > 0 {
            assert!(tool_text(&provider).contains("\"shown_to_model\":true"));
        } else {
            assert!(tool_text(&provider).contains("\"duration\":2.0"));
        }
        task.abort();
    }
}

#[tokio::test]
async fn a_model_without_images_gets_frame_times_and_no_image_bytes_or_artifacts() {
    let host = crate::r03_tests::Fixture::new();
    let provider =
        Provider::scenario(scenario("media.frames", json!({"path":"fixture.webm","count":2})))
            .await;
    let (runtime, session, model) = prepare(&host, &provider).await;
    images(&runtime, model, false).await;
    std::fs::write(host.root.path().join("waiting-workspace/fixture.webm"), SOURCE).unwrap();
    let (seen, task) = play_host(&runtime);
    let started = start(&runtime, session, model, AgentTaskMode::Normal).await;
    finish(&runtime, &started).await;
    assert_eq!(seen.lock().unwrap().len(), 1, "{}", tool_text(&provider));
    let requests = provider.requests.lock().unwrap();
    assert!(
        !requests.iter().any(|request| request.to_string().contains("data:image/jpeg;base64,"))
    );
    drop(requests);
    let text = tool_text(&provider);
    assert!(
        text.contains("\"shown_to_model\":false") && text.contains("\"actual_time\""),
        "{text}"
    );
    let agent = services(&runtime).agent_required();
    let journal = agent.execution.journal.lock().unwrap();
    let images: i64 = journal
        .database
        .connection()
        .query_row("SELECT count(*) FROM artifacts WHERE media_type='image/jpeg'", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(images, 0, "no discarded frame may be persisted");
    task.abort();
}
