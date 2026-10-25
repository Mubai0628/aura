use aura_domain_common::{IpcChannelId, StreamId};
use aura_runtime_contract::{BinaryStreamFrame, IpcFrameHeader};
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use zeroize::Zeroizing;

use crate::secret::{
    SECRET_INPUT_FIXED_HEADER_SIZE, SecretInputFrameV1, append_secret_input_payload,
    decode_secret_input_payload,
};
use crate::tool_conformance_runtime_secret::{
    ToolConformanceRuntimeSecretFrameV1, append_tool_conformance_runtime_secret_payload,
    decode_tool_conformance_runtime_secret_payload,
};
use crate::tool_runtime_secret::{
    ToolRuntimeSecretFrameV1, append_tool_runtime_secret_payload,
    decode_tool_runtime_secret_payload,
};

pub const FRAME_LENGTH_PREFIX_SIZE: usize = 4;
pub const MAX_FRAME_LENGTH: usize = 16 * 1024 * 1024;
/// Control frames carry JSON-RPC commands, command results, and projection
/// snapshots. Provider Catalog V1 is a single, non-paginated document whose
/// production payload can reach 5 MiB, so reserve another MiB for the JSON-RPC
/// envelope and future additive protocol metadata.
pub const MAX_CONTROL_PAYLOAD_BYTES: usize = 6 * 1024 * 1024;
pub const MAX_BINARY_CHUNK_BYTES: usize = 256 * 1024;
pub const BINARY_CHUNK_HEADER_SIZE: usize = 16 + 8 + 1;
pub const AUTHENTICATED_FRAME_HEADER_SIZE: usize = 16 + 8 + 8;
pub const BINARY_END_OF_MESSAGE: u8 = 0b0000_0001;
const BINARY_KNOWN_FLAGS: u8 = BINARY_END_OF_MESSAGE;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum FrameType {
    Control = 1,
    BinaryChunk = 2,
    SecretInput = 3,
    ToolRuntimeSecret = 4,
    ToolConformanceRuntimeSecret = 5,
}

impl TryFrom<u8> for FrameType {
    type Error = FrameDecodeError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Control),
            2 => Ok(Self::BinaryChunk),
            3 => Ok(Self::SecretInput),
            4 => Ok(Self::ToolRuntimeSecret),
            5 => Ok(Self::ToolConformanceRuntimeSecret),
            other => Err(FrameDecodeError::UnknownFrameType(other)),
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum WireFrame {
    Control(Vec<u8>),
    BinaryChunk { metadata: BinaryStreamFrame, data: Vec<u8> },
    SecretInput(SecretInputFrameV1),
    ToolRuntimeSecret(ToolRuntimeSecretFrameV1),
    ToolConformanceRuntimeSecret(ToolConformanceRuntimeSecretFrameV1),
}

#[derive(Debug, Eq, PartialEq)]
pub struct AuthenticatedWireFrame {
    pub header: IpcFrameHeader,
    pub frame: WireFrame,
}

impl WireFrame {
    #[must_use]
    pub fn encoded_len(&self) -> usize {
        FRAME_LENGTH_PREFIX_SIZE
            + 1
            + match self {
                Self::Control(payload) => payload.len(),
                Self::BinaryChunk { data, .. } => BINARY_CHUNK_HEADER_SIZE + data.len(),
                Self::SecretInput(frame) => {
                    SECRET_INPUT_FIXED_HEADER_SIZE + frame.secret().expose().len()
                }
                Self::ToolRuntimeSecret(frame) => frame.payload_len(),
                Self::ToolConformanceRuntimeSecret(frame) => frame.payload_len(),
            }
    }
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum FrameDecodeError {
    #[error("frame length must include one frame type byte")]
    EmptyFrame,
    #[error("frame length {actual} exceeds the {maximum}-byte limit")]
    FrameTooLarge { actual: usize, maximum: usize },
    #[error("control payload length {actual} exceeds the {maximum}-byte limit")]
    ControlPayloadTooLarge { actual: usize, maximum: usize },
    #[error("binary chunk length {actual} exceeds the {maximum}-byte limit")]
    BinaryChunkTooLarge { actual: usize, maximum: usize },
    #[error("binary metadata declares {declared} bytes but the frame contains {actual}")]
    BinaryDataLengthMismatch { declared: u32, actual: usize },
    #[error("binary frame is shorter than its fixed {BINARY_CHUNK_HEADER_SIZE}-byte header")]
    TruncatedBinaryHeader,
    #[error(
        "authenticated frame is shorter than its fixed {AUTHENTICATED_FRAME_HEADER_SIZE}-byte channel header"
    )]
    TruncatedAuthenticatedHeader,
    #[error("binary frame uses unknown flags 0x{0:02x}")]
    UnknownBinaryFlags(u8),
    #[error("unknown frame type 0x{0:02x}")]
    UnknownFrameType(u8),
    #[error("secret-input frame is malformed")]
    InvalidSecretInput,
    #[error("tool-runtime-secret frame is malformed")]
    InvalidToolRuntimeSecret,
    #[error("tool-conformance-runtime-secret frame is malformed")]
    InvalidToolConformanceRuntimeSecret,
    #[error("frame length cannot be represented by the protocol")]
    LengthOverflow,
    #[error("I/O error while transferring an IPC frame: {0}")]
    Io(String),
}

