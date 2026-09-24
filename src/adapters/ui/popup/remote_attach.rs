//! 원격 워크스페이스 조회·연결. 프로필을 고르면 워커에서 workspace.list와 attach.list를 조회한다.
//! 사용자가 Connect를 누르면 조회 터널을 재사용해 mirror 워크스페이스를 만든다.
//! 사용자 입력 경로이므로 pending_gui_attach_user에서 새 mirror로 포커스를 옮긴다.
//! release에서는 dispatch_pending_gui_attach가 자기 인스턴스로의 연결을 거절한다.

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tasty_type_geometry::length::LogicalPx;

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

/// attach kind(같은 레지스트리의 예약 kind, ADR-0020). remote_tool Attach 탭이 편집하고
/// 이 팝업은 **소비만** 한다.
const ATTACH_KIND: &str = "tasty-attach";

const LEFT_W: LogicalPx = LogicalPx(240.0);
const HEADER_H: LogicalPx = LogicalPx(47.0);
const FOOTER_H: LogicalPx = LogicalPx(49.0);

// 중앙 블록 글리프 크기는 `tasty-ui-widgets::tokens` 가 단일 출처다.
use tasty_ui_widgets::tokens::{CENTER_GLYPH_SIZE, STRUCT_GAP_2};
const CAPS_H: LogicalPx = LogicalPx(30.0);
const PROFILE_ROW_H: LogicalPx = LogicalPx(50.0);
const WS_ROW_H: LogicalPx = LogicalPx(34.0);
const BADGE_H: LogicalPx = LogicalPx(16.0);
const HEADER_PAD_L: LogicalPx = LogicalPx(14.0);

/// 생성 중에도 목록을 남겨 두되 흐리게 표시한다.
const LIST_DIM_WHILE_CREATING: f32 = 0.5;

/// 조회 결과가 없으면 다음 UI 폴링에서 이 경과 시간을 기준으로 취소한다.
/// SSH 발견·연결·IPC 읽기의 개별 제한과 별개이며 워커 전체 종료 시간을 보장하지 않는다.
const BROWSE_DEADLINE: Duration = Duration::from_secs(20);

/// 생성 결과가 없을 때 UI 대기를 끝낼 기준. 이미 수립한 터널을 사용하므로 조회보다 짧다.
/// 소켓의 개별 읽기·쓰기 제한과 달리 시작 후 경과 시간을 폴링에서 확인한다.
const CREATE_DEADLINE: Duration = Duration::from_secs(10);

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

/// 조회 자식 SSH에 취소를 보내고 결과 슬롯을 놓는다. 늦게 온 결과는 읽지 않으며
/// 워커가 마지막 Arc를 놓으면 결과에 든 터널도 함께 정리된다.
fn cancel_job(job: Option<BrowseJob>) {
    if let Some(job) = job {
        job.cancel.cancel();
    }
}

/// 조회 화면 상태. 목록이 비어도 새 워크스페이스를 만드는 행은 표시한다.
#[derive(Clone, Default)]
enum Conn {
    #[default]
    Initial,
    Connecting,
    Error(String),
    Loaded(Vec<RemoteWorkspace>),
}

/// 기존 워크스페이스와 새로 만들기 행을 하나의 선택값으로 다룬다.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum WsSel {
    New,
    Existing(u32),
}

/// 생성 진행·실패는 행 안에서 표시해 기존 목록을 계속 볼 수 있게 한다.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
enum NewWsPhase {
    #[default]
    Rest,
    Creating,
    Failed(String),
}

/// 기존 터널로 생성 요청을 보내는 워커. 별도 SSH 취소 핸들은 없으며 UI는 결과 대기를
/// 중단할 수 있다. 소켓 읽기·쓰기 제한이 워커 전체 실행 시간을 보장하지는 않는다.
#[derive(Clone)]
struct CreateJob {
    slot: Arc<Mutex<Option<Result<u32, String>>>>,
    started_at: Instant,
}

#[derive(Clone, Default)]
struct UiState {
    /// 선택된 attach 프로필명.
    attach_sel: Option<String>,
    conn: Conn,
    /// 선택된 목록 행(기존 원격 ws 또는 "+ 새 워크스페이스").
    ws_sel: Option<WsSel>,
    /// "+ 새 워크스페이스" 행의 진행 상태.
    phase: NewWsPhase,
    /// 진행 중 조회 워커.
    job: Option<BrowseJob>,
    /// 진행 중 생성 워커.
    create: Option<CreateJob>,
    /// browse 성공 후 보관한 엔드포인트(Connect 재사용). Cancel/재선택 시 drop → ssh kill.
    ready: Option<Arc<Mutex<Option<ReadyConn>>>>,
}

impl UiState {
    /// 생성 왕복 중인가 — Connect 재클릭 차단과 행 렌더가 같이 본다.
    fn creating(&self) -> bool {
        self.phase == NewWsPhase::Creating
    }

    /// 목록이 있고 행을 골랐으며 생성 중이 아닐 때 확정할 수 있다. 빈 목록의 새로 만들기도 포함한다.
    fn can_connect(&self) -> bool {
        matches!(&self.conn, Conn::Loaded(_)) && self.ws_sel.is_some() && !self.creating()
    }

