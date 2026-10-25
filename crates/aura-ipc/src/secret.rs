use std::{
    collections::BTreeMap,
    fmt,
    time::{Duration, Instant},
};

use aura_domain_common::{CommandInvocationId, CorrelationId, IpcChannelId};
use aura_windows_platform::SecretBytes;
use thiserror::Error;
use zeroize::{Zeroize, Zeroizing};

pub const SECRET_INPUT_FRAME_VERSION: u8 = 1;
pub const SECRET_INPUT_FIXED_HEADER_SIZE: usize = 40;
pub const MAX_SECRET_INPUT_BYTES: usize = 2_560;
pub const DEFAULT_PENDING_SECRET_INPUT_TTL: Duration = Duration::from_secs(30);
pub const SECRET_INPUT_V1_FEATURE: &str = "aura.ipc.secret-input.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SecretInputOperation {
    ModelsEndpointCreateWithCredential,
    ModelsEndpointConfigureCredential,
    ModelsEndpointReplaceCredential,
    ToolsCredentialCreate,
    ToolsCredentialRotate,
    TerminalSensitiveInput,
    /// One MCP server's secret.
    McpSecretSet,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SecretInputSlot {
    ApiKey,
    TerminalInput,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SecretInputEncoding {
    Utf8,
}

pub struct SecretInputFrameV1 {
    pub correlation_id: CorrelationId,
    pub command_invocation_id: CommandInvocationId,
    pub operation: SecretInputOperation,
    pub slot: SecretInputSlot,
    pub encoding: SecretInputEncoding,
    secret: SecretBytes,
}

impl SecretInputFrameV1 {
    pub fn new(
        correlation_id: CorrelationId,
        command_invocation_id: CommandInvocationId,
        operation: SecretInputOperation,
        slot: SecretInputSlot,
        secret: Vec<u8>,
    ) -> Result<Self, SecretInputValidationError> {
        let secret = SecretBytes::new(secret);
        if (operation == SecretInputOperation::TerminalSensitiveInput)
            != (slot == SecretInputSlot::TerminalInput)
        {
            return Err(SecretInputValidationError::InvalidSlot);
        }
        if secret.expose().is_empty() {
            return Err(SecretInputValidationError::Empty);
        }
        if secret.expose().len() > MAX_SECRET_INPUT_BYTES {
            return Err(SecretInputValidationError::TooLarge);
        }
        let value = std::str::from_utf8(secret.expose())
            .map_err(|_| SecretInputValidationError::InvalidUtf8)?;
        if value.chars().any(|character| {
            matches!(character, '\u{0000}'..='\u{001f}' | '\u{007f}' | '\u{2028}' | '\u{2029}')
        }) {
            return Err(SecretInputValidationError::ProhibitedCharacter);
        }
        Ok(Self {
            correlation_id,
            command_invocation_id,
            operation,
            slot,
            encoding: SecretInputEncoding::Utf8,
            secret,
        })
    }

    #[must_use]
    pub const fn frame_version(&self) -> u8 {
        SECRET_INPUT_FRAME_VERSION
    }

    #[must_use]
    pub fn secret(&self) -> &SecretBytes {
        &self.secret
    }

    #[must_use]
    pub fn into_secret(self) -> SecretBytes {
        self.secret
    }
}

impl PartialEq for SecretInputFrameV1 {
    fn eq(&self, other: &Self) -> bool {
        self.correlation_id == other.correlation_id
            && self.command_invocation_id == other.command_invocation_id
            && self.operation == other.operation
            && self.slot == other.slot
            && self.encoding == other.encoding
            && self.secret.expose() == other.secret.expose()
    }
}

impl Eq for SecretInputFrameV1 {}

impl fmt::Debug for SecretInputFrameV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SecretInputFrameV1")
            .field("frame_version", &SECRET_INPUT_FRAME_VERSION)
            .field("correlation_id", &self.correlation_id)
            .field("command_invocation_id", &self.command_invocation_id)
            .field("operation", &self.operation)
            .field("slot", &self.slot)
            .field("encoding", &self.encoding)
            .field("secret", &"REDACTED")
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum SecretInputValidationError {
    #[error("ipc.secret_input.invalid_slot")]
    InvalidSlot,
    #[error("ipc.secret_input.empty")]
    Empty,
    #[error("ipc.secret_input.too_large")]
    TooLarge,
    #[error("ipc.secret_input.invalid_utf8")]
    InvalidUtf8,
    #[error("ipc.secret_input.prohibited_character")]
    ProhibitedCharacter,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
#[error("ipc.secret_input.malformed")]
pub struct SecretInputDecodeError;

#[must_use]
pub fn secret_input_v1_is_negotiated(
    negotiated_protocol_version: aura_domain_common::ContractVersion,
    local_features: &[String],
    peer_features: &[String],
) -> bool {
    negotiated_protocol_version.major == 1
        && negotiated_protocol_version.minor == 7
        && local_features.iter().any(|feature| feature == SECRET_INPUT_V1_FEATURE)
        && peer_features.iter().any(|feature| feature == SECRET_INPUT_V1_FEATURE)
}

#[must_use]
pub fn encode_secret_input_payload(frame: &SecretInputFrameV1) -> Zeroizing<Vec<u8>> {
    let secret = frame.secret.expose();
    let mut payload =
        Zeroizing::new(Vec::with_capacity(SECRET_INPUT_FIXED_HEADER_SIZE + secret.len()));
    append_secret_input_payload(frame, &mut payload);
    payload
}

pub(crate) fn append_secret_input_payload(frame: &SecretInputFrameV1, payload: &mut Vec<u8>) {
    let secret = frame.secret.expose();
    payload.push(SECRET_INPUT_FRAME_VERSION);
    payload.push(operation_discriminant(frame.operation));
    payload.extend_from_slice(&frame.correlation_id.to_bytes());
    payload.extend_from_slice(&frame.command_invocation_id.to_bytes());
    payload.push(slot_discriminant(frame.slot));
    payload.push(encoding_discriminant(frame.encoding));
    let secret_length = u32::try_from(secret.len()).expect("validated SecretInput length fits u32");
    payload.extend_from_slice(&secret_length.to_le_bytes());
    payload.extend_from_slice(secret);
}

pub fn decode_secret_input_payload(
    payload: &mut [u8],
) -> Result<SecretInputFrameV1, SecretInputDecodeError> {
    let result = (|| {
        if payload.len() < SECRET_INPUT_FIXED_HEADER_SIZE
            || payload[0] != SECRET_INPUT_FRAME_VERSION
        {
            return Err(SecretInputDecodeError);
        }
        let operation = parse_operation(payload[1])?;
        let correlation_bytes: [u8; 16] =
            payload[2..18].try_into().map_err(|_| SecretInputDecodeError)?;
        let invocation_bytes: [u8; 16] =
            payload[18..34].try_into().map_err(|_| SecretInputDecodeError)?;
        let slot = parse_slot(payload[34])?;
        let encoding = parse_encoding(payload[35])?;
        let declared_length =
            u32::from_le_bytes(payload[36..40].try_into().map_err(|_| SecretInputDecodeError)?)
                as usize;
        if !(1..=MAX_SECRET_INPUT_BYTES).contains(&declared_length)
            || payload.len() != SECRET_INPUT_FIXED_HEADER_SIZE + declared_length
        {
            return Err(SecretInputDecodeError);
        }
        let secret = payload[SECRET_INPUT_FIXED_HEADER_SIZE..].to_vec();
        let mut frame = SecretInputFrameV1::new(
            CorrelationId::from_bytes(correlation_bytes),
            CommandInvocationId::from_bytes(invocation_bytes),
            operation,
            slot,
            secret,
        )
        .map_err(|_| SecretInputDecodeError)?;
        frame.encoding = encoding;
        Ok(frame)
    })();
    payload.zeroize();
    result
}

const fn operation_discriminant(operation: SecretInputOperation) -> u8 {
    match operation {
        SecretInputOperation::ModelsEndpointCreateWithCredential => 1,
        SecretInputOperation::ModelsEndpointConfigureCredential => 2,
        SecretInputOperation::ModelsEndpointReplaceCredential => 3,
        SecretInputOperation::ToolsCredentialCreate => 4,
        SecretInputOperation::ToolsCredentialRotate => 5,
        SecretInputOperation::TerminalSensitiveInput => 6,
        SecretInputOperation::McpSecretSet => 7,
    }
}

const fn slot_discriminant(slot: SecretInputSlot) -> u8 {
    match slot {
        SecretInputSlot::ApiKey => 1,
        SecretInputSlot::TerminalInput => 2,
    }
}

const fn encoding_discriminant(encoding: SecretInputEncoding) -> u8 {
    match encoding {
        SecretInputEncoding::Utf8 => 1,
    }
}

fn parse_operation(value: u8) -> Result<SecretInputOperation, SecretInputDecodeError> {
    match value {
        1 => Ok(SecretInputOperation::ModelsEndpointCreateWithCredential),
        2 => Ok(SecretInputOperation::ModelsEndpointConfigureCredential),
        3 => Ok(SecretInputOperation::ModelsEndpointReplaceCredential),
        4 => Ok(SecretInputOperation::ToolsCredentialCreate),
        5 => Ok(SecretInputOperation::ToolsCredentialRotate),
        6 => Ok(SecretInputOperation::TerminalSensitiveInput),
        7 => Ok(SecretInputOperation::McpSecretSet),
        _ => Err(SecretInputDecodeError),
    }
}

fn parse_slot(value: u8) -> Result<SecretInputSlot, SecretInputDecodeError> {
    match value {
        1 => Ok(SecretInputSlot::ApiKey),
        2 => Ok(SecretInputSlot::TerminalInput),
        _ => Err(SecretInputDecodeError),
    }
}

fn parse_encoding(value: u8) -> Result<SecretInputEncoding, SecretInputDecodeError> {
    match value {
        1 => Ok(SecretInputEncoding::Utf8),
        _ => Err(SecretInputDecodeError),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PendingSecretInput {
    pub correlation_id: CorrelationId,
    pub command_invocation_id: CommandInvocationId,
    pub operation: SecretInputOperation,
    pub slot: SecretInputSlot,
    pub channel_id: IpcChannelId,
    pub connection_epoch: u64,
    expires_at: Instant,
}

impl PendingSecretInput {
    #[must_use]
    pub fn new(
        correlation_id: CorrelationId,
        command_invocation_id: CommandInvocationId,
        operation: SecretInputOperation,
        slot: SecretInputSlot,
        channel_id: IpcChannelId,
        connection_epoch: u64,
        accepted_at: Instant,
    ) -> Self {
        Self {
            correlation_id,
            command_invocation_id,
            operation,
            slot,
            channel_id,
            connection_epoch,
            expires_at: accepted_at + DEFAULT_PENDING_SECRET_INPUT_TTL,
        }
    }

    #[must_use]
    pub fn is_expired_at(&self, now: Instant) -> bool {
        now >= self.expires_at
    }
}

#[derive(Default)]
pub struct PendingSecretInputRegistry {
    pending: BTreeMap<CommandInvocationId, PendingSecretInput>,
}

impl PendingSecretInputRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, pending: PendingSecretInput) -> Result<(), PendingSecretInputError> {
        if pending.connection_epoch == 0 {
            return Err(PendingSecretInputError::InvalidChannelEpoch);
        }
        if self.pending.contains_key(&pending.command_invocation_id) {
            return Err(PendingSecretInputError::AlreadyPending);
        }
        self.pending.insert(pending.command_invocation_id, pending);
        Ok(())
    }

    pub fn consume(
        &mut self,
        channel_id: IpcChannelId,
        connection_epoch: u64,
        frame: SecretInputFrameV1,
        now: Instant,
    ) -> Result<SecretBytes, PendingSecretInputError> {
        let result = (|| {
            let Some(pending) = self.pending.remove(&frame.command_invocation_id) else {
                return Err(PendingSecretInputError::NotPending);
            };
            if pending.is_expired_at(now) {
                return Err(PendingSecretInputError::Expired);
            }
            if pending.channel_id != channel_id || pending.connection_epoch != connection_epoch {
                return Err(PendingSecretInputError::ChannelMismatch);
            }
            if pending.correlation_id != frame.correlation_id
                || pending.operation != frame.operation
                || pending.slot != frame.slot
                || frame.encoding != SecretInputEncoding::Utf8
            {
                return Err(PendingSecretInputError::BindingMismatch);
            }
            Ok(frame.into_secret())
        })();
        if result.is_err() {
            self.invalidate_channel(channel_id, connection_epoch);
        }
        result
    }

    pub fn invalidate_channel(&mut self, channel_id: IpcChannelId, connection_epoch: u64) -> usize {
        let before = self.pending.len();
        self.pending.retain(|_, pending| {
            pending.channel_id != channel_id || pending.connection_epoch != connection_epoch
        });
        before.saturating_sub(self.pending.len())
    }

    pub fn purge_expired(&mut self, now: Instant) -> usize {
        self.take_expired(now).len()
    }

    pub fn take_expired(&mut self, now: Instant) -> Vec<CommandInvocationId> {
        let expired = self
            .pending
            .iter()
            .filter_map(|(command_invocation_id, pending)| {
                pending.is_expired_at(now).then_some(*command_invocation_id)
            })
            .collect::<Vec<_>>();
        for command_invocation_id in &expired {
            self.pending.remove(command_invocation_id);
        }
        expired
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.pending.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum PendingSecretInputError {
    #[error("ipc.secret_input.invalid_channel_epoch")]
    InvalidChannelEpoch,
    #[error("ipc.secret_input.already_pending")]
    AlreadyPending,
    #[error("ipc.secret_input.not_pending")]
    NotPending,
    #[error("ipc.secret_input.expired")]
    Expired,
    #[error("ipc.secret_input.channel_mismatch")]
    ChannelMismatch,
    #[error("ipc.secret_input.binding_mismatch")]
    BindingMismatch,
}

#[cfg(test)]
mod tests {
    #[test]
    fn l02_terminal_secret_slot_is_distinct_from_credentials_and_source_is_zeroized() {
        use aura_domain_common::{CommandInvocationId, CorrelationId};
        let correlation = CorrelationId::new();
        let invocation = CommandInvocationId::new();
        let frame = super::SecretInputFrameV1::new(
            correlation,
            invocation,
            super::SecretInputOperation::TerminalSensitiveInput,
            super::SecretInputSlot::TerminalInput,
            b"temporary-slot-test".to_vec(),
        )
        .expect("terminal secret frame fact");
        let mut payload = super::encode_secret_input_payload(&frame);
        assert_eq!(payload[1], 6);
        assert_eq!(payload[34], 2);
        assert_eq!(
            super::decode_secret_input_payload(&mut payload).expect("terminal secret frame fact"),
            frame
        );
        assert!(payload.iter().all(|byte| *byte == 0));
        assert!(
            super::SecretInputFrameV1::new(
                correlation,
                invocation,
                super::SecretInputOperation::TerminalSensitiveInput,
                super::SecretInputSlot::ApiKey,
                b"temporary-slot-test".to_vec()
            )
            .is_err()
        );
    }
    use std::time::{Duration, Instant};

    use aura_domain_common::{CommandInvocationId, ContractVersion, CorrelationId, IpcChannelId};

    use super::{
        DEFAULT_PENDING_SECRET_INPUT_TTL, MAX_SECRET_INPUT_BYTES, PendingSecretInput,
        PendingSecretInputError, PendingSecretInputRegistry, SECRET_INPUT_V1_FEATURE,
        SecretInputFrameV1, SecretInputOperation, SecretInputSlot, SecretInputValidationError,
        decode_secret_input_payload, encode_secret_input_payload, secret_input_v1_is_negotiated,
    };

    #[test]
    fn secret_input_keeps_exact_utf8_bytes_without_exposing_characteristics_in_debug() {
        let correlation_id = CorrelationId::new();
        let command_invocation_id = CommandInvocationId::new();
        let bytes = " leading 密钥 ".as_bytes().to_vec();
        let frame = SecretInputFrameV1::new(
            correlation_id,
            command_invocation_id,
            SecretInputOperation::ModelsEndpointConfigureCredential,
            SecretInputSlot::ApiKey,
            bytes.clone(),
        )
        .expect("valid input");

        assert_eq!(frame.secret().expose(), bytes);
        let debug = format!("{frame:?}");
        assert!(debug.contains("REDACTED"));
        assert!(!debug.contains("leading"));
        let different = SecretInputFrameV1::new(
            correlation_id,
            command_invocation_id,
            SecretInputOperation::ModelsEndpointConfigureCredential,
            SecretInputSlot::ApiKey,
            b"x".to_vec(),
        )
        .expect("different valid input");
        assert_eq!(debug, format!("{different:?}"));
    }

    #[test]
    fn secret_input_rejects_empty_oversized_and_invalid_utf8_without_lengths() {
        let build = |secret| {
            SecretInputFrameV1::new(
                CorrelationId::new(),
                CommandInvocationId::new(),
                SecretInputOperation::ModelsEndpointReplaceCredential,
                SecretInputSlot::ApiKey,
                secret,
            )
        };
        assert!(matches!(build(Vec::new()), Err(SecretInputValidationError::Empty)));
        assert!(matches!(
            build(vec![b'x'; MAX_SECRET_INPUT_BYTES + 1]),
            Err(SecretInputValidationError::TooLarge)
        ));
        assert!(matches!(build(vec![0xff]), Err(SecretInputValidationError::InvalidUtf8)));
        assert!(matches!(
            build(b"line\nbreak".to_vec()),
            Err(SecretInputValidationError::ProhibitedCharacter)
        ));
    }

    #[test]
    fn frozen_secret_input_golden_payload_round_trips_and_clears_the_source() {
        let correlation_bytes = [
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d,
            0x0e, 0x0f,
        ];
        let invocation_bytes = [
            0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d,
            0x1e, 0x1f,
        ];
        let expected = [
            0x01, 0x01, 0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b,
            0x0c, 0x0d, 0x0e, 0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19,
            0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f, 0x01, 0x01, 0x03, 0x00, 0x00, 0x00, 0x61, 0x62,
            0x63,
        ];
        let frame = SecretInputFrameV1::new(
            CorrelationId::from_bytes(correlation_bytes),
            CommandInvocationId::from_bytes(invocation_bytes),
            SecretInputOperation::ModelsEndpointCreateWithCredential,
            SecretInputSlot::ApiKey,
            b"abc".to_vec(),
        )
        .expect("golden frame");
        assert_eq!(encode_secret_input_payload(&frame).as_slice(), expected);

        let mut payload = expected;
        let decoded = decode_secret_input_payload(&mut payload).expect("decode golden payload");
        assert_eq!(decoded, frame);
        assert!(payload.iter().all(|byte| *byte == 0));
    }

    #[test]
    fn malformed_secret_input_is_rejected_and_zeroized() {
        let frame = SecretInputFrameV1::new(
            CorrelationId::new(),
            CommandInvocationId::new(),
            SecretInputOperation::ModelsEndpointConfigureCredential,
            SecretInputSlot::ApiKey,
            b"abc".to_vec(),
        )
        .expect("frame");
        let mut malformed = encode_secret_input_payload(&frame).to_vec();
        malformed[1] = 0;
        assert!(decode_secret_input_payload(&mut malformed).is_err());
        assert!(malformed.iter().all(|byte| *byte == 0));

        let mut trailing = encode_secret_input_payload(&frame).to_vec();
        trailing.push(0xff);
        assert!(decode_secret_input_payload(&mut trailing).is_err());
        assert!(trailing.iter().all(|byte| *byte == 0));
    }

    #[test]
    fn secret_input_requires_exact_ipc_1_7_and_bilateral_feature_support() {
        let supported = vec![SECRET_INPUT_V1_FEATURE.to_owned()];
        assert!(secret_input_v1_is_negotiated(ContractVersion::new(1, 7), &supported, &supported,));
        assert!(
            !secret_input_v1_is_negotiated(ContractVersion::new(1, 6), &supported, &supported,)
        );
        assert!(!secret_input_v1_is_negotiated(ContractVersion::new(1, 7), &supported, &[],));
    }

    #[test]
    fn pending_secret_is_single_use_channel_bound_and_destroys_mismatches() {
        let accepted_at = Instant::now();
        let channel_id = IpcChannelId::new();
        let correlation_id = CorrelationId::new();
        let command_invocation_id = CommandInvocationId::new();
        let mut registry = PendingSecretInputRegistry::new();
        registry
            .register(PendingSecretInput::new(
                correlation_id,
                command_invocation_id,
                SecretInputOperation::ModelsEndpointCreateWithCredential,
                SecretInputSlot::ApiKey,
                channel_id,
                4,
                accepted_at,
            ))
            .expect("register pending command");
        let wrong_channel = SecretInputFrameV1::new(
            correlation_id,
            command_invocation_id,
            SecretInputOperation::ModelsEndpointCreateWithCredential,
            SecretInputSlot::ApiKey,
            b"secret".to_vec(),
        )
        .expect("frame");
        assert!(matches!(
            registry.consume(IpcChannelId::new(), 4, wrong_channel, accepted_at),
            Err(PendingSecretInputError::ChannelMismatch)
        ));
        assert!(registry.is_empty());

        let replay = SecretInputFrameV1::new(
            correlation_id,
            command_invocation_id,
            SecretInputOperation::ModelsEndpointCreateWithCredential,
            SecretInputSlot::ApiKey,
            b"secret".to_vec(),
        )
        .expect("replay frame");
        assert!(matches!(
            registry.consume(channel_id, 4, replay, accepted_at),
            Err(PendingSecretInputError::NotPending)
        ));
    }

    #[test]
    fn pending_secret_expires_after_the_frozen_transfer_window() {
        let accepted_at = Instant::now();
        let channel_id = IpcChannelId::new();
        let correlation_id = CorrelationId::new();
        let command_invocation_id = CommandInvocationId::new();
        let mut registry = PendingSecretInputRegistry::new();
        registry
            .register(PendingSecretInput::new(
                correlation_id,
                command_invocation_id,
                SecretInputOperation::ModelsEndpointConfigureCredential,
                SecretInputSlot::ApiKey,
                channel_id,
                1,
                accepted_at,
            ))
            .expect("register pending command");
        let frame = SecretInputFrameV1::new(
            correlation_id,
            command_invocation_id,
            SecretInputOperation::ModelsEndpointConfigureCredential,
            SecretInputSlot::ApiKey,
            b"secret".to_vec(),
        )
        .expect("frame");
        assert!(matches!(
            registry.consume(
                channel_id,
                1,
                frame,
                accepted_at + DEFAULT_PENDING_SECRET_INPUT_TTL + Duration::from_nanos(1),
            ),
            Err(PendingSecretInputError::Expired)
        ));
        assert!(registry.is_empty());
    }
}
