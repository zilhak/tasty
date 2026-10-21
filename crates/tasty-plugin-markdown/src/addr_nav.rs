//! 주소창 이동 신호의 해석과 원격 주소창 이동 결과의 처리.
//! 원격 이동의 최종 결과는 `run.result` 로 읽고, 실패는 주소창 오류 상태로 보인다
//! (docs/surfaces/markdown/index.md 의 attach mirror 문서 절).

use serde_json::{Value, json};
use tasty_plugin_sdk::HostHandle;

use crate::{MarkdownPlugin, MdDoc, RemoteRequester, render};

/// 원격 주소창 이동의 최종 결과를 워커에 돌려주는 자기 호출 메서드. 외부 호출자는 쓸 수 없다.
pub(crate) const ADDR_RESULT_METHOD: &str = "markdown.address_result";

/// 원격 주소창 이동의 `run.result` 를 묻는 간격과 상한. CLI 의 `file-handler dispatch` 대기와 같은 값이다.
const ADDR_RESULT_POLL: std::time::Duration = std::time::Duration::from_millis(200);
const ADDR_RESULT_DEADLINE: std::time::Duration = std::time::Duration::from_secs(30);

/// 원격 주소창 이동의 최종 결과(`run.result`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AddrOutcome {
    Succeeded,
    Failed(render::AddrFailureKind),
}

impl AddrOutcome {
    pub(crate) fn as_wire(self) -> &'static str {
        match self {
            AddrOutcome::Succeeded => "succeeded",
            AddrOutcome::Failed(render::AddrFailureKind::NotFound) => "not_found",
            AddrOutcome::Failed(render::AddrFailureKind::Other) => "failed",
        }
    }

    pub(crate) fn from_wire(s: &str) -> Option<Self> {
        match s {
            "succeeded" => Some(AddrOutcome::Succeeded),
            "not_found" => Some(AddrOutcome::Failed(render::AddrFailureKind::NotFound)),
            "failed" => Some(AddrOutcome::Failed(render::AddrFailureKind::Other)),
            _ => None,
        }
    }
}

/// `run.result` 응답을 분류한다. 끝나지 않았으면 None이다. 원격에 파일이 없다는 실패만 `reason_code`
/// `not_found` 로 구분되고, 나머지 실패와 취소는 모두 Other다(문구가 정해진 것은 not_found 뿐이다).
pub(crate) fn classify_navigate_run(v: &Value) -> Option<AddrOutcome> {
    if v.get("done").and_then(Value::as_bool) != Some(true) {
        return None;
    }
    Some(match v.get("state").and_then(Value::as_str) {
        Some("succeeded") => AddrOutcome::Succeeded,
        Some("failed") if v.get("reason_code").and_then(Value::as_str) == Some("not_found") => {
            AddrOutcome::Failed(render::AddrFailureKind::NotFound)
        }
        _ => AddrOutcome::Failed(render::AddrFailureKind::Other),
    })
}

/// WebView 이동 요청 하나에 plugin 이 할 일.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NavAction {
    Ignore,
    /// mirror 문서의 원문을 다시 요청한다.
    RefetchRemote,
    /// 대용량 대기 상태의 로컬 문서를 다시 연다.
    OpenDeferred,
    /// mirror 문서의 외부 링크.
    External(String),
    /// 로컬 문서의 링크.
    Link(String),
    /// 같은 surface 를 원격 경로로 바꾼다. 실패는 주소창 오류 상태로 보인다.
    NavigateRemote(String),
    /// 같은 surface 를 로컬 경로로 바꾼다.
    NavigateLocal(String),
    /// 주소창 오류 행이 지워졌다.
    DismissAddr,
    /// 주소창 폴더 버튼. 이 폴더(없으면 host 기본 위치)에서 파일 피커를 연다.
    Browse(Option<String>),
}

/// 이동 요청을 문서 종류에 맞는 동작으로 바꾼다. mirror 문서의 파일 링크는 원격 호스트의 경로라
/// 이 머신에서 열지 않는다. 주소창 경로는 원격으로 보내 원격이 존재를 판정한다.
pub(crate) fn nav_action(intent: render::NavIntent, doc: Option<&MdDoc>) -> NavAction {
    let remote = doc.and_then(|d| d.remote.as_ref());
    match (intent, remote) {
        (render::NavIntent::Refresh, Some(_)) => NavAction::RefetchRemote,
        // 로컬 문서에서는 대용량 대기 상태의 "Open file…" 버튼만 이 요청을 낸다.
        (render::NavIntent::Refresh, None) => NavAction::OpenDeferred,
        (render::NavIntent::Link(dest), Some(_)) => match render::classify_link(&dest, None) {
            Some(render::LinkClick::External(url)) => NavAction::External(url),
            _ => NavAction::Ignore,
        },
        (render::NavIntent::Link(dest), None) => NavAction::Link(dest),
        // 끊긴 문서의 주소창은 읽기 전용이다. 늦게 온 신호도 보내지 않는다.
        (render::NavIntent::Addr(_), Some(r)) if r.disconnected => NavAction::Ignore,
        (render::NavIntent::Addr(path), Some(_)) => NavAction::NavigateRemote(path),
        (render::NavIntent::Addr(path), None) => NavAction::NavigateLocal(path),
        (render::NavIntent::AddrDismiss, _) => NavAction::DismissAddr,
        (render::NavIntent::Browse, Some(r)) if r.disconnected => NavAction::Ignore,
        (render::NavIntent::Browse, _) => match doc {
            Some(d) => NavAction::Browse(crate::browse::start_dir(d)),
            None => NavAction::Ignore,
        },
    }
}