    /// 새 행이 선택돼 있는가 — footer 라벨이 `Create & connect` 로 바뀌는 조건.
    fn create_mode(&self) -> bool {
        self.ws_sel == Some(WsSel::New)
    }
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

/// attach 프로필의 표시 요약.
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

/// SSH·IPC 조회는 UI를 막지 않도록 워커에서 실행한다.
fn spawn_browse(ctx: &egui::Context, profile: String) -> BrowseJob {
    let slot: Arc<Mutex<Option<Result<BrowseOk, String>>>> = Arc::new(Mutex::new(None));
    let slot_w = Arc::clone(&slot);
    let ctx_w = ctx.clone();
    let cancel = SshCancel::new();
    let cancel_w = cancel.clone();
    let name = profile;
    std::thread::spawn(move || {
        // 포트 발견에 사용한 자식 SSH를 취소할 수 있도록 등록한다.
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
            // 연결 뒤 취소가 들어왔으면 터널을 놓고 다음 조회를 하지 않는다.
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
        *crate::poison::recover_mutex(
            slot_w.lock(),
            BROWSE_SLOT_WHAT,
            &BROWSE_SLOT_POISON_REPORTED,
        ) = Some(res);
        ctx_w.request_repaint();
    });
    BrowseJob {
        slot,
        started_at: Instant::now(),
        cancel,
    }
}

/// 결과 도착과 경과 시간을 함께 보는 폴링 판정. 워커가 결과를 못 채워도 UI 대기는 끝낼 수 있다.
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

/// Option 슬롯은 poison 후에도 복구해 읽을 수 있다. 각 락의 첫 오류를 기록한다.
/// 근거: docs/dev-guide/error-handling.md의 락 poison 규칙.
static BROWSE_SLOT_POISON_REPORTED: AtomicBool = AtomicBool::new(false);
static CREATE_SLOT_POISON_REPORTED: AtomicBool = AtomicBool::new(false);
static READY_CONN_POISON_REPORTED: AtomicBool = AtomicBool::new(false);

const BROWSE_SLOT_WHAT: &str = "remote attach browse slot";
const CREATE_SLOT_WHAT: &str = "remote attach create slot";
const READY_CONN_WHAT: &str = "remote attach ready connection";

/// 렌더링 때 결과·경과 시간을 확인한다. 연결 중에는 Spinner가 다시 그리기를 요청한다.
fn poll_browse(st: &mut UiState, deadline: Duration) {
    let Some(job) = st.job.as_ref() else { return };
    let filled = crate::poison::recover_mutex(
        job.slot.lock(),
        BROWSE_SLOT_WHAT,
        &BROWSE_SLOT_POISON_REPORTED,
    )
    .is_some();
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
    let outcome = crate::poison::recover_mutex(
        job.slot.lock(),
        BROWSE_SLOT_WHAT,
        &BROWSE_SLOT_POISON_REPORTED,
    )
    .take();
    match outcome {
        Some(Ok(ok)) => {
            // 빈 원격 목록에서는 새로 만들기를 미리 선택한다.
            st.ws_sel = ok.workspaces.is_empty().then_some(WsSel::New);
            st.conn = Conn::Loaded(ok.workspaces.clone());
            st.phase = NewWsPhase::Rest;
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
            st.conn = Conn::Error(t("remote_attach.error_generic").to_string());
        }
    }
}

/// 기존 터널 포트로 workspace.create를 보낸다. 터널 소유권은 UI에 남긴다.
/// 빈 params로 원격 기본 이름·cwd와 terminal 타입을 사용한다.
fn spawn_create(ctx: &egui::Context, port: u16) -> CreateJob {
    let slot: Arc<Mutex<Option<Result<u32, String>>>> = Arc::new(Mutex::new(None));
    let slot_w = Arc::clone(&slot);
    let ctx_w = ctx.clone();
    std::thread::spawn(move || {
        let res = remote_browse::probe_method(port, "workspace.create", serde_json::json!({}))
            .map_err(|e| e.to_string())
            .and_then(|v| {
                v.get("id")
                    .and_then(|v| v.as_u64())
                    .map(|n| n as u32)
                    .ok_or_else(|| t("remote_attach.create_failed_generic").to_string())
            });
        *crate::poison::recover_mutex(
            slot_w.lock(),
            CREATE_SLOT_WHAT,
            &CREATE_SLOT_POISON_REPORTED,
        ) = Some(res);
        ctx_w.request_repaint();
    });
    CreateJob {
        slot,
        started_at: Instant::now(),
    }
}

/// 이미 생성 중이면 새 워커를 시작하지 않는다. 버튼 비활성화와 별개로 중복 생성을 막는다.
fn start_create(ctx: &egui::Context, st: &mut UiState) {
    if st.create.is_some() {
        return;
    }
    let Some(port) = st.ready.as_ref().and_then(|a| {
        crate::poison::recover_mutex(a.lock(), READY_CONN_WHAT, &READY_CONN_POISON_REPORTED)
            .as_ref()
            .map(|r| r.port)
    }) else {
        st.phase = NewWsPhase::Failed(t("remote_attach.error_generic").to_string());
        return;
    };
    st.phase = NewWsPhase::Creating;
    st.create = Some(spawn_create(ctx, port));
}

/// 생성 성공이면 워크스페이스 ID를 반환한다. 실패·대기 만료 시 목록과 팝업을 남겨
/// 기존 워크스페이스 선택이나 재시도가 가능하게 한다.
fn poll_create(st: &mut UiState, deadline: Duration) -> Option<u32> {
    let job = st.create.as_ref()?;
    let filled = crate::poison::recover_mutex(
        job.slot.lock(),
        CREATE_SLOT_WHAT,
        &CREATE_SLOT_POISON_REPORTED,
    )
    .is_some();
    match poll_decision(filled, job.started_at.elapsed(), deadline) {
        PollDecision::Wait => return None,
        PollDecision::TimedOut => {
            st.create = None;
            st.phase = NewWsPhase::Failed(
                t("remote_attach.create_timeout")
                    .replace("{secs}", &deadline.as_secs().to_string()),
            );
            return None;
        }
        PollDecision::Take => {}
    }
    let job = st.create.take()?;
    match crate::poison::recover_mutex(
        job.slot.lock(),
        CREATE_SLOT_WHAT,
        &CREATE_SLOT_POISON_REPORTED,
    )
    .take()
    {
        Some(Ok(id)) => Some(id),
        Some(Err(e)) => {
            st.phase = NewWsPhase::Failed(e);
            None
        }
        None => {
            st.phase = NewWsPhase::Failed(t("remote_attach.error_generic").to_string());
            None
        }
    }
}

/// 기존·새 워크스페이스 모두 같은 사용자 연결 큐에 넣는다. 포커스 이동도 그 처리 경로에서 맡는다.
fn push_attach(engine: &mut CoreState, st: &mut UiState, workspace: u32) {
    let Some(ready_arc) = st.ready.take() else {
        return;
    };
    let Some(ReadyConn { port, tunnel }) = crate::poison::recover_mutex(
        ready_arc.lock(),
        READY_CONN_WHAT,
        &READY_CONN_POISON_REPORTED,
    )
    .take() else {
        return;
    };
    engine
        .pending_gui_attach_user
        .push(crate::core::GuiAttachUserReq {
            port,
            workspace,
            tunnel,
        });
}

/// 프로필 선택 → 조회 시작(상태 리셋 + 워커 spawn).
fn connect(ctx: &egui::Context, st: &mut UiState, name: String) {
    cancel_job(st.job.take()); // 재선택 = 이전 조회 중단(자식 ssh 회수).
    st.attach_sel = Some(name.clone());
    st.ws_sel = None;
    st.phase = NewWsPhase::Rest;
    st.create = None;
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
    st.phase = NewWsPhase::Rest;
    st.create = None;
    st.ready = None;
}

/// 진행 중 조회를 취소하고 UI 상태를 비운다. 상태를 놓는 것만으로는 워커가 소유한
/// 포트 발견 자식 SSH가 종료되지 않으므로 취소 신호도 보낸다.
fn cleanup(ctx: &egui::Context) {
    cancel_job(read_ui(ctx).job);
    clear_ui(ctx);
}

/// 직접 닫기 외의 경로에서도 진행 중 조회와 UI가 보관한 터널을 정리한다.
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