pub fn encode_frame(frame: &WireFrame) -> Result<Zeroizing<Vec<u8>>, FrameDecodeError> {
    let body_length = frame
        .encoded_len()
        .checked_sub(FRAME_LENGTH_PREFIX_SIZE)
        .ok_or(FrameDecodeError::LengthOverflow)?;
    if body_length > MAX_FRAME_LENGTH {
        return Err(FrameDecodeError::FrameTooLarge {
            actual: body_length,
            maximum: MAX_FRAME_LENGTH,
        });
    }
    let length = u32::try_from(body_length).map_err(|_| FrameDecodeError::LengthOverflow)?;
    let mut encoded = Zeroizing::new(Vec::with_capacity(FRAME_LENGTH_PREFIX_SIZE + body_length));
    encoded.extend_from_slice(&length.to_le_bytes());
    match frame {
        WireFrame::Control(payload) => {
            if payload.len() > MAX_CONTROL_PAYLOAD_BYTES {
                return Err(FrameDecodeError::ControlPayloadTooLarge {
                    actual: payload.len(),
                    maximum: MAX_CONTROL_PAYLOAD_BYTES,
                });
            }
            encoded.push(FrameType::Control as u8);
            encoded.extend_from_slice(payload);
        }
        WireFrame::BinaryChunk { metadata, data } => {
            if data.len() > MAX_BINARY_CHUNK_BYTES {
                return Err(FrameDecodeError::BinaryChunkTooLarge {
                    actual: data.len(),
                    maximum: MAX_BINARY_CHUNK_BYTES,
                });
            }
            if usize::try_from(metadata.data_length).ok() != Some(data.len()) {
                return Err(FrameDecodeError::BinaryDataLengthMismatch {
                    declared: metadata.data_length,
                    actual: data.len(),
                });
            }
            encoded.push(FrameType::BinaryChunk as u8);
            encoded.extend_from_slice(&metadata.stream_id.to_bytes());
            encoded.extend_from_slice(&metadata.chunk_sequence.to_le_bytes());
            encoded.push(u8::from(metadata.end_of_message) * BINARY_END_OF_MESSAGE);
            encoded.extend_from_slice(data);
        }
        WireFrame::SecretInput(frame) => {
            encoded.push(FrameType::SecretInput as u8);
            append_secret_input_payload(frame, &mut encoded);
        }
        WireFrame::ToolRuntimeSecret(frame) => {
            encoded.push(FrameType::ToolRuntimeSecret as u8);
            append_tool_runtime_secret_payload(frame, &mut encoded)
                .map_err(|_| FrameDecodeError::InvalidToolRuntimeSecret)?;
        }
        WireFrame::ToolConformanceRuntimeSecret(frame) => {
            encoded.push(FrameType::ToolConformanceRuntimeSecret as u8);
            append_tool_conformance_runtime_secret_payload(frame, &mut encoded)
                .map_err(|_| FrameDecodeError::InvalidToolConformanceRuntimeSecret)?;
        }
    }
    Ok(encoded)
}

