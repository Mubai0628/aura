#![forbid(unsafe_code)]

use std::{
    env,
    io::{self, Read as _, Write as _},
    str::FromStr,
};

use aura_domain_common::ContentHash;
use aura_domain_common::{ToolCallId, ToolExecutionAttemptId};
use aura_tool_contract::{
    AURA_STDIO_TOOL_CALL_ID_ENVIRONMENT_VARIABLE_V1,
    AURA_STDIO_TOOL_EXECUTION_ATTEMPT_ID_ENVIRONMENT_VARIABLE_V1,
    AURA_TOOL_OUTPUT_ROOT_ENVIRONMENT_VARIABLE_V1, STDIO_PROTOCOL_VERSION_V1,
    StdioArtifactReadyPayloadV1, StdioFinalResultPayloadV1, StdioHelloPayloadV1,
    StdioProgressPayloadV1, StdioProtocolFrameV1, ToolOutputRelativePathV1, ToolOutputSlotKeyV1,
};
use aura_tool_runtime::{decode_stdio_frame_payload, encode_stdio_frame_bytes};

const FRAME_PREFIX_BYTES: usize = 4;

fn main() {
    if let Err(error) = run() {
        let _ = writeln!(io::stderr().lock(), "Aura Stdio fixture failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), &'static str> {
    let tool_call_id =
        read_typed_environment::<ToolCallId>(AURA_STDIO_TOOL_CALL_ID_ENVIRONMENT_VARIABLE_V1)?;
    let tool_execution_attempt_id = read_typed_environment::<ToolExecutionAttemptId>(
        AURA_STDIO_TOOL_EXECUTION_ATTEMPT_ID_ENVIRONMENT_VARIABLE_V1,
    )?;

    let hello = StdioProtocolFrameV1::Hello {
        protocol_version: STDIO_PROTOCOL_VERSION_V1,
        tool_call_id,
        tool_execution_attempt_id,
        sequence: 1,
        payload: StdioHelloPayloadV1 {
            runtime_name: "aura-stdio-fixture".to_owned(),
            runtime_version: env!("CARGO_PKG_VERSION").to_owned(),
        },
    };
    write_frame(&hello)?;

    let execute = read_frame()?;
    let StdioProtocolFrameV1::Execute {
        tool_call_id: received_tool_call_id,
        tool_execution_attempt_id: received_attempt_id,
        sequence,
        payload,
        ..
    } = execute
    else {
        return Err("first Core frame was not execute");
    };
    if received_tool_call_id != tool_call_id
        || received_attempt_id != tool_execution_attempt_id
        || sequence != 1
    {
        return Err("execute identity or sequence did not match bootstrap identity");
    }
    if env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("--missing-terminal")) {
        return Ok(());
    }

    let progress = StdioProtocolFrameV1::Progress {
        protocol_version: STDIO_PROTOCOL_VERSION_V1,
        tool_call_id,
        tool_execution_attempt_id,
        sequence: 2,
        payload: StdioProgressPayloadV1 {
            percentage: Some(50),
            message: Some("fixture running".to_owned()),
        },
    };
    write_frame(&progress)?;

    let output_root = env::var_os(AURA_TOOL_OUTPUT_ROOT_ENVIRONMENT_VARIABLE_V1)
        .ok_or("required output root environment variable is missing")?;
    let relative_path = ToolOutputRelativePathV1::new("fixture-output.txt")
        .map_err(|_| "fixture output path failed validation")?;
    let output_bytes = b"authorized Aura Stdio fixture output\n";
    std::fs::write(std::path::Path::new(&output_root).join(relative_path.as_str()), output_bytes)
        .map_err(|_| "fixture output could not be written")?;
    let artifact = StdioProtocolFrameV1::ArtifactReady {
        protocol_version: STDIO_PROTOCOL_VERSION_V1,
        tool_call_id,
        tool_execution_attempt_id,
        sequence: 3,
        payload: StdioArtifactReadyPayloadV1 {
            slot: ToolOutputSlotKeyV1::new("fixture-output")
                .map_err(|_| "fixture output slot failed validation")?,
            relative_path,
            media_type: "text/plain; charset=utf-8".to_owned(),
            byte_length: u64::try_from(output_bytes.len())
                .map_err(|_| "fixture output length overflowed")?,
            content_hash: ContentHash::sha256(output_bytes),
        },
    };
    write_frame(&artifact)?;

    let mut result = serde_json::json!({
        "arguments": payload.arguments,
        "arguments_hash": payload.arguments_hash,
    });
    if env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("--report-identity")) {
        result["tool_call_id"] = serde_json::json!(tool_call_id);
        result["tool_execution_attempt_id"] = serde_json::json!(tool_execution_attempt_id);
    }
    let terminal = StdioProtocolFrameV1::FinalResult {
        protocol_version: STDIO_PROTOCOL_VERSION_V1,
        tool_call_id,
        tool_execution_attempt_id,
        sequence: 4,
        payload: StdioFinalResultPayloadV1 { result },
    };
    write_frame(&terminal)
}

fn read_typed_environment<T>(name: &'static str) -> Result<T, &'static str>
where
    T: FromStr,
{
    env::var(name)
        .map_err(|_| "required bootstrap environment variable is missing")?
        .parse::<T>()
        .map_err(|_| "bootstrap environment variable is not a canonical typed ID")
}

fn read_frame() -> Result<StdioProtocolFrameV1, &'static str> {
    let mut prefix = [0_u8; FRAME_PREFIX_BYTES];
    io::stdin()
        .lock()
        .read_exact(&mut prefix)
        .map_err(|_| "Core frame prefix was missing or truncated")?;
    let payload_length =
        usize::try_from(u32::from_be_bytes(prefix)).map_err(|_| "Core frame length was invalid")?;
    if payload_length == 0 || payload_length > aura_tool_contract::MAX_STDIO_FRAME_BYTES {
        return Err("Core frame length was outside the V1 limit");
    }
    let mut payload = vec![0_u8; payload_length];
    io::stdin().lock().read_exact(&mut payload).map_err(|_| "Core frame payload was truncated")?;
    decode_stdio_frame_payload(&payload).map_err(|_| "Core frame failed V1 validation")
}

fn write_frame(frame: &StdioProtocolFrameV1) -> Result<(), &'static str> {
    let bytes =
        encode_stdio_frame_bytes(frame).map_err(|_| "Runtime frame failed V1 validation")?;
    let mut stdout = io::stdout().lock();
    stdout
        .write_all(&bytes)
        .and_then(|()| stdout.flush())
        .map_err(|_| "Runtime frame could not be written")
}
