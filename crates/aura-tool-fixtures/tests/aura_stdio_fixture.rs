#![forbid(unsafe_code)]

use std::{
    io::{Read, Write as _},
    process::{Child, ChildStdout, Command, Stdio},
};

use aura_domain_common::ContentHash;
use aura_domain_common::{ToolCallId, ToolExecutionAttemptId};
use aura_tool_contract::{
    AURA_STDIO_TOOL_CALL_ID_ENVIRONMENT_VARIABLE_V1,
    AURA_STDIO_TOOL_EXECUTION_ATTEMPT_ID_ENVIRONMENT_VARIABLE_V1,
    AURA_TOOL_OUTPUT_ROOT_ENVIRONMENT_VARIABLE_V1, STDIO_PROTOCOL_VERSION_V1,
    StdioExecutePayloadV1, StdioProtocolFrameV1,
};
use aura_tool_runtime::{decode_stdio_frame_payload, encode_stdio_frame_bytes};
use serde_json::json;

const FRAME_PREFIX_BYTES: usize = 4;

struct FixtureProcess {
    child: Child,
    stdout: ChildStdout,
    _output_root: tempfile::TempDir,
}

impl FixtureProcess {
    fn spawn(tool_call_id: ToolCallId, attempt_id: ToolExecutionAttemptId) -> Self {
        let output_root = tempfile::tempdir().expect("create fixture output root");
        let mut command = Command::new(env!("CARGO_BIN_EXE_aura-stdio-fixture"));
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env_clear()
            .env(AURA_STDIO_TOOL_CALL_ID_ENVIRONMENT_VARIABLE_V1, tool_call_id.to_string())
            .env(
                AURA_STDIO_TOOL_EXECUTION_ATTEMPT_ID_ENVIRONMENT_VARIABLE_V1,
                attempt_id.to_string(),
            )
            .env(AURA_TOOL_OUTPUT_ROOT_ENVIRONMENT_VARIABLE_V1, output_root.path());
        if let Ok(system_root) = std::env::var("SystemRoot") {
            command.env("SystemRoot", system_root);
        }
        let mut child = command.spawn().expect("spawn Aura Stdio fixture");
        let stdout = child.stdout.take().expect("fixture stdout");
        Self { child, stdout, _output_root: output_root }
    }

    fn read(&mut self) -> StdioProtocolFrameV1 {
        read_frame(&mut self.stdout)
    }

    fn send(&mut self, frame: &StdioProtocolFrameV1) {
        let bytes = encode_stdio_frame_bytes(frame).expect("encode Core fixture frame");
        let stdin = self.child.stdin.as_mut().expect("fixture stdin");
        stdin.write_all(&bytes).expect("write Core fixture frame");
        stdin.flush().expect("flush Core fixture frame");
    }

