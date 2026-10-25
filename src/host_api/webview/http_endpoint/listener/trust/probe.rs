//! A separate authenticated TLS-only listener. No HTTP decoder or target connector exists here.
use super::*;
struct Stopped {
    slot: Arc<Slot>,
    clean: bool,
}
impl Drop for Stopped {
    fn drop(&mut self) {
        if !self.clean {
            self.slot.failed.store(true, Ordering::Release);
            self.slot.cancel();
        }
        self.slot.stopped.store(true, Ordering::Release);
        self.slot.changed.notify_waiters();
    }
}
pub(super) async fn listen(slot: Arc<Slot>, listener: TcpListener, budget: EndpointBudget) {
    let mut stopped = Stopped {
        slot: slot.clone(),
        clean: false,
    };
    let mut tasks = JoinSet::new();
    while slot.current() {
        tokio::select! {
            result=listener.accept()=>match result {
                Ok((socket,_))=>if let Ok(permit)=budget.probe() { let slot=slot.clone(); tasks.spawn(async move { let _permit=permit; if let Err(error)=connection(slot,socket).await { tracing::debug!(?error,"native TLS-only probe ended"); } }); },
                Err(error)=>{tracing::warn!(%error,"native TLS-only listener failed");slot.cancel();break;}
            },
            _=slot.changed.notified()=>{},
            _=tokio::time::sleep(Duration::from_millis(10))=>{},
            result=tasks.join_next(),if !tasks.is_empty()=>if let Some(Err(error))=result { tracing::warn!(%error,"native TLS-only socket task failed"); slot.cancel(); }
        }
    }
    drop(listener);
    tasks.abort_all();
    while let Some(result) = tasks.join_next().await {
        if let Err(error) = result
            && !error.is_cancelled()
        {
            tracing::warn!(%error,"native TLS-only socket retirement failed");
        }
    }
    stopped.clean = true;
}
async fn connection(slot: Arc<Slot>, socket: TcpStream) -> Result<(), EndpointError> {
    let deadline = slot
        .owner
        .upgrade()
        .ok_or(EndpointError::Retired)?
        .config
        .limits
        .io_timeout
        .min(
            slot.deadline
                .saturating_duration_since(tokio::time::Instant::now()),
        );
    let weak = Arc::downgrade(&slot);
    let current: Current = Arc::new(move || weak.upgrade().is_some_and(|s| s.current()));
    let mut io = CurrentConnection::new(socket, current);
    let route = endpoint::authenticate(&mut io, &slot.token, deadline)
        .await
        .map_err(|_| EndpointError::Auth)?;
    if !Authority::new(&route.host, route.port).is_ok_and(|a| a == slot.authority) {
        endpoint::reject_route(&mut io)
            .await
            .map_err(|_| EndpointError::Io)?;
        return Err(EndpointError::Destination);
    }
    endpoint::accept_route(&mut io)
        .await
        .map_err(|_| EndpointError::Io)?;
    let mut io = hello::capture(io, &slot.authority, deadline).await?;
    io.flight = Some(Arc::downgrade(&slot));
    // Drop either the completed TLS stream or the issuer-rejection error immediately.
    // Even unexpectedly trusted clients cannot send data to an HTTP/body handler here.
    match slot.certificate.accept(io, deadline).await {
        Ok(stream) => drop(stream),
        Err(error) => tracing::debug!(?error, "native TLS-only handshake ended"),
    }
    if slot.flight.load(Ordering::Acquire) {
        slot.finished.store(true, Ordering::Release);
        slot.changed.notify_waiters();
    }
    Ok(())
}
