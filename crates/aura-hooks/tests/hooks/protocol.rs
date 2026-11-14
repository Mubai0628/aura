use aura_hooks::{HookDecision, HookEvent, HookResultError, HookTransform, parse_result};
use serde_json::json;

fn parse(event: HookEvent, value: serde_json::Value) -> Result<HookDecision, HookResultError> {
    parse_result(event, value.to_string().as_bytes())
}

#[test]
fn continue_block_and_the_events_own_transform_are_the_only_answers() {
    assert_eq!(
        parse(HookEvent::TurnCompleted, json!({"decision":"continue"})),
        Ok(HookDecision::Continue)
    );
    assert_eq!(
        parse(HookEvent::ToolBeforeCall, json!({"decision":"block","reason":"不允许写 .env"})),
        Ok(HookDecision::Block { reason: "不允许写 .env".into() })
    );
    assert_eq!(
        parse(
            HookEvent::ToolBeforeCall,
            json!({"decision":"transform","arguments":{"path":"a.txt"}})
        ),
        Ok(HookDecision::Transform(HookTransform::Arguments(json!({"path":"a.txt"}))))
    );
    assert_eq!(
        parse(HookEvent::ContextBeforeBuild, json!({"decision":"transform","inject":"注意测试"})),
        Ok(HookDecision::Transform(HookTransform::Inject("注意测试".into())))
    );
    assert_eq!(
        parse(HookEvent::ToolAfterCall, json!({"decision":"transform","result":"已省略"})),
        Ok(HookDecision::Transform(HookTransform::Result("已省略".into())))
    );
}

#[test]
fn nothing_a_hook_returns_can_approve_grant_ask_or_skip_an_audit() {
    for field in ["allow", "grant_permission", "ask_user", "skip_audit", "permission"] {
        let mut value = json!({"decision":"continue"});
        value[field] = json!(true);
        assert!(
            matches!(parse(HookEvent::ToolBeforeCall, value), Err(HookResultError::NotJson(_))),
            "{field} must make the result invalid"
        );
    }
    assert_eq!(
        parse(HookEvent::ToolBeforeCall, json!({"decision":"allow"})),
        Err(HookResultError::UnknownDecision)
    );
}

#[test]
fn a_transform_outside_the_events_boundary_is_refused() {
    for (event, value) in [
        (HookEvent::ToolAfterCall, json!({"decision":"transform","arguments":{}})),
        (HookEvent::TurnBeforeStart, json!({"decision":"transform","text":"x"})),
        (
            HookEvent::ModelBeforeCall,
            json!({"decision":"transform","parameters":{"model":"other"}}),
        ),
        (
            HookEvent::ModelBeforeCall,
            json!({"decision":"transform","parameters":{"temperature":3.5}}),
        ),
        (HookEvent::ToolBeforeCall, json!({"decision":"transform","arguments":"not an object"})),
        (HookEvent::ToolBeforeCall, json!({"decision":"transform","arguments":{},"reason":"x"})),
        (HookEvent::ToolBeforeCall, json!({"decision":"block"})),
        (HookEvent::ToolBeforeCall, json!({"decision":"continue","text":"x"})),
    ] {
        assert!(parse(event, value.clone()).is_err(), "{} {value}", event.name());
    }
}

#[test]
fn oversized_or_non_json_output_is_a_failure() {
    let large = format!("{{\"decision\":\"transform\",\"inject\":\"{}\"}}", "x".repeat(17 * 1024));
    assert!(matches!(
        parse_result(HookEvent::ContextBeforeBuild, large.as_bytes()),
        Err(HookResultError::OutOfBounds(_))
    ));
    assert!(matches!(
        parse_result(HookEvent::TurnCompleted, b"ok"),
        Err(HookResultError::NotJson(_))
    ));
    assert_eq!(
        parse_result(HookEvent::TurnCompleted, &vec![b' '; aura_hooks::MAX_RESULT_BYTES + 1]),
        Err(HookResultError::TooLarge)
    );
}

#[test]
fn only_pre_events_stop_the_action_that_follows() {
    let blocking = HookEvent::ALL.into_iter().filter(|e| e.blocks_action()).collect::<Vec<_>>();
    assert_eq!(
        blocking,
        [
            HookEvent::MessageBeforeAccept,
            HookEvent::TurnBeforeStart,
            HookEvent::ContextBeforeBuild,
            HookEvent::ModelBeforeCall,
            HookEvent::ToolBeforeCall,
            HookEvent::TurnBeforeComplete,
        ]
    );
}
