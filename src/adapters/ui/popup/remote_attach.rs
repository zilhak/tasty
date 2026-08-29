//! 원격 워크스페이스 추가 팝업 (사이드바 우클릭 > 원격 워크스페이스 추가).
//!
//! 좌 240px attach 프로필 리스트(single select) → 우 flex pane. 프로필을 고르면
//! 워커 스레드가 RA01 공유 코어(`tasty_remote::browse`)로 원격 tasty 에 붙어
//! `workspace.list`+`attach.list` 를 병합 조회하고, 우측을 4상태(initial / connecting /
//! error / loaded[+empty])로 표시한다. 원격 ws 를 골라 **Connect** 하면 조회에 쓴 터널을
//! 재사용해 로컬 mirror workspace 로 attach 한다.
//!
//! 구조 전사: 디자인 `RemoteAttach`(680×460 headless 2-pane) 1:1. remote_tool 과 같은
//! shell 언어(headless 헤더 · bg-panel 프레임 · ghost/primary footer). 갤러리 specimen
//! (`tasty-gallery` `remote_attach`)의 정적 미러를 실 IPC 위에 얹은 것.
//!
//! 원칙 1: 이 팝업의 조회/attach 는 **사용자 입력 경로**다. Connect 확정 시 focus 가 새
//! mirror ws 로 이동하는데(사용자 동작), 그 focus 이동은 `pending_gui_attach_user` 큐를
//! 통해 이 경로에서만 일어난다(release IPC/에이전트 경로엔 없음). self(loopback) attach 는
//! release 에서 `dispatch_pending_gui_attach` 게이트가 차단한다(원칙 1②).

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tasty_remote::browse::{self as remote_browse, RemoteWorkspace};
use tasty_remote_profiles::RemoteProfiles;
use tasty_ssh::{SshCancel, SshTunnel};
use tasty_ui_widgets::{Button, ButtonVariant, StatusKind, status_dot};

use crate::adapters::ui::icons;
use crate::adapters::ui::popup::PopupAction;
use crate::core::CoreState;
use crate::i18n::t;
use crate::state::AppState;
use crate::theme;
use crate::theme::Theme;

pub const REMOTE_ATTACH_POPUP_ID: &str = "remote_attach";

const UI_MEMORY_ID: &str = "remote_attach.ui";

/// attach kind(같은 레지스트리의 예약 kind, ADR-0032). remote_tool Attach 탭이 편집하고
/// 이 팝업은 **소비만** 한다.
const ATTACH_KIND: &str = "tasty-attach";

// ── 레이아웃 고정 치수 (디자인 raw px — 화면 전용) ──
const LEFT_W: f32 = 240.0;
const HEADER_H: f32 = 47.0;
const FOOTER_H: f32 = 49.0;
const CAPS_H: f32 = 30.0;
const PROFILE_ROW_H: f32 = 50.0;
const WS_ROW_H: f32 = 34.0;
const BADGE_H: f32 = 16.0;
const HEADER_PAD_L: f32 = 14.0;
const SELECT_BAR_W: f32 = 2.0;

/// 워커가 결과를 채우지 않아도 UI 가 Connecting 을 벗어나는 상한(soft timeout).
///
/// 워커 자체에도 상한이 있다(포트 발견 전체 `PORT_DISCOVERY_TOTAL_TIMEOUT` 45초 +
/// 터널 ready 5초 + IPC 프로브 5초, ADR-0070) — 하지만 그건 최악 ~55초라, 그동안 UI 가
/// 워커의 완료만 기다리면 사용자는 팝업을 닫는 것 외에 할 수 있는 게 없다. 그래서 **UI 가
/// 먼저 포기**하고 워커를 취소한다(취소 = 자식 ssh kill, `SshCancel`). 두 상한의 관계는
/// 의도적으로 **UI 가 먼저**다 — 워커 상한이 먼저 만료되면 UI 는 워커가 만든 정상 에러
/// 문구를 그대로 받고, UI 가 먼저면 워커를 끊고 이 파일의 타임아웃 문구를 쓴다. 어느
/// 쪽이든 UI 는 이 시간 안에 조작 가능한 상태로 돌아온다.
/// 원격 file picker 의 soft timeout(ADR-0053, 8초)과 같은 매 프레임 판정 방식이되,
/// SSH 연결 수립이 포함되므로 값은 더 길게 잡는다.
const BROWSE_DEADLINE: Duration = Duration::from_secs(20);

/// 워커가 채우는 browse 결과. 성공 시 터널을 살려 Connect 로 넘긴다.
struct BrowseOk {
    port: u16,
    tunnel: Option<SshTunnel>,
    workspaces: Vec<RemoteWorkspace>,
}

/// Connect 시 재사용할 접속 엔드포인트(성공 후 보관). 터널이 살아 있어야 mirror 세션이
/// 유지되므로 Arc<Mutex> 로 담아 UiState(Clone)에 싣는다.
struct ReadyConn {
    port: u16,
    tunnel: Option<SshTunnel>,
}

/// 진행 중 조회 워커. `slot` 은 완료 시 결과가 들어오는 폴링 슬롯.
#[derive(Clone)]
struct BrowseJob {
    slot: Arc<Mutex<Option<Result<BrowseOk, String>>>>,
    /// 시작 시각 — UI 가 경과를 재 [`BROWSE_DEADLINE`] 을 판정한다.
    started_at: Instant,
    /// 워커의 포트 발견 자식 ssh 를 kill 하는 핸들(취소 프로토콜의 유일한 신호).
    cancel: SshCancel,
}

