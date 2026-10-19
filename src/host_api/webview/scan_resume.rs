//! 탐색 결정을 보류할 수 없는 backend(Windows)에서 스캔 결과가 JS 값을 정하는 탐색을 처리한다.
//!
//! WebView2의 `NavigationStarting`에는 결정을 미루는 deferral이 없다. 그래서 그 탐색을 취소하고
//! 작업 스레드에서 파일을 읽은 뒤, UI 스레드에서 표를 확인하고 같은 종류의 탐색으로 다시 시작한다.
//! 다시 시작한 탐색은 파일을 다시 읽지 않고 이미 읽은 결과로 JS를 정한다(docs/surfaces/html/index.md).
//!
//! 이 모듈은 상태 전이만 가진다. native 호출(취소·재시작·JS 적용)은 backend가 한다.
//!
//! 다시 시작한 탐색을 확정할 식별자는 없다. `Navigate`·`Reload`·`GoBack`·`GoForward`는
//! `NavigationId`를 돌려주지 않고, WebView2 API로 시작한 탐색은 `IsUserInitiated`가 참이다.
//! 그래서 읽어 둔 결과를 탐색이 아니라 파일 판에 묶는다. 재시작을 기다리는 동안 온
//! `NavigationStarting`은 주소, 다시 시작한 종류, 사용자 시작 여부가 모두 맞고, 그 경로의 파일 판이
//! 스캔한 판과 같을 때만 결과를 받는다([`FileStamp`]). 하나라도 다르면 새 탐색으로 보고 다시 취소해
//! 자기 스캔을 받는다. 식별값이 같아도 내용이 같다는 보장은 없으므로, 재시작과 구별하지 못한 같은
//! 조건의 탐색이 다른 내용에 앞 결과를 받는 경로가 남는다(docs/surfaces/html/index.md의 한계).

use tasty_model::html_script::ScriptScan;

use std::path::PathBuf;

use super::file_stamp::FileStamp;
use super::script_gate::{DeferredScan, ReadSlot, Response, ScriptGate};

/// `NavigationStarting`이 알려 준 탐색 종류.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartKind {
    Reload,
    NewDocument,
    BackOrForward,
    /// 구형 런타임 등으로 종류를 읽지 못했다.
    Unknown,
}

/// 호스트가 요청한 히스토리 이동. 취소한 뒤로/앞으로 탐색을 같은 방향으로 다시 시작할 때 쓴다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryStep {
    Back,
    Forward,
}

/// 취소한 탐색을 다시 시작하는 방법. 원래 종류를 지켜 재로드가 히스토리 항목을 늘리지 않게 한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Replay {
    Reload,
    Navigate,
    Back,
    Forward,
}

/// `NavigationStarting`에서 backend가 할 일.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Start {
    /// 이 JS 값을 적용하고 탐색을 진행한다.
    Js(bool),
    /// 이 모듈이 다시 시작한 탐색이다. JS 값을 적용하고 진행하되, 탐색 시도는 취소한 원래 탐색에서
    /// 이미 기록했으므로 다시 기록하지 않는다(사용자 제스처 여부를 원래 탐색 것으로 남긴다).
    Resumed(bool),
    /// 탐색을 취소한다. 취소가 실패하면 [`ScanResume::cancel_failed`]를 부른다.
    Cancel,
}

enum State {
    Idle,
    /// 취소한 탐색의 파일을 읽는 중.
    Scanning {
        scan: DeferredScan,
        read: ReadSlot,
        url: String,
        replay: Replay,
    },
    /// 표를 확인했고 다시 시작한 탐색의 `NavigationStarting`을 기다린다. 한 번만 쓴다.
    Resuming {
        url: String,
        kind: StartKind,
        scan: Option<ScriptScan>,
        /// 스캔한 파일 판. 읽는 동안 파일이 바뀌었으면 없고, 그러면 결과를 누구에게도 넘기지 않는다.
        stamp: Option<FileStamp>,
        path: PathBuf,
    },
}

