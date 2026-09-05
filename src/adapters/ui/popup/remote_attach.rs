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

/// attach kind(같은 레지스트리의 예약 kind, ADR-0032). remote_tool Attach 탭이 편집하고
/// 이 팝업은 **소비만** 한다.
const ATTACH_KIND: &str = "tasty-attach";

// ── 레이아웃 고정 치수 (디자인 raw px — 화면 전용) ──
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

/// 생성 왕복 중 아래 ws 목록의 불투명도 — 목록을 지우지 않고 물러나게만 한다.
const LIST_DIM_WHILE_CREATING: f32 = 0.5;

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

/// 원격 `workspace.create` 왕복이 UI 를 붙잡는 상한.
///
/// [`BROWSE_DEADLINE`] 과 같은 이유(UI 가 먼저 포기)로 두지만 값이 훨씬 짧다 — 이
/// 단계는 SSH 수립이 이미 끝난 뒤라 살아 있는 터널 localport 로 JSON-RPC 를 한 번
/// 보내는 것뿐이고, 그 소켓 자체가 `remote_browse::PROBE_TIMEOUT`(5초) read/write
/// 타임아웃을 건다. 상한을 그 두 배로 잡아 워커가 자기 타임아웃으로 정상 에러를 만들
/// 여지를 먼저 준다.
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
///
/// `Loaded` 는 원격 ws 개수와 무관하게 **목록 경로 하나**다 — 0 개여도 caps 헤더와
/// "+ 새 워크스페이스" 행은 그대로 나오고 그 아래 muted 한 줄만 붙는다. 그래서 빈
/// 원격이 막다른 길이 되지 않는다.
#[derive(Clone, Default)]
enum Conn {
    #[default]
    Initial,
    Connecting,
    Error(String),
    Loaded(Vec<RemoteWorkspace>),
}

/// 우측 목록의 선택. "+ 새 워크스페이스" 행은 원격 ws id 가 없지만 **목록 안의 행**
/// 이므로, 두 번째 boolean 을 만들지 않고 같은 단일 선택 필드에 sentinel 로 함께
/// 담는다(갤러리 specimen 의 `NewRow`/`sel_ws` 구조와 동형).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum WsSel {
    New,
    Existing(u32),
}

/// "+ 새 워크스페이스" 행의 진행 상태. 생성 왕복은 pane 을 통째로 바꾸지 않고 **행
/// 안에서** 표현한다 — 왕복이 1~3초이고, 그동안 사용자가 읽던 목록을 버리지 않기
/// 위해서다. 실패도 같은 이유로 행 하단 인라인이다(connect-error center-state 는
/// "목록 자체를 못 받은" 경우의 어휘).
#[derive(Clone, Default, PartialEq, Eq, Debug)]
enum NewWsPhase {
    #[default]
    Rest,
    Creating,
    Failed(String),
}

/// 진행 중 생성 워커. 조회 워커와 달리 취소 핸들이 없다 — 자식 ssh 를 새로 띄우지
/// 않고 **이미 살아 있는 터널 localport** 로 TCP 왕복 1회를 할 뿐이라, 소켓 자신의
/// 타임아웃으로 반드시 끝난다. 팝업이 먼저 닫히면 터널이 drop 되며 그 왕복도 끊긴다.
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

    /// footer primary 활성 조건 = 목록이 떠 있고 · 행이 선택됐고 · 생성 왕복 중이 아님.
    ///
    /// 목록이 비었는지는 조건이 **아니다** — 빈 원격에서도 "+ 새 워크스페이스" 를 고를
    /// 수 있고, 그때 이 버튼이 그 확정 수단이다.
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

/// 조회 슬롯 · 생성 슬롯 · 터널 슬롯의 poison 을 각각 첫 1 회만 보고한다.
///
/// 세 락 모두 임계구역이 `Option<_>` 한 칸이라 패닉이 나도 불변식이 성립한다 — 복구가
/// 맞다. 반대로 여기서 패닉하면 폴링이 **메인(렌더) 스레드**라 모든 창이 죽는다.
/// 조용히 버리면 사용자에게는 "왜인지 모르게 시간 초과" 나 "Connect 를 눌렀는데
/// 아무 일도 안 일어남" 으로만 보인다. 근거 `docs/dev-guide/error-handling.md` "락 poison".
static BROWSE_SLOT_POISON_REPORTED: AtomicBool = AtomicBool::new(false);
static CREATE_SLOT_POISON_REPORTED: AtomicBool = AtomicBool::new(false);
static READY_CONN_POISON_REPORTED: AtomicBool = AtomicBool::new(false);