/// 조회를 중단한다 — 자식 ssh 를 kill 하고 결과 슬롯을 포기한다.
///
/// 슬롯을 포기해도 워커는 자기 몫의 `Arc` 를 쥐고 있어 결과를 쓸 수는 있지만, 그 결과는
/// 아무도 읽지 않고 워커 종료와 함께 drop 된다 — 그때 `BrowseOk.tunnel` 의 `SshTunnel`
/// 도 함께 drop 되어 kill 되므로 터널이 새지 않는다(반대 방향 누수 방지).
fn cancel_job(job: Option<BrowseJob>) {
    if let Some(job) = job {
        job.cancel.cancel();
    }
}

/// 우측 pane 상태 머신.
#[derive(Clone, Default)]
enum Conn {
    #[default]
    Initial,
    Connecting,
    Error(String),
    Loaded(Vec<RemoteWorkspace>),
}

#[derive(Clone, Default)]
struct UiState {
    /// 선택된 attach 프로필명.
    attach_sel: Option<String>,
    conn: Conn,
    /// 선택된 원격 ws id.
    ws_sel: Option<u32>,
    /// 진행 중 조회 워커.
    job: Option<BrowseJob>,
    /// browse 성공 후 보관한 엔드포인트(Connect 재사용). Cancel/재선택 시 drop → ssh kill.
    ready: Option<Arc<Mutex<Option<ReadyConn>>>>,
}

fn read_ui(ctx: &egui::Context) -> UiState {
    ctx.memory(|m| {
        m.data
            .get_temp::<UiState>(egui::Id::new(UI_MEMORY_ID))
            .unwrap_or_default()
    })
}
fn write_ui(ctx: &egui::Context, ui: UiState) {
    ctx.memory_mut(|m| m.data.insert_temp(egui::Id::new(UI_MEMORY_ID), ui));
}
fn clear_ui(ctx: &egui::Context) {
    ctx.memory_mut(|m| m.data.remove::<UiState>(egui::Id::new(UI_MEMORY_ID)));
}

/// attach 프로필 1개의 표시용 요약(디자인 좌측 row) — remote_tool `draw_attach_row` 의
/// target/inactive 도출 로직 재사용.
struct ProfileSummary {
    name: String,
    label: String,
    target: String,
    inactive: bool,
}

fn attach_summaries(profiles: &RemoteProfiles) -> Vec<ProfileSummary> {
    profiles
        .profiles
        .iter()
        .filter(|p| p.kind == ATTACH_KIND)
        .filter_map(|p| {
            let v = p.as_attach()?;
            let (missing, inactive) = match v.ssh_ref() {
                Some(r) => {
                    let referenced = profiles.get(r).filter(|rp| rp.kind == "ssh");
                    let disabled = referenced
                        .and_then(|rp| rp.as_ssh())
                        .map(|s| s.is_disabled())
                        .unwrap_or(false);
                    (referenced.is_none(), disabled)
                }
                None => (false, v.detect_failed()),
            };
            let target = match v.ssh_ref() {
                Some(r) => format!("→ {}", if r.is_empty() { "?" } else { r }),
                None => {
                    let mut s = v.ssh_destination();
                    if s.is_empty() {
                        s = "?".into();
                    }
                    if let Some(port) = v.port()
                        && port != 22
                    {
                        s = format!("{s}:{port}");
                    }
                    s
                }
            };
            Some(ProfileSummary {
                name: p.name.clone(),
                label: p.label.clone().unwrap_or_default(),
                target,
                // dangling ref(missing) 도 연결하면 실패하므로 inactive 로 표시.
                inactive: inactive || missing,
            })
        })
        .collect()
}

/// 프로필명 → 원격 ws 목록/에러(+ 재사용 터널)를 워커 스레드로 조회한다. UI 스레드에서
/// SSH/IPC 를 직접 블록하지 않는다(원칙 2 headless — 코어는 CLI/IPC 와 공유).
fn spawn_browse(ctx: &egui::Context, profile: String) -> BrowseJob {
    let slot: Arc<Mutex<Option<Result<BrowseOk, String>>>> = Arc::new(Mutex::new(None));
    let slot_w = Arc::clone(&slot);
    let ctx_w = ctx.clone();
    let cancel = SshCancel::new();
    let cancel_w = cancel.clone();
    let name = profile;
    std::thread::spawn(move || {
        // 이 스레드의 포트 발견 자식 ssh 를 취소 대상으로 등록한다 — 이게 없으면
        // `Command` 안에 갇힌 자식을 밖에서 kill 할 방법이 없다.
        let _scope = cancel_w.scope();
        let res = (|| -> Result<BrowseOk, String> {
            let (target, remote_tasty, port_mode, port_file) =
                remote_browse::resolve_connection_spec(Some(&name), None, "", "")
                    .map_err(|e| e.to_string())?;
            let (tunnel, port) = remote_browse::resolve_endpoint(
                &target,
                &remote_tasty,
                &port_mode,
                port_file.as_deref(),
            )
            .map_err(|e| e.to_string())?;
            // 터널 수립 뒤에 취소가 들어왔으면 여기서 접는다 — `tunnel` 이 이 지점에서
            // drop 되어 자식 ssh 가 즉시 kill 된다(다음 단계까지 끌고 가지 않는다).
            if cancel_w.is_cancelled() {
                return Err(t("remote_attach.error_generic").to_string());
            }
            let workspaces = remote_browse::browse_via_port(port).map_err(|e| e.to_string())?;
            Ok(BrowseOk {
                port,
                tunnel,
                workspaces,
            })
        })();
        if let Ok(mut g) = slot_w.lock() {
            *g = Some(res);
        }
        ctx_w.request_repaint();
    });
    BrowseJob {
        slot,
        started_at: Instant::now(),
        cancel,
    }
}

