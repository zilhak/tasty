//! native mirror 끝점의 listener 와 accept 한 연결은 상속 플래그가 꺼져 있다(Windows). 이음새
//! (`tasty_utils::socket_inherit::seam`)가 만든 직후 상속을 일부러 켜고, 끈 뒤의 값을 본다. 끄는 호출이
//! 빠지면 std·tokio 가 돌려준 값과 상관없이 실패한다.

use super::super::{SEAM_ACCEPT, SEAM_LISTENER};
use super::*;
use tasty_utils::socket_inherit::seam::{self, Scope, Seen};

fn set_then_cleared(created: &[Seen], cleared: &[Seen]) -> bool {
    !created.is_empty()
        && created.iter().all(|seen| seen.inheritable)
        && !cleared.is_empty()
        && cleared.iter().all(|seen| !seen.inheritable)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_endpoint_sockets_are_not_inheritable() {
    // 끝점의 bind 는 `Endpoint::start` 를 기다리는 이 스레드에서 돈다.
    let listener_seam = seam::arm(SEAM_LISTENER, Scope::Thread);
    let handler: Handler = Arc::new(|request| Box::pin(async move { Ok(reply(request)) }));
    let budget = EndpointBudget::new();
    let endpoint = Endpoint::start(
        Handle::current(),
        config("http://127.0.0.1:9/doc".into(), handler),
        budget.clone(),
    )
    .await
    .unwrap_or_else(|_| panic!("endpoint"));
    let accept_seam = seam::arm(SEAM_ACCEPT, Scope::Port(endpoint.port()));
    // 인증 응답이 오면 끝점이 그 연결을 accept 해 처리한 뒤다.
    let socket = authenticated(&endpoint, None).await;
    drop(socket);
    endpoint.join().await.unwrap();
    let listener_seen = (listener_seam.created(), listener_seam.cleared());
    let accept_seen = (accept_seam.created(), accept_seam.cleared());
    assert!(
        set_then_cleared(&listener_seen.0, &listener_seen.1)
            && set_then_cleared(&accept_seen.0, &accept_seen.1),
        "listener {listener_seen:?}, accepted {accept_seen:?}"
    );
}