/// 한 webview의 취소·재시작 상태.
pub struct ScanResume {
    surface_id: u32,
    state: State,
    /// 이 모듈이 취소한 탐색의 NavigationId. 그 완료 신호는 로드 종료로 보지 않는다.
    cancelled: Vec<u64>,
    /// 호스트가 마지막으로 요청한 히스토리 이동. 다음 `NavigationStarting`에서 한 번 쓴다.
    host_step: Option<HistoryStep>,
    /// 재시작했지만 그 재시작으로 알아본 시작 신호가 아직 없는 횟수. 다시 시작한 탐색의 종류가
    /// 예상과 달라 매번 새 탐색으로 보이면 취소와 재시작이 끝나지 않으므로 상한을 둔다.
    unmatched_replays: u32,
}

/// 알아보지 못한 재시작이 이만큼 이어지면 더 취소하지 않고 스크립트 없이 진행한다.
const MAX_UNMATCHED_REPLAYS: u32 = 2;

impl Replay {
    /// 이 방법으로 다시 시작한 탐색의 `NavigationStarting` 종류.
    fn start_kind(self) -> StartKind {
        match self {
            Replay::Reload => StartKind::Reload,
            Replay::Navigate => StartKind::NewDocument,
            Replay::Back | Replay::Forward => StartKind::BackOrForward,
        }
    }
}

impl ScanResume {
    pub fn new(surface_id: u32) -> Self {
        Self {
            surface_id,
            state: State::Idle,
            cancelled: Vec::new(),
            host_step: None,
            unmatched_replays: 0,
        }
    }

    /// 호스트가 로드·재로드·히스토리 이동을 요청했다. 앞에서 기다리던 재시작은 버린다.
    pub fn host_request(&mut self, step: Option<HistoryStep>) {
        self.clear();
        self.unmatched_replays = 0;
        self.host_step = step;
    }

    /// main frame `NavigationStarting`. 게이트 로드를 시작하고 JS 값이나 취소를 정한다.
    /// `user_initiated`는 `IsUserInitiated`이며 읽지 못했으면 `false`다.
    pub fn start(
        &mut self,
        gate: &ScriptGate,
        url: &str,
        nav_id: Option<u64>,
        kind: StartKind,
        user_initiated: bool,
    ) -> Start {
        let step = self.host_step.take();
        if let Some(scan) = self.take_resume(url, kind, user_initiated) {
            self.unmatched_replays = 0;
            gate.load_started();
            return Start::Resumed(gate.main_response_scanned(url, scan));
        }
        gate.load_started();
        // 새 탐색이 앞에서 기다리던 스캔과 재시작을 대신한다. load_started 뒤라 그 표는 이미 낡았다.
        self.clear();
        let scan = match gate.main_response(url) {
            Response::Decided(js) => {
                self.unmatched_replays = 0;
                return Start::Js(js);
            }
            Response::Deferred(scan) => scan,
        };
        // 종류를 모르면 다시 시작한 탐색을 종류로 알아볼 수 없다(구형 런타임).
        let replay = match kind {
            StartKind::Reload => Some(Replay::Reload),
            StartKind::NewDocument => Some(Replay::Navigate),
            StartKind::Unknown => None,
            StartKind::BackOrForward => step.map(|step| match step {
                HistoryStep::Back => Replay::Back,
                HistoryStep::Forward => Replay::Forward,
            }),
        };
        if self.unmatched_replays >= MAX_UNMATCHED_REPLAYS {
            tracing::warn!(
                "WebView surface {}: {} replays in a row were not recognised; \
                 the document loads without scripts",
                self.surface_id,
                self.unmatched_replays
            );
            self.unmatched_replays = 0;
            scan.abandon();
            return Start::Js(false);
        }
        match (nav_id, replay) {
            (Some(id), Some(replay)) => {
                self.cancelled.push(id);
                self.state = State::Scanning {
                    read: scan.spawn_read(),
                    scan,
                    url: url.to_string(),
                    replay,
                };
                Start::Cancel
            }
            _ => {
                // 취소한 탐색을 나중에 구별하거나 같은 방향으로 다시 시작할 수 없다. UI 스레드에서
                // 읽지 않고 지문 없는 문서로 둔다(허용 불가).
                tracing::warn!(
                    "WebView surface {}: cannot resume this navigation after its scan \
                     (id known: {}, kind: {kind:?}); the document loads without scripts",
                    self.surface_id,
                    nav_id.is_some()
                );
                scan.abandon();
                Start::Js(false)
            }
        }
    }