/// 폴링 판정(순수 함수 — 시간 축 주입 가능).
///
/// 워커의 정상 완료(`slot_filled`)에만 의존하지 않는다는 점이 핵심이다. 워커가 늦거나,
/// 패닉해서 슬롯을 영영 못 채우더라도 경과 시간만으로 Connecting 을 벗어난다.
#[derive(Debug, PartialEq, Eq)]
enum PollDecision {
    /// 아직 대기(상한 이내 + 결과 없음).
    Wait,
    /// 결과 도착 — 슬롯을 회수해 전이한다.
    Take,
    /// 상한 초과 — 조회를 취소하고 에러로 전이한다.
    TimedOut,
}

fn poll_decision(slot_filled: bool, elapsed: Duration, deadline: Duration) -> PollDecision {
    if slot_filled {
        PollDecision::Take
    } else if elapsed >= deadline {
        PollDecision::TimedOut
    } else {
        PollDecision::Wait
    }
}

/// 워커 완료/상한 폴링 — 완료 시 job → conn(+ready) 전이. 재렌더당 1회.
///
/// Connecting 상태에서는 Spinner 가 매 프레임 repaint 를 요청하므로 경과 시간 판정이
/// 매 프레임 돈다.
fn poll_browse(st: &mut UiState, deadline: Duration) {
    let Some(job) = st.job.as_ref() else { return };
    // 뮤텍스가 poisoned 면 결과를 못 읽으므로 채워진 것으로 보고 아래에서 에러 전이.
    let filled = job.slot.lock().map(|g| g.is_some()).unwrap_or(true);
    match poll_decision(filled, job.started_at.elapsed(), deadline) {
        PollDecision::Wait => return,
        PollDecision::TimedOut => {
            cancel_job(st.job.take());
            st.conn = Conn::Error(
                t("remote_attach.timeout").replace("{secs}", &deadline.as_secs().to_string()),
            );
            st.ready = None;
            return;
        }
        PollDecision::Take => {}
    }
    let Some(job) = st.job.take() else { return };
    let outcome = job.slot.lock().ok().and_then(|mut g| g.take());
    match outcome {
        Some(Ok(ok)) => {
            st.conn = Conn::Loaded(ok.workspaces.clone());
            st.ws_sel = None;
            st.ready = Some(Arc::new(Mutex::new(Some(ReadyConn {
                port: ok.port,
                tunnel: ok.tunnel,
            }))));
        }
        Some(Err(e)) => {
            st.conn = Conn::Error(e);
            st.ready = None;
        }
        None => {
            // 슬롯이 비었는데 done — 이론상 도달 안 함. 안전하게 에러로.
            st.conn = Conn::Error(t("remote_attach.error_generic").to_string());
        }
    }
}

/// 프로필 선택 → 조회 시작(상태 리셋 + 워커 spawn).
fn connect(ctx: &egui::Context, st: &mut UiState, name: String) {
    cancel_job(st.job.take()); // 재선택 = 이전 조회 중단(자식 ssh 회수).
    st.attach_sel = Some(name.clone());
    st.ws_sel = None;
    st.ready = None; // 이전 터널 정리(재선택 = 새 연결).
    st.conn = Conn::Connecting;
    st.job = Some(spawn_browse(ctx, name));
}

/// 조회 중단 — 팝업은 열어둔 채 Initial 로 되돌린다(Connecting 탈출 수단).
fn cancel_browse(st: &mut UiState) {
    cancel_job(st.job.take());
    st.conn = Conn::Initial;
    st.attach_sel = None;
    st.ws_sel = None;
    st.ready = None;
}

/// 팝업 정리 — 진행 중 조회 워커의 자식 ssh 를 kill 하고 `UiState` 를 drop 한다.
///
/// `clear_ui` 만으로는 부족하다: `UiState` drop 은 슬롯 `Arc` 와 `SshTunnel` 핸들만
/// 회수하고, 포트 발견 단계의 자식 ssh 는 워커가 `Command` 안에 쥐고 있어 취소 신호를
/// 보내야만 죽는다. 여러 번 불려도 안전하다(취소는 멱등, 정리된 뒤엔 job 이 없다).
fn cleanup(ctx: &egui::Context) {
    cancel_job(read_ui(ctx).job);
    clear_ui(ctx);
}

/// PopupDef::on_close 진입점 — 어떤 경로로 닫히든 진행 중 조회 워커(자식 ssh 포함)와
/// 재사용 터널을 정리한다. draw_fn 자신의 Escape/Cancel/Connect 경로도 같은
/// [`cleanup`] 을 부르지만, headless(X 버튼) + `UiIntent::ClosePopup`(디버그 IPC 포함)
/// 처럼 draw_fn 을 거치지 않는 닫힘 경로가 있으므로 이 훅이 단일 choke point 다
/// (ADR-0063).
pub fn on_close_remote_attach_popup(
    ctx: &egui::Context,
    _state: &mut AppState,
    _engine: &mut CoreState,
) {
    cleanup(ctx);
}