pub fn decode_frame(encoded: &mut [u8]) -> Result<WireFrame, FrameDecodeError> {
    let secret_frame = matches!(
        encoded.get(FRAME_LENGTH_PREFIX_SIZE),
        Some(value)
            if *value == FrameType::SecretInput as u8
                || *value == FrameType::ToolRuntimeSecret as u8
                || *value == FrameType::ToolConformanceRuntimeSecret as u8
    );
    let result = decode_frame_inner(encoded);
    if secret_frame && encoded.len() > FRAME_LENGTH_PREFIX_SIZE + 1 {
        encoded[(FRAME_LENGTH_PREFIX_SIZE + 1)..].fill(0);
    }
    if encoded.get(FRAME_LENGTH_PREFIX_SIZE) == Some(&(FrameType::SecretInput as u8)) {
        result.map_err(|_| FrameDecodeError::InvalidSecretInput)
    } else if encoded.get(FRAME_LENGTH_PREFIX_SIZE)
        == Some(&(FrameType::ToolConformanceRuntimeSecret as u8))
    {
        result.map_err(|_| FrameDecodeError::InvalidToolConformanceRuntimeSecret)
    } else if secret_frame {
        result.map_err(|_| FrameDecodeError::InvalidToolRuntimeSecret)
    } else {
        result
    }
}

fn decode_frame_inner(encoded: &mut [u8]) -> Result<WireFrame, FrameDecodeError> {
    if encoded.len() < FRAME_LENGTH_PREFIX_SIZE {
        return Err(FrameDecodeError::EmptyFrame);
    }
    let length = u32::from_le_bytes(
        encoded[..FRAME_LENGTH_PREFIX_SIZE].try_into().map_err(|_| FrameDecodeError::EmptyFrame)?,
    ) as usize;
    if length == 0 {
        return Err(FrameDecodeError::EmptyFrame);
    }
    if length > MAX_FRAME_LENGTH {
        return Err(FrameDecodeError::FrameTooLarge { actual: length, maximum: MAX_FRAME_LENGTH });
    }
    if encoded.len() != FRAME_LENGTH_PREFIX_SIZE + length {
        return Err(FrameDecodeError::Io("encoded frame length does not match its prefix".into()));
    }
    let frame_type = FrameType::try_from(encoded[FRAME_LENGTH_PREFIX_SIZE])?;
    let payload = &mut encoded[(FRAME_LENGTH_PREFIX_SIZE + 1)..];

    match frame_type {
        FrameType::Control => {
            if payload.len() > MAX_CONTROL_PAYLOAD_BYTES {
                return Err(FrameDecodeError::ControlPayloadTooLarge {
                    actual: payload.len(),
                    maximum: MAX_CONTROL_PAYLOAD_BYTES,
                });
            }
            Ok(WireFrame::Control(payload.to_vec()))
        }
        FrameType::BinaryChunk => {
            if payload.len() < BINARY_CHUNK_HEADER_SIZE {
                return Err(FrameDecodeError::TruncatedBinaryHeader);
            }
            let stream_bytes =
                payload[..16].try_into().map_err(|_| FrameDecodeError::TruncatedBinaryHeader)?;
            let sequence = u64::from_le_bytes(
                payload[16..24].try_into().map_err(|_| FrameDecodeError::TruncatedBinaryHeader)?,
            );
            let flags = payload[24];
            if flags & !BINARY_KNOWN_FLAGS != 0 {
                return Err(FrameDecodeError::UnknownBinaryFlags(flags));
            }
            let data = payload[BINARY_CHUNK_HEADER_SIZE..].to_vec();
            if data.len() > MAX_BINARY_CHUNK_BYTES {
                return Err(FrameDecodeError::BinaryChunkTooLarge {
                    actual: data.len(),
                    maximum: MAX_BINARY_CHUNK_BYTES,
                });
            }
            Ok(WireFrame::BinaryChunk {
                metadata: BinaryStreamFrame {
                    stream_id: StreamId::from_bytes(stream_bytes),
                    chunk_sequence: sequence,
                    end_of_message: flags & BINARY_END_OF_MESSAGE != 0,
                    data_length: u32::try_from(data.len())
                        .map_err(|_| FrameDecodeError::LengthOverflow)?,
                },
                data,
            })
        }
        FrameType::SecretInput => decode_secret_input_payload(payload)
            .map(WireFrame::SecretInput)
            .map_err(|_| FrameDecodeError::InvalidSecretInput),
        FrameType::ToolRuntimeSecret => decode_tool_runtime_secret_payload(payload)
            .map(WireFrame::ToolRuntimeSecret)
            .map_err(|_| FrameDecodeError::InvalidToolRuntimeSecret),
        FrameType::ToolConformanceRuntimeSecret => {
            decode_tool_conformance_runtime_secret_payload(payload)
                .map(WireFrame::ToolConformanceRuntimeSecret)
                .map_err(|_| FrameDecodeError::InvalidToolConformanceRuntimeSecret)
        }
    }
}

