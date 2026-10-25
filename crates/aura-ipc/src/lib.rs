#![forbid(unsafe_code)]

mod frame;
mod ledger;
mod queue;
pub mod routes;
mod rpc;
mod secret;
mod sidecar;
mod tool_conformance_runtime_secret;
mod tool_runtime_secret;
mod trace;

pub use frame::{
    AUTHENTICATED_FRAME_HEADER_SIZE, AuthenticatedFrameReader, AuthenticatedWireFrame,
    BINARY_CHUNK_HEADER_SIZE, BINARY_END_OF_MESSAGE, FRAME_LENGTH_PREFIX_SIZE, FrameDecodeError,
    FrameRead, FrameType, MAX_BINARY_CHUNK_BYTES, MAX_CONTROL_PAYLOAD_BYTES, MAX_FRAME_LENGTH,
    WireFrame, decode_authenticated_frame, decode_frame, encode_authenticated_frame, encode_frame,
    read_authenticated_frame, read_frame, write_authenticated_frame, write_frame,
};
pub use ledger::{CancellationOutcome, CancellationSettlement, LedgerError, RequestLedger};
pub use queue::{AsyncPriorityQueue, OutboundFrame, QueueClosed, QueuePriority, QueuePushError};
pub use rpc::{
    JSON_RPC_VERSION, JsonRpcError, JsonRpcMessage, JsonRpcNotification, JsonRpcRequest,
    JsonRpcResponse, JsonRpcSuccess, method,
};
pub use secret::{
    DEFAULT_PENDING_SECRET_INPUT_TTL, MAX_SECRET_INPUT_BYTES, PendingSecretInput,
    PendingSecretInputError, PendingSecretInputRegistry, SECRET_INPUT_FIXED_HEADER_SIZE,
    SECRET_INPUT_FRAME_VERSION, SECRET_INPUT_V1_FEATURE, SecretInputDecodeError,
    SecretInputEncoding, SecretInputFrameV1, SecretInputOperation, SecretInputSlot,
    SecretInputValidationError, decode_secret_input_payload, encode_secret_input_payload,
    secret_input_v1_is_negotiated,
};
pub use sidecar::{SidecarBootstrapConfig, SidecarHandshakeError, connect_authenticated_sidecar};
pub use tool_conformance_runtime_secret::{
    TOOL_CONFORMANCE_RUNTIME_SECRET_FRAME_VERSION, ToolConformanceRuntimeSecretDecodeError,
    ToolConformanceRuntimeSecretFrameV1, ToolConformanceRuntimeSecretValidationError,
    decode_tool_conformance_runtime_secret_payload, encode_tool_conformance_runtime_secret_payload,
};
pub use tool_runtime_secret::{
    TOOL_RUNTIME_SECRET_FRAME_VERSION, ToolRuntimeSecretDecodeError, ToolRuntimeSecretFrameV1,
    ToolRuntimeSecretValidationError, decode_tool_runtime_secret_payload,
    encode_tool_runtime_secret_payload,
};
pub use trace::trace;