/// PopupDef.draw_fn 진입점.
pub fn draw_remote_attach_popup(
    ui: &mut egui::Ui,
    _state: &mut AppState,
    engine: &mut CoreState,
) -> PopupAction {
    let th = theme::theme();
    let ctx = ui.ctx().clone();
    let mut st = read_ui(&ctx);

    // 리스트 항목은 텍스트가 아니라 목록으로 다뤄야 한다 — 라벨을 비선택으로 만들어
    // 글자 위 I-beam/드래그 선택을 막는다(egui 기본 selectable_labels=true). 이 팝업의
    // 모든 child ui 는 이 root ui 에서 파생되어 스타일을 상속한다.
    ui.style_mut().interaction.selectable_labels = false;

    poll_browse(&mut st, BROWSE_DEADLINE);

    // Escape: 닫기(진행 중 조회 워커의 자식 ssh + 터널은 cleanup 이 회수).
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        cleanup(&ctx);
        return PopupAction::Close;
    }

    let profiles = RemoteProfiles::load();
    let summaries = attach_summaries(&profiles);

    let full = ui.max_rect();
    ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);

    let mut close = false;
    let mut do_connect = false;

    // ── 헤더 ──
    let header_rect = egui::Rect::from_min_size(full.min, egui::vec2(full.width(), HEADER_H));
    if draw_header(ui, &th, header_rect) {
        close = true;
    }

    // ── footer ──
    let footer_rect = egui::Rect::from_min_size(
        egui::pos2(full.left(), full.bottom() - FOOTER_H),
        egui::vec2(full.width(), FOOTER_H),
    );

    // ── body(2-pane) ──
    let body_rect = egui::Rect::from_min_max(
        egui::pos2(full.left(), header_rect.bottom()),
        egui::pos2(full.right(), footer_rect.top()),
    );
    let left_rect =
        egui::Rect::from_min_size(body_rect.min, egui::vec2(LEFT_W, body_rect.height()));
    let right_rect = egui::Rect::from_min_max(
        egui::pos2(body_rect.left() + LEFT_W, body_rect.top()),
        body_rect.max,
    );
    // 좌 pane 배경(bg-sidebar) + borderRight separator.
    ui.painter().rect_filled(left_rect, 0.0, th.bg_sidebar());
    ui.painter().vline(
        left_rect.right(),
        left_rect.y_range(),
        egui::Stroke::new(th.border_width.value(), th.separator.to_egui()),
    );

    // 좌: attach 프로필 리스트.
    if let Some(name) = draw_left_pane(ui, &th, left_rect, &summaries, st.attach_sel.as_deref()) {
        connect(&ctx, &mut st, name);
    }
    // 우: 4상태. error 상태의 Retry 클릭 시 선택 프로필로 재조회.
    let retry = draw_right_pane(ui, &th, right_rect, &mut st);
    if retry && let Some(name) = st.attach_sel.clone() {
        connect(&ctx, &mut st, name);
    }

    // footer(Connect 활성 조건 = loaded && 선택 ws 존재).
    let can_connect = matches!(&st.conn, Conn::Loaded(ws) if !ws.is_empty()) && st.ws_sel.is_some();
    let connecting = matches!(&st.conn, Conn::Connecting);
    match draw_footer(ui, &th, footer_rect, can_connect, connecting) {
        // 조회 중에는 같은 ghost 버튼이 "중단" 이다 — 팝업을 닫지 않고 조회만 끊어
        // Initial 로 돌아간다(닫기는 헤더 X / Escape).
        FooterAction::Cancel if connecting => cancel_browse(&mut st),
        FooterAction::Cancel => close = true,
        FooterAction::Connect => do_connect = true,
        FooterAction::None => {}
    }

    // Connect 실행 — 재사용 터널 + 선택 ws 를 사용자-경로 큐에 넣는다(메인 루프 drain).
    if do_connect
        && can_connect
        && let Some(ws) = st.ws_sel
        && let Some(ready_arc) = st.ready.take()
    {
        let ready = ready_arc.lock().ok().and_then(|mut g| g.take());
        if let Some(ReadyConn { port, tunnel }) = ready {
            // focus 이동은 큐 drain(dispatch_pending_gui_attach) 이 담당(원칙 1 —
            // 사용자 입력 경로에서만 새 mirror ws 로 focus 이동).
            engine
                .pending_gui_attach_user
                .push(crate::core::GuiAttachUserReq {
                    port,
                    workspace: ws,
                    tunnel,
                });
        }
        close = true;
    }

    if close {
        cleanup(&ctx);
        PopupAction::Close
    } else {
        write_ui(&ctx, st);
        PopupAction::None
    }
}

// ════════════════════════════════════════════════════════════════════════
// 헤더
// ════════════════════════════════════════════════════════════════════════
fn draw_header(ui: &mut egui::Ui, th: &Theme, rect: egui::Rect) -> bool {
    ui.painter().hline(
        rect.x_range(),
        rect.bottom(),
        egui::Stroke::new(th.border_width.value(), th.separator.to_egui()),
    );
    let inner = egui::Rect::from_min_max(
        egui::pos2(rect.left() + HEADER_PAD_L, rect.top()),
        egui::pos2(rect.right() - th.spacing_sm.value(), rect.bottom()),
    );
    let mut close = false;
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    child.spacing_mut().item_spacing.x = th.spacing_sm.value();
    child.add(icons::TERMINAL_PROMPT.image(16.0, th.text_muted().into()));
    child.label(
        egui::RichText::new(t("remote_attach.heading"))
            .color(th.text_primary())
            .size(th.font_size_heading.value())
            .strong(),
    );
    child.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if ui
            .add(
                egui::ImageButton::new(icons::CLOSE.image(16.0, th.text_muted().into()))
                    .frame(false),
            )
            .on_hover_text(t("remote_attach.close"))
            .clicked()
        {
            close = true;
        }
    });
    close
}