pub fn encode_authenticated_frame(
    authenticated: &AuthenticatedWireFrame,
) -> Result<Zeroizing<Vec<u8>>, FrameDecodeError> {
    let legacy = encode_frame(&authenticated.frame)?;
    let legacy_body = &legacy[FRAME_LENGTH_PREFIX_SIZE..];
    let body_length = legacy_body
        .len()
        .checked_add(AUTHENTICATED_FRAME_HEADER_SIZE)
        .ok_or(FrameDecodeError::LengthOverflow)?;
    if body_length > MAX_FRAME_LENGTH {
        return Err(FrameDecodeError::FrameTooLarge {
            actual: body_length,
            maximum: MAX_FRAME_LENGTH,
        });
    }
    let length = u32::try_from(body_length).map_err(|_| FrameDecodeError::LengthOverflow)?;
    let mut encoded = Zeroizing::new(Vec::with_capacity(FRAME_LENGTH_PREFIX_SIZE + body_length));
    encoded.extend_from_slice(&length.to_le_bytes());
    encoded.push(legacy_body[0]);
    encoded.extend_from_slice(&authenticated.header.channel_id.to_bytes());
    encoded.extend_from_slice(&authenticated.header.connection_epoch.to_le_bytes());
    encoded.extend_from_slice(&authenticated.header.sequence.to_le_bytes());
    encoded.extend_from_slice(&legacy_body[1..]);
    Ok(encoded)
}

pub fn decode_authenticated_frame(
    encoded: &mut [u8],
) -> Result<AuthenticatedWireFrame, FrameDecodeError> {
    let secret_frame = matches!(
        encoded.get(FRAME_LENGTH_PREFIX_SIZE),
        Some(value)
            if *value == FrameType::SecretInput as u8
                || *value == FrameType::ToolRuntimeSecret as u8
                || *value == FrameType::ToolConformanceRuntimeSecret as u8
    );
    let result = decode_authenticated_frame_inner(encoded);
    if secret_frame && encoded.len() > FRAME_LENGTH_PREFIX_SIZE + 1 {
        encoded[(FRAME_LENGTH_PREFIX_SIZE + 1)..].fill(0);
    }
    if encoded.get(FRAME_LENGTH_PREFIX_SIZE) == Some(&(FrameType::SecretInput as u8)) {
        result.map_err(|_| FrameDecodeError::InvalidSecretInput)
    } else if encoded.get(FRAME_LENGTH_PREFIX_SIZE)
        == Some(&(FrameType::ToolConformanceRuntimeSecret as u8))
    {
        result.map_err(|_| FrameDecodeError::InvalidToolConformanceRuntimeSecret)
    } else if secret_frame {
        result.map_err(|_| FrameDecodeError::InvalidToolRuntimeSecret)
    } else {
        result
    }
}

fn decode_authenticated_frame_inner(
    encoded: &mut [u8],
) -> Result<AuthenticatedWireFrame, FrameDecodeError> {
    if encoded.len() < FRAME_LENGTH_PREFIX_SIZE {
        return Err(FrameDecodeError::EmptyFrame);
    }
    let length = u32::from_le_bytes(
        encoded[..FRAME_LENGTH_PREFIX_SIZE].try_into().map_err(|_| FrameDecodeError::EmptyFrame)?,
    ) as usize;
    if length < 1 + AUTHENTICATED_FRAME_HEADER_SIZE {
        return Err(FrameDecodeError::TruncatedAuthenticatedHeader);
    }
    if length > MAX_FRAME_LENGTH {
        return Err(FrameDecodeError::FrameTooLarge { actual: length, maximum: MAX_FRAME_LENGTH });
    }
    if encoded.len() != FRAME_LENGTH_PREFIX_SIZE + length {
        return Err(FrameDecodeError::Io("encoded frame length does not match its prefix".into()));
    }
    let channel_start = FRAME_LENGTH_PREFIX_SIZE + 1;
    let epoch_start = channel_start + 16;
    let sequence_start = epoch_start + 8;
    let payload_start = sequence_start + 8;
    let channel_id = IpcChannelId::from_bytes(
        encoded[channel_start..epoch_start]
            .try_into()
            .map_err(|_| FrameDecodeError::TruncatedAuthenticatedHeader)?,
    );
    let connection_epoch = u64::from_le_bytes(
        encoded[epoch_start..sequence_start]
            .try_into()
            .map_err(|_| FrameDecodeError::TruncatedAuthenticatedHeader)?,
    );
    let sequence = u64::from_le_bytes(
        encoded[sequence_start..payload_start]
            .try_into()
            .map_err(|_| FrameDecodeError::TruncatedAuthenticatedHeader)?,
    );
    let legacy_body_length = 1 + encoded.len().saturating_sub(payload_start);
    let legacy_length =
        u32::try_from(legacy_body_length).map_err(|_| FrameDecodeError::LengthOverflow)?;
    let mut legacy =
        Zeroizing::new(Vec::with_capacity(FRAME_LENGTH_PREFIX_SIZE + legacy_body_length));
    legacy.extend_from_slice(&legacy_length.to_le_bytes());
    legacy.push(encoded[FRAME_LENGTH_PREFIX_SIZE]);
    legacy.extend_from_slice(&encoded[payload_start..]);
    let frame = decode_frame(&mut legacy);
    let frame = frame?;
    Ok(AuthenticatedWireFrame {
        header: IpcFrameHeader { channel_id, connection_epoch, sequence },
        frame,
    })
}