const BROWSE_SLOT_WHAT: &str = "remote attach browse slot";
const CREATE_SLOT_WHAT: &str = "remote attach create slot";
const READY_CONN_WHAT: &str = "remote attach ready connection";

/// 워커 완료/상한 폴링 — 완료 시 job → conn(+ready) 전이. 재렌더당 1회.
///
/// Connecting 상태에서는 Spinner 가 매 프레임 repaint 를 요청하므로 경과 시간 판정이
/// 매 프레임 돈다.
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
            // 원격에 ws 가 없으면 "+ 새 워크스페이스" 행을 **미리 선택**해 둔다 — pane
            // 이 뜬 순간부터 Connect 가 살아 있어, 컨트롤을 늘리지 않고 막다른 길이
            // 사라진다. ws 가 있으면 종전대로 선택 없이 시작한다.
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
            // 슬롯이 비었는데 done — 이론상 도달 안 함. 안전하게 에러로.
            st.conn = Conn::Error(t("remote_attach.error_generic").to_string());
        }
    }
}

/// 살아 있는 터널 localport 로 원격 `workspace.create` 를 1회 보내 새 ws id 를
/// 받아오는 워커.
///
/// 터널 핸들(`SshTunnel`)은 넘기지 않는다 — `UiState` 가 계속 쥐고 있어야 Drop 되지
/// 않으므로 워커에는 **`port` 복사본만** 준다(왕복 동안 터널은 살아 있다).
///
/// params 는 **빈 객체**다: `type` 미지정 → `terminal`(surface 1개를 가진 ws 가 되어
/// 곧바로 attach 대상이 된다), `name`/`cwd` 미지정 → 원격의 기본값. 클라이언트는 원격
/// 파일시스템 경로를 모르므로 cwd 를 지어내지 않는다.
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

/// "+ 새 워크스페이스" 확정 — 생성 워커를 띄운다.
///
/// 이미 진행 중이면 아무것도 하지 않는다: Connect 는 `can_connect` 가 이미 막지만,
/// 중복 요청이 원격에 워크스페이스를 두 개 만드는 사고는 버튼 비활성 하나에만
/// 기대기엔 대가가 크다(생성은 되돌릴 수단이 없다 — `workspace.close` IPC 가 없다).
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

/// 생성 워커 완료/상한 폴링. 성공 시 새 원격 ws id 를 돌려주고(호출자가 attach 로
/// 이어간다), 실패/상한이면 `phase` 를 `Failed` 로 두고 팝업은 열어 둔다 — 목록을
/// 가리지 않아야 사용자가 곧바로 기존 워크스페이스를 고를 수 있다.
///
/// [`poll_browse`] 와 같은 판정([`poll_decision`])을 쓴다: 워커가 늦거나 패닉해도
/// 경과 시간만으로 Creating 을 벗어난다.
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
            // 슬롯이 비었는데 done — 이론상 도달 안 함. 안전하게 실패로.
            st.phase = NewWsPhase::Failed(t("remote_attach.error_generic").to_string());
            None
        }
    }
}