// ════════════════════════════════════════════════════════════════════════
// 좌: attach 프로필 리스트
// ════════════════════════════════════════════════════════════════════════
fn draw_left_pane(
    ui: &mut egui::Ui,
    th: &Theme,
    rect: egui::Rect,
    profiles: &[ProfileSummary],
    selected: Option<&str>,
) -> Option<String> {
    let mut clicked: Option<String> = None;
    let mut col = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    col.set_clip_rect(rect);
    col.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
    // caps 헤더.
    caps_header(&mut col, th, t("remote_attach.attach_profiles"), None);
    let list_rect =
        egui::Rect::from_min_max(egui::pos2(rect.left(), rect.top() + CAPS_H), rect.max);
    let mut list = col.new_child(
        egui::UiBuilder::new()
            .max_rect(list_rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    list.set_clip_rect(list_rect);
    egui::ScrollArea::vertical()
        .id_salt("remote_attach.profiles")
        .show(&mut list, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
            if profiles.is_empty() {
                ui.add_space(th.spacing_md.value());
                let mut inner = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(egui::Rect::from_min_size(
                            egui::pos2(
                                ui.max_rect().left() + th.spacing_md.value(),
                                ui.cursor().top(),
                            ),
                            egui::vec2(LEFT_W - th.spacing_md.value() * 2.0, 40.0),
                        ))
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                inner.label(
                    egui::RichText::new(t("remote_attach.no_profiles"))
                        .color(th.text_muted())
                        .italics()
                        .size(th.font_size_caption.value()),
                );
                return;
            }
            for p in profiles {
                if profile_row(ui, th, p, Some(p.name.as_str()) == selected) {
                    clicked = Some(p.name.clone());
                }
            }
        });
    clicked
}

fn profile_row(ui: &mut egui::Ui, th: &Theme, p: &ProfileSummary, selected: bool) -> bool {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, PROFILE_ROW_H), egui::Sense::click());
    if selected {
        ui.painter().rect_filled(rect, 0.0, th.surface_active());
        let bar = egui::Rect::from_min_size(rect.min, egui::vec2(SELECT_BAR_W, rect.height()));
        ui.painter().rect_filled(bar, 0.0, th.accent_primary());
    } else if resp.hovered() {
        ui.painter()
            .rect_filled(rect, 0.0, th.hover_overlay.to_egui_premultiplied());
    }
    let inner = egui::Rect::from_min_max(
        egui::pos2(
            rect.left() + th.spacing_md.value(),
            rect.top() + th.spacing_sm.value(),
        ),
        egui::pos2(
            rect.right() - th.spacing_md.value(),
            rect.bottom() - th.spacing_sm.value(),
        ),
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    // 클립은 가로만(truncate 오버플로 가드) — 세로는 행 전체 높이로 둔다. inner 는
    // 상하 spacing_sm 를 깎아 두 줄(name+target)을 담기엔 낮아, inner 로 세로까지
    // 클립하면 둘째 줄(target) 하단 descender 가 잘린다.
    child.set_clip_rect(egui::Rect::from_min_max(
        egui::pos2(inner.left(), rect.top()),
        egui::pos2(inner.right(), rect.bottom()),
    ));
    child.spacing_mut().item_spacing.y = 2.0;
    child.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = th.spacing_sm.value();
        let name_c = if selected {
            th.text_primary()
        } else {
            th.text_secondary()
        };
        ui.add(
            egui::Label::new(
                egui::RichText::new(&p.name)
                    .size(th.font_size_body.value())
                    .strong()
                    .color(name_c),
            )
            .truncate(),
        );
        if !p.label.is_empty() {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(format!("({})", p.label))
                        .size(th.font_size_body.value())
                        .color(th.text_muted()),
                )
                .truncate(),
            );
        }
        if p.inactive {
            badge(
                ui,
                th,
                t("remote_attach.inactive"),
                th.accent_warning().into(),
                0.12,
                0.40,
                true,
            );
        }
    });
    child.label(
        egui::RichText::new(&p.target)
            .monospace()
            .size(th.font_size_caption.value())
            .color(th.text_muted()),
    );
    resp.clicked()
}

// ════════════════════════════════════════════════════════════════════════
// 우: 4상태
// ════════════════════════════════════════════════════════════════════════
/// 우측 pane 렌더. 반환값 = error 상태의 **Retry 버튼 클릭** 여부(caller 가 재조회).
fn draw_right_pane(ui: &mut egui::Ui, th: &Theme, rect: egui::Rect, st: &mut UiState) -> bool {
    let sel_name = st.attach_sel.clone().unwrap_or_default();
    match &st.conn {
        Conn::Initial => {
            center_state(
                ui,
                th,
                rect,
                CenterKind::Glyph(icons::TERMINAL_PROMPT, th.text_placeholder().into()),
                t("remote_attach.select_profile"),
                th.text_muted(),
                t("remote_attach.select_profile_hint"),
                false,
            );
            false
        }
        Conn::Connecting => {
            center_state(
                ui,
                th,
                rect,
                CenterKind::Spinner,
                t("remote_attach.connecting"),
                th.text_secondary(),
                &t("remote_attach.connecting_hint")
                    .replace("{name}", &sel_name)
                    .replace("{secs}", &BROWSE_DEADLINE.as_secs().to_string()),
                false,
            );
            false
        }
        Conn::Error(msg) => {
            let msg = msg.clone();
            center_state(
                ui,
                th,
                rect,
                CenterKind::Glyph(icons::ALERT_TRIANGLE, th.accent_danger().into()),
                t("remote_attach.cant_connect"),
                th.text_primary(),
                &msg,
                true,
            )
        }
        Conn::Loaded(ws) => {
            if ws.is_empty() {
                center_state(
                    ui,
                    th,
                    rect,
                    CenterKind::Glyph(icons::FOLDER, th.text_placeholder().into()),
                    t("remote_attach.no_workspaces"),
                    th.text_muted(),
                    &t("remote_attach.no_workspaces_hint").replace("{name}", &sel_name),
                    false,
                );
            } else {
                let ws = ws.clone();
                if let Some(sel) = draw_ws_list(ui, th, rect, &ws, &sel_name, st.ws_sel) {
                    st.ws_sel = Some(sel);
                }
            }
            false
        }
    }
}