pub async fn read_frame<R>(reader: &mut R) -> Result<WireFrame, FrameDecodeError>
where
    R: AsyncRead + Unpin,
{
    let mut prefix = [0_u8; FRAME_LENGTH_PREFIX_SIZE];
    reader
        .read_exact(&mut prefix)
        .await
        .map_err(|error| FrameDecodeError::Io(error.to_string()))?;
    let length = u32::from_le_bytes(prefix) as usize;
    if length == 0 {
        return Err(FrameDecodeError::EmptyFrame);
    }
    if length > MAX_FRAME_LENGTH {
        return Err(FrameDecodeError::FrameTooLarge { actual: length, maximum: MAX_FRAME_LENGTH });
    }
    let mut encoded = Vec::with_capacity(FRAME_LENGTH_PREFIX_SIZE + length);
    encoded.extend_from_slice(&prefix);
    encoded.resize(FRAME_LENGTH_PREFIX_SIZE + length, 0);
    reader
        .read_exact(&mut encoded[FRAME_LENGTH_PREFIX_SIZE..])
        .await
        .map_err(|error| FrameDecodeError::Io(error.to_string()))?;
    decode_frame(&mut encoded)
}

pub async fn write_frame<W>(writer: &mut W, frame: &WireFrame) -> Result<(), FrameDecodeError>
where
    W: AsyncWrite + Unpin,
{
    let encoded = encode_frame(frame)?;
    writer.write_all(&encoded).await.map_err(|error| FrameDecodeError::Io(error.to_string()))?;
    writer.flush().await.map_err(|error| FrameDecodeError::Io(error.to_string()))
}

pub async fn read_authenticated_frame<R>(
    reader: &mut R,
) -> Result<AuthenticatedWireFrame, FrameDecodeError>
where
    R: AsyncRead + Unpin,
{
    let mut prefix = [0_u8; FRAME_LENGTH_PREFIX_SIZE];
    reader
        .read_exact(&mut prefix)
        .await
        .map_err(|error| FrameDecodeError::Io(error.to_string()))?;
    let length = u32::from_le_bytes(prefix) as usize;
    if length < 1 + AUTHENTICATED_FRAME_HEADER_SIZE {
        return Err(FrameDecodeError::TruncatedAuthenticatedHeader);
    }
    if length > MAX_FRAME_LENGTH {
        return Err(FrameDecodeError::FrameTooLarge { actual: length, maximum: MAX_FRAME_LENGTH });
    }
    let mut encoded = Vec::with_capacity(FRAME_LENGTH_PREFIX_SIZE + length);
    encoded.extend_from_slice(&prefix);
    encoded.resize(FRAME_LENGTH_PREFIX_SIZE + length, 0);
    reader
        .read_exact(&mut encoded[FRAME_LENGTH_PREFIX_SIZE..])
        .await
        .map_err(|error| FrameDecodeError::Io(error.to_string()))?;
    decode_authenticated_frame(&mut encoded)
}

/// Result of one incremental read attempt.
pub enum FrameRead {
    /// A complete frame was decoded.
    Frame(Box<AuthenticatedWireFrame>),
    /// The underlying reader had nothing more to give. Any bytes already
    /// consumed are retained for the next call.
    Pending,
    /// The reader reached end of stream with a partial frame outstanding.
    Eof,
    /// The bytes read so far cannot form a valid frame.
    Invalid(FrameDecodeError),
    /// The reader itself failed.
    Failed(std::io::Error),
}

