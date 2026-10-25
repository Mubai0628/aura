use std::str::FromStr;

use aura_domain_common::{CommandInvocationId, CorrelationId, IpcChannelId, StreamId};
use aura_ipc::{
    AsyncPriorityQueue, AuthenticatedWireFrame, BINARY_END_OF_MESSAGE, CancellationOutcome,
    FrameDecodeError, MAX_BINARY_CHUNK_BYTES, MAX_CONTROL_PAYLOAD_BYTES, OutboundFrame,
    QueuePriority, RequestLedger, SecretInputFrameV1, SecretInputOperation, SecretInputSlot,
    WireFrame, decode_authenticated_frame, decode_frame, encode_authenticated_frame, encode_frame,
};
use aura_runtime_contract::{BinaryStreamFrame, IpcFrameHeader, IpcRequestTerminal};
use tokio::time::{Duration, timeout};

fn control(priority: QueuePriority, byte: u8) -> OutboundFrame {
    OutboundFrame { priority, frame: WireFrame::Control(vec![byte]) }
}

#[test]
fn frames_use_the_frozen_length_and_binary_header_layout() {
    let stream_id =
        StreamId::from_str("str_01ARZ3NDEKTSV4RRFFQ69G5FAV").expect("fixture stream ID");
    let frame = WireFrame::BinaryChunk {
        metadata: BinaryStreamFrame {
            stream_id,
            chunk_sequence: 9,
            end_of_message: true,
            data_length: 3,
        },
        data: vec![1, 2, 3],
    };
    let mut encoded = encode_frame(&frame).expect("encode");
    assert_eq!(
        u32::from_le_bytes(encoded[..4].try_into().expect("prefix")) as usize,
        encoded.len() - 4
    );
    assert_eq!(encoded[4], 2);
    assert_eq!(&encoded[5..21], &stream_id.to_bytes());
    assert_eq!(encoded[29], BINARY_END_OF_MESSAGE);
    assert_eq!(decode_frame(&mut encoded).expect("decode"), frame);
}