    /// [`Start::Cancel`]을 받았지만 취소하지 못했다. 탐색은 스크립트 없이 진행된다.
    pub fn cancel_failed(&mut self, nav_id: Option<u64>) {
        if let Some(id) = nav_id {
            self.cancelled.retain(|c| *c != id);
        }
        self.clear();
    }

    /// 탐색 완료 신호. 이 모듈이 취소한 탐색이면 `true`이고, backend는 그 신호를 로드 종료로 보지 않는다.
    pub fn completed(&mut self, nav_id: Option<u64>) -> bool {
        let Some(id) = nav_id else {
            return false;
        };
        let Some(at) = self.cancelled.iter().position(|c| *c == id) else {
            return false;
        };
        self.cancelled.swap_remove(at);
        true
    }

    /// 읽기가 끝났으면 표를 확인하고 다시 시작할 탐색을 준다. UI 스레드에서 부른다.
    /// 읽는 중이거나 표가 낡았으면 `None`이다.
    pub fn poll(&mut self) -> Option<(Replay, String)> {
        let read = match &self.state {
            State::Scanning { read, .. } => read.take_stamped()?,
            _ => return None,
        };
        let State::Scanning {
            scan, url, replay, ..
        } = std::mem::replace(&mut self.state, State::Idle)
        else {
            unreachable!("matched above");
        };
        let path = scan.path().to_path_buf();
        // 표 확인과 상태 반영은 다시 시작하기 직전 이 UI 스레드 호출 안에서 한다.
        scan.apply(read.scan)?;
        self.unmatched_replays += 1;
        self.state = State::Resuming {
            url: url.clone(),
            kind: replay.start_kind(),
            scan: read.scan,
            stamp: read.stamp,
            path,
        };
        Some((replay, url))
    }

    /// 다시 시작하는 호출이 실패했다. 기다리던 재시작을 버린다.
    pub fn replay_failed(&mut self) {
        self.clear();
    }

    /// 다시 시작한 탐색이면 읽어 둔 결과를 꺼낸다. 주소·종류·사용자 시작 여부 중 하나라도
    /// 다르거나, 경로의 파일 판이 스캔한 판과 다르면 꺼내지 않는다(모듈 설명 참고).
    /// 파일 판 확인은 메타데이터 조회 한 번이며 내용을 읽지 않는다.
    fn take_resume(
        &mut self,
        url: &str,
        kind: StartKind,
        user_initiated: bool,
    ) -> Option<Option<ScriptScan>> {
        match &self.state {
            State::Resuming {
                url: waiting,
                kind: expected,
                stamp: Some(stamp),
                path,
                ..
            } if waiting == url
                && *expected == kind
                && user_initiated
                && FileStamp::of_path(path).as_ref() == Some(stamp) => {}
            _ => return None,
        }
        match std::mem::replace(&mut self.state, State::Idle) {
            State::Resuming { scan, .. } => Some(scan),
            _ => unreachable!("matched above"),
        }
    }

    /// 스캔이나 재시작을 기다리는 중인지.
    #[cfg(test)]
    fn waiting(&self) -> bool {
        !matches!(self.state, State::Idle)
    }

    /// 기다리던 스캔과 재시작을 버린다.
    pub fn clear(&mut self) {
        if let State::Scanning { scan, .. } = std::mem::replace(&mut self.state, State::Idle) {
            scan.abandon();
        }
    }
}

#[cfg(test)]
#[path = "scan_resume/tests.rs"]
mod tests;
