//! What a caller sees when a response does not fit the client's decoding limits.

use std::time::Duration;

use opcua::types::{AttributeId, NodeId, ReadValueId, StatusCode, TimestampsToReturn, VariableId};

use crate::utils::{default_client, test_server, Tester};

fn read_ids(count: usize) -> Vec<ReadValueId> {
    let node: NodeId = VariableId::Server_ServerStatus_CurrentTime.into();
    vec![
        ReadValueId {
            node_id: node,
            attribute_id: AttributeId::Value as u32,
            ..Default::default()
        };
        count
    ]
}

/// A response with more elements than the client's `max_array_length` is a decoding error and the
/// transport is closed (the stream is no longer trustworthy). The request that was waiting for that
/// response used to be told only "BadConnectionClosed: Fatal error, connection closed", because its
/// reply channel was dropped without being used: the cause was only in the log. The caller of the
/// request must get the real status, so a too-small limit is a visible, fixable configuration
/// error instead of an unexplained disconnection.
#[tokio::test]
async fn response_larger_than_the_client_array_limit_reports_the_decoding_error() {
    let _ = env_logger::try_init();
    let client = default_client(0, false).max_array_length(1_000);
    let mut tester = Tester::new_custom_client(test_server(), client).await;
    let (session, event_loop) = tester.connect_default().await.unwrap();
    event_loop.spawn();
    assert!(session.wait_for_connection().await);

    let err = session
        .read(&read_ids(1_500), TimestampsToReturn::Neither, 0.0)
        .await
        .expect_err("a response of 1500 values cannot be decoded with a limit of 1000");
    assert_eq!(
        err.status(),
        StatusCode::BadDecodingError,
        "the caller must be told why the request failed, got: {err}"
    );
    assert!(
        err.to_string().contains("1000"),
        "the error should name the limit that was exceeded, got: {err}"
    );
}

/// After such a failure the session must come back and serve requests that fit the limits.
#[tokio::test]
async fn session_recovers_after_a_response_exceeded_the_decoding_limit() {
    let _ = env_logger::try_init();
    let client = default_client(0, false).max_array_length(1_000);
    let mut tester = Tester::new_custom_client(test_server(), client).await;
    let (session, event_loop) = tester.connect_default().await.unwrap();
    event_loop.spawn();
    assert!(session.wait_for_connection().await);

    let _ = session
        .read(&read_ids(1_500), TimestampsToReturn::Neither, 0.0)
        .await;

    // The connection is re-established by the event loop; give it a moment, then a request that
    // fits the limits must work.
    let mut recovered = false;
    for _ in 0..50 {
        if session
            .read(&read_ids(10), TimestampsToReturn::Neither, 0.0)
            .await
            .is_ok_and(|values| values.len() == 10)
        {
            recovered = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        recovered,
        "the session did not recover after the decoding failure"
    );
}
