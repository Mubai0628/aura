//! Which of the history's screenshots a request carries as the history grows.
use super::*;
use serde_json::json;

fn result(tool: &str, value: serde_json::Value, is_error: bool) -> LogicalConversationItem {
    let outcome = ModelVisibleToolResultOutcomeV1::Succeeded {
        payload: ModelVisibleToolSuccessPayloadV1::StructuredJson { value },
    };
    let content = json!({ "outcome": outcome }).to_string();
    LogicalConversationItem::ToolResult {
        source_event_id: EventId::new(),
        tool_call_id: ToolCallId::new(),
        tool_name: tool.to_owned(),
        content_hash: ContentHash::sha256(content.as_bytes()),
        content,
        is_error,
    }
}

/// A computer call's result, which carries the screenshot it took after acting.
fn acted(id: ArtifactId) -> LogicalConversationItem {
    result(
        "computer.act",
        json!({"screenshot": {"image_artifact_id": id, "width": 1280, "height": 768, "shown_to_model": true}}),
        false,
    )
}

fn carried(items: &[LogicalConversationItem]) -> Vec<ArtifactId> {
    tool_images(items).into_iter().map(|image| image.artifact_id).collect()
}

#[test]
fn older_screenshots_leave_the_request_three_at_a_time() {
    let ids = (0..12).map(|_| ArtifactId::new()).collect::<Vec<_>>();
    let history = ids.iter().copied().map(acted).collect::<Vec<_>>();
    // With n screenshots, the newest ones carried: at least three, and the older ones until three
    // more have come.
    for (count, newest) in
        [(1, 1), (2, 2), (3, 3), (4, 4), (5, 5), (6, 3), (7, 4), (8, 5), (9, 3), (12, 3)]
    {
        assert_eq!(
            carried(&history[..count]),
            ids[count - newest..count],
            "{count} screenshots in the history"
        );
    }
}

#[test]
fn a_new_screenshot_changes_the_older_part_of_the_request_once_per_three() {
    let ids = (0..13).map(|_| ArtifactId::new()).collect::<Vec<_>>();
    let mut history = Vec::new();
    let mut previous = Vec::new();
    let mut removals = Vec::new();
    for (count, id) in ids.iter().copied().enumerate() {
        history.push(acted(id));
        let now = carried(&history);
        // A screenshot that leaves the request changes it where its result is.
        if previous.iter().any(|id| !now.contains(id)) {
            removals.push(count + 1);
        }
        previous = now;
    }
    assert_eq!(removals, [6, 9, 12]);
}

#[test]
fn screenshots_the_model_did_not_see_or_that_failed_do_not_count() {
    let ids = (0..6).map(|_| ArtifactId::new()).collect::<Vec<_>>();
    let mut history = ids[..4].iter().copied().map(acted).collect::<Vec<_>>();
    history.push(result(
        "computer.screenshot",
        json!({"image_artifact_id": ids[4], "width": 1280, "height": 768, "shown_to_model": false}),
        false,
    ));
    history.push(result(
        "browser.screenshot",
        json!({"image_artifact_id": ids[5], "shown_to_model": true}),
        true,
    ));
    assert_eq!(carried(&history), ids[..4], "four seen screenshots are all carried");
}

#[test]
fn the_latest_video_frames_keep_their_place_among_many_screenshots() {
    let shots = (0..8).map(|_| ArtifactId::new()).collect::<Vec<_>>();
    let frames = (0..2).map(|_| ArtifactId::new()).collect::<Vec<_>>();
    let mut history = shots[..4].iter().copied().map(acted).collect::<Vec<_>>();
    history.push(result(
        "media.frames",
        json!({
            "frames": frames.iter().enumerate().map(|(i, id)| json!({"requested_time": i, "actual_time": i, "image_artifact_id": id})).collect::<Vec<_>>(),
            "sheet_artifact_id": null,
            "shown_to_model": true,
            "note": "",
        }),
        false,
    ));
    history.extend(shots[4..].iter().copied().map(acted));
    // Eight screenshots carry their newest five; the frames sit where their result is.
    let expected = [&shots[3..4], &frames[..], &shots[4..]].concat();
    assert_eq!(carried(&history), expected);
}