/// Cancel-safe incremental frame reader.
///
/// [`read_authenticated_frame`] uses `read_exact`, which is *not* cancel-safe:
/// dropping it mid-frame (for example when a caller wraps it in
/// `tokio::time::timeout`) consumes the length prefix and may lose part of the
/// body, so the next call would treat body bytes as a new prefix and desync the
/// whole stream. This reader keeps the partial frame in its own state, so a
/// cancelled attempt can simply be retried.
pub struct AuthenticatedFrameReader<R> {
    reader: R,
    prefix: [u8; FRAME_LENGTH_PREFIX_SIZE],
    prefix_filled: usize,
    body: Vec<u8>,
    body_filled: usize,
}

impl<R> AuthenticatedFrameReader<R>
where
    R: AsyncRead + Unpin,
{
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            prefix: [0_u8; FRAME_LENGTH_PREFIX_SIZE],
            prefix_filled: 0,
            body: Vec::new(),
            body_filled: 0,
        }
    }

    /// Read until a frame completes, the reader returns `Pending`, or it ends.
    /// Cancelling this future never loses consumed bytes.
    pub async fn next_frame(&mut self) -> FrameRead {
        loop {
            if self.prefix_filled < FRAME_LENGTH_PREFIX_SIZE {
                match self.reader.read(&mut self.prefix[self.prefix_filled..]).await {
                    Ok(0) => return FrameRead::Eof,
                    Ok(count) => {
                        self.prefix_filled += count;
                        if self.prefix_filled < FRAME_LENGTH_PREFIX_SIZE {
                            continue;
                        }
                        let length = u32::from_le_bytes(self.prefix) as usize;
                        if length < 1 + AUTHENTICATED_FRAME_HEADER_SIZE {
                            return FrameRead::Invalid(
                                FrameDecodeError::TruncatedAuthenticatedHeader,
                            );
                        }
                        if length > MAX_FRAME_LENGTH {
                            return FrameRead::Invalid(FrameDecodeError::FrameTooLarge {
                                actual: length,
                                maximum: MAX_FRAME_LENGTH,
                            });
                        }
                        self.body.clear();
                        self.body.resize(length, 0);
                        self.body_filled = 0;
                    }
                    Err(error) => return FrameRead::Failed(error),
                }
            }
            if self.body_filled < self.body.len() {
                let end = self.body.len();
                match self.reader.read(&mut self.body[self.body_filled..end]).await {
                    Ok(0) => return FrameRead::Eof,
                    Ok(count) => {
                        self.body_filled += count;
                        if self.body_filled < end {
                            continue;
                        }
                    }
                    Err(error) => return FrameRead::Failed(error),
                }
            }
            let mut encoded = Vec::with_capacity(FRAME_LENGTH_PREFIX_SIZE + self.body.len());
            encoded.extend_from_slice(&self.prefix);
            encoded.extend_from_slice(&self.body);
            self.prefix_filled = 0;
            self.body = Vec::new();
            self.body_filled = 0;
            return match decode_authenticated_frame(&mut encoded) {
                Ok(frame) => FrameRead::Frame(Box::new(frame)),
                Err(error) => FrameRead::Invalid(error),
            };
        }
    }
}

pub async fn write_authenticated_frame<W>(
    writer: &mut W,
    frame: &AuthenticatedWireFrame,
) -> Result<(), FrameDecodeError>
where
    W: AsyncWrite + Unpin,
{
    let encoded = encode_authenticated_frame(frame)?;
    writer.write_all(&encoded).await.map_err(|error| FrameDecodeError::Io(error.to_string()))?;
    writer.flush().await.map_err(|error| FrameDecodeError::Io(error.to_string()))
}

#[cfg(test)]
mod tests {
    use aura_domain_common::ToolExecutionAttemptId;
    use aura_domain_common::jcs::CanonicalHashV1;
    use aura_domain_common::{CredentialId, CredentialRevision, Revision, SandboxExecutionId};
    use aura_tool_contract::{CredentialSlotKey, EnvironmentVariableNameV1};

    use super::*;

    fn tool_runtime_secret(secret: &[u8]) -> ToolRuntimeSecretFrameV1 {
        ToolRuntimeSecretFrameV1::new(
            SandboxExecutionId::from_bytes([1; 16]),
            ToolExecutionAttemptId::from_bytes([2; 16]),
            CredentialId::from_bytes([3; 16]),
            CredentialRevision::new(Revision::new(4)).expect("credential revision"),
            CredentialSlotKey::new("api-key").expect("credential slot"),
            EnvironmentVariableNameV1::new("TOOL_API_KEY").expect("environment variable"),
            CanonicalHashV1::sha256(b"runtime binding"),
            secret.to_vec(),
        )
        .expect("tool runtime secret frame")
    }