fn draw_ws_list(
    ui: &mut egui::Ui,
    th: &Theme,
    rect: egui::Rect,
    ws: &[RemoteWorkspace],
    profile_name: &str,
    ws_sel: Option<u32>,
) -> Option<u32> {
    let mut clicked = None;
    let mut col = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    col.set_clip_rect(rect);
    col.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
    caps_header(
        &mut col,
        th,
        t("remote_attach.remote_workspaces"),
        Some(profile_name),
    );
    let list_rect =
        egui::Rect::from_min_max(egui::pos2(rect.left(), rect.top() + CAPS_H), rect.max);
    let mut list = col.new_child(
        egui::UiBuilder::new()
            .max_rect(list_rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    list.set_clip_rect(list_rect);
    egui::ScrollArea::vertical()
        .id_salt("remote_attach.workspaces")
        .show(&mut list, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
            for w in ws {
                if ws_row(ui, th, w, ws_sel == Some(w.id)) {
                    clicked = Some(w.id);
                }
            }
        });
    clicked
}

fn ws_row(ui: &mut egui::Ui, th: &Theme, w: &RemoteWorkspace, selected: bool) -> bool {
    let width = ui.available_width();
    let disabled = w.attached;
    let sense = if disabled {
        egui::Sense::hover()
    } else {
        egui::Sense::click()
    };
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, WS_ROW_H), sense);
    if selected {
        ui.painter().rect_filled(rect, 0.0, th.surface_active());
        let bar = egui::Rect::from_min_size(rect.min, egui::vec2(SELECT_BAR_W, rect.height()));
        ui.painter().rect_filled(bar, 0.0, th.accent_primary());
    } else if !disabled && resp.hovered() {
        ui.painter()
            .rect_filled(rect, 0.0, th.hover_overlay.to_egui_premultiplied());
    }
    let inner = egui::Rect::from_min_max(
        egui::pos2(rect.left() + th.spacing_md.value(), rect.top()),
        egui::pos2(rect.right() - th.spacing_md.value(), rect.bottom()),
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    child.set_clip_rect(inner);
    child.spacing_mut().item_spacing.x = th.spacing_sm.value();
    let kind = if w.busy_count > 0 {
        StatusKind::Running
    } else {
        StatusKind::Idle
    };
    status_dot(&mut child, th, kind, "", w.busy_count > 0, false);
    let name_c = if disabled {
        th.text_disabled()
    } else if selected {
        th.text_primary()
    } else {
        th.text_secondary()
    };
    child.add(
        egui::Label::new(
            egui::RichText::new(&w.name)
                .size(th.font_size_body.value())
                .color(name_c),
        )
        .truncate(),
    );
    child.add(icons::SPLIT.image(th.font_size_caption.value(), th.text_muted().into()));
    child.label(
        egui::RichText::new(w.pane_count.to_string())
            .size(th.font_size_caption.value())
            .color(th.text_muted()),
    );
    child.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if disabled {
            badge(
                ui,
                th,
                t("remote_attach.in_use"),
                th.border_attached().into(),
                0.14,
                0.45,
                false,
            );
        } else if w.busy_count > 0 {
            ui.label(
                egui::RichText::new(t("remote_attach.busy"))
                    .size(th.font_size_caption.value())
                    .color(th.text_muted()),
            );
        }
    });
    resp.clicked()
}

// ════════════════════════════════════════════════════════════════════════
// center-state (initial / connecting / error / empty 공용)
// ════════════════════════════════════════════════════════════════════════
enum CenterKind {
    Glyph(icons::Icon, egui::Color32),
    Spinner,
}

#[allow(clippy::too_many_arguments)]
fn center_state(
    ui: &mut egui::Ui,
    th: &Theme,
    rect: egui::Rect,
    kind: CenterKind,
    heading: &str,
    heading_color: tasty_type_appearance::color::HexColor,
    caption: &str,
    retry: bool,
) -> bool {
    let mut col = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(egui::vec2(th.spacing_lg.value(), th.spacing_xl.value())))
            .layout(egui::Layout::top_down(egui::Align::Center)),
    );
    col.set_clip_rect(rect);
    col.add_space((rect.height() - 130.0).max(0.0) * 0.5);
    col.spacing_mut().item_spacing.y = th.spacing_sm.value();
    match kind {
        CenterKind::Glyph(g, c) => {
            col.add(g.image(22.0, c));
        }
        CenterKind::Spinner => {
            tasty_ui_widgets::Spinner::new()
                .size(22.0)
                .show(&mut col, th);
        }
    }
    col.label(
        egui::RichText::new(heading)
            .size(th.font_size_body.value())
            .strong()
            .color(heading_color),
    );
    col.label(
        egui::RichText::new(caption)
            .size(th.font_size_caption.value())
            .color(th.text_muted()),
    );
    if retry {
        col.add_space(th.spacing_xs.value());
        // Retry — 선택된 프로필로 재조회한다(caller 가 bool 을 받아 connect 재실행).
        return Button::new(t("remote_attach.retry"))
            .variant(ButtonVariant::Secondary)
            .leading_icon(&|ui, rect, c| icons::REFRESH.image(rect.height(), c).paint_at(ui, rect))
            .show(&mut col, th)
            .clicked();
    }
    false
}

