//! Endpoint/socket joins settle off GTK before a retained native document can resume.
use super::*;
impl HttpSession {
    #[cfg_attr(
        not(target_os = "linux"),
        expect(
            dead_code,
            reason = "only the WebKitGTK adapter retains HTTP reloads and probes native trust"
        )
    )]
    pub(crate) fn endpoint_joined(&self) -> Result<bool, String> {
        match self.joined.load(Ordering::Acquire) {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err("native HTTP endpoint retirement failed".into()),
        }
    }
    pub(crate) fn close(&self) {
        if self.closing.swap(true, Ordering::AcqRel) {
            return;
        }
        let body = self.entry_body();
        if let Some(body) = body
            && let Ok(mut retained) = self.final_body.lock()
        {
            *retained = Some(body);
        }
        self.bodies.retire();
        #[cfg(windows)]
        self.script.clear();
        self.close_reload();
        // Mint the exact terminal Close while the old captured document is still current.
        // BodySource is already revoked; publication/endpoint callbacks retire immediately next.
        self.bridge.close();
        self.live.store(false, Ordering::Release);
        self.view.retire();
        let Ok(mut state) = self.state.lock() else {
            self.joined.store(2, Ordering::Release);
            (self.wake)();
            tracing::error!("native HTTP teardown state poisoned");
            return;
        };
        state.setup.take();
        #[cfg(target_os = "linux")]
        state.preparation.take();
        state.trust.take();
        let endpoint = state.endpoint.take();
        let pending = state.endpoint_pending.take();
        let runtime = state.runtime.take();
        let Some(runtime) = runtime else {
            self.joined.store(1, Ordering::Release);
            (self.wake)();
            return;
        };
        if let Some(endpoint) = &endpoint {
            endpoint.close();
        }
        let joined = self.joined.clone();
        let wake = self.wake.clone();
        drop(runtime.handle().spawn(async move {
            let result = join_endpoint(endpoint, pending).await;
            if let Err(error) = &result {
                tracing::warn!(?error, "native HTTP endpoint retirement failed");
            }
            drop(runtime);
            joined.store(if result.is_ok() { 1 } else { 2 }, Ordering::Release);
            wake();
        }));
    }
}
async fn join_endpoint(
    endpoint: Option<Arc<Endpoint>>,
    pending: Option<EndpointReceipt>,
) -> Result<(), EndpointError> {
    if let Some(endpoint) = endpoint {
        return Arc::try_unwrap(endpoint)
            .map_err(|_| EndpointError::Worker)?
            .join()
            .await;
    }
    if let Some(pending) = pending {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
        loop {
            match pending.try_recv() {
                Ok(Ok((endpoint, trust))) => {
                    drop(trust);
                    return endpoint.join().await;
                }
                Ok(Err(EndpointError::Retired)) | Err(mpsc::TryRecvError::Disconnected) => {
                    return Ok(());
                }
                Ok(Err(error)) => return Err(error),
                Err(mpsc::TryRecvError::Empty) => {}
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(EndpointError::Worker);
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
    Ok(())
}