    // 목록 글자 선택이 행 조작을 가로채지 않도록 이 팝업에서 텍스트 선택을 끈다.
    ui.style_mut().interaction.selectable_labels = false;

    poll_browse(&mut st, BROWSE_DEADLINE);

    let mut close = false;
    if let Some(new_ws) = poll_create(&mut st, CREATE_DEADLINE) {
        push_attach(engine, &mut st, new_ws);
        close = true;
    }

    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        cleanup(&ctx);
        return PopupAction::Close;
    }

    let profiles = RemoteProfiles::load();
    let summaries = attach_summaries(&profiles);

    let full = ui.max_rect();
    ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);

    let mut do_connect = false;

    let header_rect =
        egui::Rect::from_min_size(full.min, egui::vec2(full.width(), HEADER_H.value()));
    if draw_header(ui, &th, header_rect) {
        close = true;
    }

    let footer_rect = egui::Rect::from_min_size(
        egui::pos2(full.left(), full.bottom() - FOOTER_H.value()),
        egui::vec2(full.width(), FOOTER_H.value()),
    );

    let body_rect = egui::Rect::from_min_max(
        egui::pos2(full.left(), header_rect.bottom()),
        egui::pos2(full.right(), footer_rect.top()),
    );
    let left_rect = egui::Rect::from_min_size(
        body_rect.min,
        egui::vec2(LEFT_W.value(), body_rect.height()),
    );
    let right_rect = egui::Rect::from_min_max(
        egui::pos2(body_rect.left() + LEFT_W.value(), body_rect.top()),
        body_rect.max,
    );
    ui.painter().rect_filled(left_rect, 0.0, th.bg_sidebar());
    ui.painter().vline(
        left_rect.right(),
        left_rect.y_range(),
        egui::Stroke::new(th.border_width.value(), th.separator.to_egui()),
    );

    if let Some(name) = draw_left_pane(ui, &th, left_rect, &summaries, st.attach_sel.as_deref()) {
        connect(&ctx, &mut st, name);
    }
    match draw_right_pane(ui, &th, right_rect, &mut st) {
        RightAction::RetryBrowse => {
            if let Some(name) = st.attach_sel.clone() {
                connect(&ctx, &mut st, name);
            }
        }
        RightAction::RetryCreate => start_create(&ctx, &mut st),
        RightAction::None => {}
    }

    let can_connect = st.can_connect();
    let connecting = matches!(&st.conn, Conn::Connecting);
    match draw_footer(
        ui,
        &th,
        footer_rect,
        can_connect,
        connecting,
        st.create_mode(),
    ) {
        FooterAction::Cancel if connecting => cancel_browse(&mut st),
        FooterAction::Cancel => close = true,
        FooterAction::Connect => do_connect = true,
        FooterAction::None => {}
    }

    // 새로 만들기는 생성 완료 뒤 연결하므로 지금 닫지 않는다.
    if do_connect && can_connect {
        match st.ws_sel {
            Some(WsSel::New) => start_create(&ctx, &mut st),
            Some(WsSel::Existing(ws)) => {
                push_attach(engine, &mut st, ws);
                close = true;
            }
            None => {}
        }
    }

    if close {
        cleanup(&ctx);
        PopupAction::Close
    } else {
        write_ui(&ctx, st);
        PopupAction::None
    }
}