    #[test]
    fn complete_tool_runtime_secret_wire_is_zeroized_after_decode() {
        let original = tool_runtime_secret(b"fixture-secret-canary");
        let mut encoded = encode_frame(&WireFrame::ToolRuntimeSecret(original)).expect("encode");

        let decoded = decode_frame(&mut encoded).expect("decode");
        let WireFrame::ToolRuntimeSecret(decoded) = decoded else {
            panic!("decoded the wrong frame type")
        };

        assert_eq!(decoded.secret().expose(), b"fixture-secret-canary");
        assert_eq!(encoded[FRAME_LENGTH_PREFIX_SIZE], FrameType::ToolRuntimeSecret as u8);
        assert!(
            encoded[(FRAME_LENGTH_PREFIX_SIZE + 1)..].iter().all(|byte| *byte == 0),
            "the consumed metadata and secret bytes must be zeroized in place"
        );
    }

    struct TestIo {
        chunks: Vec<Vec<u8>>,
        position: usize,
        offset: usize,
        ended: bool,
    }

    impl TestIo {
        fn new(chunks: Vec<Vec<u8>>) -> Self {
            Self { chunks, position: 0, offset: 0, ended: false }
        }
    }

    impl AsyncRead for TestIo {
        fn poll_read(
            mut self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
            buf: &mut tokio::io::ReadBuf<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            if self.ended {
                return std::task::Poll::Ready(Ok(()));
            }
            while self.position < self.chunks.len()
                && self.offset >= self.chunks[self.position].len()
            {
                self.position += 1;
                self.offset = 0;
            }
            if self.position >= self.chunks.len() {
                self.ended = true;
                return std::task::Poll::Ready(Ok(()));
            }
            let chunk = &self.chunks[self.position];
            let take = chunk.len().saturating_sub(self.offset).min(buf.remaining());
            let start = self.offset;
            buf.put_slice(&chunk[start..start + take]);
            self.offset += take;
            std::task::Poll::Ready(Ok(()))
        }
    }

    /// A reader whose available bytes are controlled by the test: while the
    /// queue is empty it reports `Pending` instead of end of stream, which is
    /// what a real pipe does between frames.
    struct RefillableIo {
        available: std::collections::VecDeque<u8>,
        open: bool,
    }

    impl RefillableIo {
        fn new() -> Self {
            Self { available: std::collections::VecDeque::new(), open: true }
        }

        fn extend(&mut self, bytes: &[u8]) {
            self.available.extend(bytes.iter().copied());
        }
    }

    impl AsyncRead for RefillableIo {
        fn poll_read(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &mut tokio::io::ReadBuf<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            if buf.remaining() == 0 {
                return std::task::Poll::Ready(Ok(()));
            }
            if self.available.is_empty() {
                if !self.open {
                    return std::task::Poll::Ready(Ok(()));
                }
                // Nothing to hand out yet: wake immediately so a retry makes
                // progress instead of parking forever.
                cx.waker().wake_by_ref();
                return std::task::Poll::Pending;
            }
            let take = self.available.len().min(buf.remaining());
            for _ in 0..take {
                let byte = self.available.pop_front().expect("checked length");
                buf.put_slice(&[byte]);
            }
            std::task::Poll::Ready(Ok(()))
        }
    }