// ════════════════════════════════════════════════════════════════════════
// footer
// ════════════════════════════════════════════════════════════════════════
enum FooterAction {
    None,
    Cancel,
    Connect,
}

/// `connecting` 이면 ghost 버튼이 "닫기용 취소" 가 아니라 **조회 중단**이 된다
/// (디자인 원본의 요소를 그대로 쓰되 문구만 상태에 맞춘다).
fn draw_footer(
    ui: &mut egui::Ui,
    th: &Theme,
    rect: egui::Rect,
    can_connect: bool,
    connecting: bool,
) -> FooterAction {
    ui.painter().hline(
        rect.x_range(),
        rect.top(),
        egui::Stroke::new(th.border_width.value(), th.separator.to_egui()),
    );
    let inner = egui::Rect::from_min_max(
        egui::pos2(rect.left() + th.spacing_lg.value(), rect.top()),
        egui::pos2(rect.right() - th.spacing_lg.value(), rect.bottom()),
    );
    let mut action = FooterAction::None;
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    child.spacing_mut().item_spacing.x = th.spacing_sm.value();
    if Button::new(t("remote_attach.connect"))
        .variant(ButtonVariant::Primary)
        .enabled(can_connect)
        .show(&mut child, th)
        .clicked()
    {
        action = FooterAction::Connect;
    }
    let ghost_label = if connecting {
        t("remote_attach.stop")
    } else {
        t("remote_attach.cancel")
    };
    if Button::new(ghost_label)
        .variant(ButtonVariant::Ghost)
        .show(&mut child, th)
        .clicked()
    {
        action = FooterAction::Cancel;
    }
    action
}

// ════════════════════════════════════════════════════════════════════════
// 공용 헬퍼
// ════════════════════════════════════════════════════════════════════════
/// caps 헤더 — mono micro uppercase muted. `suffix` 있으면 "· {suffix}" 를 붙인다.
fn caps_header(ui: &mut egui::Ui, th: &Theme, label: &str, suffix: Option<&str>) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), CAPS_H),
        egui::Sense::hover(),
    );
    let base_x = rect.left() + th.spacing_md.value();
    let y = rect.top() + th.spacing_md.value();
    let galley = ui.painter().layout_no_wrap(
        label.to_uppercase(),
        egui::FontId::monospace(th.font_size_micro.value()),
        th.text_muted().into(),
    );
    let w = galley.rect.width();
    ui.painter()
        .galley(egui::pos2(base_x, y), galley, th.text_muted().into());
    if let Some(s) = suffix {
        ui.painter().text(
            egui::pos2(base_x + w + th.spacing_sm.value(), y),
            egui::Align2::LEFT_TOP,
            format!("· {s}"),
            egui::FontId::proportional(th.font_size_caption.value()),
            th.text_muted().into(),
        );
    }
}

