//! Complete verified bytes and full native request identity are required before any response byte.
use super::*;
use tasty_remote::html_http_native::{Destination, read_request, write_response_with};
use tokio::io::{AsyncRead, AsyncWrite};
pub(super) async fn serve<S: AsyncRead + AsyncWrite + Unpin>(
    state: Arc<State>,
    mut io: S,
    authority: Authority,
    scheme: &str,
) -> Result<(), EndpointError> {
    let current = {
        let state = state.clone();
        let authority = authority.clone();
        let scheme = scheme.to_owned();
        move || (state.current)() && state.policy.permits(&scheme, &authority)
    };
    let destination = Destination::new(scheme, authority.host(), authority.port())
        .map_err(|_| EndpointError::Destination)?;
    let request = Arc::new(
        read_request(
            &mut io,
            &destination,
            state.config.limits,
            &state.config.native_budget,
            &current,
        )
        .await
        .map_err(|_| EndpointError::Io)?,
    );
    if !current() {
        return Err(EndpointError::Retired);
    }
    // Consent/verification is a bounded native waiter, not active network time. Owner close
    // aborts this task and drops the waiter; no I/O deadline consumes the user's question wait.
    let response = (state.config.handler)(request.clone()).await?;
    let body = response.body().clone();
    let owner = response.request().clone();
    if !current() {
        return Err(EndpointError::Retired);
    }
    if !Arc::ptr_eq(&request, &owner)
        || !state.config.identity.matches(&body)
        || body.head().logical_url != request.logical_url
        || !body.matches_native(body.head().id, &request.method, request.identity)
    {
        return Err(EndpointError::Identity);
    }
    let result = write_response_with(
        &mut io,
        body.clone(),
        body.head().id,
        &request.method,
        request.identity,
        response.local_headers(),
        state.config.limits,
        &current,
    )
    .await
    .map_err(|_| EndpointError::Io);
    response.write_completed(result.is_ok());
    drop(response);
    result
}