/// 확정된 원격 ws 를 재사용 터널과 함께 **사용자-경로 큐**에 넣는다(메인 루프 drain).
///
/// focus 이동은 이 큐의 drain(`dispatch_pending_gui_attach`)이 담당한다 — 새 경로를
/// 만들지 않는다(원칙 1). 기존 ws Connect 와 새 ws 생성 후 attach 가 **같은 한 지점**
/// 을 지나므로, 두 경로가 갈라져 서로 다른 attach 를 하는 일이 생기지 않는다.
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
    // 프로필을 바꾸면 선택과 새 행 상태가 rest 로 초기화된다 — 이전 원격의 생성 실패
    // 문구를 다른 원격의 목록 위에 남겨두지 않는다.
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

    let mut close = false;
    // 원격 생성이 끝났으면 기존 ws Connect 와 **같은 지점**으로 합류한다 — 새 ws id
    // 로 attach 를 큐에 넣고 닫는다("만들어졌다" 중간 단계 없음).
    if let Some(new_ws) = poll_create(&mut st, CREATE_DEADLINE) {
        push_attach(engine, &mut st, new_ws);
        close = true;
    }

    // Escape: 닫기(진행 중 조회 워커의 자식 ssh + 터널은 cleanup 이 회수).
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        cleanup(&ctx);
        return PopupAction::Close;
    }

    let profiles = RemoteProfiles::load();
    let summaries = attach_summaries(&profiles);

    let full = ui.max_rect();
    ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);

    let mut do_connect = false;

    // ── 헤더 ──
    let header_rect =
        egui::Rect::from_min_size(full.min, egui::vec2(full.width(), HEADER_H.value()));
    if draw_header(ui, &th, header_rect) {
        close = true;
    }

    // ── footer ──
    let footer_rect = egui::Rect::from_min_size(
        egui::pos2(full.left(), full.bottom() - FOOTER_H.value()),
        egui::vec2(full.width(), FOOTER_H.value()),
    );

    // ── body(2-pane) ──
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
    // 우: 4상태.
    match draw_right_pane(ui, &th, right_rect, &mut st) {
        // error 상태의 Retry — 선택 프로필로 재조회.
        RightAction::RetryBrowse => {
            if let Some(name) = st.attach_sel.clone() {
                connect(&ctx, &mut st, name);
            }
        }
        // 실패한 새 행의 "다시 시도" — 같은 터널로 생성만 다시 건다(조회는 유효하다).
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
        // 조회 중에는 같은 ghost 버튼이 "중단" 이다 — 팝업을 닫지 않고 조회만 끊어
        // Initial 로 돌아간다(닫기는 헤더 X / Escape).
        FooterAction::Cancel if connecting => cancel_browse(&mut st),
        FooterAction::Cancel => close = true,
        FooterAction::Connect => do_connect = true,
        FooterAction::None => {}
    }

    // Connect 실행. 기존 ws 면 곧바로 attach 큐에 넣고 닫는다. "+ 새 워크스페이스"
    // 면 그 사이에 원격 `workspace.create` 왕복 하나가 끼므로 여기서 닫지 않고 워커를
    // 띄운다 — 결과는 다음 프레임의 `poll_create` 가 받아 같은 attach 지점으로 합류한다.
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
    // 클립은 가로만(truncate 오버플로 가드) — 세로는 행 전체 높이로 둔다. inner 는
    // 상하 spacing_sm 를 깎아 두 줄(name+target)을 담기엔 낮아, inner 로 세로까지
    // 클립하면 둘째 줄(target) 하단 descender 가 잘린다.
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