/// 원격/inactive pill — fill/border alpha 는 디자인 color-mix(% transparent) 근사.
fn badge(
    ui: &mut egui::Ui,
    th: &Theme,
    text: &str,
    color: egui::Color32,
    fill_a: f32,
    border_a: f32,
    warn_icon: bool,
) {
    let font = egui::FontId::monospace(th.font_size_micro.value());
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font, egui::Color32::PLACEHOLDER);
    let pad_x = th.spacing_sm.value();
    let icon_w = if warn_icon { 12.0 + 4.0 } else { 0.0 };
    let w = pad_x * 2.0 + icon_w + galley.rect.width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, BADGE_H), egui::Sense::hover());
    let radius = th.corner_radius_sm.value();
    ui.painter()
        .rect_filled(rect, radius, color.gamma_multiply(fill_a));
    ui.painter().rect_stroke(
        rect,
        radius,
        egui::Stroke::new(th.border_width.value(), color.gamma_multiply(border_a)),
        egui::StrokeKind::Inside,
    );
    let mut tx = rect.left() + pad_x;
    if warn_icon {
        let ir = egui::Rect::from_min_size(
            egui::pos2(tx, rect.center().y - 6.0),
            egui::vec2(12.0, 12.0),
        );
        icons::ALERT_TRIANGLE.image(12.0, color).paint_at(ui, ir);
        tx += 12.0 + 4.0;
    }
    ui.painter().galley(
        egui::pos2(tx, rect.center().y - galley.rect.height() * 0.5),
        galley,
        color,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 테스트용 job — 워커 스레드 없이 슬롯/시작시각/취소 핸들만 갖춘다.
    fn test_job(started_at: Instant) -> BrowseJob {
        BrowseJob {
            slot: Arc::new(Mutex::new(None)),
            started_at,
            cancel: SshCancel::new(),
        }
    }

    /// 워커가 결과를 채우지 않아도 상한 경과 후 Connecting 을 벗어난다(무한 로딩 회귀
    /// 고정). 판정은 슬롯이 아니라 경과 시간이 만든다.
    #[test]
    fn connecting_transitions_out_after_deadline() {
        // 상한 이내 + 빈 슬롯 → 계속 대기.
        assert_eq!(
            poll_decision(false, BROWSE_DEADLINE / 2, BROWSE_DEADLINE),
            PollDecision::Wait
        );
        // 상한 경과 + 빈 슬롯 → 타임아웃.
        assert_eq!(
            poll_decision(false, BROWSE_DEADLINE, BROWSE_DEADLINE),
            PollDecision::TimedOut
        );
        // 결과가 있으면 상한을 넘겼어도 결과가 우선(워커가 이겼다).
        assert_eq!(
            poll_decision(true, BROWSE_DEADLINE * 2, BROWSE_DEADLINE),
            PollDecision::Take
        );

        // 상태 머신 수준: 빈 슬롯 그대로 상한을 넘기면(상한 주입) Error 로 전이하고
        // job(및 그 자식 ssh)이 취소된다.
        let job = test_job(Instant::now());
        let cancel = job.cancel.clone();
        let mut st = UiState {
            attach_sel: Some("blackhole-attach".into()),
            conn: Conn::Connecting,
            job: Some(job),
            ..Default::default()
        };
        // 상한 이내에는 Connecting 유지.
        poll_browse(&mut st, BROWSE_DEADLINE);
        assert!(matches!(st.conn, Conn::Connecting));
        assert!(st.job.is_some());
        // 상한 경과(=0 상한 주입) → 워커가 아무것도 안 채웠어도 벗어난다.
        poll_browse(&mut st, Duration::ZERO);
        assert!(matches!(st.conn, Conn::Error(_)), "상한 초과 → Error 전이");
        assert!(st.job.is_none(), "타임아웃 시 job 은 회수된다");
        assert!(cancel.is_cancelled(), "타임아웃은 워커 취소까지 동반한다");
    }

    /// 취소는 팝업을 닫지 않고 Initial 로 되돌린다(자식 ssh 취소 포함).
    #[test]
    fn cancel_during_connecting_returns_to_initial() {
        let job = test_job(Instant::now());
        let cancel = job.cancel.clone();
        let mut st = UiState {
            attach_sel: Some("blackhole-attach".into()),
            conn: Conn::Connecting,
            job: Some(job),
            ..Default::default()
        };

        cancel_browse(&mut st);

        assert!(matches!(st.conn, Conn::Initial), "Initial 로 복귀");
        assert!(
            st.attach_sel.is_none(),
            "선택도 초기화(Initial 문구와 정합)"
        );
        assert!(st.job.is_none());
        assert!(cancel.is_cancelled(), "워커의 자식 ssh 도 함께 취소된다");
    }

    /// 취소된 job 의 결과가 뒤늦게 도착해도 아무도 읽지 않고, 워커 종료와 함께 drop
    /// 된다 — 그때 `BrowseOk.tunnel` 의 `SshTunnel::drop` 이 자식 ssh 를 kill 하므로
    /// 터널이 새지 않는다(취소가 반대 방향 누수로 바뀌지 않게 하는 계약).
    #[test]
    fn cancelled_job_result_is_discarded_and_tunnel_dropped() {
        let job = test_job(Instant::now());
        let worker_slot = Arc::clone(&job.slot); // 워커가 쥐고 있는 몫.

        cancel_job(Some(job));

        // UI 쪽 핸들이 사라져 이제 슬롯을 보는 것은 워커뿐이다.
        assert_eq!(Arc::strong_count(&worker_slot), 1);
        // 뒤늦게 도착한 결과 — 읽는 쪽이 없다.
        *worker_slot.lock().unwrap() = Some(Ok(BrowseOk {
            port: 1234,
            tunnel: None,
            workspaces: Vec::new(),
        }));
        // 워커 종료 = 마지막 Arc drop → 결과(그리고 그 안의 터널)도 함께 drop.
        drop(worker_slot);
    }

    /// `UiState`(진행 중 조회 워커 + 재사용 터널을 쥐고 있는 egui temp memory)가
    /// 훅 호출 한 번으로 drop 되는지 확인 — draw_fn 을 거치지 않는 닫힘 경로(바깥
    /// 클릭/`UiIntent::ClosePopup`/디버그 IPC)에서도 이 훅이 정리를 담당한다.
    #[test]
    fn on_close_clears_ui_state() {
        let ctx = egui::Context::default();
        let job = test_job(Instant::now());
        let cancel = job.cancel.clone();
        write_ui(
            &ctx,
            UiState {
                attach_sel: Some("prod".to_string()),
                conn: Conn::Connecting,
                job: Some(job),
                ..Default::default()
            },
        );
        assert!(
            ctx.memory(|m| m.data.get_temp::<UiState>(egui::Id::new(UI_MEMORY_ID)))
                .is_some()
        );

        let (mut state, mut engine) = crate::state::tests::test_state();
        on_close_remote_attach_popup(&ctx, &mut state, &mut engine);

        assert!(
            ctx.memory(|m| m.data.get_temp::<UiState>(egui::Id::new(UI_MEMORY_ID)))
                .is_none()
        );
        // UiState drop 만으로는 포트 발견 자식 ssh 가 회수되지 않는다 — 훅이 취소까지
        // 책임진다(ADR-0063 단일 choke point).
        assert!(cancel.is_cancelled(), "닫힘 훅이 진행 중 조회를 취소한다");
    }
}
