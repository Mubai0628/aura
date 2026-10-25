//! Media's disconnect observer must not compete with the real frame receiver.
use super::*;
use std::time::Duration;

#[tokio::test]
async fn closed_before_subscribe_is_observed_immediately() {
    let queue = AsyncPriorityQueue::new();
    queue.close();
    tokio::time::timeout(Duration::from_millis(100), queue.closed()).await.expect("closed at once");
}

#[tokio::test]
async fn close_wakes_a_subscribed_observer() {
    let queue = AsyncPriorityQueue::new();
    let observer = queue.clone();
    let waiting = tokio::spawn(async move { observer.closed().await });
    tokio::task::yield_now().await;
    queue.close();
    tokio::time::timeout(Duration::from_millis(100), waiting)
        .await
        .expect("the observer woke")
        .expect("the observer task ended");
}

#[tokio::test]
async fn disconnect_observation_never_consumes_the_frame_receivers_wakeup() {
    let queue = AsyncPriorityQueue::new();
    let receiver = queue.clone();
    let receiving = tokio::spawn(async move { receiver.recv().await });
    let observer = queue.clone();
    let waiting = tokio::spawn(async move { observer.closed().await });
    tokio::task::yield_now().await;
    queue
        .send(OutboundFrame { priority: QueuePriority::P1, frame: WireFrame::Control(vec![7]) })
        .await
        .expect("frame queued");
    let received = tokio::time::timeout(Duration::from_millis(100), receiving)
        .await
        .expect("the receiver woke")
        .expect("the receiver task ended")
        .expect("a frame arrived");
    assert_eq!(received.frame, WireFrame::Control(vec![7]));
    assert!(!waiting.is_finished());
    queue.close();
    tokio::time::timeout(Duration::from_millis(100), waiting)
        .await
        .expect("the observer woke")
        .expect("the observer task ended");
}
