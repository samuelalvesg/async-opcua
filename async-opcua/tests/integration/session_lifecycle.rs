//! Lifecycle of the client session: what the caller can observe when the connection never comes up
//! or goes away for good.

use std::{sync::atomic::Ordering, time::Duration};

use opcua::{
    client::IdentityToken,
    crypto::SecurityPolicy,
    types::{MessageSecurityMode, UserTokenPolicy},
};
use tokio::net::TcpListener;

use crate::utils::{default_client, hostname, TEST_COUNTER};

/// A TCP port nothing listens on: bind to get a free one, then release it.
async fn closed_port() -> u16 {
    let listener = TcpListener::bind(format!("{}:0", hostname()))
        .await
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    port
}

/// `Session::wait_for_connection` must not hang once the event loop has ended for good.
///
/// With `session_retry_limit(0)` the event loop gives up after the first failed attempt and its
/// task finishes with the failure status. `wait_for_connection` only watched a state channel that
/// the finished loop never updates again, so the caller waited forever (the documentation says
/// "if the event loop ends, this method will never return"). It should return `false` instead.
#[tokio::test]
async fn wait_for_connection_returns_false_when_the_event_loop_has_ended() {
    let _ = env_logger::try_init();
    let test_id = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    let port = closed_port().await;

    let mut client = default_client(test_id, false)
        .session_retry_limit(0)
        .client()
        .unwrap();
    let (session, event_loop) = client
        .connect_to_endpoint_directly(
            (
                format!("opc.tcp://{}:{port}/", hostname()).as_str(),
                SecurityPolicy::None.to_str(),
                MessageSecurityMode::None,
                UserTokenPolicy::anonymous(),
            ),
            IdentityToken::Anonymous,
        )
        .unwrap();
    let status = event_loop.spawn();

    let connected = tokio::time::timeout(Duration::from_secs(10), session.wait_for_connection())
        .await
        .expect("wait_for_connection never returned although the event loop has ended");
    assert!(
        !connected,
        "there is no server, the session cannot be connected"
    );

    let status = status.await.unwrap();
    assert!(
        status.is_bad(),
        "the event loop must report why it ended, got {status}"
    );
}