// ════════════════════════════════════════════════════════════════════════
// 우: 4상태
// ════════════════════════════════════════════════════════════════════════
/// 우측 pane 이 caller 에게 올리는 요청 — 둘 다 워커를 새로 띄우는 동작이라
/// draw 안에서 처리하지 않고 밖으로 올린다.
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
            // 렌더 분기는 **하나**다 — ws 가 0 개여도 caps 헤더 + "+ 새 워크스페이스"
            // 행은 그대로 나오고 그 아래 muted 한 줄만 붙는다. empty 는 다른 화면이
            // 아니라 "행이 정확히 하나인 목록"이다.
            let ws = ws.clone();
            let phase = st.phase.clone();
            match draw_ws_list(ui, th, rect, &ws, &sel_name, st.ws_sel, &phase) {
                Some(ListAction::Select(sel)) => {
                    st.ws_sel = Some(sel);
                    // 실패 상태에서 행을 다시 고르면 rest 로 되돌린다(재시도 가능).
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
    // caps 헤더 문구는 그대로다 — 그룹을 설명하는 문구이고, 생성이라는 사실은 행
    // 라벨이 말한다.
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
        .show(&mut list, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
            // "+ 새 워크스페이스" — 항상 첫 행.
            if let Some(a) = new_ws_row(ui, th, phase, ws_sel == Some(WsSel::New)) {
                action = Some(a);
            }
            if ws.is_empty() {
                empty_line(ui, th, profile_name);
                return;
            }
            // 생성 왕복 중에는 아래 목록이 dim + inert 된다 — 사용자가 읽던 목록을
            // 버리지 않으면서, 그 사이 다른 행을 고르지는 못하게 한다.
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

/// "+ 새 워크스페이스" — 목록의 첫 행. ws 행과 **같은 34px 박스**이고, 실제 원격
/// 워크스페이스와는 세 채널 동시로 구분된다(`plus` 글리프 · accent 라벨 · 행 아래 1px
/// 구분선). 색 하나로만 구분하지 않는다.
///
/// 버튼이 아니라 목록 행이라 이웃 행과 같은 select-then-confirm 을 따른다 — 목록 안의
/// 행인데 혼자만 클릭 즉시 실행되면 그 자체가 불일치이고, 원격을 **변경하는** 동작
/// 직전의 되돌릴 수 있는 순간도 사라진다.
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
    // selected 에서만 accent 를 놓는다 — accent 를 surface-active 위에 남기면 대비가
    // 3.17:1 로 떨어져 고른 순간 가장 안 읽힌다. 구분은 글리프·구분선·accent 바가
    // 계속 진다.
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
    // 우측 슬롯 — status dot·pane 수·배지는 의미상 없는 행이라 캡션 하나뿐.
    child.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if !creating && !failed {
            ui.label(
                egui::RichText::new(t("remote_attach.new_workspace_on_remote"))
                    .size(th.font_size_caption.value())
                    .color(th.text_muted()),
            );
        }
    });
    // 이름을 묻지 않는 것이 이 행에서 가장 놀라운 부분이므로, 클릭이 일어나는 자리에서
    // 말한다.
    let resp = resp.on_hover_text(t("remote_attach.new_workspace_hint"));

    let mut action = resp.clicked().then_some(ListAction::Select(WsSel::New));
    if let NewWsPhase::Failed(msg) = phase
        && new_ws_error(ui, th, msg)
    {
        action = Some(ListAction::RetryCreate);
    }
    // 행 아래 1px 구분선 — 새 행 그룹을 닫는다.
    row_separator(ui, th);
    action
}

/// 이름 열 앞의 status-dot 슬롯을 할당한다. 목록의 **모든** 행이 이 한 함수로 슬롯을
/// 잡으므로 이름 열의 좌측 정렬선이 픽셀 동일해진다 — 새 행의 글리프는 슬롯보다
/// 넓지만 좌우로 대칭 overflow 하므로 정렬선을 밀지 않는다.
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

/// ws 행의 실행 dot — `dot_slot` 안에 그린다. 슬롯을 거치는 이유는 **열 정렬**이다:
/// 같은 슬롯을 `dot_slot_glyph` 도 쓰므로 실행 dot 행과 새 행의 이름 열이 같은 x 에서
/// 시작한다. (`status_dot` 자신은 라벨이 비면 dot 폭만 할당하므로 여기서 되뺄 여백은
/// 없다 — `crates/tasty-ui-widgets/tests/status_dot_width.rs` 가 그 계약을 못박는다.)
fn dot_slot_status(ui: &mut egui::Ui, th: &Theme, kind: StatusKind, pulse: bool) {
    let slot = dot_slot(ui, th);
    let mut c = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(slot)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    status_dot(&mut c, th, kind, "", pulse, false);
}

/// 생성 실패 — 행 하단 인라인. 반환값 = "다시 시도" 클릭.
///
/// connect-error center-state 를 쓰지 않는 이유: 그건 "목록 자체를 못 받은" 경우의
/// 어휘다. 여기서는 목록을 이미 쥐고 있고, 실패 후 사용자의 다음 수는 보통 기존
/// 워크스페이스를 고르는 것이므로 목록을 가리면 안 된다. 원격 메시지는 길 수 있어
/// 폭에 맞춰 줄바꿈하고 전문은 hover 툴팁으로 준다.
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
    // 새 행을 고른 상태에서는 primary 가 "만들고 연결" 이다 — 확정을 footer 가 맡기로
    // 한 이상 버튼이 둘 중 무엇을 할지 말해야 한다.
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