fn draw_header(ui: &mut egui::Ui, th: &Theme, rect: egui::Rect) -> bool {
    ui.painter().hline(
        rect.x_range(),
        rect.bottom(),
        egui::Stroke::new(th.border_width.value(), th.separator.to_egui()),
    );
    let inner = egui::Rect::from_min_max(
        egui::pos2(rect.left() + HEADER_PAD_L.value(), rect.top()),
        egui::pos2(rect.right() - th.spacing_sm.value(), rect.bottom()),
    );
    let mut close = false;
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    child.spacing_mut().item_spacing.x = th.spacing_sm.value();
    child.add(icons::TERMINAL_PROMPT.image(th.icon_glyph_size_md.value(), th.text_muted().into()));
    child.label(
        egui::RichText::new(t("remote_attach.heading"))
            .color(th.text_primary())
            .size(th.font_size_heading.value())
            .strong(),
    );
    child.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if ui
            .add(
                egui::ImageButton::new(
                    icons::CLOSE.image(th.icon_glyph_size_md.value(), th.text_muted().into()),
                )
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
    caps_header(&mut col, th, t("remote_attach.attach_profiles"), None);
    let list_rect = egui::Rect::from_min_max(
        egui::pos2(rect.left(), rect.top() + CAPS_H.value()),
        rect.max,
    );
    let mut list = col.new_child(
        egui::UiBuilder::new()
            .max_rect(list_rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    list.set_clip_rect(list_rect);
    egui::ScrollArea::vertical()
        .id_salt("remote_attach.profiles")
        .drag_to_scroll(false)
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
                            egui::vec2((LEFT_W - th.spacing_md.scaled(2.0)).value(), 40.0),
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
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(w, PROFILE_ROW_H.value()), egui::Sense::click());
    if selected {
        ui.painter().rect_filled(rect, 0.0, th.surface_active());
        let bar = egui::Rect::from_min_size(
            rect.min,
            egui::vec2(th.tab_indicator_width.value(), rect.height()),
        );
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
    // 두 번째 줄의 아래가 잘리지 않도록 세로는 행 전체로, 가로만 내부 영역으로 자른다.
    child.shrink_clip_rect(egui::Rect::from_min_max(
        egui::pos2(inner.left(), rect.top()),
        egui::pos2(inner.right(), rect.bottom()),
    ));
    child.spacing_mut().item_spacing.y = STRUCT_GAP_2.value();
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

/// 워커 시작을 호출부에 요청하는 동작.
enum RightAction {
    None,
    /// error center-state 의 Retry — 선택 프로필로 **재조회**.
    RetryBrowse,
    /// 실패한 "+ 새 워크스페이스" 행의 "다시 시도" — **재생성**.
    RetryCreate,
}

/// 우측 pane 렌더.
fn draw_right_pane(
    ui: &mut egui::Ui,
    th: &Theme,
    rect: egui::Rect,
    st: &mut UiState,
) -> RightAction {
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
            RightAction::None
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
            RightAction::None
        }
        Conn::Error(msg) => {
            let msg = msg.clone();
            if center_state(
                ui,
                th,
                rect,
                CenterKind::Glyph(icons::ALERT_TRIANGLE, th.accent_danger().into()),
                t("remote_attach.cant_connect"),
                th.text_primary(),
                &msg,
                true,
            ) {
                RightAction::RetryBrowse
            } else {
                RightAction::None
            }
        }
        Conn::Loaded(ws) => {
            let ws = ws.clone();
            let phase = st.phase.clone();
            match draw_ws_list(ui, th, rect, &ws, &sel_name, st.ws_sel, &phase) {
                Some(ListAction::Select(sel)) => {
                    st.ws_sel = Some(sel);
                    if sel == WsSel::New && matches!(st.phase, NewWsPhase::Failed(_)) {
                        st.phase = NewWsPhase::Rest;
                    }
                }
                Some(ListAction::RetryCreate) => {
                    st.ws_sel = Some(WsSel::New);
                    return RightAction::RetryCreate;
                }
                None => {}
            }
            RightAction::None
        }
    }
}

/// 목록에서 나온 사용자 동작.
enum ListAction {
    /// 행 선택(기존 ws 또는 "+ 새 워크스페이스").
    Select(WsSel),
    /// 실패한 새 행의 "다시 시도".
    RetryCreate,
}

fn draw_ws_list(
    ui: &mut egui::Ui,
    th: &Theme,
    rect: egui::Rect,
    ws: &[RemoteWorkspace],
    profile_name: &str,
    ws_sel: Option<WsSel>,
    phase: &NewWsPhase,
) -> Option<ListAction> {
    let mut action = None;
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
    let list_rect = egui::Rect::from_min_max(
        egui::pos2(rect.left(), rect.top() + CAPS_H.value()),
        rect.max,
    );
    let mut list = col.new_child(
        egui::UiBuilder::new()
            .max_rect(list_rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    list.set_clip_rect(list_rect);
    egui::ScrollArea::vertical()
        .id_salt("remote_attach.workspaces")
        .drag_to_scroll(false)
        .show(&mut list, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
            if let Some(a) = new_ws_row(ui, th, phase, ws_sel == Some(WsSel::New)) {
                action = Some(a);
            }
            if ws.is_empty() {
                empty_line(ui, th, profile_name);
                return;
            }
            // 생성 중에는 목록을 흐리게 남기고 다른 행 선택을 막는다.
            let creating = *phase == NewWsPhase::Creating;
            ui.scope(|ui| {
                if creating {
                    ui.set_opacity(LIST_DIM_WHILE_CREATING);
                    ui.disable();
                }
                for w in ws {
                    if ws_row(ui, th, w, ws_sel == Some(WsSel::Existing(w.id))) {
                        action = Some(ListAction::Select(WsSel::Existing(w.id)));
                    }
                }
            });
        });
    action
}

/// 원격이 닿기는 하는데 ws 가 없을 때 새 행 아래 붙는 muted 한 줄. 이름 열은 위 행과
/// 같은 정렬선에서 시작한다(선행 dot 슬롯 폭 스페이서).
fn empty_line(ui: &mut egui::Ui, th: &Theme, profile_name: &str) {
    let width = ui.available_width();
    let h = th.spacing_xs.value() * 2.0 + th.font_size_caption.value() * th.line_height_ui;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, h), egui::Sense::hover());
    let x =
        rect.left() + th.spacing_md.value() + th.status_dot_size().value() + th.spacing_sm.value();
    ui.painter().text(
        egui::pos2(x, rect.center().y),
        egui::Align2::LEFT_CENTER,
        t("remote_attach.no_workspaces_line").replace("{name}", profile_name),
        egui::FontId::proportional(th.font_size_caption.value()),
        th.text_muted().into(),
    );
}

/// 새로 만들기도 다른 행처럼 선택 후 Connect로 확정한다.
/// 기존 워크스페이스와는 아이콘·라벨 색·구분선으로 구별한다.
fn new_ws_row(
    ui: &mut egui::Ui,
    th: &Theme,
    phase: &NewWsPhase,
    selected: bool,
) -> Option<ListAction> {
    let creating = *phase == NewWsPhase::Creating;
    let failed = matches!(phase, NewWsPhase::Failed(_));
    let width = ui.available_width();
    let sense = if creating {
        egui::Sense::hover()
    } else {
        egui::Sense::click()
    };
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, WS_ROW_H.value()), sense);
    if selected {
        ui.painter().rect_filled(rect, 0.0, th.surface_active());
        let bar = egui::Rect::from_min_size(
            rect.min,
            egui::vec2(th.tab_indicator_width.value(), rect.height()),
        );
        ui.painter().rect_filled(bar, 0.0, th.accent_primary());
    } else if !creating && resp.hovered() {
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
    child.shrink_clip_rect(inner);
    child.spacing_mut().item_spacing.x = th.spacing_sm.value();
    let glyph_c: egui::Color32 = if creating {
        th.text_muted().into()
    } else if failed {
        th.accent_danger().into()
    } else {
        th.accent_primary().into()
    };
    dot_slot_glyph(&mut child, th, creating, failed, glyph_c);
    // 선택 배경에서 대비를 확보하도록 라벨 색을 바꾼다. 아이콘·구분선도 행 구분에 사용한다.
    let label_c = if creating {
        th.text_muted()
    } else if selected {
        th.text_primary()
    } else {
        th.accent_primary()
    };
    child.add(
        egui::Label::new(
            egui::RichText::new(if creating {
                t("remote_attach.creating_workspace")
            } else {
                t("remote_attach.new_workspace")
            })
            .size(th.font_size_body.value())
            .strong()
            .color(label_c),
        )
        .truncate(),
    );
    child.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if !creating && !failed {
            ui.label(
                egui::RichText::new(t("remote_attach.new_workspace_on_remote"))
                    .size(th.font_size_caption.value())
                    .color(th.text_muted()),
            );
        }
    });
    let resp = resp.on_hover_text(t("remote_attach.new_workspace_hint"));

    let mut action = resp.clicked().then_some(ListAction::Select(WsSel::New));
    if let NewWsPhase::Failed(msg) = phase
        && new_ws_error(ui, th, msg)
    {
        action = Some(ListAction::RetryCreate);
    }
    row_separator(ui, th);
    action
}