    fn finish(mut self) {
        drop(self.child.stdin.take());
        let output = self.child.wait_with_output().expect("wait for fixture Runtime");
        assert!(
            output.status.success(),
            "fixture Runtime failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn runtime_sends_matching_hello_before_accepting_execute() {
    let tool_call_id = ToolCallId::from_bytes([1; 16]);
    let attempt_id = ToolExecutionAttemptId::from_bytes([2; 16]);
    let mut process = FixtureProcess::spawn(tool_call_id, attempt_id);

    let hello = process.read();
    assert!(matches!(
        hello,
        StdioProtocolFrameV1::Hello {
            tool_call_id: actual_call,
            tool_execution_attempt_id: actual_attempt,
            sequence: 1,
            ..
        } if actual_call == tool_call_id && actual_attempt == attempt_id
    ));

    process.send(&execute(tool_call_id, attempt_id, json!({"value": "fixture"})));
    assert!(matches!(
        process.read(),
        StdioProtocolFrameV1::Progress {
            tool_call_id: actual_call,
            tool_execution_attempt_id: actual_attempt,
            sequence: 2,
            ..
        } if actual_call == tool_call_id && actual_attempt == attempt_id
    ));
    assert!(matches!(
        process.read(),
        StdioProtocolFrameV1::ArtifactReady {
            tool_call_id: actual_call,
            tool_execution_attempt_id: actual_attempt,
            sequence: 3,
            ..
        } if actual_call == tool_call_id && actual_attempt == attempt_id
    ));
    assert!(matches!(
        process.read(),
        StdioProtocolFrameV1::FinalResult {
            tool_call_id: actual_call,
            tool_execution_attempt_id: actual_attempt,
            sequence: 4,
            ..
        } if actual_call == tool_call_id && actual_attempt == attempt_id
    ));
    process.finish();
}

#[test]
fn concurrent_attempts_have_isolated_bootstrap_environments() {
    let tool_call_id = ToolCallId::from_bytes([3; 16]);
    let first_attempt = ToolExecutionAttemptId::from_bytes([4; 16]);
    let second_attempt = ToolExecutionAttemptId::from_bytes([5; 16]);
    let mut first = FixtureProcess::spawn(tool_call_id, first_attempt);
    let mut second = FixtureProcess::spawn(tool_call_id, second_attempt);

    assert_eq!(first.read().tool_execution_attempt_id(), first_attempt);
    assert_eq!(second.read().tool_execution_attempt_id(), second_attempt);
    first.send(&execute(tool_call_id, first_attempt, json!({"attempt": 1})));
    second.send(&execute(tool_call_id, second_attempt, json!({"attempt": 2})));
    assert_eq!(first.read().tool_execution_attempt_id(), first_attempt);
    assert_eq!(second.read().tool_execution_attempt_id(), second_attempt);
    assert_eq!(first.read().tool_execution_attempt_id(), first_attempt);
    assert_eq!(second.read().tool_execution_attempt_id(), second_attempt);
    assert_eq!(first.read().tool_execution_attempt_id(), first_attempt);
    assert_eq!(second.read().tool_execution_attempt_id(), second_attempt);
    first.finish();
    second.finish();
}

#[test]
fn missing_or_invalid_bootstrap_identity_cannot_emit_hello() {
    for environment in [None, Some("invalid")] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_aura-stdio-fixture"));
        command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).env_clear();
        if let Some(value) = environment {
            command.env(AURA_STDIO_TOOL_CALL_ID_ENVIRONMENT_VARIABLE_V1, value);
            command.env(
                AURA_STDIO_TOOL_EXECUTION_ATTEMPT_ID_ENVIRONMENT_VARIABLE_V1,
                ToolExecutionAttemptId::from_bytes([6; 16]).to_string(),
            );
        }
        let output = command.output().expect("run invalid bootstrap fixture");
        assert!(!output.status.success());
        assert!(output.stdout.is_empty(), "invalid bootstrap emitted a protocol frame");
    }
}

fn execute(
    tool_call_id: ToolCallId,
    tool_execution_attempt_id: ToolExecutionAttemptId,
    arguments: serde_json::Value,
) -> StdioProtocolFrameV1 {
    StdioProtocolFrameV1::Execute {
        protocol_version: STDIO_PROTOCOL_VERSION_V1,
        tool_call_id,
        tool_execution_attempt_id,
        sequence: 1,
        payload: StdioExecutePayloadV1 {
            arguments,
            arguments_hash: ContentHash::sha256(b"fixture arguments"),
        },
    }
}

fn read_frame(reader: &mut impl Read) -> StdioProtocolFrameV1 {
    let mut prefix = [0_u8; FRAME_PREFIX_BYTES];
    reader.read_exact(&mut prefix).expect("fixture frame prefix");
    let payload_length = usize::try_from(u32::from_be_bytes(prefix)).expect("payload length");
    let mut payload = vec![0_u8; payload_length];
    reader.read_exact(&mut payload).expect("fixture frame payload");
    decode_stdio_frame_payload(&payload).expect("valid fixture frame")
}