// ════════════════════════════════════════════════════════════════════════
// 공용 헬퍼
// ════════════════════════════════════════════════════════════════════════
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
// 테스트 본문은 `let _ =` 사유 주석 정책의 범위 밖이다(전수 가드가 제외한다) —
// 여기 경고는 조치 대상이 될 수 없어 프로덕션 신호만 가린다. error-handling.md.
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

    /// 슬롯이 poison 돼도 조회 결과가 화면까지 온다.
    ///
    /// 조용히 버리는 구현이면 `filled` 판정이 "채워졌다" 로 fallback 한 뒤 take 가
    /// `None` 을 돌려줘, 결과가 있는데도 일반 오류로 떨어진다 — 사용자에게는 원인
    /// 없는 실패로 보인다.
    ///
    /// **자동 실행 채널이 하나뿐이다.** 이 테스트는 `src/adapters/mod.rs` 의
    /// `#[cfg(feature = "gui")] pub mod ui;` 안에 있어 `--no-default-features` 조합에서는
    /// 컴파일 단계에 통째로 사라진다 — 헤드리스 잡의 초록은 이 테스트가 돌았다는 뜻이
    /// 아니다(없는 테스트는 실패하지 못한다). 실측: 두 자동 잡의 명령을 워크플로에서
    /// 그대로 읽어 `-- --list` 이름을 대조하면 기본 조합에만 뜬다. 팝업 상태를 직접
    /// 쥐고 도는 테스트라 gui 밖으로 옮길 대상이 없어 고칠 수 있는 결함이 아니고,
    /// 사실을 적어 두는 것이 맞는 처리다.
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

    /// 원격에 ws 가 하나도 없어도 "+ 새 워크스페이스" 에 도달하고 확정할 수 있다.
    ///
    /// 이전에는 `Loaded` + empty 가 center-state 로 빠져 목록 렌더 경로 자체가 없었고,
    /// Connect 활성 조건도 `!ws.is_empty()` 를 요구해 빈 원격이 막다른 길이었다.
    #[test]
    fn empty_remote_preselects_new_row_and_enables_connect() {
        let mut st = UiState {
            attach_sel: Some("loopback".into()),
            conn: Conn::Connecting,
            job: Some(test_job(Instant::now())),
            ..Default::default()
        };
        // 워커가 "연결은 됐는데 ws 가 0개" 를 돌려준 상황.
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

    /// 생성 왕복 중에는 Connect 가 비활성이고, 그래도 다시 불린 `start_create` 는
    /// 워커를 새로 띄우지 않는다 — 중복 생성은 되돌릴 수단이 없어(원격
    /// `workspace.close` IPC 부재) 버튼 비활성 하나에만 기대지 않는다.
    #[test]
    fn create_in_flight_does_not_start_a_second_worker() {
        let ctx = egui::Context::default();
        let mut st = loaded_state(Vec::new());
        st.ws_sel = Some(WsSel::New);

        start_create(&ctx, &mut st);
        assert!(st.creating());
        assert!(!st.can_connect(), "생성 중에는 Connect 재클릭이 막힌다");
        let first = Arc::as_ptr(&st.create.as_ref().unwrap().slot);

        // 그럼에도 한 번 더 들어온 확정(연타/다른 경로) — 같은 워커를 유지한다.
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

    /// 생성 성공 → attach 요청이 **정확히 한 건**만 큐에 들어간다.
    ///
    /// 한 건인 근거는 `push_attach` 가 `ready` 를 take 한다는 것이다 — 같은 프레임이
    /// 두 번 돌거나 다음 프레임이 또 들어와도 재사용 터널이 이미 없으므로 두 번째
    /// push 가 성립하지 않는다.
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

        // 중복 방어: 다시 불려도 늘지 않는다.
        push_attach(&mut engine, &mut st, new_ws);
        assert_eq!(engine.pending_gui_attach_user.len(), 1);
    }

    /// 팝업이 닫히면 진행 중 생성 워커와 재사용 터널이 함께 정리된다 — draw_fn 을
    /// 거치지 않는 닫힘 경로(바깥 클릭/`UiIntent::ClosePopup`)도 이 훅 하나를 지난다.
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