/// 기존·새 행의 아이콘 공간을 같게 잡아 이름 시작 위치를 맞춘다.
fn dot_slot(ui: &mut egui::Ui, th: &Theme) -> egui::Rect {
    let (slot, _) = ui.allocate_exact_size(
        egui::vec2(th.status_dot_size().value(), th.icon_glyph_size_sm.value()),
        egui::Sense::hover(),
    );
    slot
}

/// 새 행의 글리프 — 슬롯 중심의 `plus`(실패 시 `alertTriangle`, 생성 중이면 Spinner).
fn dot_slot_glyph(
    ui: &mut egui::Ui,
    th: &Theme,
    creating: bool,
    failed: bool,
    color: egui::Color32,
) {
    let size = th.icon_glyph_size_sm.value();
    let g = egui::Rect::from_center_size(dot_slot(ui, th).center(), egui::vec2(size, size));
    if creating {
        let mut c = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(g)
                .layout(egui::Layout::top_down(egui::Align::Center)),
        );
        tasty_ui_widgets::Spinner::new()
            .size(size)
            .color(color)
            .show(&mut c, th);
    } else {
        let glyph = if failed {
            icons::ALERT_TRIANGLE
        } else {
            icons::PLUS
        };
        glyph.image(size, color).paint_at(ui, g);
    }
}

/// 공용 아이콘 공간 안에 상태 점을 그린다. 빈 라벨의 status_dot은 점 폭만 사용한다.
fn dot_slot_status(ui: &mut egui::Ui, th: &Theme, kind: StatusKind, pulse: bool) {
    let slot = dot_slot(ui, th);
    let mut c = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(slot)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    status_dot(&mut c, th, kind, "", pulse, false);
}