/// 원격 주소창 이동의 `run.result` 를 기다려 결과를 워커에 자기 호출로 돌려준다. 상한까지 끝나지 않으면
/// 원격이 아직 처리 중일 수 있으므로 실패로 바꾸지 않고 기록만 남긴다.
pub(crate) fn spawn_addr_watch(
    host: HostHandle,
    sid: u32,
    ticket: u64,
    typed: String,
    run: u64,
    browse: bool,
) {
    let spawned = std::thread::Builder::new()
        .name("markdown-addr-result".to_string())
        .spawn(move || {
            let Some(outcome) = wait_navigate_run(&host, sid, run) else {
                return;
            };
            let params = json!({
                "surface_id": sid,
                "ticket": ticket,
                "path": typed,
                "outcome": outcome.as_wire(),
                "browse": browse,
            });
            if let Err(e) = host.self_invoke(ADDR_RESULT_METHOD, params) {
                tracing::warn!(
                    "markdown surface {sid}: could not hand back the navigate result: {e}"
                );
            }
        });
    if let Err(e) = spawned {
        tracing::warn!("markdown surface {sid}: navigate result watcher spawn failed: {e}");
    }
}

/// `run.result` 를 끝날 때까지 묻는다. 조회가 실패하면 Other 실패, 상한을 넘기면 None이다.
fn wait_navigate_run(host: &HostHandle, sid: u32, run: u64) -> Option<AddrOutcome> {
    let deadline = std::time::Instant::now() + ADDR_RESULT_DEADLINE;
    loop {
        match host.call("run.result", json!({ "run": run })) {
            Ok(v) => {
                if let Some(outcome) = classify_navigate_run(&v) {
                    return Some(outcome);
                }
            }
            Err(e) => {
                tracing::warn!("markdown surface {sid}: run.result {run} failed: {e}");
                return Some(AddrOutcome::Failed(render::AddrFailureKind::Other));
            }
        }
        if std::time::Instant::now() >= deadline {
            tracing::warn!(
                "markdown surface {sid}: remote navigate run {run} did not finish within {ADDR_RESULT_DEADLINE:?}"
            );
            return None;
        }
        std::thread::sleep(ADDR_RESULT_POLL);
    }
}

impl MarkdownPlugin {
    /// [`ADDR_RESULT_METHOD`] 자기 호출. 닫혔거나 바뀐 문서, 더 새 이동이 있는 문서면 버린다.
    pub(crate) fn on_addr_result(&mut self, params: &Value) {
        let parsed = (
            params.get("surface_id").and_then(Value::as_u64),
            params.get("ticket").and_then(Value::as_u64),
            params.get("path").and_then(Value::as_str),
            params
                .get("outcome")
                .and_then(Value::as_str)
                .and_then(AddrOutcome::from_wire),
        );
        let (Some(sid), Some(ticket), Some(path), Some(outcome)) = parsed else {
            tracing::warn!("markdown: malformed {ADDR_RESULT_METHOD} params: {params}");
            return;
        };
        let browse = params.get("browse").and_then(Value::as_bool) == Some(true);
        self.apply_addr_outcome(sid as u32, ticket, path.to_string(), outcome, browse);
    }

    /// 원격 이동 결과를 반영한다. 성공하면 원격이 바꾼 문서의 원문과 경로를 다시 받는다.
    /// 폴더 버튼(`browse`)의 실패는 host 가 사용자 toast 로 알리므로 주소창 오류로 만들지 않는다.
    pub(crate) fn apply_addr_outcome(
        &mut self,
        sid: u32,
        ticket: u64,
        typed: String,
        outcome: AddrOutcome,
        browse: bool,
    ) {
        let Some(doc) = self.docs.get_mut(&sid) else {
            return;
        };
        if doc.remote.as_ref().map(|r| r.addr_ticket) != Some(ticket) {
            return;
        }
        let redraw = match (browse, outcome) {
            (true, AddrOutcome::Failed(_)) => false,
            _ => doc.apply_addr_outcome(ticket, typed, outcome),
        };
        if outcome == AddrOutcome::Succeeded {
            self.request_remote_content(sid, RemoteRequester::User);
        } else if redraw {
            self.reload_webview(sid);
        }
    }
}
