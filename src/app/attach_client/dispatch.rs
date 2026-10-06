//! GUI attach preflight and connector entry share one decision boundary.

#[cfg(debug_assertions)]
mod debug_completion;

#[derive(Clone, Copy, Debug)]
pub(super) enum AttachSource {
    Ipc,
    User,
    /// IPC `remote.attach`와 자동 attach가 해석한 endpoint.
    Endpoint,
}

impl AttachSource {
    fn label(self) -> &'static str {
        match self {
            Self::Ipc => "attach.into_gui",
            Self::User => "remote-attach",
            Self::Endpoint => "remote.attach/auto-attach",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Outcome<T, E> {
    RejectedSelf,
    Connected(Result<T, E>),
}

fn connect_unless_self<T, E>(
    own_port: Option<u16>,
    port: u16,
    connect: impl FnOnce() -> Result<T, E>,
) -> Outcome<T, E> {
    if own_port == Some(port) {
        return Outcome::RejectedSelf;
    }
    Outcome::Connected(connect())
}

pub(super) fn dispatch_attach<T, E>(
    own_port: Option<u16>,
    port: u16,
    workspace: u32,
    source: AttachSource,
    connect: impl FnOnce() -> Result<T, E>,
) -> Outcome<T, E> {
    #[cfg(debug_assertions)]
    let observation = debug_completion::Observation::new();
    let outcome = connect_unless_self(own_port, port, || {
        #[cfg(debug_assertions)]
        observation.enter_connector();
        connect()
    });
    if matches!(outcome, Outcome::RejectedSelf) {
        let label = source.label();
        tracing::warn!(
            "self(loopback) {label} (port={port}) 는 차단됩니다 — 자기 자신을 mirror 하면 \
             그 mirror 가 자기 workspace 를 점유해 원본의 로컬 입력이 막힙니다. \
             로컬 self-mirror 는 `tasty debug attach`."
        );
    }
    #[cfg(debug_assertions)]
    observation.complete(own_port, port, workspace, source, &outcome);
    // The workspace is only diagnostic here; the caller owns its connector arguments.
    let _ = workspace;
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn every_source_rejects_self_without_entering_the_connector() {
        for source in [
            AttachSource::Ipc,
            AttachSource::User,
            AttachSource::Endpoint,
        ] {
            let calls = Cell::new(0);
            let outcome = dispatch_attach(Some(1234), 1234, 42, source, || {
                calls.set(calls.get() + 1);
                Ok::<_, &str>(7)
            });
            assert_eq!(outcome, Outcome::RejectedSelf, "{source:?}");
            assert_eq!(calls.get(), 0, "{source:?} entered its connector");
        }
    }

    #[test]
    fn every_source_enters_once_and_returns_the_connector_result() {
        for source in [
            AttachSource::Ipc,
            AttachSource::User,
            AttachSource::Endpoint,
        ] {
            for own_port in [None, Some(1234)] {
                for result in [Ok(7), Err("connector failure")] {
                    let calls = Cell::new(0);
                    let outcome = dispatch_attach(own_port, 4321, 42, source, || {
                        calls.set(calls.get() + 1);
                        result
                    });
                    assert_eq!(outcome, Outcome::Connected(result), "{source:?}");
                    assert_eq!(calls.get(), 1, "{source:?} must enter exactly once");
                }
            }
        }
    }
}