/// 생성 실패는 목록을 가리지 않고 행 아래 표시한다. 긴 오류는 줄바꿈하며 툴팁으로 전문을 제공한다.
fn new_ws_error(ui: &mut egui::Ui, th: &Theme, msg: &str) -> bool {
    let lead = th.spacing_md.value() + th.status_dot_size().value() + th.spacing_sm.value();
    let width = ui.available_width();
    let inner_w = (width - lead - th.spacing_md.value()).max(1.0);
    let galley = ui.painter().layout(
        msg.to_owned(),
        egui::FontId::proportional(th.font_size_caption.value()),
        th.accent_danger().into(),
        inner_w,
    );
    let btn_h = tasty_ui_widgets::ControlSize::Sm.height(th);
    let h = th.spacing_xs.value() * 2.0 + galley.rect.height() + btn_h + th.spacing_sm.value();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, h), egui::Sense::hover());
    let inner = egui::Rect::from_min_max(
        egui::pos2(rect.left() + lead, rect.top() + th.spacing_xs.value()),
        egui::pos2(
            rect.right() - th.spacing_md.value(),
            rect.bottom() - th.spacing_sm.value(),
        ),
    );
    let mut col = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    col.shrink_clip_rect(inner);
    col.spacing_mut().item_spacing.y = th.spacing_xs.value();
    col.add(
        egui::Label::new(
            egui::RichText::new(msg)
                .size(th.font_size_caption.value())
                .color(th.accent_danger()),
        )
        .wrap(),
    )
    .on_hover_text(msg);
    Button::new(t("remote_attach.try_again"))
        .variant(ButtonVariant::Secondary)
        .size(tasty_ui_widgets::ControlSize::Sm)
        .leading_icon(&|ui, rect, c| icons::REFRESH.image(rect.height(), c).paint_at(ui, rect))
        .show(&mut col, th)
        .clicked()
}

/// 행 아래 1px 구분선 + 위/아래 xs 마진.
fn row_separator(ui: &mut egui::Ui, th: &Theme) {
    let width = ui.available_width();
    let m = th.spacing_xs.value();
    let w = th.border_width.value();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, m * 2.0 + w), egui::Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.top() + m + w * 0.5,
        egui::Stroke::new(w, th.separator.to_egui()),
    );
}

fn ws_row(ui: &mut egui::Ui, th: &Theme, w: &RemoteWorkspace, selected: bool) -> bool {
    let width = ui.available_width();
    let disabled = w.attached;
    let sense = if disabled {
        egui::Sense::hover()
    } else {
        egui::Sense::click()
    };
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, WS_ROW_H.value()), sense);
    if selected {
        ui.painter().rect_filled(rect, 0.0, th.surface_active());
        let bar = egui::Rect::from_min_size(
            rect.min,
            egui::vec2(th.tab_indicator_width.value(), rect.height()),
        );
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
    child.shrink_clip_rect(inner);
    child.spacing_mut().item_spacing.x = th.spacing_sm.value();
    let kind = if w.busy_count > 0 {
        StatusKind::Running
    } else {
        StatusKind::Idle
    };
    dot_slot_status(&mut child, th, kind, w.busy_count > 0);
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
                th.tint_fill_alpha(),
                th.tint_border_alpha(),
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
            col.add(g.image(CENTER_GLYPH_SIZE, c));
        }
        CenterKind::Spinner => {
            tasty_ui_widgets::Spinner::new()
                .size(CENTER_GLYPH_SIZE)
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
        return Button::new(t("remote_attach.retry"))
            .variant(ButtonVariant::Secondary)
            .leading_icon(&|ui, rect, c| icons::REFRESH.image(rect.height(), c).paint_at(ui, rect))
            .show(&mut col, th)
            .clicked();
    }
    false
}

enum FooterAction {
    None,
    Cancel,
    Connect,
}

/// 조회 중에는 취소 버튼으로 창을 닫지 않고 조회만 중단한다.
fn draw_footer(
    ui: &mut egui::Ui,
    th: &Theme,
    rect: egui::Rect,
    can_connect: bool,
    connecting: bool,
    create_mode: bool,
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
    let connect_label = if create_mode {
        t("remote_attach.connect_create")
    } else {
        t("remote_attach.connect")
    };
    if Button::new(connect_label)
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

/// caps 헤더 — mono micro uppercase muted. `suffix` 있으면 "· {suffix}" 를 붙인다.
fn caps_header(ui: &mut egui::Ui, th: &Theme, label: &str, suffix: Option<&str>) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), CAPS_H.value()),
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
    // 아이콘 폭 + 라벨과의 간격. 아래 그리기·전진과 **같은 값**이어야 한다.
    let icon_sz = th.icon_glyph_size_xs.value();
    let icon_gap = th.spacing_xs.value();
    let icon_w = if warn_icon { icon_sz + icon_gap } else { 0.0 };
    let w = pad_x * 2.0 + icon_w + galley.rect.width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, BADGE_H.value()), egui::Sense::hover());
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
            egui::pos2(tx, rect.center().y - icon_sz * 0.5),
            egui::vec2(icon_sz, icon_sz),
        );
        icons::ALERT_TRIANGLE.image(icon_sz, color).paint_at(ui, ir);
        tx += icon_sz + icon_gap;
    }
    ui.painter().galley(
        egui::pos2(tx, rect.center().y - galley.rect.height() * 0.5),
        galley,
        color,
    );
}

