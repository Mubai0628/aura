use std::io::ErrorKind;

use aura_domain_common::{ContentHash, SandboxExecutionId};
use aura_domain_common::{ToolCallId, ToolExecutionAttemptId};
use aura_permission_domain::SandboxHostEvent;
use aura_tool_contract::{
    MAX_STDIO_FRAME_BYTES, MAX_STDIO_STDOUT_WIRE_BYTES, STDIO_PROTOCOL_VERSION_V1,
    StdioArtifactReadyPayloadV1, StdioEmptyPayloadV1, StdioExecutePayloadV1,
    StdioProgressPayloadV1, StdioProtocolDirectionV1, StdioProtocolFrameV1, ToolRuntimeErrorCode,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

const FRAME_PREFIX_BYTES: usize = 4;

#[derive(Debug, Error)]
pub enum StdioProtocolErrorV1 {
    #[error("stdio protocol I/O failed")]
    Io(#[source] std::io::Error),
    #[error("stdio protocol frame length must be non-zero")]
    ZeroLengthFrame,
    #[error("stdio protocol frame exceeds the fixed one MiB limit")]
    FrameTooLarge,
    #[error("stdio protocol stdout exceeds the fixed eight MiB wire limit")]
    StdoutWireLimitExceeded,
    #[error("stdio protocol frame prefix was truncated")]
    TruncatedPrefix,
    #[error("stdio protocol frame payload was truncated")]
    TruncatedPayload,
    #[error("stdio protocol frame contains invalid UTF-8, JSON, type, or fields")]
    InvalidFrame(#[source] serde_json::Error),
    #[error("stdio protocol frame contains trailing bytes")]
    TrailingBytes,
    #[error("stdio protocol frame violates the V1 contract")]
    ContractViolation,
    #[error("stdio protocol frame direction is invalid")]
    DirectionInvalid,
    #[error("stdio protocol frame identity does not match the execution attempt")]
    IdentityMismatch,
    #[error("stdio protocol sequence is not the next expected value")]
    SequenceInvalid,
    #[error("stdio protocol hello must be the first runtime frame")]
    HandshakeInvalid,
    #[error("stdio protocol execute or cancel is invalid in the current state")]
    CoreMessageStateInvalid,
    #[error("stdio protocol runtime frame is invalid in the current state")]
    RuntimeMessageStateInvalid,
    #[error("stdio protocol emitted data after a terminal frame")]
    TerminalConflict,
    #[error("stdio protocol reached EOF without a terminal frame")]
    TerminalMissing,
}

impl StdioProtocolErrorV1 {
    #[must_use]
    pub const fn error_code(&self) -> ToolRuntimeErrorCode {
        match self {
            Self::StdoutWireLimitExceeded => ToolRuntimeErrorCode::IoLimitExceeded,
            Self::SequenceInvalid => ToolRuntimeErrorCode::ProtocolSequenceInvalid,
            Self::HandshakeInvalid => ToolRuntimeErrorCode::ProtocolHandshakeInvalid,
            Self::TerminalConflict => ToolRuntimeErrorCode::ProtocolTerminalConflict,
            Self::TerminalMissing => ToolRuntimeErrorCode::ProtocolTerminalMissing,
            Self::Io(_)
            | Self::ZeroLengthFrame
            | Self::FrameTooLarge
            | Self::TruncatedPrefix
            | Self::TruncatedPayload
            | Self::InvalidFrame(_)
            | Self::TrailingBytes
            | Self::ContractViolation
            | Self::DirectionInvalid
            | Self::IdentityMismatch
            | Self::CoreMessageStateInvalid
            | Self::RuntimeMessageStateInvalid => ToolRuntimeErrorCode::ProtocolFrameInvalid,
        }
    }
}

pub async fn write_stdio_frame<W>(
    writer: &mut W,
    frame: &StdioProtocolFrameV1,
) -> Result<(), StdioProtocolErrorV1>
where
    W: AsyncWrite + Unpin,
{
    let encoded = encode_stdio_frame_bytes(frame)?;
    writer.write_all(&encoded).await.map_err(StdioProtocolErrorV1::Io)?;
    writer.flush().await.map_err(StdioProtocolErrorV1::Io)
}

pub fn encode_stdio_frame_bytes(
    frame: &StdioProtocolFrameV1,
) -> Result<Vec<u8>, StdioProtocolErrorV1> {
    frame.validate().map_err(|_| StdioProtocolErrorV1::ContractViolation)?;
    let payload = serde_json::to_vec(frame).map_err(StdioProtocolErrorV1::InvalidFrame)?;
    if payload.is_empty() {
        return Err(StdioProtocolErrorV1::ZeroLengthFrame);
    }
    if payload.len() > MAX_STDIO_FRAME_BYTES {
        return Err(StdioProtocolErrorV1::FrameTooLarge);
    }
    let payload_length =
        u32::try_from(payload.len()).map_err(|_| StdioProtocolErrorV1::FrameTooLarge)?;
    let mut encoded = Vec::with_capacity(FRAME_PREFIX_BYTES + payload.len());
    encoded.extend_from_slice(&payload_length.to_be_bytes());
    encoded.extend_from_slice(&payload);
    Ok(encoded)
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolStdioHostInputV1 {
    pub sandbox_execution_id: SandboxExecutionId,
    pub tool_execution_attempt_id: ToolExecutionAttemptId,
    pub frame: StdioProtocolFrameV1,
}

impl ToolStdioHostInputV1 {
    pub fn validate(&self) -> Result<(), StdioProtocolErrorV1> {
        self.frame.validate().map_err(|_| StdioProtocolErrorV1::ContractViolation)?;
        if self.frame.direction() != StdioProtocolDirectionV1::CoreToRuntime {
            return Err(StdioProtocolErrorV1::DirectionInvalid);
        }
        if self.frame.tool_execution_attempt_id() != self.tool_execution_attempt_id {
            return Err(StdioProtocolErrorV1::IdentityMismatch);
        }
        Ok(())
    }
}

pub struct StdioRuntimeFrameReaderV1<R> {
    reader: R,
    stdout_wire_limit: usize,
    wire_bytes_read: usize,
}

/// Incremental decoder for stdout chunks emitted by the Sandbox Host. The
/// Host event boundary is unrelated to the Aura Stdio frame boundary, so Core
/// must retain incomplete prefix/payload bytes without treating a chunk as a
/// protocol frame.
pub struct StdioRuntimeFrameDecoderV1 {
    buffer: Vec<u8>,
    stdout_wire_limit: usize,
    wire_bytes_received: usize,
}

impl Default for StdioRuntimeFrameDecoderV1 {
    fn default() -> Self {
        Self::new()
    }
}

impl StdioRuntimeFrameDecoderV1 {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            buffer: Vec::new(),
            stdout_wire_limit: MAX_STDIO_STDOUT_WIRE_BYTES,
            wire_bytes_received: 0,
        }
    }

    #[cfg(test)]
    const fn with_wire_limit(stdout_wire_limit: usize) -> Self {
        Self { buffer: Vec::new(), stdout_wire_limit, wire_bytes_received: 0 }
    }

    #[must_use]
    pub const fn wire_bytes_received(&self) -> usize {
        self.wire_bytes_received
    }

    pub fn push_chunk(
        &mut self,
        chunk: &[u8],
    ) -> Result<Vec<StdioProtocolFrameV1>, StdioProtocolErrorV1> {
        let next_total = self
            .wire_bytes_received
            .checked_add(chunk.len())
            .ok_or(StdioProtocolErrorV1::StdoutWireLimitExceeded)?;
        if next_total > self.stdout_wire_limit {
            return Err(StdioProtocolErrorV1::StdoutWireLimitExceeded);
        }
        self.wire_bytes_received = next_total;
        self.buffer.extend_from_slice(chunk);

        let mut cursor = 0_usize;
        let mut frames = Vec::new();
        loop {
            let available = self.buffer.len().saturating_sub(cursor);
            if available < FRAME_PREFIX_BYTES {
                break;
            }
            let prefix: [u8; FRAME_PREFIX_BYTES] = self.buffer[cursor..cursor + FRAME_PREFIX_BYTES]
                .try_into()
                .map_err(|_| StdioProtocolErrorV1::TruncatedPrefix)?;
            let payload_length = usize::try_from(u32::from_be_bytes(prefix))
                .map_err(|_| StdioProtocolErrorV1::FrameTooLarge)?;
            if payload_length == 0 {
                return Err(StdioProtocolErrorV1::ZeroLengthFrame);
            }
            if payload_length > MAX_STDIO_FRAME_BYTES {
                return Err(StdioProtocolErrorV1::FrameTooLarge);
            }
            let frame_length = FRAME_PREFIX_BYTES
                .checked_add(payload_length)
                .ok_or(StdioProtocolErrorV1::FrameTooLarge)?;
            if available < frame_length {
                break;
            }
            let payload_start = cursor + FRAME_PREFIX_BYTES;
            let payload_end = cursor + frame_length;
            frames.push(decode_stdio_frame_payload(&self.buffer[payload_start..payload_end])?);
            cursor = payload_end;
        }
        if cursor > 0 {
            self.buffer.drain(..cursor);
        }
        Ok(frames)
    }

    pub fn finish_on_eof(&self) -> Result<(), StdioProtocolErrorV1> {
        match self.buffer.len() {
            0 => Ok(()),
            1..=3 => Err(StdioProtocolErrorV1::TruncatedPrefix),
            _ => Err(StdioProtocolErrorV1::TruncatedPayload),
        }
    }
}

impl<R> StdioRuntimeFrameReaderV1<R>
where
    R: AsyncRead + Unpin,
{
    #[must_use]
    pub const fn new(reader: R) -> Self {
        Self { reader, stdout_wire_limit: MAX_STDIO_STDOUT_WIRE_BYTES, wire_bytes_read: 0 }
    }

    #[cfg(test)]
    const fn with_wire_limit(reader: R, stdout_wire_limit: usize) -> Self {
        Self { reader, stdout_wire_limit, wire_bytes_read: 0 }
    }

    #[must_use]
    pub const fn wire_bytes_read(&self) -> usize {
        self.wire_bytes_read
    }

    pub async fn read_frame(
        &mut self,
    ) -> Result<Option<StdioProtocolFrameV1>, StdioProtocolErrorV1> {
        let mut prefix = [0_u8; FRAME_PREFIX_BYTES];
        let first = self.reader.read(&mut prefix[..1]).await.map_err(StdioProtocolErrorV1::Io)?;
        if first == 0 {
            return Ok(None);
        }
        if let Err(error) = self.reader.read_exact(&mut prefix[1..]).await {
            return Err(if error.kind() == ErrorKind::UnexpectedEof {
                StdioProtocolErrorV1::TruncatedPrefix
            } else {
                StdioProtocolErrorV1::Io(error)
            });
        }
        let payload_length = usize::try_from(u32::from_be_bytes(prefix))
            .map_err(|_| StdioProtocolErrorV1::FrameTooLarge)?;
        if payload_length == 0 {
            return Err(StdioProtocolErrorV1::ZeroLengthFrame);
        }
        if payload_length > MAX_STDIO_FRAME_BYTES {
            return Err(StdioProtocolErrorV1::FrameTooLarge);
        }
        let next_total = self
            .wire_bytes_read
            .checked_add(FRAME_PREFIX_BYTES)
            .and_then(|value| value.checked_add(payload_length))
            .ok_or(StdioProtocolErrorV1::StdoutWireLimitExceeded)?;
        if next_total > self.stdout_wire_limit {
            return Err(StdioProtocolErrorV1::StdoutWireLimitExceeded);
        }
        let mut payload = vec![0_u8; payload_length];
        if let Err(error) = self.reader.read_exact(&mut payload).await {
            return Err(if error.kind() == ErrorKind::UnexpectedEof {
                StdioProtocolErrorV1::TruncatedPayload
            } else {
                StdioProtocolErrorV1::Io(error)
            });
        }
        let frame = decode_stdio_frame_payload(&payload)?;
        self.wire_bytes_read = next_total;
        Ok(Some(frame))
    }

    #[must_use]
    pub fn into_inner(self) -> R {
        self.reader
    }
}

pub fn decode_stdio_frame_payload(
    payload: &[u8],
) -> Result<StdioProtocolFrameV1, StdioProtocolErrorV1> {
    if payload.is_empty() {
        return Err(StdioProtocolErrorV1::ZeroLengthFrame);
    }
    if payload.len() > MAX_STDIO_FRAME_BYTES {
        return Err(StdioProtocolErrorV1::FrameTooLarge);
    }
    if payload.last().is_some_and(u8::is_ascii_whitespace) {
        return Err(StdioProtocolErrorV1::TrailingBytes);
    }
    let frame = serde_json::from_slice::<StdioProtocolFrameV1>(payload)
        .map_err(StdioProtocolErrorV1::InvalidFrame)?;
    frame.validate().map_err(|_| StdioProtocolErrorV1::ContractViolation)?;
    Ok(frame)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StdioRuntimeFrameDispositionV1 {
    HandshakeAccepted,
    Progress,
    ArtifactReady,
    CancellationAcknowledged,
    Terminal,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StdioHostTerminalEventV1 {
    Exited { exit_code: u32 },
    Cancelled,
    TimedOut,
    PolicyViolation { code: String },
}

#[derive(Debug, Error)]
pub enum StdioHostEventErrorV1 {
    #[error("Sandbox Host event identity does not match the authorized execution")]
    IdentityMismatch,
    #[error("Sandbox Host output chunk sequence is not contiguous")]
    OutputSequenceInvalid,
    #[error("Sandbox Host emitted multiple execution terminal events")]
    TerminalConflict,
    #[error("Sandbox Host emitted process output after its execution terminal")]
    OutputAfterTerminal,
    #[error(transparent)]
    Protocol(#[from] StdioProtocolErrorV1),
}

/// Core-side state for translating Sandbox Host byte chunks into one Aura
/// Stdio Attempt protocol. Host chunk boundaries are transport facts only;
/// protocol frames may span or share chunks.
pub struct StdioSandboxHostTrackerV1 {
    sandbox_execution_id: SandboxExecutionId,
    arguments: serde_json::Value,
    arguments_hash: ContentHash,
    decoder: StdioRuntimeFrameDecoderV1,
    protocol: StdioAttemptProtocolV1,
    next_stdout_sequence: u64,
    next_stderr_sequence: u64,
    stderr: Vec<u8>,
    pending_progress: Vec<StdioObservedProgressV1>,
    accepted_artifacts: Vec<StdioObservedArtifactReadyV1>,
    terminal_event: Option<StdioHostTerminalEventV1>,
    cleanup_failed: Option<String>,
}

/// One validated Runtime progress frame awaiting Core operational-state
/// persistence. This is an in-process observation, not a second public wire
/// contract and not an EventStore lifecycle event.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StdioObservedProgressV1 {
    pub sequence: u64,
    pub payload: StdioProgressPayloadV1,
}

/// One identity- and sequence-validated `artifact_ready` observation. File
/// authority, no-follow verification, byte limits, media policy and import
/// remain Core responsibilities after the Runtime reaches a terminal state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StdioObservedArtifactReadyV1 {
    pub sequence: u64,
    pub payload: StdioArtifactReadyPayloadV1,
}

impl StdioSandboxHostTrackerV1 {
    #[must_use]
    pub fn new(
        sandbox_execution_id: SandboxExecutionId,
        tool_call_id: ToolCallId,
        tool_execution_attempt_id: ToolExecutionAttemptId,
        arguments: serde_json::Value,
        arguments_hash: ContentHash,
    ) -> Self {
        Self {
            sandbox_execution_id,
            arguments,
            arguments_hash,
            decoder: StdioRuntimeFrameDecoderV1::new(),
            protocol: StdioAttemptProtocolV1::new(tool_call_id, tool_execution_attempt_id),
            next_stdout_sequence: 0,
            next_stderr_sequence: 0,
            stderr: Vec::new(),
            pending_progress: Vec::new(),
            accepted_artifacts: Vec::new(),
            terminal_event: None,
            cleanup_failed: None,
        }
    }

    /// Accepts one typed Host event and returns the Core protocol frames that
    /// must be forwarded to Runtime. V1 emits only the Execute frame in direct
    /// response to a valid Hello; cancellation is prepared explicitly.
    pub fn accept_host_event(
        &mut self,
        event: SandboxHostEvent,
    ) -> Result<Vec<StdioProtocolFrameV1>, StdioHostEventErrorV1> {
        if sandbox_event_execution_id(&event) != self.sandbox_execution_id {
            return Err(StdioHostEventErrorV1::IdentityMismatch);
        }
        match event {
            SandboxHostEvent::StdoutChunk { sequence, bytes, .. } => {
                if self.terminal_event.is_some() {
                    return Err(StdioHostEventErrorV1::OutputAfterTerminal);
                }
                if sequence != self.next_stdout_sequence {
                    return Err(StdioHostEventErrorV1::OutputSequenceInvalid);
                }
                self.next_stdout_sequence = self
                    .next_stdout_sequence
                    .checked_add(1)
                    .ok_or(StdioHostEventErrorV1::OutputSequenceInvalid)?;
                let mut outbound = Vec::new();
                let mut accepted_progress = Vec::new();
                let mut accepted_artifacts = Vec::new();
                for frame in self.decoder.push_chunk(&bytes)? {
                    let progress = match &frame {
                        StdioProtocolFrameV1::Progress { sequence, payload, .. } => {
                            Some(StdioObservedProgressV1 {
                                sequence: *sequence,
                                payload: payload.clone(),
                            })
                        }
                        _ => None,
                    };
                    let artifact = match &frame {
                        StdioProtocolFrameV1::ArtifactReady { sequence, payload, .. } => {
                            Some(StdioObservedArtifactReadyV1 {
                                sequence: *sequence,
                                payload: payload.clone(),
                            })
                        }
                        _ => None,
                    };
                    match self.protocol.accept_runtime_frame(frame)? {
                        StdioRuntimeFrameDispositionV1::HandshakeAccepted => {
                            outbound.push(self.protocol.prepare_execute(
                                self.arguments.clone(),
                                self.arguments_hash.clone(),
                            )?);
                        }
                        StdioRuntimeFrameDispositionV1::Progress => {
                            let progress =
                                progress.ok_or(StdioProtocolErrorV1::RuntimeMessageStateInvalid)?;
                            accepted_progress.push(progress);
                        }
                        StdioRuntimeFrameDispositionV1::ArtifactReady => {
                            accepted_artifacts.push(
                                artifact.ok_or(StdioProtocolErrorV1::RuntimeMessageStateInvalid)?,
                            );
                        }
                        StdioRuntimeFrameDispositionV1::CancellationAcknowledged
                        | StdioRuntimeFrameDispositionV1::Terminal => {}
                    }
                }
                self.pending_progress.extend(accepted_progress);
                self.accepted_artifacts.extend(accepted_artifacts);
                Ok(outbound)
            }
            SandboxHostEvent::StderrChunk { sequence, bytes, .. } => {
                if self.terminal_event.is_some() {
                    return Err(StdioHostEventErrorV1::OutputAfterTerminal);
                }
                if sequence != self.next_stderr_sequence {
                    return Err(StdioHostEventErrorV1::OutputSequenceInvalid);
                }
                self.next_stderr_sequence = self
                    .next_stderr_sequence
                    .checked_add(1)
                    .ok_or(StdioHostEventErrorV1::OutputSequenceInvalid)?;
                self.stderr.extend_from_slice(&bytes);
                Ok(Vec::new())
            }
            SandboxHostEvent::Exit { exit_code, .. } => {
                self.set_terminal(StdioHostTerminalEventV1::Exited { exit_code })?;
                Ok(Vec::new())
            }
            SandboxHostEvent::Cancelled { .. } => {
                self.set_terminal(StdioHostTerminalEventV1::Cancelled)?;
                Ok(Vec::new())
            }
            SandboxHostEvent::Timeout { .. } => {
                self.set_terminal(StdioHostTerminalEventV1::TimedOut)?;
                Ok(Vec::new())
            }
            SandboxHostEvent::PolicyViolation { code, .. } => {
                self.set_terminal(StdioHostTerminalEventV1::PolicyViolation { code })?;
                Ok(Vec::new())
            }
            SandboxHostEvent::CleanupFailed { code, .. } => {
                self.cleanup_failed = Some(code);
                Ok(Vec::new())
            }
            SandboxHostEvent::Started { .. }
            | SandboxHostEvent::ProcessStarted { .. }
            | SandboxHostEvent::CleanupCompleted { .. } => Ok(Vec::new()),
        }
    }

    #[must_use]
    pub const fn sandbox_execution_id(&self) -> SandboxExecutionId {
        self.sandbox_execution_id
    }

    pub fn prepare_cancel(&mut self) -> Result<StdioProtocolFrameV1, StdioHostEventErrorV1> {
        self.protocol.prepare_cancel().map_err(Into::into)
    }

    pub fn finish(&self) -> Result<(), StdioHostEventErrorV1> {
        self.decoder.finish_on_eof()?;
        self.protocol.finish_on_eof()?;
        Ok(())
    }

    #[must_use]
    pub const fn terminal_frame(&self) -> Option<&StdioProtocolFrameV1> {
        self.protocol.terminal_frame()
    }

    #[must_use]
    pub const fn terminal_event(&self) -> Option<&StdioHostTerminalEventV1> {
        self.terminal_event.as_ref()
    }

    #[must_use]
    pub fn stderr(&self) -> &[u8] {
        &self.stderr
    }

    /// Drains progress only after the containing Host event was accepted in
    /// full. A later invalid frame in the same stdout chunk therefore cannot
    /// publish a partially accepted operational update.
    pub fn take_progress(&mut self) -> Vec<StdioObservedProgressV1> {
        std::mem::take(&mut self.pending_progress)
    }

    #[must_use]
    pub fn accepted_artifacts(&self) -> &[StdioObservedArtifactReadyV1] {
        &self.accepted_artifacts
    }

    #[must_use]
    pub fn cleanup_failed(&self) -> Option<&str> {
        self.cleanup_failed.as_deref()
    }

    fn set_terminal(
        &mut self,
        terminal: StdioHostTerminalEventV1,
    ) -> Result<(), StdioHostEventErrorV1> {
        if self.terminal_event.replace(terminal).is_some() {
            return Err(StdioHostEventErrorV1::TerminalConflict);
        }
        Ok(())
    }
}

fn sandbox_event_execution_id(event: &SandboxHostEvent) -> SandboxExecutionId {
    match event {
        SandboxHostEvent::Started { execution_id }
        | SandboxHostEvent::ProcessStarted { execution_id, .. }
        | SandboxHostEvent::StdoutChunk { execution_id, .. }
        | SandboxHostEvent::StderrChunk { execution_id, .. }
        | SandboxHostEvent::Exit { execution_id, .. }
        | SandboxHostEvent::Cancelled { execution_id }
        | SandboxHostEvent::Timeout { execution_id }
        | SandboxHostEvent::PolicyViolation { execution_id, .. }
        | SandboxHostEvent::CleanupCompleted { execution_id }
        | SandboxHostEvent::CleanupFailed { execution_id, .. } => *execution_id,
    }
}

pub struct StdioAttemptProtocolV1 {
    tool_call_id: ToolCallId,
    tool_execution_attempt_id: ToolExecutionAttemptId,
    next_core_sequence: u64,
    next_runtime_sequence: u64,
    hello_received: bool,
    execute_sent: bool,
    cancel_sent: bool,
    cancel_acknowledged: bool,
    terminal_frame: Option<StdioProtocolFrameV1>,
}

impl StdioAttemptProtocolV1 {
    #[must_use]
    pub const fn new(
        tool_call_id: ToolCallId,
        tool_execution_attempt_id: ToolExecutionAttemptId,
    ) -> Self {
        Self {
            tool_call_id,
            tool_execution_attempt_id,
            next_core_sequence: 1,
            next_runtime_sequence: 1,
            hello_received: false,
            execute_sent: false,
            cancel_sent: false,
            cancel_acknowledged: false,
            terminal_frame: None,
        }
    }

    pub fn prepare_execute(
        &mut self,
        arguments: serde_json::Value,
        arguments_hash: ContentHash,
    ) -> Result<StdioProtocolFrameV1, StdioProtocolErrorV1> {
        if !self.hello_received || self.execute_sent || self.terminal_frame.is_some() {
            return Err(StdioProtocolErrorV1::CoreMessageStateInvalid);
        }
        let frame = StdioProtocolFrameV1::Execute {
            protocol_version: STDIO_PROTOCOL_VERSION_V1,
            tool_call_id: self.tool_call_id,
            tool_execution_attempt_id: self.tool_execution_attempt_id,
            sequence: self.next_core_sequence,
            payload: StdioExecutePayloadV1 { arguments, arguments_hash },
        };
        self.next_core_sequence = increment_sequence(self.next_core_sequence)?;
        self.execute_sent = true;
        Ok(frame)
    }

    pub fn prepare_cancel(&mut self) -> Result<StdioProtocolFrameV1, StdioProtocolErrorV1> {
        if !self.execute_sent || self.cancel_sent || self.terminal_frame.is_some() {
            return Err(StdioProtocolErrorV1::CoreMessageStateInvalid);
        }
        let frame = StdioProtocolFrameV1::Cancel {
            protocol_version: STDIO_PROTOCOL_VERSION_V1,
            tool_call_id: self.tool_call_id,
            tool_execution_attempt_id: self.tool_execution_attempt_id,
            sequence: self.next_core_sequence,
            payload: StdioEmptyPayloadV1::default(),
        };
        self.next_core_sequence = increment_sequence(self.next_core_sequence)?;
        self.cancel_sent = true;
        Ok(frame)
    }

    pub fn accept_runtime_frame(
        &mut self,
        frame: StdioProtocolFrameV1,
    ) -> Result<StdioRuntimeFrameDispositionV1, StdioProtocolErrorV1> {
        frame.validate().map_err(|_| StdioProtocolErrorV1::ContractViolation)?;
        if frame.direction() != StdioProtocolDirectionV1::RuntimeToCore {
            return Err(StdioProtocolErrorV1::DirectionInvalid);
        }
        if frame.tool_call_id() != self.tool_call_id
            || frame.tool_execution_attempt_id() != self.tool_execution_attempt_id
        {
            return Err(StdioProtocolErrorV1::IdentityMismatch);
        }
        if frame.sequence() != self.next_runtime_sequence {
            return Err(StdioProtocolErrorV1::SequenceInvalid);
        }
        if self.terminal_frame.is_some() {
            return Err(StdioProtocolErrorV1::TerminalConflict);
        }

        let disposition = match &frame {
            StdioProtocolFrameV1::Hello { .. } if !self.hello_received && !self.execute_sent => {
                self.hello_received = true;
                StdioRuntimeFrameDispositionV1::HandshakeAccepted
            }
            StdioProtocolFrameV1::Hello { .. } => {
                return Err(StdioProtocolErrorV1::HandshakeInvalid);
            }
            StdioProtocolFrameV1::Progress { .. } if self.execute_sent => {
                StdioRuntimeFrameDispositionV1::Progress
            }
            StdioProtocolFrameV1::ArtifactReady { .. } if self.execute_sent => {
                StdioRuntimeFrameDispositionV1::ArtifactReady
            }
            StdioProtocolFrameV1::CancelAcknowledged { .. }
                if self.cancel_sent && !self.cancel_acknowledged =>
            {
                self.cancel_acknowledged = true;
                StdioRuntimeFrameDispositionV1::CancellationAcknowledged
            }
            StdioProtocolFrameV1::FinalResult { .. }
            | StdioProtocolFrameV1::FinalFailure { .. }
                if self.execute_sent =>
            {
                self.terminal_frame = Some(frame.clone());
                StdioRuntimeFrameDispositionV1::Terminal
            }
            StdioProtocolFrameV1::Execute { .. } | StdioProtocolFrameV1::Cancel { .. } => {
                return Err(StdioProtocolErrorV1::DirectionInvalid);
            }
            _ if !self.hello_received => {
                return Err(StdioProtocolErrorV1::HandshakeInvalid);
            }
            _ => return Err(StdioProtocolErrorV1::RuntimeMessageStateInvalid),
        };
        self.next_runtime_sequence = increment_sequence(self.next_runtime_sequence)?;
        Ok(disposition)
    }

    pub fn finish_on_eof(&self) -> Result<(), StdioProtocolErrorV1> {
        self.terminal_frame.as_ref().map(|_| ()).ok_or(StdioProtocolErrorV1::TerminalMissing)
    }

    #[must_use]
    pub const fn terminal_frame(&self) -> Option<&StdioProtocolFrameV1> {
        self.terminal_frame.as_ref()
    }

    #[must_use]
    pub const fn cancellation_acknowledged(&self) -> bool {
        self.cancel_acknowledged
    }
}

fn increment_sequence(sequence: u64) -> Result<u64, StdioProtocolErrorV1> {
    sequence.checked_add(1).ok_or(StdioProtocolErrorV1::SequenceInvalid)
}

#[cfg(test)]
mod tests {
    use aura_domain_common::ContentHash;
    use aura_domain_common::{ToolCallId, ToolExecutionAttemptId};
    use aura_tool_contract::{
        STDIO_PROTOCOL_VERSION_V1, StdioEmptyPayloadV1, StdioFinalResultPayloadV1,
        StdioHelloPayloadV1, StdioProgressPayloadV1, StdioProtocolFrameV1, ToolRuntimeErrorCode,
    };
    use serde_json::json;
    use tokio::io::{AsyncWriteExt, duplex};

    use super::*;

    fn ids() -> (ToolCallId, ToolExecutionAttemptId) {
        (ToolCallId::from_bytes([1; 16]), ToolExecutionAttemptId::from_bytes([2; 16]))
    }

    fn hello(sequence: u64) -> StdioProtocolFrameV1 {
        let (tool_call_id, tool_execution_attempt_id) = ids();
        StdioProtocolFrameV1::Hello {
            protocol_version: STDIO_PROTOCOL_VERSION_V1,
            tool_call_id,
            tool_execution_attempt_id,
            sequence,
            payload: StdioHelloPayloadV1 {
                runtime_name: "fixture-runtime".to_owned(),
                runtime_version: "1.0.0".to_owned(),
            },
        }
    }

    fn progress(sequence: u64) -> StdioProtocolFrameV1 {
        let (tool_call_id, tool_execution_attempt_id) = ids();
        StdioProtocolFrameV1::Progress {
            protocol_version: STDIO_PROTOCOL_VERSION_V1,
            tool_call_id,
            tool_execution_attempt_id,
            sequence,
            payload: StdioProgressPayloadV1 {
                percentage: Some(50),
                message: Some("halfway".to_owned()),
            },
        }
    }

    fn final_result(sequence: u64) -> StdioProtocolFrameV1 {
        let (tool_call_id, tool_execution_attempt_id) = ids();
        StdioProtocolFrameV1::FinalResult {
            protocol_version: STDIO_PROTOCOL_VERSION_V1,
            tool_call_id,
            tool_execution_attempt_id,
            sequence,
            payload: StdioFinalResultPayloadV1 { result: json!({"ok": true}) },
        }
    }

    fn stdout_event(
        execution_id: SandboxExecutionId,
        sequence: u64,
        bytes: Vec<u8>,
    ) -> SandboxHostEvent {
        SandboxHostEvent::StdoutChunk { execution_id, sequence, bytes }
    }

    #[test]
    fn sandbox_host_tracker_drives_fragmented_hello_execute_and_terminal_chain() {
        let execution_id = SandboxExecutionId::new();
        let (tool_call_id, tool_execution_attempt_id) = ids();
        let arguments = serde_json::json!({"query": "fixture"});
        let arguments_hash = ContentHash::sha256(b"fixture arguments");
        let mut tracker = StdioSandboxHostTrackerV1::new(
            execution_id,
            tool_call_id,
            tool_execution_attempt_id,
            arguments.clone(),
            arguments_hash.clone(),
        );
        let hello = encode_stdio_frame_bytes(&hello(1)).expect("encode hello");
        let split = hello.len() / 2;
        assert!(
            tracker
                .accept_host_event(stdout_event(execution_id, 0, hello[..split].to_vec()))
                .expect("first hello fragment")
                .is_empty()
        );
        let outbound = tracker
            .accept_host_event(stdout_event(execution_id, 1, hello[split..].to_vec()))
            .expect("second hello fragment");
        assert_eq!(outbound.len(), 1);
        assert!(matches!(
            &outbound[0],
            StdioProtocolFrameV1::Execute {
                sequence: 1,
                payload: StdioExecutePayloadV1 {
                    arguments: actual_arguments,
                    arguments_hash: actual_hash,
                },
                ..
            } if actual_arguments == &arguments && actual_hash == &arguments_hash
        ));

        tracker
            .accept_host_event(stdout_event(
                execution_id,
                2,
                encode_stdio_frame_bytes(&progress(2)).expect("encode progress"),
            ))
            .expect("progress frame");
        assert_eq!(
            tracker.take_progress(),
            vec![StdioObservedProgressV1 {
                sequence: 2,
                payload: StdioProgressPayloadV1 {
                    percentage: Some(50),
                    message: Some("halfway".to_owned()),
                },
            }]
        );
        assert!(tracker.take_progress().is_empty());

        tracker
            .accept_host_event(stdout_event(
                execution_id,
                3,
                encode_stdio_frame_bytes(&final_result(3)).expect("encode final"),
            ))
            .expect("terminal frame");
        tracker
            .accept_host_event(SandboxHostEvent::Exit { execution_id, exit_code: 0 })
            .expect("host exit");
        tracker.finish().expect("complete protocol and byte stream");
        assert!(matches!(tracker.terminal_frame(), Some(StdioProtocolFrameV1::FinalResult { .. })));
        assert_eq!(
            tracker.terminal_event(),
            Some(&StdioHostTerminalEventV1::Exited { exit_code: 0 })
        );
    }

    #[test]
    fn sandbox_host_tracker_rejects_identity_sequence_and_post_terminal_output() {
        let execution_id = SandboxExecutionId::new();
        let other_execution_id = SandboxExecutionId::new();
        let (tool_call_id, tool_execution_attempt_id) = ids();
        let mut tracker = StdioSandboxHostTrackerV1::new(
            execution_id,
            tool_call_id,
            tool_execution_attempt_id,
            serde_json::json!({}),
            ContentHash::sha256(b"arguments"),
        );
        assert!(matches!(
            tracker
                .accept_host_event(SandboxHostEvent::Started { execution_id: other_execution_id }),
            Err(StdioHostEventErrorV1::IdentityMismatch)
        ));
        assert!(matches!(
            tracker.accept_host_event(stdout_event(execution_id, 1, vec![1])),
            Err(StdioHostEventErrorV1::OutputSequenceInvalid)
        ));

        tracker
            .accept_host_event(SandboxHostEvent::PolicyViolation {
                execution_id,
                code: "sandbox.fixture".to_owned(),
            })
            .expect("first terminal");
        assert!(matches!(
            tracker.accept_host_event(stdout_event(execution_id, 0, vec![1])),
            Err(StdioHostEventErrorV1::OutputAfterTerminal)
        ));
        assert!(matches!(
            tracker.accept_host_event(SandboxHostEvent::Exit { execution_id, exit_code: 1 }),
            Err(StdioHostEventErrorV1::TerminalConflict)
        ));
    }

    #[test]
    fn sandbox_host_tracker_does_not_publish_progress_from_a_rejected_stdout_event() {
        let execution_id = SandboxExecutionId::new();
        let (tool_call_id, tool_execution_attempt_id) = ids();
        let mut tracker = StdioSandboxHostTrackerV1::new(
            execution_id,
            tool_call_id,
            tool_execution_attempt_id,
            serde_json::json!({}),
            ContentHash::sha256(b"arguments"),
        );
        tracker
            .accept_host_event(stdout_event(
                execution_id,
                0,
                encode_stdio_frame_bytes(&hello(1)).expect("encode hello"),
            ))
            .expect("accept hello");

        let mut frames = encode_stdio_frame_bytes(&progress(2)).expect("encode first progress");
        frames.extend(encode_stdio_frame_bytes(&progress(4)).expect("encode invalid progress"));
        assert!(matches!(
            tracker.accept_host_event(stdout_event(execution_id, 1, frames)),
            Err(StdioHostEventErrorV1::Protocol(StdioProtocolErrorV1::SequenceInvalid))
        ));
        assert!(tracker.take_progress().is_empty());
    }

    #[tokio::test]
    async fn length_prefixed_frame_round_trips_and_counts_wire_bytes() {
        let (mut writer, reader) = duplex(4096);
        let expected = hello(1);
        let writer_task = tokio::spawn(async move {
            write_stdio_frame(&mut writer, &expected).await.expect("write frame");
            expected
        });
        let mut reader = StdioRuntimeFrameReaderV1::new(reader);
        let actual = reader.read_frame().await.expect("read frame").expect("one frame");
        let expected = writer_task.await.expect("writer task");
        assert_eq!(actual, expected);
        assert!(reader.wire_bytes_read() > FRAME_PREFIX_BYTES);
    }

    #[tokio::test]
    async fn framing_rejects_zero_truncation_trailing_and_wire_overflow() {
        let (mut zero_writer, zero_reader) = duplex(16);
        zero_writer.write_all(&0_u32.to_be_bytes()).await.expect("write zero");
        drop(zero_writer);
        assert!(matches!(
            StdioRuntimeFrameReaderV1::new(zero_reader).read_frame().await,
            Err(StdioProtocolErrorV1::ZeroLengthFrame)
        ));

        let (mut prefix_writer, prefix_reader) = duplex(16);
        prefix_writer.write_all(&[0, 0]).await.expect("write partial prefix");
        drop(prefix_writer);
        assert!(matches!(
            StdioRuntimeFrameReaderV1::new(prefix_reader).read_frame().await,
            Err(StdioProtocolErrorV1::TruncatedPrefix)
        ));

        let (mut payload_writer, payload_reader) = duplex(32);
        payload_writer.write_all(&10_u32.to_be_bytes()).await.expect("write length");
        payload_writer.write_all(b"short").await.expect("write partial payload");
        drop(payload_writer);
        assert!(matches!(
            StdioRuntimeFrameReaderV1::new(payload_reader).read_frame().await,
            Err(StdioProtocolErrorV1::TruncatedPayload)
        ));

        let mut trailing = serde_json::to_vec(&hello(1)).expect("hello JSON");
        trailing.push(b' ');
        assert!(matches!(
            decode_stdio_frame_payload(&trailing),
            Err(StdioProtocolErrorV1::TrailingBytes)
        ));

        let payload = serde_json::to_vec(&hello(1)).expect("hello JSON");
        let (mut limit_writer, limit_reader) = duplex(4096);
        limit_writer
            .write_all(&u32::try_from(payload.len()).expect("length").to_be_bytes())
            .await
            .expect("write length");
        limit_writer.write_all(&payload).await.expect("write payload");
        let mut reader = StdioRuntimeFrameReaderV1::with_wire_limit(limit_reader, payload.len());
        let error = reader.read_frame().await.expect_err("prefix must count toward wire limit");
        assert!(matches!(error, StdioProtocolErrorV1::StdoutWireLimitExceeded));
        assert_eq!(error.error_code(), ToolRuntimeErrorCode::IoLimitExceeded);
    }

    #[test]
    fn chunk_decoder_preserves_frame_boundaries_and_rejects_incomplete_eof() {
        let first = encode_stdio_frame_bytes(&hello(1)).expect("encode hello");
        let second = encode_stdio_frame_bytes(&progress(2)).expect("encode progress");
        let split = first.len() / 2;
        let mut decoder = StdioRuntimeFrameDecoderV1::new();

        assert!(decoder.push_chunk(&first[..split]).expect("partial hello").is_empty());
        let mut combined = first[split..].to_vec();
        combined.extend_from_slice(&second);
        assert_eq!(
            decoder.push_chunk(&combined).expect("remaining frames"),
            vec![hello(1), progress(2)]
        );
        assert_eq!(decoder.wire_bytes_received(), first.len() + second.len());
        decoder.finish_on_eof().expect("complete frames at EOF");

        let mut partial_prefix = StdioRuntimeFrameDecoderV1::new();
        partial_prefix.push_chunk(&[0, 0]).expect("partial prefix");
        assert!(matches!(
            partial_prefix.finish_on_eof(),
            Err(StdioProtocolErrorV1::TruncatedPrefix)
        ));

        let mut partial_payload = StdioRuntimeFrameDecoderV1::new();
        partial_payload.push_chunk(&10_u32.to_be_bytes()).expect("declared payload");
        assert!(matches!(
            partial_payload.finish_on_eof(),
            Err(StdioProtocolErrorV1::TruncatedPayload)
        ));

        let mut limited = StdioRuntimeFrameDecoderV1::with_wire_limit(first.len() - 1);
        assert!(matches!(
            limited.push_chunk(&first),
            Err(StdioProtocolErrorV1::StdoutWireLimitExceeded)
        ));
    }

    #[test]
    fn payload_decoder_rejects_unknown_message_types_and_fields() {
        let (tool_call_id, tool_execution_attempt_id) = ids();
        let unknown_type = serde_json::to_vec(&json!({
            "message_type": "keep_alive",
            "protocol_version": STDIO_PROTOCOL_VERSION_V1,
            "tool_call_id": tool_call_id,
            "tool_execution_attempt_id": tool_execution_attempt_id,
            "sequence": 1,
            "payload": {}
        }))
        .expect("unknown type JSON");
        assert!(matches!(
            decode_stdio_frame_payload(&unknown_type),
            Err(StdioProtocolErrorV1::InvalidFrame(_))
        ));

        let unknown_field = serde_json::to_vec(&json!({
            "message_type": "hello",
            "protocol_version": STDIO_PROTOCOL_VERSION_V1,
            "tool_call_id": tool_call_id,
            "tool_execution_attempt_id": tool_execution_attempt_id,
            "sequence": 1,
            "payload": {
                "runtime_name": "fixture-runtime",
                "runtime_version": "1.0.0",
                "keep_alive": true
            }
        }))
        .expect("unknown field JSON");
        assert!(matches!(
            decode_stdio_frame_payload(&unknown_field),
            Err(StdioProtocolErrorV1::InvalidFrame(_))
        ));
    }

    #[test]
    fn protocol_state_requires_hello_execute_and_one_terminal_with_independent_sequences() {
        let (tool_call_id, attempt_id) = ids();
        let mut protocol = StdioAttemptProtocolV1::new(tool_call_id, attempt_id);
        assert!(matches!(
            protocol.accept_runtime_frame(progress(1)),
            Err(StdioProtocolErrorV1::HandshakeInvalid)
        ));
        assert_eq!(
            protocol.accept_runtime_frame(hello(1)).expect("hello"),
            StdioRuntimeFrameDispositionV1::HandshakeAccepted
        );
        let execute = protocol
            .prepare_execute(json!({"value": "fixture"}), ContentHash::sha256(b"arguments"))
            .expect("execute");
        assert_eq!(execute.sequence(), 1);
        assert!(matches!(execute, StdioProtocolFrameV1::Execute { .. }));
        assert!(matches!(
            protocol.prepare_execute(json!({}), ContentHash::sha256(b"again")),
            Err(StdioProtocolErrorV1::CoreMessageStateInvalid)
        ));
        assert_eq!(
            protocol.accept_runtime_frame(progress(2)).expect("progress"),
            StdioRuntimeFrameDispositionV1::Progress
        );
        assert_eq!(
            protocol.accept_runtime_frame(final_result(3)).expect("terminal"),
            StdioRuntimeFrameDispositionV1::Terminal
        );
        protocol.finish_on_eof().expect("terminal before EOF");
        assert!(matches!(
            protocol.accept_runtime_frame(progress(4)),
            Err(StdioProtocolErrorV1::TerminalConflict)
        ));
    }

    #[test]
    fn cancellation_is_core_intent_and_acknowledgement_is_not_a_terminal() {
        let (tool_call_id, attempt_id) = ids();
        let mut protocol = StdioAttemptProtocolV1::new(tool_call_id, attempt_id);
        protocol.accept_runtime_frame(hello(1)).expect("hello");
        protocol.prepare_execute(json!({}), ContentHash::sha256(b"arguments")).expect("execute");
        let cancel = protocol.prepare_cancel().expect("cancel");
        assert_eq!(cancel.sequence(), 2);
        let acknowledged = StdioProtocolFrameV1::CancelAcknowledged {
            protocol_version: STDIO_PROTOCOL_VERSION_V1,
            tool_call_id,
            tool_execution_attempt_id: attempt_id,
            sequence: 2,
            payload: StdioEmptyPayloadV1::default(),
        };
        assert_eq!(
            protocol.accept_runtime_frame(acknowledged).expect("acknowledgement"),
            StdioRuntimeFrameDispositionV1::CancellationAcknowledged
        );
        assert!(protocol.cancellation_acknowledged());
        assert!(matches!(protocol.finish_on_eof(), Err(StdioProtocolErrorV1::TerminalMissing)));
    }

    #[test]
    fn identity_and_sequence_mismatches_fail_closed_without_advancing_state() {
        let (tool_call_id, attempt_id) = ids();
        let mut protocol = StdioAttemptProtocolV1::new(tool_call_id, attempt_id);
        let mut wrong_identity = hello(1);
        if let StdioProtocolFrameV1::Hello { tool_call_id, .. } = &mut wrong_identity {
            *tool_call_id = ToolCallId::from_bytes([9; 16]);
        }
        assert!(matches!(
            protocol.accept_runtime_frame(wrong_identity),
            Err(StdioProtocolErrorV1::IdentityMismatch)
        ));
        assert!(matches!(
            protocol.accept_runtime_frame(hello(2)),
            Err(StdioProtocolErrorV1::SequenceInvalid)
        ));
        protocol.accept_runtime_frame(hello(1)).expect("state remained at sequence one");
    }
}