    fn control_frame(epoch: u64, sequence: u64) -> AuthenticatedWireFrame {
        AuthenticatedWireFrame {
            header: IpcFrameHeader {
                channel_id: IpcChannelId::from_bytes([7; 16]),
                connection_epoch: epoch,
                sequence,
            },
            frame: WireFrame::Control(br#"{"jsonrpc":"2.0","id":"cor_x","method":"m"}"#.to_vec()),
        }
    }

    fn encoded(frame: &AuthenticatedWireFrame) -> Vec<u8> {
        encode_authenticated_frame(frame).expect("encode").to_vec()
    }

    #[tokio::test]
    async fn frames_split_across_reads_still_decode_in_order() {
        let first = encoded(&control_frame(1, 2));
        let second = encoded(&control_frame(1, 3));
        // One byte at a time: every call returns Pending or a frame, never a
        // desynchronised stream.
        let chunks: Vec<Vec<u8>> =
            first.iter().chain(second.iter()).map(|byte| vec![*byte]).collect();
        let mut reader = AuthenticatedFrameReader::new(TestIo::new(chunks));
        let mut sequences = Vec::new();
        for _ in 0..2 {
            loop {
                match reader.next_frame().await {
                    FrameRead::Frame(frame) => {
                        sequences.push(frame.header.sequence);
                        break;
                    }
                    FrameRead::Pending => continue,
                    other => panic!("unexpected read outcome: {}", describe(&other)),
                }
            }
        }
        assert_eq!(sequences, vec![2, 3]);
    }

    #[tokio::test]
    async fn coalesced_frames_decode_one_at_a_time() {
        let mut bytes = encoded(&control_frame(1, 2));
        bytes.extend(encoded(&control_frame(1, 3)));
        let mut reader = AuthenticatedFrameReader::new(TestIo::new(vec![bytes]));
        let first = match reader.next_frame().await {
            FrameRead::Frame(frame) => frame,
            other => panic!("unexpected read outcome: {}", describe(&other)),
        };
        assert_eq!(first.header.sequence, 2, "one call must consume exactly one frame");
        let second = match reader.next_frame().await {
            FrameRead::Frame(frame) => frame,
            other => panic!("unexpected read outcome: {}", describe(&other)),
        };
        assert_eq!(second.header.sequence, 3, "the second frame must follow the first");
    }

    #[tokio::test]
    async fn a_cancelled_mid_frame_read_keeps_the_partial_frame() {
        let first = encoded(&control_frame(1, 2));
        let second = encoded(&control_frame(1, 3));
        let split = first.len() / 2;
        let io = RefillableIo::new();
        let probe = io.available.len();
        assert_eq!(probe, 0);
        let io = std::sync::Arc::new(std::sync::Mutex::new(io));
        // Only the first half of the frame is available: the read must block,
        // and a zero timeout must cancel it mid-frame.
        io.lock().expect("io").extend(&first[..split]);
        let mut reader = AuthenticatedFrameReader::new(SharedIo(std::sync::Arc::clone(&io)));
        let cancelled =
            tokio::time::timeout(std::time::Duration::from_millis(0), reader.next_frame()).await;
        assert!(cancelled.is_err(), "a blocked mid-frame read must be cancellable");
        assert_eq!(
            io.lock().expect("io").available.len(),
            0,
            "the first half was consumed and must be remembered by the reader"
        );
        // The rest of the frame arrives; the retry must complete it in order.
        io.lock().expect("io").extend(&first[split..]);
        io.lock().expect("io").extend(&second);
        let frame = loop {
            match reader.next_frame().await {
                FrameRead::Frame(frame) => break frame,
                FrameRead::Pending => {
                    tokio::task::yield_now().await;
                }
                other => panic!("unexpected read outcome: {}", describe(&other)),
            }
        };
        assert_eq!(frame.header.sequence, 2, "the cancelled attempt must not skip the frame");
        let next = loop {
            match reader.next_frame().await {
                FrameRead::Frame(frame) => break frame,
                FrameRead::Pending => {
                    tokio::task::yield_now().await;
                }
                other => panic!("unexpected read outcome: {}", describe(&other)),
            }
        };
        assert_eq!(next.header.sequence, 3, "the following frame must stay in order");
    }

    /// Shares one refillable reader with the test so bytes can be added while
    /// the parser is mid-frame.
    struct SharedIo(std::sync::Arc<std::sync::Mutex<RefillableIo>>);

    impl AsyncRead for SharedIo {
        fn poll_read(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &mut tokio::io::ReadBuf<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            let mut io = match self.0.lock() {
                Ok(io) => io,
                Err(_) => return std::task::Poll::Ready(Err(std::io::Error::other("poisoned"))),
            };
            std::pin::Pin::new(&mut *io).poll_read(cx, buf)
        }
    }

    #[tokio::test]
    async fn a_partial_frame_then_eof_is_reported_as_end_of_stream() {
        let first = encoded(&control_frame(1, 2));
        let split = first.len() / 2;
        let mut reader = AuthenticatedFrameReader::new(TestIo::new(vec![first[..split].to_vec()]));
        loop {
            match reader.next_frame().await {
                FrameRead::Pending => continue,
                FrameRead::Eof => break,
                other => panic!("unexpected read outcome: {}", describe(&other)),
            }
        }
    }

    fn describe(outcome: &FrameRead) -> &'static str {
        match outcome {
            FrameRead::Frame(_) => "frame",
            FrameRead::Pending => "pending",
            FrameRead::Eof => "eof",
            FrameRead::Invalid(_) => "invalid",
            FrameRead::Failed(_) => "failed",
        }
    }
}