#[cfg(test)]
// 테스트는 의도적으로 무시하는 결과가 많아 let _ 사유 검사에서 제외한다.
#[allow(clippy::let_underscore_must_use)]
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

    /// poison 이후에도 조회 결과를 복구한다. UI 모듈이므로 gui 기능이 있는 조합에서만 컴파일된다.
    #[test]
    fn a_poisoned_browse_slot_still_delivers_the_workspace_list() {
        let mut st = UiState {
            attach_sel: Some("loopback".into()),
            conn: Conn::Connecting,
            job: Some(test_job(Instant::now())),
            ..Default::default()
        };
        let slot = Arc::clone(&st.job.as_ref().unwrap().slot);
        *slot.lock().unwrap() = Some(Ok(BrowseOk {
            port: 4321,
            tunnel: None,
            workspaces: Vec::new(),
        }));

        let poisoner = Arc::clone(&slot);
        // 이유: 이 스레드는 패닉하는 것이 목적이라 join 결과는 항상 Err 다 — 버린다.
        let _ = std::thread::spawn(move || {
            let _guard = poisoner.lock().expect("fresh lock");
            panic!("poison the browse slot on purpose");
        })
        .join();
        assert!(
            slot.is_poisoned(),
            "락이 실제로 poison 됐어야 전제가 성립한다"
        );

        poll_browse(&mut st, BROWSE_DEADLINE);
        assert!(
            matches!(st.conn, Conn::Loaded(ref ws) if ws.is_empty()),
            "poison 이후에도 조회 결과가 그대로 반영돼야 한다"
        );
    }

    #[test]
    fn connecting_transitions_out_after_deadline() {
        assert_eq!(
            poll_decision(false, BROWSE_DEADLINE / 2, BROWSE_DEADLINE),
            PollDecision::Wait
        );
        assert_eq!(
            poll_decision(false, BROWSE_DEADLINE, BROWSE_DEADLINE),
            PollDecision::TimedOut
        );
        // 결과가 있으면 상한을 넘겼어도 결과가 우선(워커가 이겼다).
        assert_eq!(
            poll_decision(true, BROWSE_DEADLINE * 2, BROWSE_DEADLINE),
            PollDecision::Take
        );

        let job = test_job(Instant::now());
        let cancel = job.cancel.clone();
        let mut st = UiState {
            attach_sel: Some("blackhole-attach".into()),
            conn: Conn::Connecting,
            job: Some(job),
            ..Default::default()
        };
        poll_browse(&mut st, BROWSE_DEADLINE);
        assert!(matches!(st.conn, Conn::Connecting));
        assert!(st.job.is_some());
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

    /// 취소 뒤 늦게 온 결과도 마지막 Arc와 함께 정리된다.
    #[test]
    fn cancelled_job_result_is_discarded_and_tunnel_dropped() {
        let job = test_job(Instant::now());
        let worker_slot = Arc::clone(&job.slot); // 워커가 쥐고 있는 몫.

        cancel_job(Some(job));

        assert_eq!(Arc::strong_count(&worker_slot), 1);
        *worker_slot.lock().unwrap() = Some(Ok(BrowseOk {
            port: 1234,
            tunnel: None,
            workspaces: Vec::new(),
        }));
        drop(worker_slot);
    }

    /// 결과가 이미 들어 있는 생성 워커 슬롯 — 워커 스레드 없이 완료 상태를 만든다.
    fn done_create(res: Result<u32, String>) -> CreateJob {
        CreateJob {
            slot: Arc::new(Mutex::new(Some(res))),
            started_at: Instant::now(),
        }
    }

    /// Connect 로 넘어갈 수 있는 상태(목록 조회 성공 + 터널 보관)를 만든다.
    fn loaded_state(workspaces: Vec<RemoteWorkspace>) -> UiState {
        UiState {
            attach_sel: Some("loopback".into()),
            conn: Conn::Loaded(workspaces),
            ready: Some(Arc::new(Mutex::new(Some(ReadyConn {
                port: 4321,
                tunnel: None,
            })))),
            ..Default::default()
        }
    }

    /// 빈 원격 목록에서도 새 워크스페이스를 만들 수 있다.
    #[test]
    fn empty_remote_preselects_new_row_and_enables_connect() {
        let mut st = UiState {
            attach_sel: Some("loopback".into()),
            conn: Conn::Connecting,
            job: Some(test_job(Instant::now())),
            ..Default::default()
        };
        *st.job.as_ref().unwrap().slot.lock().unwrap() = Some(Ok(BrowseOk {
            port: 4321,
            tunnel: None,
            workspaces: Vec::new(),
        }));
        poll_browse(&mut st, BROWSE_DEADLINE);

        assert!(matches!(st.conn, Conn::Loaded(ref ws) if ws.is_empty()));
        assert_eq!(
            st.ws_sel,
            Some(WsSel::New),
            "빈 원격은 새 행을 미리 선택해 둔다"
        );
        assert!(st.can_connect(), "빈 목록에서도 확정할 수 있어야 한다");
        assert!(st.create_mode(), "footer 는 'Create & connect' 를 말한다");
    }

    /// ws 가 있으면 종전대로 선택 없이 시작한다(자동 선택은 empty 한정).
    #[test]
    fn non_empty_remote_starts_with_no_selection() {
        let mut st = UiState {
            conn: Conn::Connecting,
            job: Some(test_job(Instant::now())),
            ..Default::default()
        };
        *st.job.as_ref().unwrap().slot.lock().unwrap() = Some(Ok(BrowseOk {
            port: 4321,
            tunnel: None,
            workspaces: vec![RemoteWorkspace {
                id: 7,
                name: "agents".into(),
                subtitle: None,
                description: None,
                pane_count: 1,
                busy_count: 0,
                attached: false,
                holder: None,
            }],
        }));
        poll_browse(&mut st, BROWSE_DEADLINE);

        assert_eq!(st.ws_sel, None);
        assert!(!st.can_connect(), "고른 행이 없으면 Connect 는 비활성");
    }

    #[test]
    fn create_in_flight_does_not_start_a_second_worker() {
        let ctx = egui::Context::default();
        let mut st = loaded_state(Vec::new());
        st.ws_sel = Some(WsSel::New);

        start_create(&ctx, &mut st);
        assert!(st.creating());
        assert!(!st.can_connect(), "생성 중에는 Connect 재클릭이 막힌다");
        let first = Arc::as_ptr(&st.create.as_ref().unwrap().slot);

        start_create(&ctx, &mut st);
        let second = Arc::as_ptr(&st.create.as_ref().unwrap().slot);
        assert_eq!(first, second, "진행 중 워커가 교체되지 않는다");
    }

    /// 생성 실패는 팝업을 닫지 않는다 — 목록과 터널을 그대로 쥔 채 재시도 가능한
    /// 상태로 남고, 사용자는 곧바로 기존 워크스페이스를 고를 수도 있다.
    #[test]
    fn create_failure_keeps_popup_open_and_retryable() {
        let mut st = loaded_state(Vec::new());
        st.ws_sel = Some(WsSel::New);
        st.phase = NewWsPhase::Creating;
        st.create = Some(done_create(Err("remote is read-only".into())));

        let attached = poll_create(&mut st, CREATE_DEADLINE);

        assert_eq!(attached, None, "실패는 attach 로 이어지지 않는다");
        assert_eq!(
            st.phase,
            NewWsPhase::Failed("remote is read-only".into()),
            "원격 메시지를 그대로 행 하단에 싣는다"
        );
        assert!(st.create.is_none(), "워커 슬롯은 회수된다");
        assert!(st.ready.is_some(), "터널은 살아 있어야 재시도가 된다");
        assert!(st.can_connect(), "실패 후 곧바로 다시 확정할 수 있다");
    }

    /// 워커가 결과를 채우지 않아도 상한 경과 후 Creating 을 벗어난다(무한 스피너 방지).
    #[test]
    fn create_transitions_out_after_deadline() {
        let mut st = loaded_state(Vec::new());
        st.ws_sel = Some(WsSel::New);
        st.phase = NewWsPhase::Creating;
        st.create = Some(CreateJob {
            slot: Arc::new(Mutex::new(None)),
            started_at: Instant::now(),
        });

        assert_eq!(poll_create(&mut st, CREATE_DEADLINE), None);
        assert!(st.creating(), "상한 이내에는 Creating 유지");

        assert_eq!(poll_create(&mut st, Duration::ZERO), None);
        assert!(
            matches!(st.phase, NewWsPhase::Failed(_)),
            "상한 초과 → 실패"
        );
        assert!(st.create.is_none());
    }

    /// 연결 큐에 넣을 때 ready를 가져가므로 같은 결과를 두 번 넣지 않는다.
    #[test]
    fn create_success_pushes_exactly_one_attach() {
        let (_state, mut engine) = crate::state::tests::test_state();
        let mut st = loaded_state(Vec::new());
        st.ws_sel = Some(WsSel::New);
        st.phase = NewWsPhase::Creating;
        st.create = Some(done_create(Ok(99)));

        let new_ws = poll_create(&mut st, CREATE_DEADLINE).expect("생성 성공");
        assert_eq!(new_ws, 99, "응답의 id 를 그대로 attach 대상으로 쓴다");
        push_attach(&mut engine, &mut st, new_ws);

        assert_eq!(engine.pending_gui_attach_user.len(), 1);
        let req = &engine.pending_gui_attach_user[0];
        assert_eq!(req.port, 4321, "조회에 쓴 터널 포트를 그대로 재사용한다");
        assert_eq!(req.workspace, 99);

        push_attach(&mut engine, &mut st, new_ws);
        assert_eq!(engine.pending_gui_attach_user.len(), 1);
    }

    /// 닫기 훅이 UI의 생성 슬롯과 터널 핸들을 정리한다.
    #[test]
    fn on_close_clears_in_flight_create_and_tunnel() {
        let ctx = egui::Context::default();
        let mut st = loaded_state(Vec::new());
        st.ws_sel = Some(WsSel::New);
        st.phase = NewWsPhase::Creating;
        st.create = Some(CreateJob {
            slot: Arc::new(Mutex::new(None)),
            started_at: Instant::now(),
        });
        let create_slot = Arc::clone(&st.create.as_ref().unwrap().slot);
        let ready = Arc::clone(st.ready.as_ref().unwrap());
        write_ui(&ctx, st);

        let (mut state, mut engine) = crate::state::tests::test_state();
        on_close_remote_attach_popup(&ctx, &mut state, &mut engine);

        assert!(
            ctx.memory(|m| m.data.get_temp::<UiState>(egui::Id::new(UI_MEMORY_ID)))
                .is_none()
        );
        assert_eq!(
            Arc::strong_count(&create_slot),
            1,
            "UiState 가 쥐고 있던 생성 슬롯이 회수된다"
        );
        assert_eq!(
            Arc::strong_count(&ready),
            1,
            "재사용 터널 핸들도 함께 회수된다(Drop 시 ssh kill)"
        );
    }

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
        assert!(cancel.is_cancelled(), "닫힘 훅이 진행 중 조회를 취소한다");
    }
}
