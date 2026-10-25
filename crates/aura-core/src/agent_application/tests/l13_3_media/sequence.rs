//! In one turn the model probed an attached
//! external video, read a contact sheet, then asked for frames at chosen times, and the third call
//! was refused before it ran ("agent.tool.reserve: persistent revision or identity conflict").
//! Every call of such a turn must run.
use super::*;
use aura_domain_common::CommandInvocationId;

fn leak(value: Value) -> &'static str {
    Box::leak(value.to_string().into_boxed_str())
}

/// The calls DeepSeek made: both tools described first, then each called by its own name.
fn described_then_called(path: &str) -> &'static [(&'static str, &'static str)] {
    Box::leak(
        vec![
            ("tools.describe", leak(json!({"tool_id": "tol_01M2DG020980080021AN94202M"}))),
            ("tools.describe", leak(json!({"tool_id": "tol_01M2DG020980080021AN94202N"}))),
            ("media.probe", leak(json!({"path": path}))),
            ("media.frames", leak(json!({"path": path, "count": 9, "sheet": true}))),
            // DeepSeek wrote whole seconds as integers.
            ("media.frames", leak(json!({"path": path, "times": [0.5, 1]}))),
        ]
        .into_boxed_slice(),
    )
}

#[tokio::test]
async fn one_turn_reads_an_attached_external_video_three_times() {
    let host = crate::r03_tests::Fixture::new();
    let video = host.root.path().join("outside-clip.webm");
    std::fs::write(&video, SOURCE).unwrap();
    let calls = described_then_called(video.to_str().unwrap());
    let provider = Provider::scenario(Scenario::Sequence { calls }).await;
    let (runtime, session, model) = prepare(&host, &provider).await;
    images(&runtime, model, true).await;
    let (seen, task) = play_host(&runtime);
    let started = services(&runtime)
        .agent_required()
        .start(
            CommandInvocationId::new(),
            serde_json::from_value(json!({"session_id":session,"model_profile_id":model,
                "text":"看看这个视频","mode":"normal","goal_budget":null,
                "attachments":[{"kind":"file","path":video}]}))
            .unwrap(),
        )
        .await
        .expect("the external video is attached");
    finish(&runtime, &started).await;
    let text = tool_text(&provider);
    assert!(!text.contains("dispatch_refused"), "{text}");
    assert_eq!(seen.lock().unwrap().len(), 3, "{text}");
    task.abort();
}
