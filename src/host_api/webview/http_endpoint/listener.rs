//! Response-aware loopback SOCKS endpoint. It never opens a connection to the logical target.
mod budget;
mod policy;
mod request;
use super::{
    certificates::{Authority, Certificates},
    io::{Current, CurrentConnection},
};
use crate::remote::html_http::endpoint::{self, EndpointToken};
pub(crate) use budget::EndpointBudget;
use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tasty_remote::{
    html_http_body::VerifiedBody,
    html_http_native::{NativeBudget, NativeLimits, NativeRequest},
};
use tokio::{
    net::{TcpListener, TcpStream},
    runtime::Handle,
    sync::watch,
    task::{JoinHandle, JoinSet},
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EndpointError {
    Invalid,
    Retired,
    Capacity,
    Auth,
    Destination,
    Tls,
    Io,
    Body,
    Identity,
    Worker,
}
pub(crate) trait ResponseOwner: Send {
    fn body(&self) -> &Arc<VerifiedBody>;
    fn request(&self) -> &Arc<NativeRequest>;
}
pub(crate) type ResponseFuture =
    Pin<Box<dyn Future<Output = Result<Box<dyn ResponseOwner>, EndpointError>> + Send>>;
pub(crate) type Handler = Arc<dyn Fn(Arc<NativeRequest>) -> ResponseFuture + Send + Sync>;
#[derive(Clone, Copy)]
pub(crate) struct DocumentIdentity {
    pub epoch: u64,
    pub surface: u32,
    pub binding: u64,
    pub document: u64,
    pub source_generation: u64,
}
impl DocumentIdentity {
    fn matches(&self, body: &VerifiedBody) -> bool {
        let id = body.head().id;
        let request = id.request;
        id.serial > 0
            && id.source_generation == self.source_generation
            && (
                request.epoch,
                request.surface_id,
                request.binding,
                request.document,
            ) == (self.epoch, self.surface, self.binding, self.document)
    }
}
pub(crate) struct EndpointConfig {
    pub identity: DocumentIdentity,
    pub entry_url: String,
    pub allow_remote_content: bool,
    pub https: bool,
    pub current: Current,
    pub limits: NativeLimits,
    pub native_budget: NativeBudget,
    pub handler: Handler,
}
struct State {
    config: EndpointConfig,
    policy: policy::Policy,
    current: Current,
    stop: Arc<AtomicBool>,
    token: EndpointToken,
    certificates: Arc<Certificates>,
    budget: EndpointBudget,
}
pub(crate) struct Endpoint {
    state: Arc<State>,
    proxy_uri: String,
    stop: watch::Sender<bool>,
    task: Option<JoinHandle<()>>,
}
impl Endpoint {
    /// Schedule on the injected runtime, never poll socket setup on the native/engine loop.
    pub(crate) async fn start(
        runtime: Handle,
        config: EndpointConfig,
        budget: EndpointBudget,
    ) -> Result<Self, EndpointError> {
        let identity = config.identity;
        if [
            identity.epoch,
            u64::from(identity.surface),
            identity.binding,
            identity.document,
            identity.source_generation,
        ]
        .contains(&0)
            || config.limits.upload_bytes == 0
            || config.limits.upload_bytes > tasty_ipc::admission::QUEUED_BYTES_LIMIT
            || config.limits.io_timeout.is_zero()
        {
            return Err(EndpointError::Invalid);
        }
        if !(config.current)() {
            return Err(EndpointError::Retired);
        }
        let policy = policy::Policy::new(&config)?;
        let owner = budget.owner()?;
        let stop = Arc::new(AtomicBool::new(false));
        let current: Current = {
            let stop = stop.clone();
            let current = config.current.clone();
            Arc::new(move || !stop.load(Ordering::Acquire) && current())
        };
        let mut entropy = [0; 32];
        use rand::TryRngCore;
        rand::rngs::OsRng
            .try_fill_bytes(&mut entropy)
            .map_err(|_| EndpointError::Worker)?;
        let token = EndpointToken::from_random_bytes(entropy);
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|_| EndpointError::Io)?;
        let port = listener.local_addr().map_err(|_| EndpointError::Io)?.port();
        let proxy_uri = token.proxy_uri(port);
        if !current() {
            return Err(EndpointError::Retired);
        }
        let certificates =
            Certificates::new(current.clone(), budget.certificates(), runtime.clone());
        let state = Arc::new(State {
            config,
            policy,
            current,
            stop,
            token,
            certificates,
            budget,
        });
        let (stop, receiver) = watch::channel(false);
        let task = runtime.spawn(listen(state.clone(), listener, receiver, owner));
        Ok(Self {
            state,
            proxy_uri,
            stop,
            task: Some(task),
        })
    }
    pub(crate) fn proxy_uri(&self) -> String {
        self.proxy_uri.clone()
    }
    pub(crate) fn expected_leaf(&self, host: &str, port: u16) -> Option<[u8; 32]> {
        self.state.certificates.expected_leaf(host, port)
    }
    pub(crate) fn close(&self) {
        self.state.stop.store(true, Ordering::Release);
        self.stop.send_replace(true);
    }
    pub(crate) async fn join(mut self) -> Result<(), EndpointError> {
        self.close();
        if let Some(task) = self.task.take() {
            task.await.map_err(|_| EndpointError::Worker)?;
        }
        Ok(())
    }
}
impl Drop for Endpoint {
    fn drop(&mut self) {
        self.close();
    }
}
async fn listen(
    state: Arc<State>,
    listener: TcpListener,
    mut stop: watch::Receiver<bool>,
    _owner: budget::Owner,
) {
    let mut tasks = JoinSet::new();
    while (state.current)() {
        tokio::select! {
            _=stop.changed()=>break,
            result=listener.accept()=>match result {
                Ok((socket,_))=>{if let Ok(permit)=state.budget.socket() {let state=state.clone();tasks.spawn(async move {let _permit=permit;if let Err(error)=connection(state,socket).await {tracing::debug!(?error,"native mirror connection ended");}});}else{drop(socket);}},
                Err(error)=>{tracing::warn!(%error,"native mirror listener failed");break;}
            },
            result=tasks.join_next(),if !tasks.is_empty()=>{if let Some(Err(error))=result {tracing::warn!(%error,"native mirror connection task failed");}}
        }
    }
    drop(listener);
    state.certificates.retire();
    tasks.abort_all();
    while let Some(result) = tasks.join_next().await {
        if let Err(error) = result
            && !error.is_cancelled()
        {
            tracing::warn!(%error,"native mirror connection retirement failed");
        }
    }
    state.certificates.settled().await;
}
async fn connection(state: Arc<State>, socket: TcpStream) -> Result<(), EndpointError> {
    let deadline = state.config.limits.io_timeout;
    let mut io = CurrentConnection::new(socket, state.current.clone());
    let route = endpoint::authenticate(&mut io, &state.token, deadline)
        .await
        .map_err(|_| EndpointError::Auth)?;
    let authority =
        Authority::new(&route.host, route.port).map_err(|_| EndpointError::Destination)?;
    if !(state.current)() || !state.policy.candidate(&authority) {
        endpoint::reject_route(&mut io)
            .await
            .map_err(|_| EndpointError::Io)?;
        return Err(EndpointError::Destination);
    }
    endpoint::accept_route(&mut io)
        .await
        .map_err(|_| EndpointError::Io)?;
    let first = tokio::time::timeout(deadline, io.first_byte())
        .await
        .map_err(|_| EndpointError::Io)?
        .map_err(|_| EndpointError::Io)?;
    let scheme = if first == 22 { "https" } else { "http" };
    if !(state.current)() || !state.policy.permits(scheme, &authority) {
        return Err(EndpointError::Destination);
    }
    if scheme == "https" {
        let certificate = state
            .certificates
            .certificate(authority.clone())
            .await
            .map_err(|_| EndpointError::Tls)?;
        let io = certificate
            .accept(io, deadline)
            .await
            .map_err(|_| EndpointError::Tls)?;
        if !sni_matches(&authority, io.server_name()) {
            return Err(EndpointError::Destination);
        }
        request::serve(state, io, authority, scheme).await
    } else {
        request::serve(state, io, authority, scheme).await
    }
}
fn sni_matches(authority: &Authority, sni: Option<&str>) -> bool {
    match sni {
        None => authority.host().parse::<std::net::IpAddr>().is_ok(),
        Some(host) => Authority::new(host, authority.port())
            .is_ok_and(|supplied| supplied.host() == authority.host()),
    }
}
#[cfg(test)]
mod tests;