#[test]
fn authenticated_frames_bind_channel_epoch_and_sequence() {
    let channel_id =
        IpcChannelId::from_str("chn_01ARZ3NDEKTSV4RRFFQ69G5FAV").expect("fixture channel ID");
    let frame = AuthenticatedWireFrame {
        header: IpcFrameHeader { channel_id, connection_epoch: 7, sequence: 42 },
        frame: WireFrame::Control(br#"{"jsonrpc":"2.0"}"#.to_vec()),
    };
    let mut encoded = encode_authenticated_frame(&frame).expect("encode authenticated frame");
    assert_eq!(
        u32::from_le_bytes(encoded[..4].try_into().expect("prefix")) as usize,
        encoded.len() - 4
    );
    assert_eq!(encoded[4], 1);
    assert_eq!(&encoded[5..21], &channel_id.to_bytes());
    assert_eq!(u64::from_le_bytes(encoded[21..29].try_into().expect("epoch")), 7);
    assert_eq!(u64::from_le_bytes(encoded[29..37].try_into().expect("sequence")), 42);
    assert_eq!(decode_authenticated_frame(&mut encoded).expect("decode"), frame);
}

#[test]
fn authenticated_secret_input_uses_frame_kind_three_and_zeroizes_decoded_bytes() {
    let frame = AuthenticatedWireFrame {
        header: IpcFrameHeader {
            channel_id: IpcChannelId::new(),
            connection_epoch: 2,
            sequence: 3,
        },
        frame: WireFrame::SecretInput(
            SecretInputFrameV1::new(
                CorrelationId::new(),
                CommandInvocationId::new(),
                SecretInputOperation::ModelsEndpointConfigureCredential,
                SecretInputSlot::ApiKey,
                b"abc".to_vec(),
            )
            .expect("secret frame"),
        ),
    };
    let mut encoded = encode_authenticated_frame(&frame).expect("encode secret frame");
    assert_eq!(encoded[4], 3);
    assert_eq!(decode_authenticated_frame(&mut encoded).expect("decode secret frame"), frame);
    assert!(encoded[37..].iter().all(|byte| *byte == 0));
}

#[test]
fn authenticated_frames_reject_a_truncated_channel_header() {
    let mut encoded = vec![0_u8; 4 + 32];
    encoded[..4].copy_from_slice(&32_u32.to_le_bytes());
    encoded[4] = 1;
    assert_eq!(
        decode_authenticated_frame(&mut encoded),
        Err(FrameDecodeError::TruncatedAuthenticatedHeader)
    );
}

#[test]
fn unknown_binary_flags_are_protocol_errors() {
    let stream_id =
        StreamId::from_str("str_01ARZ3NDEKTSV4RRFFQ69G5FAV").expect("fixture stream ID");
    let frame = WireFrame::BinaryChunk {
        metadata: BinaryStreamFrame {
            stream_id,
            chunk_sequence: 0,
            end_of_message: false,
            data_length: 0,
        },
        data: Vec::new(),
    };
    let mut encoded = encode_frame(&frame).expect("encode");
    encoded[29] = 0x80;
    assert_eq!(decode_frame(&mut encoded), Err(FrameDecodeError::UnknownBinaryFlags(0x80)));
}

#[test]
fn maximum_size_control_and_binary_frames_round_trip() {
    let control_frame = WireFrame::Control(vec![0x5a; MAX_CONTROL_PAYLOAD_BYTES]);
    let mut encoded_control = encode_frame(&control_frame).expect("encode maximum control frame");
    assert_eq!(
        decode_frame(&mut encoded_control).expect("decode maximum control frame"),
        control_frame
    );

    let stream_id =
        StreamId::from_str("str_01ARZ3NDEKTSV4RRFFQ69G5FAV").expect("fixture stream ID");
    let binary_frame = WireFrame::BinaryChunk {
        metadata: BinaryStreamFrame {
            stream_id,
            chunk_sequence: u64::MAX,
            end_of_message: true,
            data_length: u32::try_from(MAX_BINARY_CHUNK_BYTES).expect("chunk limit fits u32"),
        },
        data: vec![0xa5; MAX_BINARY_CHUNK_BYTES],
    };
    let mut encoded_binary = encode_frame(&binary_frame).expect("encode maximum binary frame");
    assert_eq!(
        decode_frame(&mut encoded_binary).expect("decode maximum binary frame"),
        binary_frame
    );
}

#[test]
fn catalog_sized_control_frames_round_trip_without_chunking() {
    for payload_size in [2 * 1024 * 1024, 5 * 1024 * 1024] {
        let frame = WireFrame::Control(vec![0x43; payload_size]);
        let mut encoded = encode_frame(&frame).expect("encode catalog-sized control frame");
        assert_eq!(decode_frame(&mut encoded).expect("decode catalog-sized control frame"), frame);
    }
}

#[tokio::test]
async fn rejected_oversized_control_payload_does_not_close_the_priority_queue() {
    let oversized = MAX_CONTROL_PAYLOAD_BYTES + 1;
    assert!(matches!(
        encode_frame(&WireFrame::Control(vec![0; oversized])),
        Err(FrameDecodeError::ControlPayloadTooLarge { .. })
    ));

    let queue = AsyncPriorityQueue::new();
    let catalog = OutboundFrame {
        priority: QueuePriority::P1,
        frame: WireFrame::Control(vec![0x43; 5 * 1024 * 1024]),
    };
    queue.send(catalog).await.expect("enqueue a 5 MiB P1 catalog frame");
    assert_eq!(queue.recv().await.expect("receive catalog frame").priority, QueuePriority::P1);
    queue.send(control(QueuePriority::P0, 0x7f)).await.expect("queue remains open");
    assert_eq!(queue.recv().await.expect("receive after rejection").priority, QueuePriority::P0);
}

#[tokio::test]
async fn p1_reserve_accepts_a_maximum_control_frame_under_bulk_load() {
    let queue = AsyncPriorityQueue::new();
    for payload_size in [5 * 1024 * 1024, 19 * 256 * 1024] {
        queue
            .send(OutboundFrame {
                priority: QueuePriority::P2,
                frame: WireFrame::Control(vec![0x22; payload_size]),
            })
            .await
            .expect("bulk frame fits below its partition ceiling");
    }

    timeout(
        Duration::from_millis(250),
        queue.send(OutboundFrame {
            priority: QueuePriority::P1,
            frame: WireFrame::Control(vec![0x43; MAX_CONTROL_PAYLOAD_BYTES]),
        }),
    )
    .await
    .expect("the P1 reservation accepts a maximum catalog frame without draining bulk")
    .expect("enqueue maximum P1 frame");
    queue.send(control(QueuePriority::P0, 0x7f)).await.expect("P0 reserve remains available");

    assert_eq!(queue.recv().await.expect("receive P0").priority, QueuePriority::P0);
    assert_eq!(queue.recv().await.expect("receive P1").priority, QueuePriority::P1);
}

#[test]
fn oversized_control_and_binary_frames_are_rejected() {
    let control_size = MAX_CONTROL_PAYLOAD_BYTES + 1;
    assert_eq!(
        encode_frame(&WireFrame::Control(vec![0; control_size])),
        Err(FrameDecodeError::ControlPayloadTooLarge {
            actual: control_size,
            maximum: MAX_CONTROL_PAYLOAD_BYTES,
        })
    );

    let stream_id =
        StreamId::from_str("str_01ARZ3NDEKTSV4RRFFQ69G5FAV").expect("fixture stream ID");
    let binary_size = MAX_BINARY_CHUNK_BYTES + 1;
    assert_eq!(
        encode_frame(&WireFrame::BinaryChunk {
            metadata: BinaryStreamFrame {
                stream_id,
                chunk_sequence: 0,
                end_of_message: false,
                data_length: u32::try_from(binary_size).expect("oversized fixture fits u32"),
            },
            data: vec![0; binary_size],
        }),
        Err(FrameDecodeError::BinaryChunkTooLarge {
            actual: binary_size,
            maximum: MAX_BINARY_CHUNK_BYTES,
        })
    );
}

#[tokio::test]
async fn priority_queue_preempts_bulk_without_starving_it() {
    let queue = AsyncPriorityQueue::new();
    queue.send(control(QueuePriority::P3, 3)).await.expect("P3");
    queue.send(control(QueuePriority::P2, 2)).await.expect("P2");
    queue.send(control(QueuePriority::P1, 1)).await.expect("P1");
    queue.send(control(QueuePriority::P0, 0)).await.expect("P0");

    assert_eq!(queue.recv().await.expect("receive").priority, QueuePriority::P0);
    assert_eq!(queue.recv().await.expect("receive").priority, QueuePriority::P1);
    assert_eq!(queue.recv().await.expect("receive").priority, QueuePriority::P2);
    assert_eq!(queue.recv().await.expect("receive").priority, QueuePriority::P3);
}

#[tokio::test]
async fn p0_reserve_applies_backpressure_until_space_is_released() {
    let queue = AsyncPriorityQueue::new();
    for byte in 0_u8..8 {
        queue.send(control(QueuePriority::P0, byte)).await.expect("fill P0 reserve");
    }
    assert!(
        timeout(Duration::from_millis(25), queue.send(control(QueuePriority::P0, 8)))
            .await
            .is_err()
    );
    let _ = queue.recv().await.expect("release one P0 slot");
    timeout(Duration::from_millis(250), queue.send(control(QueuePriority::P0, 8)))
        .await
        .expect("P0 enqueue should resume after space is released")
        .expect("P0 enqueue");
}

#[test]
fn request_ledger_has_exactly_one_local_terminal() {
    let request =
        CorrelationId::from_str("cor_01ARZ3NDEKTSV4RRFFQ69G5FAV").expect("fixture request ID");
    let mut ledger = RequestLedger::new();
    ledger.begin(request).expect("begin");
    assert!(!ledger.cancellation_requested(request));
    assert_eq!(
        ledger.cancel(request),
        CancellationOutcome::QueuedNotStarted,
        "a request that has not started is cancelled while it is still queued"
    );
    assert!(ledger.cancellation_requested(request));
    ledger.finish(request, IpcRequestTerminal::Cancelled).expect("terminal");
    assert_eq!(ledger.terminal(request), Some(IpcRequestTerminal::Cancelled));
    assert!(ledger.finish(request, IpcRequestTerminal::Succeeded).is_err());
}

#[test]
fn duplicate_request_does_not_replace_the_original_ledger_state() {
    let request =
        CorrelationId::from_str("cor_01ARZ3NDEKTSV4RRFFQ69G5FAV").expect("fixture request ID");
    let mut ledger = RequestLedger::new();
    ledger.begin(request).expect("begin");
    ledger.cancel(request);
    assert!(ledger.begin(request).is_err());
    assert!(ledger.cancellation_requested(request));
}
