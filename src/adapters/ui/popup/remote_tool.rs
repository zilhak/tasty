//! 원격 접속 도구 팝업 (도구 메뉴 > Remote connections). 3탭: 원격 접속 프로필 /
//! Attach / Passkey.
//!
//! `~/.tasty/remote-profiles.toml`(`RemoteProfiles`) + `~/.tasty/passkeys.toml`(`Passkeys`)
//! 를 GUI 에서 CRUD 한다. CLI/IPC 와 같은 저장 로직을 재사용하므로 표면이 즉시 일관된다.
//! 프로필은 비밀을 담지 않고 passkey 를 이름으로 참조만 한다. 한 탭 안에서 List/Form/
//! ConfirmDelete 를 라우팅한다(세 탭이 동일 패턴으로 인스턴스화). headless PopupDef.
//!
//! Attach 탭(가운데)은 같은 레지스트리의 `tasty-attach` kind 프로필(ADR-0032)을
//! 다룬다 — ssh 프로필 **참조(ref)** 또는 **인라인** 연결정보 + 원격 tasty 실행파일/
//! 포트 발견 모드. tasty-attach kind 는 Profiles 탭 목록·프로토콜 필터에서 제외된다
//! (Attach 탭이 전담).

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use tasty_remote_profiles::{
    KNOWN_PASSKEY_KINDS, PORT_MODES, Passkey, Passkeys, RemoteProfile, RemoteProfiles, SHELLS,
    SshConfigHost, config_availability, enumerate_hosts_at, imported_as, is_builtin_kind,
    is_valid_passkey_name, is_valid_port_mode, is_valid_shell, user_config_path,
};

use crate::adapters::ui::icons;
use crate::adapters::ui::popup::PopupAction;
use crate::core::CoreState;
use crate::i18n::{t, t_fmt};
use crate::state::AppState;
use crate::theme;
use crate::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::tokens::{STRUCT_GAP_1, STRUCT_GAP_2};
use tasty_ui_widgets::vspace;

pub const REMOTE_TOOL_POPUP_ID: &str = "remote_tool";

/// 콤보박스 제안용 알려진 프로필 타입(열린 string — 자유 입력 허용).
/// 헤더·본문의 좌측 안쪽 여백. 디자인 전사값 14 로 4px 그리드 밖이다
/// (`spacing_md`=12 와 2px 차). `egui::Margin` 필드가 `i8` 이라 타입을 맞춰 둔다.
const PANEL_PAD_L: i8 = 14;
/// 헤더 상하 여백. 디자인 전사값 11 로 그리드 밖이다(`spacing_md`=12 와 1px 차).
const HEADER_PAD_Y: i8 = 11;
/// 본문 상단 여백. 디자인 전사값 10 으로 그리드 밖이다(하단은 `spacing_sm`=8 로
/// 비대칭 — 디자인 전사 그대로다).
const CONTENT_PAD_TOP: i8 = 10;
/// 헤더 아이템 가로 간격. 디자인 전사값 9 로 그리드 밖이다(`spacing_sm`=8 과 1px 차).
const HEADER_GAP_X: LogicalPx = LogicalPx(9.0);

const KNOWN_TYPES: &[&str] = &["ssh", "smb", "http"];

const UI_MEMORY_ID: &str = "remote_tool.ui";

/// 프로토콜 필터 드롭다운의 최소 폭. 대응 `Theme` 토큰이 없는 이 팝업 고유 레이아웃
/// 치수라 명명 구조 상수로 둔다(같은 파일의 `LABEL_COL_WIDTH` 선례).
const FILTER_DROPDOWN_MIN_WIDTH: LogicalPx = LogicalPx(216.0);
/// 프로토콜 필터 목록의 최대 높이 — 넘으면 스크롤. 위와 같은 성격의 구조 상수다.
const FILTER_DROPDOWN_MAX_HEIGHT: LogicalPx = LogicalPx(168.0);

/// 프로토콜 필터의 *적용된* hidden(=제외) 집합 저장 키. **`UI_MEMORY_ID` 와 분리** —
/// `clear_ui` 가 popup 닫힘마다 `UI_MEMORY_ID` 만 지우므로 이 키는 보존되어 popup
/// 재오픈에도 필터가 유지된다(디자인: session-only / NON-PERSISTENT). egui temp
/// 메모리라 tasty 종료 시 사라져 "재시작 = 전체 선택" 비영속 정책도 자동 충족.
const FILTER_MEMORY_ID: &str = "remote_tool.filter";

/// 프로토콜 필터 드롭다운 egui popup id. Escape/바깥클릭 닫힘 판정에 사용.
const FILTER_POPUP_ID: &str = "remote_tool.filter_popup";

/// attach 레코드의 kind (같은 레지스트리 안의 예약 kind, ADR-0032).
const ATTACH_KIND: &str = "tasty-attach";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
enum Tab {
    #[default]
    Profiles,
    Attach,
    Passkeys,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
enum Sub {
    #[default]
    List,
    Form,
    ConfirmDelete(String),
}

/// 프로필 폼 버퍼. ssh 는 전용 필드, 그 외는 generic key-value(`fields`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct ProfileForm {
    kind: String,
    name: String,
    label: String,
    // ssh 전용
    host: String,
    user: String,
    port: String,
    shell: String,
    // 공통
    passkey_ref: String,
    fields: Vec<(String, String)>, // generic(비-ssh)
    editing_original: Option<String>,
}

/// Passkey 폼 버퍼.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct PasskeyForm {
    name: String,
    kind: String,
    value: String,
    editing_original: Option<String>,
}

/// Attach 폼 버퍼. Connection 은 ref(ssh 프로필 참조) ↔ inline(자체 연결정보) 토글.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct AttachForm {
    name: String,
    label: String,
    /// true = ref 모드(`ssh_ref`), false = inline 모드(host/user/…).
    mode_ref: bool,
    ssh_ref: String,
    // inline 전용
    host: String,
    user: String,
    port: String,
    shell: String,
    passkey_ref: String,
    // Remote tasty 그룹 (모드 무관 공통)
    remote_tasty: String,
    port_mode: String,
    port_file: String,
    editing_original: Option<String>,
}

#[derive(Clone, Debug)]
struct DetectJob {
    name: String,
    slot: Arc<Mutex<Option<Result<String, String>>>>,
}

#[derive(Clone, Debug, Default)]
struct UiState {
    tab: Tab,
    profile_view: Sub,
    attach_view: Sub,
    passkey_view: Sub,
    pform: ProfileForm,
    aform: AttachForm,
    kform: PasskeyForm,
    perr: Option<String>,
    aerr: Option<String>,
    kerr: Option<String>,
    revealed: HashSet<String>,
    detecting: Option<DetectJob>,
    /// 필터 드롭다운이 열려 있는 동안의 편집 중 제외 집합(draft). Apply 눌러야
    /// `FILTER_MEMORY_ID` 의 적용 집합에 반영(Apply-on-confirm). popup 닫힘 시
    /// `clear_ui` 로 함께 사라지는 순수 편집 상태라 여기 둔다.
    filter_draft: HashSet<String>,
    /// 로컬 ssh config 열거 결과 캐시. **`None` = 아직 안 읽음**.
    ///
    /// egui 는 매 프레임 목록을 다시 그리므로 여기 캐시하지 않으면 프레임마다
    /// `~/.ssh/config` + Include 를 통째로 읽는다. `UI_MEMORY_ID` 에 얹어 두면
    /// `clear_ui`(popup 닫힘)가 무효화까지 맡아 "열 때마다 1 회" 가 성립한다 —
    /// 필터(`FILTER_MEMORY_ID`)처럼 재오픈에도 살아남으면 안 되는 값이다.
    local: Option<LocalSshCache>,
}

/// 로컬 ssh config 스냅샷. 목록 렌더가 파일 I/O 없이 돌 수 있는 최소 정보다.
#[derive(Clone, Debug, Default)]
struct LocalSshCache {
    hosts: Vec<SshConfigHost>,
    /// 표시용 config 경로(`~/.ssh/config`).
    path: String,
    /// 파일 자체가 있는지. "설정이 없다" 와 "있는데 alias 가 0 건" 은 사용자가 할 일이
    /// 다르다.
    exists: bool,
    /// 파일은 있는데 **열 수 없는** 경우(권한 등). `exists && hosts.is_empty()` 만
    /// 보면 "정말 빈 설정" 과 구분되지 않는데, 사용자가 할 일은 정반대다(전자는
    /// Host 를 적는 것, 후자는 권한을 고치는 것). 부정형으로 둔 것은 `Default` 가
    /// "권한 문제 없음" 이 되게 하기 위해서다.
    unreadable: bool,
}

/// 로컬 섹션의 빈 상태 문구 키 — 원인 3 갈래를 가른다(없음 / 못 읽음 / 정말 0 건).
fn local_ssh_empty_key(local: &LocalSshCache) -> &'static str {
    if !local.exists {
        "remote_tool.local_ssh_missing"
    } else if local.unreadable {
        "remote_tool.local_ssh_unreadable"
    } else {
        "remote_tool.local_ssh_empty"
    }
}

/// ssh config 를 한 번 읽어 캐시를 만든다. 호출 지점은 "캐시가 비었을 때" 와
/// "새로고침 아이콘" 둘뿐이다.
fn load_local_ssh() -> LocalSshCache {
    local_ssh_cache_at(user_config_path())
}

/// [`load_local_ssh`] 의 경로 주입 버전 — 실제 홈에 의존하지 않아 픽스처로 검증할 수
/// 있다. 존재/가독 판정은 CLI·IPC 가 쓰는 것과 **같은** 코어 함수
/// (`tasty_remote_profiles::config_availability`)를 재사용한다. GUI 가 따로
/// 구현해두면 (그 자리에 디렉토리가 있는 경우 같은) 엣지케이스 수정이 한쪽에만
/// 반영되고, 세 표면에서 "권한 실패" 의 정의가 조용히 갈라진다.
fn local_ssh_cache_at(path: Option<std::path::PathBuf>) -> LocalSshCache {
    if path.is_none() {
        tracing::warn!("ssh_config: cannot resolve home directory — skipping enumeration");
    }
    let avail = config_availability(path.as_deref());
    LocalSshCache {
        hosts: path.as_deref().map(enumerate_hosts_at).unwrap_or_default(),
        path: path
            .as_deref()
            .map(tasty_utils::path::tilde_abbreviate)
            .unwrap_or_else(|| "~/.ssh/config".into()),
        exists: avail.exists,
        // 코어는 부재를 `readable: false` 로 표현한다 — 여기서 필요한 건 "있는데 못
        // 읽음" 이므로 `exists` 와 함께 봐야 한다(부재는 별도 문구가 담당).
        unreadable: avail.exists && !avail.readable,
    }
}

/// 로컬 섹션 행에 붙는 요약 — 그 Host 블록에 **직접 적힌** `HostName[:Port]`.
///
/// `Host *` 의 전역 설정이나 `Match` 블록이 실제 접속 시 이 값을 덮어쓸 수 있어
/// 정확하지 않다. **표시 전용**이며 가져오기에는 alias 만 쓴다.
fn local_target_hint(h: &SshConfigHost) -> String {
    match (&h.hostname, h.port) {
        (Some(host), Some(port)) => format!("{host}:{port}"),
        (Some(host), None) => host.clone(),
        // HostName 이 없으면 ssh 가 alias 를 호스트 이름으로 쓴다 — 포트만 보여준다.
        (None, Some(port)) => format!("{}:{port}", h.alias),
        (None, None) => "—".into(),
    }
}

/// 가져오기 클릭 시 여는 폼의 프리필. **alias 만 `host` 에 넣고 user/port 는 비운다** —
/// 채우면 ssh config 위임이 깨지고(그 값이 config 를 덮어쓴다) config 수정 시 어긋난다.
/// 이름은 alias 를 기본값으로 주되 폼에서 바꿀 수 있다.
fn import_prefill(alias: &str) -> ProfileForm {
    ProfileForm {
        kind: "ssh".into(),
        name: alias.to_string(),
        host: alias.to_string(),
        shell: "auto".into(),
        ..Default::default()
    }
}

/// 프로필 섹션의 빈 상태 문구 키. `None` = 프로필 행을 그린다.
///
/// 예전에는 두 빈 상태에서 곧바로 `return` 했다 — 로컬 ssh config 섹션이 생긴 뒤로는
/// 그러면 안 된다. 프로필이 0 건인 사용자야말로 "가져올 호스트가 여기 있다" 를 봐야
/// 하는 쪽이다.
fn profile_empty_key(has_non_attach: bool, any_visible: bool) -> Option<&'static str> {
    match (has_non_attach, any_visible) {
        (false, _) => Some("remote_tool.profile_empty"),
        (true, false) => Some("remote_tool.profile_filter_empty"),
        (true, true) => None,
    }
}

/// 로컬 섹션에서 나오는 액션.
enum LocalRowAction {
    /// 이 alias 를 프로필 폼 프리필로 연다.
    Import(String),
    /// ssh config 를 다시 읽는다(파일이 바뀌었을 때).
    Reload,
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

/// 적용된 필터 제외 집합(hidden) 읽기. 미설정=빈 set=필터 없음(전체 표시).
fn read_filter(ctx: &egui::Context) -> HashSet<String> {
    ctx.memory(|m| {
        m.data
            .get_temp::<HashSet<String>>(egui::Id::new(FILTER_MEMORY_ID))
            .unwrap_or_default()
    })
}
fn write_filter(ctx: &egui::Context, hidden: HashSet<String>) {
    ctx.memory_mut(|m| m.data.insert_temp(egui::Id::new(FILTER_MEMORY_ID), hidden));
}

/// 현재 프로필들의 `kind` 집합(=프로토콜). KNOWN_TYPES 순서 우선, 나머지(플러그인/
/// unknown)는 알파벳. 디자인 `protocols` 도출 로직 전사. 프로필 0개인 kind 는 없음.
/// `tasty-attach` 는 Attach 탭 전담이라 프로토콜이 아니다 — 항상 제외.
fn protocol_set(profiles: &[RemoteProfile]) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    for p in profiles {
        let k = p.kind.trim();
        if !k.is_empty() && k != ATTACH_KIND && !seen.iter().any(|s| s == k) {
            seen.push(k.to_string());
        }
    }
    let mut out: Vec<String> = KNOWN_TYPES
        .iter()
        .filter(|t| seen.iter().any(|s| s == *t))
        .map(|t| t.to_string())
        .collect();
    let mut extra: Vec<String> = seen
        .into_iter()
        .filter(|s| !KNOWN_TYPES.contains(&s.as_str()))
        .collect();
    extra.sort();
    out.extend(extra);
    out
}

/// 코어/플러그인이 모르는 kind(필터 목록에서 ⚠ 표식 대상). row 배지 로직과 동일.
fn is_unknown_kind(kind: &str) -> bool {
    !is_builtin_kind(kind) && !KNOWN_TYPES.contains(&kind)
}

/// PopupDef::on_close 진입점 — 어떤 경로로 닫히든 `UiState`(폼 버퍼, detect 워커,
/// 필터 드래프트)를 drop 한다. `FILTER_MEMORY_ID`(적용된 프로토콜 필터)는 별도
/// 키라 건드리지 않는다 — session-only 로 popup 재오픈에도 유지되는 것이 의도.
/// remote_attach 와 동형의 구조적 결함(불변식이 우연에 의존)이었다.
pub fn on_close_remote_tool_popup(
    ctx: &egui::Context,
    _state: &mut AppState,
    _engine: &mut CoreState,
) {
    clear_ui(ctx);
}

/// PopupDef.draw_fn 진입점.
pub fn draw_remote_tool_popup(
    ui: &mut egui::Ui,
    _state: &mut AppState,
    _engine: &mut CoreState,
) -> PopupAction {
    let th = theme::theme();
    let ctx = ui.ctx().clone();
    let mut st = read_ui(&ctx);

    // detect 워커 완료 polling.
    poll_detect(&mut st);

    let mut profiles = RemoteProfiles::load();
    let passkeys = Passkeys::load();

    // Escape: Form/Confirm 은 뒤로, List 면 닫기.
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        // 필터 드롭다운이 열려 있으면 그것만 닫고 popup 은 유지(디자인 ProtocolFilter
        // 의 stopImmediatePropagation 대응). popup 위젯이 같은 프레임에 닫히지 않으므로
        // 여기서 명시적으로 닫는다.
        if ctx.memory(|m| m.is_popup_open(egui::Id::new(FILTER_POPUP_ID))) {
            ctx.memory_mut(|m| m.close_popup());
            write_ui(&ctx, st);
            return PopupAction::None;
        }
        let sub = match st.tab {
            Tab::Profiles => &mut st.profile_view,
            Tab::Attach => &mut st.attach_view,
            Tab::Passkeys => &mut st.passkey_view,
        };
        if *sub == Sub::List {
            clear_ui(&ctx);
            return PopupAction::Close;
        }
        *sub = Sub::List;
        write_ui(&ctx, st);
        return PopupAction::None;
    }

    let mut close = false;
    // 디자인(remote_tool.jsx) 컨테이너는 패딩 0 이고 각 구역이 자체 패딩을 가진다.
    // popup content_margin 은 remote_tool 한정 0 (popup.rs) 이라 full 은 popup 가장자리.
    // egui 자동 간격을 죽이고(아래) 구역 divider 는 각 구역 Frame 의 실제 bottom 좌표에
    // 그린다 (design-parity: 어림 add_space 금지).
    let full = ui.max_rect();
    // 구역(헤더/탭바/콘텐츠)을 디자인처럼 딱 붙이려면 세로 자동간격만 죽인다. x 간격은
    // 건드리지 않는다(콘텐츠 행 내부 gap 이 망가지지 않게). 콘텐츠 영역은 아래에서
    // 원래 spacing 을 복원해 행 레이아웃을 보존한다.
    let saved_spacing = ui.spacing().item_spacing;
    ui.spacing_mut().item_spacing.y = 0.0;
    let sep = egui::Stroke::new(th.border_width.value(), th.border_strong());

    // 헤더 — 디자인 padding T11 R12 B11 L14 + borderBottom separator.
    let header_ir = egui::Frame::NONE
        .inner_margin(egui::Margin {
            left: PANEL_PAD_L,
            right: th.spacing_md.value() as i8,
            top: HEADER_PAD_Y,
            bottom: HEADER_PAD_Y,
        })
        .show(ui, |ui| draw_header(ui, &th));
    if header_ir.inner {
        close = true;
    }
    ui.painter()
        .hline(full.x_range(), header_ir.response.rect.bottom(), sep);
    // 헤더 전체(전체폭 × 실측 헤더 높이)를 드래그 이동 영역으로 매니저에 보고한다.
    // 좁은 정적 띠(panel_header_drag_strip) 대신 이 rect 가 hit-test 에 우선 사용된다.
    super::report_header_drag_rect(
        ui.ctx(),
        REMOTE_TOOL_POPUP_ID,
        egui::Rect::from_x_y_ranges(
            full.x_range(),
            full.top()..=header_ir.response.rect.bottom(),
        ),
    );

    // 탭바 — 디자인 bg-sidebar(mantle) 전체폭, TabBtn height 35, padding L8.
    // 자체 하단 borderBottom 까지 내부에서 그린다.
    draw_tab_bar(ui, &th, &mut st, full.x_range());

    // 콘텐츠 — 리스트는 좌우 14/top 10/bottom 8. 폼(Sub::Form)은 디자인 rtScrollPad/rtFooter
    // 가 자체 패딩(좌우 16)과 하단 고정 footer 를 소유하므로 외곽 margin 0 으로 두고
    // 폼이 패딩·전체폭 separator 를 직접 그린다.
    let is_form = matches!(
        match st.tab {
            Tab::Profiles => &st.profile_view,
            Tab::Attach => &st.attach_view,
            Tab::Passkeys => &st.passkey_view,
        },
        Sub::Form
    );
    let content_margin = if is_form {
        egui::Margin::ZERO
    } else {
        egui::Margin {
            left: PANEL_PAD_L,
            right: PANEL_PAD_L,
            top: CONTENT_PAD_TOP,
            bottom: th.spacing_sm.value() as i8,
        }
    };
    egui::Frame::NONE
        .inner_margin(content_margin)
        .show(ui, |ui| {
            // 콘텐츠 행 레이아웃은 기존 spacing 으로 복원(프레임 정합과 분리).
            ui.spacing_mut().item_spacing = saved_spacing;
            match st.tab {
                Tab::Profiles => {
                    draw_profiles_tab(ui, &th, &ctx, &mut st, &mut profiles, &passkeys)
                }
                Tab::Attach => draw_attach_tab(ui, &th, &mut st, &mut profiles, &passkeys),
                Tab::Passkeys => draw_passkeys_tab(ui, &th, &mut st, &passkeys),
            }
        });

    if close {
        clear_ui(&ctx);
        PopupAction::Close
    } else {
        write_ui(&ctx, st);
        PopupAction::None
    }
}

/// 디자인 secondary 버튼 (Button.jsx `--secondary`): surface-raised(surface0) 채움.
/// base(bg-panel) 패널 배경 위에서 한 단계 밝게 떠 보인다. (egui inactive 기본 버튼은
/// fill=base 라 base 패널 위에서 묻히므로 fill 을 명시한다.)
fn secondary_button(ui: &mut egui::Ui, th: &Theme, label: &str) -> egui::Response {
    ui.add(
        egui::Button::new(
            egui::RichText::new(label)
                .color(th.text_primary())
                .size(th.font_size_body.value()),
        )
        .fill(th.surface_raised())
        .stroke(egui::Stroke::new(
            th.border_width.value(),
            th.border_strong(),
        )),
    )
}

/// Primary 버튼 — accent 채움 + on-accent 텍스트. 디자인 `Button variant="primary"`.
/// 폼 footer 의 Save 액션에 사용.
fn primary_button(ui: &mut egui::Ui, th: &Theme, label: &str) -> egui::Response {
    ui.add(
        egui::Button::new(
            egui::RichText::new(label)
                .color(th.text_on_accent())
                .size(th.font_size_body.value()),
        )
        .fill(th.accent_primary()),
    )
}

/// Ghost 버튼 — 투명 배경 + secondary 텍스트(hover 시 overlay). 디자인
/// `Button variant="ghost"`. 폼 footer 의 Cancel, generic 폼의 Add field 에 사용.
fn ghost_button(ui: &mut egui::Ui, th: &Theme, label: &str) -> egui::Response {
    ui.add(
        egui::Button::new(
            egui::RichText::new(label)
                .color(th.text_secondary())
                .size(th.font_size_body.value()),
        )
        .fill(egui::Color32::TRANSPARENT)
        .stroke(egui::Stroke::NONE),
    )
}

/// 목록 스크롤 영역 — **스크롤바를 항상 숨기고** 스크롤 여지가 있는 쪽 가장자리에
/// 배경색 페이드를 그린다.
///
/// egui 기본 스크롤바는 콘텐츠 위에 **오버레이**로 뜬다(레이아웃 폭을 미리 빼지 않는다).
/// 이 팝업의 행은 우측 끝에 아이콘(가져오기·편집·삭제·재감지)을 두므로, 커서를 그 위로
/// 가져가는 순간 스크롤바가 같은 자리에 나타나 클릭을 먹는다. 스크롤바 폭만큼 콘텐츠를
/// 비켜 그리는 방식(`port_scanner` 의 `scrollbar_reserve`)은 "커서가 스크롤바 위 =
/// 클릭 불가" 라는 구조 자체를 남겨 다른 폭·해상도에서 재발한다 — 스크롤바를 아예
/// 숨기면 이 부류의 버그가 사라진다.
///
/// 숨긴 대신 "더 있다" 는 정보는 가장자리 페이드로 보존한다. 스크롤(휠·드래그·키보드)은
/// 그대로 동작한다 — 숨긴 것은 표시뿐이다.
///
/// 이 방식이 tasty 의 스크롤 어포던스 표준이다 — `docs/adr/0079-scroll-affordance-standard.md`.
fn scroll_list_with_fade<R>(
    ui: &mut egui::Ui,
    th: &Theme,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let out = egui::ScrollArea::vertical()
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .show(ui, add_contents);
    let (up, down) = fade_edges(
        out.state.offset.y,
        out.content_size.y,
        out.inner_rect.height(),
    );
    if up {
        paint_edge_fade(ui, th, out.inner_rect, FadeEdge::Top);
    }
    if down {
        paint_edge_fade(ui, th, out.inner_rect, FadeEdge::Bottom);
    }
    out.inner
}

/// `(위쪽 페이드, 아래쪽 페이드)` 판정. 위/아래로 각각 남은 스크롤 여지가 있는지만 본다.
fn fade_edges(offset: f32, content_h: f32, view_h: f32) -> (bool, bool) {
    // 부동소수 오차로 끝까지 스크롤한 뒤에도 페이드가 남는 것을 막는 여유값.
    // 1px 미만 차이는 사람 눈에 "더 있다" 로 읽히지도 않는다.
    const EPS: LogicalPx = LogicalPx(1.0);
    let max_offset = (LogicalPx(content_h) - LogicalPx(view_h)).max(LogicalPx(0.0));
    let offset = LogicalPx(offset);
    (offset > EPS, offset < max_offset - EPS)
}

/// 페이드를 그릴 가장자리.
#[derive(Clone, Copy, PartialEq, Eq)]
enum FadeEdge {
    Top,
    Bottom,
}

/// 페이드 띠 높이 — 기본은 `space-xl` 이되 **뷰포트 절반**을 넘지 않는다.
///
/// 위·아래 페이드는 독립적으로 그려지므로 각각이 뷰포트 절반 이하일 때만 서로 겹치지
/// 않는다. 뷰포트 높이로만 클램프하면 양방향 스크롤 가능한 좁은 뷰포트(2×`space-xl`
/// 미만)에서 두 띠가 포개져 콘텐츠가 거의 배경색에 덮인다.
fn fade_height(max: f32, view_h: f32) -> f32 {
    max.min(view_h * 0.5)
}

/// 스크롤 가장자리 페이드 — 패널 배경색에서 투명으로 이어지는 세로 그라디언트.
///
/// egui 에는 그라디언트 헬퍼가 없어 정점 색이 다른 사각형 메시를 직접 만든다. 여러 겹의
/// 반투명 사각형을 쌓는 근사보다 단(band)이 지지 않고, 그리는 도형도 하나다.
/// `Color32` 는 premultiplied 라 불투명 배경색 → `TRANSPARENT`(0,0,0,0) 보간이
/// 그대로 올바른 페이드가 된다(중간에 검게 뜨지 않는다).
fn paint_edge_fade(ui: &egui::Ui, th: &Theme, rect: egui::Rect, edge: FadeEdge) {
    let height = fade_height(th.spacing_xl.value(), rect.height());
    let (solid_y, clear_y) = match edge {
        FadeEdge::Top => (rect.top(), rect.top() + height),
        FadeEdge::Bottom => (rect.bottom(), rect.bottom() - height),
    };
    let solid: egui::Color32 = th.bg_panel().into();
    let clear = egui::Color32::TRANSPARENT;
    let mut mesh = egui::epaint::Mesh::default();
    mesh.colored_vertex(egui::pos2(rect.left(), solid_y), solid);
    mesh.colored_vertex(egui::pos2(rect.right(), solid_y), solid);
    mesh.colored_vertex(egui::pos2(rect.left(), clear_y), clear);
    mesh.colored_vertex(egui::pos2(rect.right(), clear_y), clear);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(2, 1, 3);
    ui.painter().add(egui::Shape::mesh(mesh));
}

/// 디자인 separator 선. egui `ui.separator()` 는 theme stroke 색이 배경과 가까워
/// 사실상 비가시 → surface1 색 명시적 hline 으로 그린다.
fn hsep(ui: &mut egui::Ui, th: &Theme) {
    vspace(ui, STRUCT_GAP_2);
    let r = ui.max_rect();
    ui.painter().hline(
        r.x_range(),
        ui.cursor().top(),
        egui::Stroke::new(th.border_width.value(), th.border_strong()),
    );
    vspace(ui, STRUCT_GAP_2);
}

fn draw_header(ui: &mut egui::Ui, th: &Theme) -> bool {
    let mut close = false;
    // 헤더 제목 라벨을 비선택으로 만들어 press 시 포인터를 가져가지 않게 한다
    // (egui 기본 selectable_labels=true 면 글자 위 드래그가 텍스트 선택이 됨).
    // 헤더 프레임 서브트리에만 적용 — 본문(탭·리스트) 라벨 선택성은 불변.
    ui.style_mut().interaction.selectable_labels = false;
    ui.horizontal(|ui| {
        // 디자인 헤더 콘텐츠 높이 ~24 (title fontSize14 line-height). egui label/icon 은
        // 텍스트 박스가 더 낮아(~18) 헤더가 얕아진다 → min_height 로 디자인 높이 강제.
        // popup border 가 stroke Outside 라 콘텐츠가 1px 위에서 시작 → +2 보정해 26.
        ui.set_min_height(th.remote_tool_header_min_height().value());
        ui.spacing_mut().item_spacing.x = HEADER_GAP_X.value();
        // 헤더 앞 터미널 프롬프트 아이콘(`>_`) — 디자인 remote_tool.jsx 헤더.
        ui.add(icons::TERMINAL_PROMPT.image(th.icon_glyph_size_md.value(), th.text_muted().into()));
        ui.label(
            egui::RichText::new(t("remote_tool.heading"))
                .color(th.text_primary())
                .size(th.font_size_heading.value())
                .strong(),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add(
                    egui::ImageButton::new(
                        icons::CLOSE.image(th.icon_glyph_size_md.value(), th.text_muted().into()),
                    )
                    .frame(false),
                )
                .on_hover_text(t("remote_tool.close"))
                .clicked()
            {
                close = true;
            }
        });
    });
    close
}

fn draw_tab_bar(ui: &mut egui::Ui, th: &Theme, st: &mut UiState, x_range: egui::Rangef) {
    // 언더라인 탭 (디자인 remote_tool.jsx TabBtn): 전체폭 bg-sidebar(mantle), height 35,
    // padding L8 / TabBtn padding 0 13 / gap 2. 활성 = text-primary + 하단 2px accent,
    // 비활성 = text-muted. 좌표를 직접 계산해 그린다(egui 자동 배치 우회).
    let tab_h = 36.0; // 디자인 TabBtn 35 + borderBottom 1 = 탭바 컨테이너 36
    let pad_l = 8.0; // 디자인 탭바 padding-left
    let pad_x = 13.0; // 디자인 TabBtn padding 0 13
    let gap = 2.0; // 디자인 탭바 gap
    let font = egui::FontId::proportional(th.font_size_body.value());

    let top = ui.cursor().top();
    let bar = egui::Rect::from_min_size(
        egui::pos2(x_range.min, top),
        egui::vec2(x_range.span(), tab_h),
    );
    // bg-sidebar 전체폭 + 하단 borderBottom separator (mantle 위 → surface1 근사).
    ui.painter().rect_filled(bar, 0.0, th.bg_sidebar());
    ui.painter().hline(
        x_range,
        bar.max.y,
        egui::Stroke::new(th.border_width.value(), th.border_strong()),
    );

    let mut x = x_range.min + pad_l;
    for (tab, key) in [
        (Tab::Profiles, "remote_tool.tab_profiles"),
        (Tab::Attach, "remote_tool.tab_attach"),
        (Tab::Passkeys, "remote_tool.tab_passkeys"),
    ] {
        let on = st.tab == tab;
        let label = t(key);
        let text_w = ui.fonts(|f| {
            f.layout_no_wrap(label.to_string(), font.clone(), th.text_primary().into())
                .size()
                .x
        });
        let w = text_w + pad_x * 2.0;
        let rect = egui::Rect::from_min_size(egui::pos2(x, top), egui::vec2(w, tab_h));
        let resp = ui.interact(rect, ui.id().with((key, "rt_tab")), egui::Sense::click());
        if resp.hovered() && !on {
            ui.painter()
                .rect_filled(rect, 0.0, th.hover_overlay.to_egui_premultiplied());
        }
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            label,
            font.clone(),
            if on {
                th.text_primary().into()
            } else {
                th.text_muted().into()
            },
        );
        // 활성 탭 하단 accent bar — separator 위에 그려 덮는다. 굵기는 다른 탭
        // 지표(explorer/preset)와 같은 `tab_indicator_width`, 덮을 separator 두께는
        // `border_width`. 둘 다 얇은 구조선이라 zoom 을 타지 않는다(값 불변).
        if on {
            ui.painter().hline(
                rect.x_range(),
                bar.max.y - th.border_width.value(),
                egui::Stroke::new(th.tab_indicator_width.value(), th.accent_primary()),
            );
        }
        if resp.clicked() && st.tab != tab {
            st.tab = tab;
            st.profile_view = Sub::List;
            st.attach_view = Sub::List;
            st.passkey_view = Sub::List;
            st.perr = None;
            st.aerr = None;
            st.kerr = None;
        }
        x += w + gap;
    }
    // 탭바 영역만큼 커서 전진 → 다음 구역(콘텐츠)이 그 아래로.
    ui.allocate_rect(bar, egui::Sense::hover());
}

// ── 경고 배지 ────────────────────────────────────────────────────────────
fn warn_badge(ui: &mut egui::Ui, th: &Theme, text: &str, tooltip: &str) {
    selectable_label(
        ui,
        &format!("⚠ {text}"),
        th.accent_warning(),
        th.font_size_caption.value(),
        false,
    )
    .on_hover_text(tooltip);
}

// ════════════════════════════════════════════════════════════════════════
// TAB A — 원격 접속 프로필
// ════════════════════════════════════════════════════════════════════════
fn draw_profiles_tab(
    ui: &mut egui::Ui,
    th: &Theme,
    ctx: &egui::Context,
    st: &mut UiState,
    profiles: &mut RemoteProfiles,
    passkeys: &Passkeys,
) {
    match st.profile_view.clone() {
        Sub::List => draw_profile_list(ui, th, st, profiles, passkeys),
        Sub::Form => draw_profile_form(ui, th, ctx, st, profiles, passkeys),
        Sub::ConfirmDelete(name) => {
            if let Some(act) =
                draw_confirm_delete(ui, th, t("remote_tool.noun_profile"), &name, None)
            {
                if act {
                    profiles.remove(&name);
                    if let Err(e) = profiles.save() {
                        tracing::warn!("remote profile 삭제 후 저장 실패: {e}");
                    }
                }
                st.profile_view = Sub::List;
            }
        }
    }
}

fn draw_profile_list(
    ui: &mut egui::Ui,
    th: &Theme,
    st: &mut UiState,
    profiles: &RemoteProfiles,
    passkeys: &Passkeys,
) {
    let ctx = ui.ctx().clone();
    let protocols = protocol_set(&profiles.profiles);
    let applied_hidden = read_filter(&ctx);

    // add-bar: 좌측 Add + (프로토콜 2종 이상이면) 우측 정렬 프로토콜 필터 버튼.
    let mut add_clicked = false;
    let mut new_filter: Option<HashSet<String>> = None;
    ui.horizontal(|ui| {
        add_clicked = secondary_button(ui, th, t("remote_tool.profile_add")).clicked();
        if protocols.len() >= 2 {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                new_filter = draw_protocol_filter(ui, th, st, &protocols, &applied_hidden);
            });
        }
    });
    if let Some(h) = &new_filter {
        write_filter(&ctx, h.clone());
    }
    // 이 프레임에 Apply 됐으면 즉시 반영된 집합으로 목록을 그린다.
    let applied_hidden = new_filter.unwrap_or(applied_hidden);

    if add_clicked {
        st.pform = ProfileForm {
            kind: "ssh".into(),
            shell: "auto".into(),
            ..Default::default()
        };
        st.perr = None;
        st.profile_view = Sub::Form;
        return;
    }
    ui.add_space(th.spacing_xs.value());
    // tasty-attach kind 는 Attach 탭 전담 — 이 목록/빈 상태 판정에서 제외.
    let has_non_attach = profiles
        .profiles
        .iter()
        .any(|p| p.kind.trim() != ATTACH_KIND);
    // 필터로 전부 가려졌으면 "프로필 없음" 과 구분되는 별도 빈 상태.
    let any_visible = profiles
        .profiles
        .iter()
        .any(|p| p.kind.trim() != ATTACH_KIND && !applied_hidden.contains(p.kind.trim()));
    let detecting = st.detecting.as_ref().map(|j| j.name.clone());
    let known: Vec<String> = passkeys.passkeys.iter().map(|k| k.name.clone()).collect();
    // 로컬 ssh config 는 popup 을 열 때 1 회만 읽는다(캐시 주석 참조). 캐시를
    // 복제하지 않고 빌려 쓴다 — 아래 클로저는 `st` 를 잡지 않으므로 이 대여가
    // 클로저 밖까지 살아 있을 필요가 없다.
    let local = st.local.get_or_insert_with(load_local_ssh);
    let mut action: Option<(usize, ProfileRowAction)> = None;
    let mut local_action: Option<LocalRowAction> = None;
    // 두 섹션이 한 스크롤을 공유한다 — 로컬 섹션이 프로필 목록 **아래**에 이어지는
    // 목업 배치라, 스크롤을 나누면 프로필이 길 때 로컬 섹션에 닿을 수 없다.
    scroll_list_with_fade(ui, th, |ui| {
        if let Some(key) = profile_empty_key(has_non_attach, any_visible) {
            // 빈 상태에서도 return 하지 않는다 — 로컬 섹션은 계속 보여야 "아직 안 올린
            // 호스트가 여기 있다" 를 알 수 있다(이 화면의 용건).
            let msg = t(key);
            ui.vertical_centered(|ui| {
                ui.add_space(th.spacing_lg.value());
                selectable_text(
                    ui,
                    msg,
                    th.text_muted(),
                    th.font_size_body.value(),
                    false,
                    true,
                    TextWrap::None,
                );
                ui.add_space(th.spacing_lg.value());
            });
        } else {
            for (i, p) in profiles.profiles.iter().enumerate() {
                if p.kind.trim() == ATTACH_KIND || applied_hidden.contains(p.kind.trim()) {
                    continue;
                }
                if let Some(a) = draw_profile_row(ui, th, p, detecting.as_deref(), &known) {
                    action = Some((i, a));
                }
            }
        }
        local_action = draw_local_ssh_section(ui, th, local, profiles);
    });
    match local_action {
        Some(LocalRowAction::Reload) => {
            st.local = Some(load_local_ssh());
            return;
        }
        Some(LocalRowAction::Import(alias)) => {
            st.pform = import_prefill(&alias);
            st.perr = None;
            st.profile_view = Sub::Form;
            return;
        }
        None => {}
    }
    if let Some((i, a)) = action {
        let p = &profiles.profiles[i];
        match a {
            ProfileRowAction::Edit => {
                st.pform = form_from_profile(p, passkeys);
                st.perr = None;
                st.profile_view = Sub::Form;
            }
            ProfileRowAction::Delete => {
                st.profile_view = Sub::ConfirmDelete(p.name.clone());
            }
            ProfileRowAction::Redetect => {
                if st.detecting.is_none() {
                    st.detecting = Some(spawn_detect(ui.ctx(), p.name.clone()));
                }
            }
        }
    }
}

/// 로컬 ssh config 섹션 — tasty 프로필 목록 **아래**에 구분선으로 갈라 붙인다.
///
/// 여기 나열되는 것은 tasty 가 소유한 레코드가 아니라 사용자의 `~/.ssh/config` 다.
/// 그래서 **읽기 전용**이고 행 액션은 가져오기 하나뿐이다 — 편집/삭제는 사용자 자산을
/// tasty 가 고치는 일이라 범위 밖이다.
///
/// **프로토콜 필터를 적용받지 않는다.** 필터는 프로필의 `kind` 집합으로 만들어지는데
/// ssh config 항목에는 kind 라는 개념 자체가 없다. 필터로 프로필이 전부 가려진
/// 상태에서도 이 섹션은 그대로 남는다.
fn draw_local_ssh_section(
    ui: &mut egui::Ui,
    th: &Theme,
    local: &LocalSshCache,
    profiles: &RemoteProfiles,
) -> Option<LocalRowAction> {
    let mut out = None;
    hsep(ui, th);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = th.spacing_sm.value();
        selectable_label(
            ui,
            t("remote_tool.local_ssh_heading"),
            th.text_secondary(),
            th.font_size_caption.value(),
            false,
        );
        selectable_label(
            ui,
            &local.path,
            th.text_muted(),
            th.font_size_caption.value(),
            true,
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // 프로필 행의 재감지와 같은 글리프지만 하는 일이 다르다(원격 프로브가
            // 아니라 로컬 파일 재로드) — 툴팁으로 가른다.
            if ui
                .add(
                    egui::ImageButton::new(icons::REFRESH.image(
                        th.icon_glyph_size_row_action.value(),
                        th.text_muted().into(),
                    ))
                    .frame(false),
                )
                .on_hover_text(t("remote_tool.local_ssh_refresh"))
                .clicked()
            {
                out = Some(LocalRowAction::Reload);
            }
        });
    });
    ui.add_space(th.spacing_xs.value());
    if local.hosts.is_empty() {
        // 섹션을 통째로 숨기지 않는다 — 문구가 있어야 "가져올 게 없다" 와 "그런 기능이
        // 없다" 가 구분된다.
        selectable_text(
            ui,
            t(local_ssh_empty_key(local)),
            th.text_muted(),
            th.font_size_caption.value(),
            false,
            true,
            TextWrap::None,
        );
        ui.add_space(th.spacing_xs.value());
        return out;
    }
    for h in &local.hosts {
        if let Some(a) = draw_local_ssh_row(ui, th, h, imported_as(profiles, &h.alias)) {
            out = Some(a);
        }
    }
    out
}

/// alias 행 한 줄 — 이름 / hint caption / 우측 가져오기.
fn draw_local_ssh_row(
    ui: &mut egui::Ui,
    th: &Theme,
    h: &SshConfigHost,
    imported: Option<&str>,
) -> Option<LocalRowAction> {
    let mut out = None;
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = STRUCT_GAP_1.value();
            selectable_label(
                ui,
                &h.alias,
                th.text_primary(),
                th.font_size_body.value(),
                false,
            );
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = th.spacing_sm.value();
                selectable_label(
                    ui,
                    &local_target_hint(h),
                    th.text_muted(),
                    th.font_size_caption.value(),
                    true,
                );
                if let Some(name) = imported {
                    selectable_label(
                        ui,
                        &t_fmt("remote_tool.local_ssh_imported", name),
                        th.text_muted(),
                        th.font_size_caption.value(),
                        false,
                    );
                }
            });
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = th.spacing_xs.value();
            // 이미 가져온 alias 는 비활성 — 같은 호스트를 두 번 등록하는 사고를 막는다.
            let btn = ui.add_enabled(
                imported.is_none(),
                egui::ImageButton::new(icons::DOWNLOAD.image(
                    th.icon_glyph_size_row_action.value(),
                    th.text_muted().into(),
                ))
                .frame(false),
            );
            if imported.is_none()
                && btn
                    .on_hover_text(t("remote_tool.local_ssh_import"))
                    .clicked()
            {
                out = Some(LocalRowAction::Import(h.alias.clone()));
            }
        });
    });
    ui.add_space(th.spacing_xs.value());
    out
}

/// 프로토콜 필터 버튼(funnel + 라벨). filtered 면 primary(accent), 아니면 secondary.
fn filter_button(ui: &mut egui::Ui, th: &Theme, label: &str, filtered: bool) -> egui::Response {
    let text_col: egui::Color32 = if filtered {
        th.text_on_accent().into()
    } else {
        th.text_primary().into()
    };
    let fill: egui::Color32 = if filtered {
        th.accent_primary().into()
    } else {
        th.surface_raised().into()
    };
    let stroke = if filtered {
        egui::Stroke::NONE
    } else {
        egui::Stroke::new(th.border_width.value(), th.border_strong())
    };
    ui.add(
        egui::Button::image_and_text(
            icons::FUNNEL.image(th.icon_glyph_size_sm.value(), text_col),
            egui::RichText::new(label)
                .color(text_col)
                .size(th.font_size_body.value()),
        )
        .fill(fill)
        .stroke(stroke),
    )
}

/// 프로토콜 필터 버튼 + 드롭다운(체크박스 목록 + 모두선택/모두해제/초기화/적용).
/// Apply-on-confirm: 패널 편집은 `st.filter_draft` 에만 쌓이고 Apply 눌러야 반영.
/// Apply 시 `draft ∩ protocols` 로 보정한 새 hidden 집합을 반환(없으면 None).
fn draw_protocol_filter(
    ui: &mut egui::Ui,
    th: &Theme,
    st: &mut UiState,
    protocols: &[String],
    applied_hidden: &HashSet<String>,
) -> Option<HashSet<String>> {
    let popup_id = egui::Id::new(FILTER_POPUP_ID);
    let total = protocols.len();
    let active_hidden = protocols
        .iter()
        .filter(|p| applied_hidden.contains(*p))
        .count();
    let filtered = active_hidden > 0;
    let selected = total - active_hidden;
    let label = if filtered {
        format!("{} · {}/{}", t("remote_tool.filter"), selected, total)
    } else {
        t("remote_tool.filter").to_string()
    };

    let btn = filter_button(ui, th, &label, filtered);
    if btn.clicked() {
        // 열릴 때 draft 를 현재 적용 집합으로 시드.
        if !ui.memory(|m| m.is_popup_open(popup_id)) {
            st.filter_draft = applied_hidden.clone();
        }
        ui.memory_mut(|m| m.toggle_popup(popup_id));
    }

    let mut applied: Option<HashSet<String>> = None;
    egui::popup::popup_above_or_below_widget(
        ui,
        popup_id,
        &btn,
        egui::AboveOrBelow::Below,
        egui::PopupCloseBehavior::CloseOnClickOutside,
        |ui| {
            ui.set_min_width(FILTER_DROPDOWN_MIN_WIDTH.value());
            selectable_label(
                ui,
                t("remote_tool.filter_title"),
                th.text_muted(),
                th.font_size_caption.value(),
                true,
            );
            ui.add_space(th.spacing_xs.value());
            egui::ScrollArea::vertical()
                .max_height(FILTER_DROPDOWN_MAX_HEIGHT.value())
                .show(ui, |ui| {
                    for proto in protocols {
                        ui.horizontal(|ui| {
                            // draft 는 제외 집합 → checked = 미제외.
                            let mut checked = !st.filter_draft.contains(proto);
                            if tasty_ui_widgets::checkbox(ui, th, &mut checked, proto, true)
                                .changed()
                            {
                                if checked {
                                    st.filter_draft.remove(proto);
                                } else {
                                    st.filter_draft.insert(proto.clone());
                                }
                            }
                            if is_unknown_kind(proto) {
                                warn_badge(
                                    ui,
                                    th,
                                    t("remote_tool.filter_unknown"),
                                    t("remote_tool.type_unknown_hint"),
                                );
                            }
                        });
                    }
                });
            hsep(ui, th);
            // 일괄 조작: 모두 선택(=빈 제외) / 모두 해제(=전체 제외).
            ui.horizontal(|ui| {
                if ghost_button(ui, th, t("remote_tool.filter_select_all")).clicked() {
                    st.filter_draft.clear();
                }
                if ghost_button(ui, th, t("remote_tool.filter_deselect_all")).clicked() {
                    st.filter_draft = protocols.iter().cloned().collect();
                }
            });
            // 초기화(=전체 선택) / 적용.
            ui.horizontal(|ui| {
                if ghost_button(ui, th, t("remote_tool.filter_reset")).clicked() {
                    st.filter_draft.clear();
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if primary_button(ui, th, t("remote_tool.filter_apply")).clicked() {
                        applied = Some(
                            st.filter_draft
                                .iter()
                                .filter(|p| protocols.iter().any(|x| x == *p))
                                .cloned()
                                .collect(),
                        );
                    }
                });
            });
        },
    );
    // 드롭다운이 popup_rect 밖으로 삐져나가도 그 위 클릭이 outside-click 으로
    // 오판되지 않도록 실측 rect 를 매니저에 보고(닫혀 있으면 None 으로 정리).
    // remote_tool 은 현재 close_on_outside_click=false 라 증상이 드러나지 않지만,
    // 구조적으로 port_scanner 와 동일한 결함을 갖고 있어 함께 고쳐둔다.
    let overlay_rect = ui
        .memory(|m| m.is_popup_open(popup_id))
        .then(|| ui.memory(|m| m.area_rect(popup_id)))
        .flatten();
    super::report_child_overlay_rect(
        ui.ctx(),
        REMOTE_TOOL_POPUP_ID,
        FILTER_POPUP_ID,
        overlay_rect,
    );
    if applied.is_some() {
        ui.memory_mut(|m| m.close_popup());
    }
    applied
}

enum ProfileRowAction {
    Edit,
    Delete,
    Redetect,
}

fn draw_profile_row(
    ui: &mut egui::Ui,
    th: &Theme,
    p: &RemoteProfile,
    detecting: Option<&str>,
    passkey_names: &[String],
) -> Option<ProfileRowAction> {
    let mut out = None;
    let ssh = p.as_ssh();
    let is_ssh = ssh.is_some();
    let disabled = ssh.as_ref().map(|v| v.is_disabled()).unwrap_or(false);
    let detecting_now = detecting == Some(p.name.as_str());
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = STRUCT_GAP_1.value();
            // row1: name + type badge
            ui.horizontal(|ui| {
                let title = match &p.label {
                    Some(l) if !l.is_empty() => format!("{}  ({})", p.name, l),
                    _ => p.name.clone(),
                };
                selectable_label(
                    ui,
                    &title,
                    // divergence: overlay0=disabled-role 이나 값은 placeholder(neutral-600), 코드값 보존
                    if disabled {
                        th.text_placeholder()
                    } else {
                        th.text_primary()
                    },
                    th.font_size_body.value(),
                    false,
                );
                if is_builtin_kind(&p.kind) || KNOWN_TYPES.contains(&p.kind.as_str()) {
                    selectable_label(
                        ui,
                        &p.kind,
                        th.text_muted(),
                        th.font_size_caption.value(),
                        false,
                    );
                } else {
                    warn_badge(ui, th, &p.kind, t("remote_tool.type_unknown_hint"));
                }
            });
            // row2: target summary
            selectable_label(
                ui,
                &profile_summary(p),
                th.text_muted(),
                th.font_size_caption.value(),
                true,
            );
            // row3: passkey + (ssh) shell/state
            ui.horizontal(|ui| {
                match &p.passkey_ref {
                    Some(pr) if !pr.is_empty() => {
                        selectable_label(
                            ui,
                            &format!("passkey: {pr}"),
                            th.text_muted(),
                            th.font_size_caption.value(),
                            false,
                        );
                        if !passkey_names.contains(pr) {
                            warn_badge(
                                ui,
                                th,
                                t("remote_tool.passkey_missing"),
                                t("remote_tool.passkey_missing_hint"),
                            );
                        }
                    }
                    _ => {
                        selectable_label(
                            ui,
                            "passkey: —",
                            th.text_muted(),
                            th.font_size_caption.value(),
                            false,
                        );
                    }
                }
                if let Some(v) = &ssh {
                    selectable_label(
                        ui,
                        &format!("shell: {}", v.shell()),
                        th.text_muted(),
                        th.font_size_caption.value(),
                        false,
                    );
                    if detecting_now {
                        ui.add(egui::Spinner::new().size(th.font_size_caption.value()));
                        selectable_label(
                            ui,
                            t("remote_tool.detecting"),
                            th.text_muted(),
                            th.font_size_caption.value(),
                            false,
                        );
                    } else if disabled {
                        selectable_label(
                            ui,
                            t("remote_tool.detect_failed"),
                            th.accent_danger(),
                            th.font_size_caption.value(),
                            false,
                        );
                    }
                }
            });
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = th.spacing_xs.value();
            // 아이콘 버튼 (디자인 IconButton): delete / edit / re-detect.
            // right_to_left 이라 추가 순서 = 우→좌. 디자인 우측 끝이 trash.
            if ui
                .add(
                    egui::ImageButton::new(icons::TRASH.image(
                        th.icon_glyph_size_row_action.value(),
                        th.text_muted().into(),
                    ))
                    .frame(false),
                )
                .on_hover_text(t("remote_tool.delete"))
                .clicked()
            {
                out = Some(ProfileRowAction::Delete);
            }
            if ui
                .add(
                    egui::ImageButton::new(icons::EDIT.image(
                        th.icon_glyph_size_row_action.value(),
                        th.text_muted().into(),
                    ))
                    .frame(false),
                )
                .on_hover_text(t("remote_tool.edit"))
                .clicked()
            {
                out = Some(ProfileRowAction::Edit);
            }
            if is_ssh
                && ui
                    .add_enabled(
                        !detecting_now,
                        egui::ImageButton::new(icons::REFRESH.image(
                            th.icon_glyph_size_row_action.value(),
                            th.text_muted().into(),
                        ))
                        .frame(false),
                    )
                    .on_hover_text(t("remote_tool.refresh_tooltip"))
                    .clicked()
            {
                out = Some(ProfileRowAction::Redetect);
            }
        });
    });
    hsep(ui, th);
    out
}

fn profile_summary(p: &RemoteProfile) -> String {
    if let Some(v) = p.as_ssh() {
        let mut s = v.ssh_destination();
        if let Some(port) = v.port()
            && port != 22
        {
            s = format!("{s}:{port}");
        }
        s
    } else {
        let parts: Vec<String> = p
            .fields
            .iter()
            .filter_map(|(k, val)| val.as_str().map(|v| format!("{k}={v}")))
            .take(2)
            .collect();
        if parts.is_empty() {
            "—".into()
        } else {
            parts.join("  ")
        }
    }
}

fn form_from_profile(p: &RemoteProfile, _passkeys: &Passkeys) -> ProfileForm {
    let mut f = ProfileForm {
        kind: p.kind.clone(),
        name: p.name.clone(),
        label: p.label.clone().unwrap_or_default(),
        passkey_ref: p.passkey_ref.clone().unwrap_or_default(),
        editing_original: Some(p.name.clone()),
        shell: "auto".into(),
        ..Default::default()
    };
    if let Some(v) = p.as_ssh() {
        f.host = v.host().unwrap_or("").to_string();
        f.user = v.user().unwrap_or("").to_string();
        f.port = v.port().map(|n| n.to_string()).unwrap_or_default();
        f.shell = v.shell().to_string();
    } else {
        f.fields = p
            .fields
            .iter()
            .filter_map(|(k, val)| val.as_str().map(|v| (k.clone(), v.to_string())))
            .collect();
    }
    f
}

fn draw_profile_form(
    ui: &mut egui::Ui,
    th: &Theme,
    ctx: &egui::Context,
    st: &mut UiState,
    profiles: &mut RemoteProfiles,
    passkeys: &Passkeys,
) {
    // 디자인 ProfileForm 구조 = rtScrollPad(flex:1 스크롤 본문) + rtFooter(flex:none, 패널
    // 하단 고정 borderTop). 외곽 content Frame margin 은 폼일 때 0 이라(상위 draw 분기)
    // 이 함수가 패딩(좌우 space-lg 16)과 전체폭 separator 를 직접 소유한다.
    let full_x = ui.clip_rect().x_range();
    let sep = egui::Stroke::new(th.border_width.value(), th.border_strong());
    let pad_lg = th.spacing_lg.value() as i8;
    let pad_md = th.spacing_md.value() as i8;

    // ── footer (rtFooter — 하단 고정, padding space-md/space-lg, [Cancel ghost][Save primary]) ──
    let mut do_save = false;
    let mut do_cancel = false;
    let footer = egui::TopBottomPanel::bottom("remote_tool.profile_footer")
        .resizable(false)
        .show_separator_line(false)
        .frame(egui::Frame::NONE.inner_margin(egui::Margin {
            left: pad_lg,
            right: pad_lg,
            top: pad_md,
            bottom: pad_md,
        }))
        .show_inside(ui, |ui| {
            // right_to_left: 먼저 추가한 위젯이 우측 끝 → Save(우측), 그 왼쪽에 Cancel.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if primary_button(ui, th, t("remote_tool.save")).clicked() {
                    do_save = true;
                }
                ui.add_space(th.spacing_sm.value());
                if ghost_button(ui, th, t("remote_tool.cancel")).clicked() {
                    do_cancel = true;
                }
            });
        });
    // borderTop — footer div 전체폭(팝업 전체폭) separator.
    ui.painter()
        .hline(full_x, footer.response.rect.top() + 0.5, sep);

    // ── 스크롤 본문 (rtScrollPad — flex:1 로 가용 높이를 채워 footer 를 하단에 고정) ──
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE)
        .show_inside(ui, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    egui::Frame::NONE
                        .inner_margin(egui::Margin {
                            left: pad_lg,
                            right: pad_lg,
                            top: pad_md,
                            bottom: pad_md,
                        })
                        .show(ui, |ui| {
                            let editing = st.pform.editing_original.is_some();
                            selectable_label(
                                ui,
                                if editing {
                                    t("remote_tool.profile_form_edit")
                                } else {
                                    t("remote_tool.profile_form_add")
                                },
                                th.text_primary(),
                                th.font_size_body.value(),
                                false,
                            );
                            ui.add_space(th.spacing_md.value());

                            let f = &mut st.pform;
                            let is_ssh = f.kind.trim() == "ssh";
                            let unknown = !f.kind.trim().is_empty()
                                && !is_builtin_kind(f.kind.trim())
                                && !KNOWN_TYPES.contains(&f.kind.trim());

                            // 행 간 세로 간격 = 디자인 rowGap(space-sm 8) — 수동 2컬럼 행에 일괄.
                            ui.spacing_mut().item_spacing.y = th.spacing_sm.value();

                            // Type — 디자인은 datalist 단일 입력. egui 엔 datalist 가 없어 텍스트 입력 +
                            // 제안 콤보(▾) 2위젯이 기능 대체. 한 컨트롤처럼 붙여 그린다(내부 간격 spacing_xs).
                            form_row(ui, th, t("remote_tool.field_type"), |ui| {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = th.spacing_xs.value();
                                    let combo_w = th.item_height_interactive.value();
                                    let edit_w = (ui.available_width()
                                        - combo_w
                                        - ui.spacing().item_spacing.x)
                                        .max(0.0);
                                    ui.add(
                                        egui::TextEdit::singleline(&mut f.kind)
                                            .desired_width(edit_w)
                                            .font(egui::TextStyle::Monospace),
                                    );
                                    egui::ComboBox::from_id_salt("remote_tool.type_suggest")
                                        .selected_text("▾")
                                        .width(combo_w)
                                        .show_ui(ui, |ui| {
                                            for kt in KNOWN_TYPES {
                                                ui.selectable_value(
                                                    &mut f.kind,
                                                    (*kt).to_string(),
                                                    *kt,
                                                );
                                            }
                                        });
                                });
                            });
                            if unknown {
                                indented_hint(
                                    ui,
                                    th,
                                    t("remote_tool.type_unknown_hint"),
                                    th.accent_warning(),
                                    false,
                                );
                            }

                            if is_ssh {
                                // placeholder/mono 는 디자인 SSH_FIELDS 표(remote_tool.jsx).
                                text_row(
                                    ui,
                                    th,
                                    t("remote_tool.field_name"),
                                    &mut f.name,
                                    "prod-web",
                                    false,
                                );
                                text_row(
                                    ui,
                                    th,
                                    t("remote_tool.field_host"),
                                    &mut f.host,
                                    "10.0.4.12",
                                    true,
                                );
                                text_row(
                                    ui,
                                    th,
                                    t("remote_tool.field_user"),
                                    &mut f.user,
                                    "deploy",
                                    false,
                                );
                                text_row(
                                    ui,
                                    th,
                                    t("remote_tool.field_port"),
                                    &mut f.port,
                                    "22",
                                    true,
                                );
                                text_row(
                                    ui,
                                    th,
                                    t("remote_tool.field_label"),
                                    &mut f.label,
                                    "us-east",
                                    false,
                                );
                                form_row(ui, th, t("remote_tool.field_shell"), |ui| {
                                    egui::ComboBox::from_id_salt("remote_tool.shell")
                                        .selected_text(f.shell.clone())
                                        .width(ui.available_width())
                                        .show_ui(ui, |ui| {
                                            for sh in SHELLS {
                                                ui.selectable_value(
                                                    &mut f.shell,
                                                    (*sh).to_string(),
                                                    *sh,
                                                );
                                            }
                                        });
                                });
                                passkey_dropdown_row(ui, th, &mut f.passkey_ref, passkeys);
                                if f.shell == "auto" {
                                    indented_hint(
                                        ui,
                                        th,
                                        t("remote_tool.shell_auto_hint"),
                                        th.text_muted(),
                                        false,
                                    );
                                }
                            } else {
                                text_row(
                                    ui,
                                    th,
                                    t("remote_tool.field_name"),
                                    &mut f.name,
                                    "media-nas",
                                    false,
                                );
                                text_row(
                                    ui,
                                    th,
                                    t("remote_tool.field_label"),
                                    &mut f.label,
                                    "lab",
                                    false,
                                );
                                passkey_dropdown_row(ui, th, &mut f.passkey_ref, passkeys);
                                ui.add_space(th.spacing_xs.value());
                                // Fields 헤더 — 좌측 mono caption 라벨 + 우측 ghost "Add field"(space-between).
                                ui.horizontal(|ui| {
                                    selectable_label(
                                        ui,
                                        t("remote_tool.fields_section"),
                                        th.text_muted(),
                                        th.font_size_caption.value(),
                                        true,
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if ghost_button(ui, th, t("remote_tool.field_add"))
                                                .clicked()
                                            {
                                                f.fields.push((String::new(), String::new()));
                                            }
                                        },
                                    );
                                });
                                if f.fields.is_empty() {
                                    indented_hint(
                                        ui,
                                        th,
                                        t("remote_tool.fields_empty"),
                                        th.text_muted(),
                                        true,
                                    );
                                }
                                let mut remove_idx = None;
                                for (i, (k, v)) in f.fields.iter_mut().enumerate() {
                                    // 디자인 generic 필드 행 grid `[112px 1fr control-height(28)]`, gap space-sm(8).
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = th.spacing_sm.value();
                                        ui.add(
                                            egui::TextEdit::singleline(k)
                                                .desired_width(LABEL_COL_WIDTH.value())
                                                .hint_text("key")
                                                .font(egui::TextStyle::Monospace),
                                        );
                                        let btn_w = th.item_height_interactive.value();
                                        let val_w = (ui.available_width()
                                            - btn_w
                                            - ui.spacing().item_spacing.x)
                                            .max(0.0);
                                        ui.add(
                                            egui::TextEdit::singleline(v)
                                                .desired_width(val_w)
                                                .hint_text("value")
                                                .font(egui::TextStyle::Monospace),
                                        );
                                        if ghost_button(ui, th, "×").clicked() {
                                            remove_idx = Some(i);
                                        }
                                    });
                                }
                                if let Some(i) = remove_idx {
                                    f.fields.remove(i);
                                }
                            }

                            if let Some(err) = &st.perr {
                                indented_hint(ui, th, err, th.accent_danger(), false);
                            }
                        });
                });
        });

    if do_cancel {
        st.perr = None;
        st.profile_view = Sub::List;
        return;
    }
    if do_save {
        match save_profile(ctx, st, profiles, passkeys) {
            Ok(()) => {
                st.perr = None;
                st.profile_view = Sub::List;
            }
            Err(e) => st.perr = Some(e),
        }
    }
}

fn passkey_dropdown_row(ui: &mut egui::Ui, th: &Theme, value: &mut String, passkeys: &Passkeys) {
    form_row(ui, th, t("remote_tool.field_passkey"), |ui| {
        let sel = if value.is_empty() {
            t("remote_tool.passkey_none").to_string()
        } else {
            value.clone()
        };
        // 디자인 PasskeySelect 는 block(1fr) — 잔여폭을 채운다.
        egui::ComboBox::from_id_salt("remote_tool.passkey_ref")
            .selected_text(sel)
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                ui.selectable_value(value, String::new(), t("remote_tool.passkey_none"));
                for k in &passkeys.passkeys {
                    ui.selectable_value(value, k.name.clone(), &k.name);
                }
            });
    });
}

fn save_profile(
    ctx: &egui::Context,
    st: &mut UiState,
    profiles: &mut RemoteProfiles,
    _passkeys: &Passkeys,
) -> Result<(), String> {
    let f = st.pform.clone();
    let kind = f.kind.trim();
    if kind.is_empty() {
        return Err(t("remote_tool.err_type_empty").to_string());
    }
    let name = f.name.trim();
    if name.is_empty() {
        return Err(t("remote_tool.err_name_empty").to_string());
    }
    let is_ssh = kind == "ssh";
    if is_ssh {
        if f.host.trim().is_empty() {
            return Err(t("remote_tool.err_host_empty").to_string());
        }
        if !f.port.trim().is_empty() && f.port.trim().parse::<u16>().is_err() {
            return Err(t("remote_tool.err_port_invalid").to_string());
        }
    }
    // 이름 중복(자기 자신 제외).
    if profiles
        .profiles
        .iter()
        .any(|p| p.name == name && Some(p.name.as_str()) != st.pform.editing_original.as_deref())
    {
        return Err(t("remote_tool.err_name_dup").to_string());
    }

    let mut p = RemoteProfile::new(name, kind);
    if !f.label.trim().is_empty() {
        p.label = Some(f.label.trim().to_string());
    }
    if !f.passkey_ref.is_empty() {
        p.passkey_ref = Some(f.passkey_ref.clone());
    }
    let mut needs_detect = false;
    if is_ssh {
        p.set_field("host", f.host.trim().to_string());
        if !f.user.trim().is_empty() {
            p.set_field("user", f.user.trim().to_string());
        }
        if !f.port.trim().is_empty() {
            p.set_field("port", f.port.trim().to_string());
        }
        let shell = if is_valid_shell(&f.shell) {
            f.shell.clone()
        } else {
            "auto".into()
        };
        p.set_field("shell", shell.clone());
        // 명시 셸 → port_mode 즉시 도출, auto → 저장 후 워커 감지.
        needs_detect = tasty_remote_profiles::shell_to_port_mode(&shell).is_none();
        if let Some(mode) = tasty_remote_profiles::shell_to_port_mode(&shell) {
            p.set_field("port_mode", mode);
        }
    } else {
        for (k, v) in &f.fields {
            if !k.trim().is_empty() {
                p.set_field(k.trim().to_string(), v.clone());
            }
        }
    }

    // rename: 원래 name 과 다르면 옛 항목 제거.
    if let Some(orig) = &st.pform.editing_original
        && orig != name
    {
        profiles.remove(orig);
    }
    profiles.upsert(p);
    profiles.save().map_err(|e| format!("save: {e}"))?;
    if needs_detect && st.detecting.is_none() {
        st.detecting = Some(spawn_detect(ctx, name.to_string()));
    }
    Ok(())
}

// ════════════════════════════════════════════════════════════════════════
// TAB B — Attach (tasty-attach 대상, 디자인 remote_tool.jsx TAB C 섹션)
// ════════════════════════════════════════════════════════════════════════
fn draw_attach_tab(
    ui: &mut egui::Ui,
    th: &Theme,
    st: &mut UiState,
    profiles: &mut RemoteProfiles,
    passkeys: &Passkeys,
) {
    match st.attach_view.clone() {
        Sub::List => draw_attach_list(ui, th, st, profiles),
        Sub::Form => draw_attach_form(ui, th, st, profiles, passkeys),
        Sub::ConfirmDelete(name) => {
            if let Some(act) =
                draw_confirm_delete(ui, th, t("remote_tool.noun_attach"), &name, None)
            {
                if act {
                    profiles.remove(&name);
                    if let Err(e) = profiles.save() {
                        tracing::warn!("attach 삭제 후 저장 실패: {e}");
                    }
                }
                st.attach_view = Sub::List;
            }
        }
    }
}

fn draw_attach_list(ui: &mut egui::Ui, th: &Theme, st: &mut UiState, profiles: &RemoteProfiles) {
    // add-bar: Add attach 만 — 프로토콜 필터 없음(디자인: Profiles 전용).
    if secondary_button(ui, th, t("remote_tool.attach_add")).clicked() {
        st.aform = AttachForm {
            mode_ref: true,
            shell: "auto".into(),
            remote_tasty: "tasty".into(),
            port_mode: "auto".into(),
            ..Default::default()
        };
        st.aerr = None;
        st.attach_view = Sub::Form;
        return;
    }
    ui.add_space(th.spacing_xs.value());
    let has_attach = profiles.profiles.iter().any(|p| p.kind == ATTACH_KIND);
    if !has_attach {
        ui.vertical_centered(|ui| {
            ui.add_space(th.spacing_lg.value());
            selectable_text(
                ui,
                t("remote_tool.attach_empty"),
                th.text_muted(),
                th.font_size_body.value(),
                false,
                true,
                TextWrap::None,
            );
        });
        return;
    }
    let mut action: Option<(String, AttachRowAction)> = None;
    scroll_list_with_fade(ui, th, |ui| {
        for p in profiles.profiles.iter().filter(|p| p.kind == ATTACH_KIND) {
            if let Some(a) = draw_attach_row(ui, th, p, profiles) {
                action = Some((p.name.clone(), a));
            }
        }
    });
    if let Some((name, a)) = action {
        match a {
            AttachRowAction::Edit => {
                if let Some(p) = profiles.get(&name) {
                    st.aform = form_from_attach(p);
                    st.aerr = None;
                    st.attach_view = Sub::Form;
                }
            }
            AttachRowAction::Delete => {
                st.attach_view = Sub::ConfirmDelete(name);
            }
        }
    }
}

enum AttachRowAction {
    Edit,
    Delete,
}

/// 디자인 AttachRow 전사 — row1 name+(label)+mode 태그+inactive 배지 / row2 target
/// 요약(+dangling ref 배지) / row3 tasty:·port: 캡션 / 우측 edit·delete 액션.
fn draw_attach_row(
    ui: &mut egui::Ui,
    th: &Theme,
    p: &RemoteProfile,
    profiles: &RemoteProfiles,
) -> Option<AttachRowAction> {
    let v = p.as_attach()?;
    // ref 모드: 참조 ssh 프로필을 resolve — 없으면 dangling(missing), 감지실패면
    // inactive. inline 모드: 자기 detect_failed 가 inactive. hard-error 없음.
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
    let mode_tag = if v.ssh_ref().is_some() {
        t("remote_tool.attach_tag_profile")
    } else {
        t("remote_tool.attach_tag_inline")
    };
    let mut out = None;
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = STRUCT_GAP_1.value();
            // row1: name + (label) + mode 태그 + inactive 배지
            ui.horizontal(|ui| {
                let title = match &p.label {
                    Some(l) if !l.is_empty() => format!("{}  ({})", p.name, l),
                    _ => p.name.clone(),
                };
                selectable_label(
                    ui,
                    &title,
                    if inactive {
                        th.text_disabled()
                    } else {
                        th.text_primary()
                    },
                    th.font_size_body.value(),
                    false,
                );
                selectable_label(
                    ui,
                    mode_tag,
                    th.text_muted(),
                    th.font_size_caption.value(),
                    false,
                );
                if inactive {
                    warn_badge(
                        ui,
                        th,
                        t("remote_tool.attach_inactive"),
                        t("remote_tool.attach_inactive_hint"),
                    );
                }
            });
            // row2: target 요약 + dangling ref 배지
            ui.horizontal(|ui| {
                selectable_label(
                    ui,
                    &target,
                    th.text_muted(),
                    th.font_size_caption.value(),
                    true,
                );
                if missing {
                    warn_badge(
                        ui,
                        th,
                        t("remote_tool.attach_profile_missing"),
                        t("remote_tool.attach_profile_missing_hint"),
                    );
                }
            });
            // row3: remote tasty + port mode 캡션
            ui.horizontal(|ui| {
                selectable_label(
                    ui,
                    &format!("tasty: {}", v.remote_tasty()),
                    th.text_muted(),
                    th.font_size_caption.value(),
                    false,
                );
                selectable_label(
                    ui,
                    &format!("port: {}", v.port_mode()),
                    th.text_muted(),
                    th.font_size_caption.value(),
                    false,
                );
            });
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = th.spacing_xs.value();
            // 아이콘 버튼 (디자인 IconButton): delete / edit. RTL 이라 우측 끝이 trash.
            if ui
                .add(
                    egui::ImageButton::new(icons::TRASH.image(
                        th.icon_glyph_size_row_action.value(),
                        th.text_muted().into(),
                    ))
                    .frame(false),
                )
                .on_hover_text(t("remote_tool.delete"))
                .clicked()
            {
                out = Some(AttachRowAction::Delete);
            }
            if ui
                .add(
                    egui::ImageButton::new(icons::EDIT.image(
                        th.icon_glyph_size_row_action.value(),
                        th.text_muted().into(),
                    ))
                    .frame(false),
                )
                .on_hover_text(t("remote_tool.edit"))
                .clicked()
            {
                out = Some(AttachRowAction::Edit);
            }
        });
    });
    hsep(ui, th);
    out
}

fn form_from_attach(p: &RemoteProfile) -> AttachForm {
    let Some(v) = p.as_attach() else {
        return AttachForm::default();
    };
    AttachForm {
        name: p.name.clone(),
        label: p.label.clone().unwrap_or_default(),
        mode_ref: v.ssh_ref().is_some(),
        ssh_ref: v.ssh_ref().unwrap_or("").to_string(),
        host: v.host().unwrap_or("").to_string(),
        user: v.user().unwrap_or("").to_string(),
        port: v.port().map(|n| n.to_string()).unwrap_or_default(),
        shell: v.shell().to_string(),
        passkey_ref: p.passkey_ref.clone().unwrap_or_default(),
        remote_tasty: v.remote_tasty().to_string(),
        port_mode: v.port_mode().to_string(),
        port_file: v.port_file().unwrap_or("").to_string(),
        editing_original: Some(p.name.clone()),
    }
}

fn draw_attach_form(
    ui: &mut egui::Ui,
    th: &Theme,
    st: &mut UiState,
    profiles: &mut RemoteProfiles,
    passkeys: &Passkeys,
) {
    // 디자인 AttachForm 구조 = rtScrollPad + rtFooter — 프로필 폼과 동일 골격.
    let full_x = ui.clip_rect().x_range();
    let sep = egui::Stroke::new(th.border_width.value(), th.border_strong());
    let pad_lg = th.spacing_lg.value() as i8;
    let pad_md = th.spacing_md.value() as i8;

    let mut do_save = false;
    let mut do_cancel = false;
    let footer = egui::TopBottomPanel::bottom("remote_tool.attach_footer")
        .resizable(false)
        .show_separator_line(false)
        .frame(egui::Frame::NONE.inner_margin(egui::Margin {
            left: pad_lg,
            right: pad_lg,
            top: pad_md,
            bottom: pad_md,
        }))
        .show_inside(ui, |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if primary_button(ui, th, t("remote_tool.save")).clicked() {
                    do_save = true;
                }
                ui.add_space(th.spacing_sm.value());
                if ghost_button(ui, th, t("remote_tool.cancel")).clicked() {
                    do_cancel = true;
                }
            });
        });
    ui.painter()
        .hline(full_x, footer.response.rect.top() + 0.5, sep);

    egui::CentralPanel::default()
        .frame(egui::Frame::NONE)
        .show_inside(ui, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    egui::Frame::NONE
                        .inner_margin(egui::Margin {
                            left: pad_lg,
                            right: pad_lg,
                            top: pad_md,
                            bottom: pad_md,
                        })
                        .show(ui, |ui| {
                            let editing = st.aform.editing_original.is_some();
                            selectable_label(
                                ui,
                                if editing {
                                    t("remote_tool.attach_form_edit")
                                } else {
                                    t("remote_tool.attach_form_add")
                                },
                                th.text_primary(),
                                th.font_size_body.value(),
                                false,
                            );
                            ui.add_space(th.spacing_md.value());

                            let f = &mut st.aform;
                            // 행 간 세로 간격 = 디자인 rowGap(space-sm 8).
                            ui.spacing_mut().item_spacing.y = th.spacing_sm.value();

                            text_row(
                                ui,
                                th,
                                t("remote_tool.field_name"),
                                &mut f.name,
                                "gb10",
                                false,
                            );
                            text_row(
                                ui,
                                th,
                                t("remote_tool.field_label"),
                                &mut f.label,
                                "us-east",
                                false,
                            );
                            // Connection — 디자인 세그먼트 토글 (ref ↔ inline).
                            form_row(ui, th, t("remote_tool.field_connection"), |ui| {
                                let selected = if f.mode_ref { 0 } else { 1 };
                                if let Some(i) = tasty_ui_widgets::segmented(
                                    ui,
                                    th,
                                    &[
                                        t("remote_tool.attach_mode_ref"),
                                        t("remote_tool.attach_mode_inline"),
                                    ],
                                    selected,
                                ) {
                                    f.mode_ref = i == 0;
                                }
                            });
                            ui.add_space(th.spacing_xs.value());

                            if f.mode_ref {
                                // ssh 프로필 참조 드롭다운 — ssh kind 만 나열.
                                form_row(ui, th, t("remote_tool.field_ssh_ref"), |ui| {
                                    let sel = if f.ssh_ref.is_empty() {
                                        t("remote_tool.ssh_ref_none").to_string()
                                    } else {
                                        f.ssh_ref.clone()
                                    };
                                    egui::ComboBox::from_id_salt("remote_tool.attach_ssh_ref")
                                        .selected_text(sel)
                                        .width(ui.available_width())
                                        .show_ui(ui, |ui| {
                                            for sp in profiles
                                                .profiles
                                                .iter()
                                                .filter(|sp| sp.kind == "ssh")
                                            {
                                                let display = match &sp.label {
                                                    Some(l) if !l.is_empty() => {
                                                        format!("{} ({})", sp.name, l)
                                                    }
                                                    _ => sp.name.clone(),
                                                };
                                                ui.selectable_value(
                                                    &mut f.ssh_ref,
                                                    sp.name.clone(),
                                                    display,
                                                );
                                            }
                                        });
                                });
                            } else {
                                // 인라인 ssh 필드셋 — ssh 프로필 폼과 동일 구성.
                                text_row(
                                    ui,
                                    th,
                                    t("remote_tool.field_host"),
                                    &mut f.host,
                                    "10.0.4.12",
                                    true,
                                );
                                text_row(
                                    ui,
                                    th,
                                    t("remote_tool.field_user"),
                                    &mut f.user,
                                    "deploy",
                                    false,
                                );
                                text_row(
                                    ui,
                                    th,
                                    t("remote_tool.field_port"),
                                    &mut f.port,
                                    "22",
                                    true,
                                );
                                form_row(ui, th, t("remote_tool.field_shell"), |ui| {
                                    egui::ComboBox::from_id_salt("remote_tool.attach_shell")
                                        .selected_text(f.shell.clone())
                                        .width(ui.available_width())
                                        .show_ui(ui, |ui| {
                                            for sh in SHELLS {
                                                ui.selectable_value(
                                                    &mut f.shell,
                                                    (*sh).to_string(),
                                                    *sh,
                                                );
                                            }
                                        });
                                });
                                passkey_dropdown_row(ui, th, &mut f.passkey_ref, passkeys);
                            }

                            // Remote tasty 그룹 — 모드 무관 공통 (디자인 mono caps 헤더).
                            ui.add_space(th.spacing_xs.value());
                            selectable_label(
                                ui,
                                t("remote_tool.remote_tasty_section"),
                                th.text_muted(),
                                th.font_size_caption.value(),
                                true,
                            );
                            text_row(
                                ui,
                                th,
                                t("remote_tool.field_executable"),
                                &mut f.remote_tasty,
                                "tasty",
                                true,
                            );
                            form_row(ui, th, t("remote_tool.field_port_mode"), |ui| {
                                egui::ComboBox::from_id_salt("remote_tool.attach_port_mode")
                                    .selected_text(f.port_mode.clone())
                                    .width(ui.available_width())
                                    .show_ui(ui, |ui| {
                                        for m in PORT_MODES {
                                            ui.selectable_value(
                                                &mut f.port_mode,
                                                (*m).to_string(),
                                                *m,
                                            );
                                        }
                                    });
                            });
                            text_row(
                                ui,
                                th,
                                t("remote_tool.field_port_file"),
                                &mut f.port_file,
                                t("remote_tool.attach_port_file_ph"),
                                true,
                            );
                            indented_hint(
                                ui,
                                th,
                                t("remote_tool.attach_exec_hint"),
                                th.text_muted(),
                                false,
                            );

                            if let Some(err) = &st.aerr {
                                indented_hint(ui, th, err, th.accent_danger(), false);
                            }
                        });
                });
        });

    if do_cancel {
        st.aerr = None;
        st.attach_view = Sub::List;
        return;
    }
    if do_save {
        match save_attach(st, profiles) {
            Ok(()) => {
                st.aerr = None;
                st.attach_view = Sub::List;
            }
            Err(e) => st.aerr = Some(e),
        }
    }
}

fn save_attach(st: &mut UiState, profiles: &mut RemoteProfiles) -> Result<(), String> {
    let f = st.aform.clone();
    let name = f.name.trim();
    if name.is_empty() {
        return Err(t("remote_tool.err_name_empty").to_string());
    }
    if f.mode_ref {
        if f.ssh_ref.is_empty() {
            return Err(t("remote_tool.err_ssh_ref_empty").to_string());
        }
    } else {
        if f.host.trim().is_empty() {
            return Err(t("remote_tool.err_host_empty").to_string());
        }
        if !f.port.trim().is_empty() && f.port.trim().parse::<u16>().is_err() {
            return Err(t("remote_tool.err_port_invalid").to_string());
        }
    }
    // 이름 중복 — 같은 레지스트리를 쓰므로 attach 뿐 아니라 전체 프로필과 겹치면 안 된다
    // (`RemoteProfiles::upsert` 가 name 전역 교체 시맨틱).
    if profiles
        .profiles
        .iter()
        .any(|p| p.name == name && Some(p.name.as_str()) != f.editing_original.as_deref())
    {
        return Err(t("remote_tool.err_name_dup").to_string());
    }

    let mut p = RemoteProfile::new(name, ATTACH_KIND);
    if !f.label.trim().is_empty() {
        p.label = Some(f.label.trim().to_string());
    }
    if f.mode_ref {
        p.set_field("ssh_ref", f.ssh_ref.clone());
    } else {
        p.set_field("host", f.host.trim().to_string());
        if !f.user.trim().is_empty() {
            p.set_field("user", f.user.trim().to_string());
        }
        if !f.port.trim().is_empty() {
            p.set_field("port", f.port.trim().to_string());
        }
        let shell = if is_valid_shell(&f.shell) {
            f.shell.clone()
        } else {
            "auto".into()
        };
        // "auto" 는 AttachView 기본값 — 파일을 깨끗하게 유지하려 기본값은 쓰지 않는다.
        if shell != "auto" {
            p.set_field("shell", shell);
        }
        if !f.passkey_ref.is_empty() {
            p.passkey_ref = Some(f.passkey_ref.clone());
        }
    }
    let rt = f.remote_tasty.trim();
    if !rt.is_empty() && rt != "tasty" {
        p.set_field("remote_tasty", rt.to_string());
    }
    if is_valid_port_mode(&f.port_mode) && f.port_mode != "auto" {
        p.set_field("port_mode", f.port_mode.clone());
    }
    if !f.port_file.trim().is_empty() {
        p.set_field("port_file", f.port_file.trim().to_string());
    }

    // rename: 원래 name 과 다르면 옛 항목 제거.
    if let Some(orig) = &f.editing_original
        && orig != name
    {
        profiles.remove(orig);
    }
    profiles.upsert(p);
    profiles.save().map_err(|e| format!("save: {e}"))?;
    Ok(())
}

// ════════════════════════════════════════════════════════════════════════
// TAB C — Passkey
// ════════════════════════════════════════════════════════════════════════
fn draw_passkeys_tab(ui: &mut egui::Ui, th: &Theme, st: &mut UiState, passkeys: &Passkeys) {
    match st.passkey_view.clone() {
        Sub::List => draw_passkey_list(ui, th, st, passkeys),
        Sub::Form => draw_passkey_form(ui, th, st),
        Sub::ConfirmDelete(name) => {
            if let Some(act) = draw_confirm_delete(
                ui,
                th,
                t("remote_tool.noun_passkey"),
                &name,
                Some(t("remote_tool.passkey_delete_hint")),
            ) {
                if act {
                    let mut pk = Passkeys::load();
                    pk.remove(&name);
                    if let Err(e) = pk.save() {
                        tracing::warn!("passkey 삭제 후 저장 실패: {e}");
                    }
                }
                st.passkey_view = Sub::List;
            }
        }
    }
}

fn draw_passkey_list(ui: &mut egui::Ui, th: &Theme, st: &mut UiState, passkeys: &Passkeys) {
    if secondary_button(ui, th, t("remote_tool.passkey_add")).clicked() {
        st.kform = PasskeyForm {
            kind: "path".into(),
            ..Default::default()
        };
        st.kerr = None;
        st.passkey_view = Sub::Form;
        return;
    }
    ui.add_space(th.spacing_xs.value());
    if passkeys.passkeys.is_empty() {
        ui.vertical_centered(|ui| {
            ui.add_space(th.spacing_lg.value());
            selectable_text(
                ui,
                t("remote_tool.passkey_empty"),
                th.text_muted(),
                th.font_size_body.value(),
                false,
                true,
                TextWrap::None,
            );
        });
        return;
    }
    let mut action: Option<(String, PasskeyRowAction)> = None;
    scroll_list_with_fade(ui, th, |ui| {
        for k in &passkeys.passkeys {
            let revealed = st.revealed.contains(&k.name);
            if let Some(a) = draw_passkey_row(ui, th, k, revealed) {
                action = Some((k.name.clone(), a));
            }
        }
    });
    if let Some((name, a)) = action {
        match a {
            PasskeyRowAction::Reveal => {
                if st.revealed.contains(&name) {
                    st.revealed.remove(&name);
                } else {
                    st.revealed.insert(name);
                }
            }
            PasskeyRowAction::Edit => {
                if let Some(k) = passkeys.get(&name) {
                    st.kform = PasskeyForm {
                        name: k.name.clone(),
                        kind: k.kind.clone(),
                        value: reveal_value(k),
                        editing_original: Some(k.name.clone()),
                    };
                    st.kerr = None;
                    st.passkey_view = Sub::Form;
                }
            }
            PasskeyRowAction::Delete => {
                st.passkey_view = Sub::ConfirmDelete(name);
            }
        }
    }
}

enum PasskeyRowAction {
    Reveal,
    Edit,
    Delete,
}

fn draw_passkey_row(
    ui: &mut egui::Ui,
    th: &Theme,
    k: &Passkey,
    revealed: bool,
) -> Option<PasskeyRowAction> {
    let mut out = None;
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = STRUCT_GAP_1.value();
            ui.horizontal(|ui| {
                selectable_label(
                    ui,
                    &k.name,
                    th.text_primary(),
                    th.font_size_body.value(),
                    false,
                );
                if KNOWN_PASSKEY_KINDS.contains(&k.kind.as_str()) {
                    selectable_label(
                        ui,
                        &k.kind,
                        th.text_muted(),
                        th.font_size_caption.value(),
                        false,
                    );
                } else {
                    warn_badge(ui, th, &k.kind, t("remote_tool.kind_unknown_hint"));
                }
            });
            let val = if revealed {
                reveal_value(k)
            } else {
                "••••••••".into()
            };
            selectable_label(
                ui,
                &format!("{} · {}", k.kind, val),
                th.text_muted(),
                th.font_size_caption.value(),
                true,
            );
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = th.spacing_xs.value();
            // 아이콘 버튼 (디자인 IconButton): delete / edit / reveal(eye 토글).
            if ui
                .add(
                    egui::ImageButton::new(icons::TRASH.image(
                        th.icon_glyph_size_row_action.value(),
                        th.text_muted().into(),
                    ))
                    .frame(false),
                )
                .on_hover_text(t("remote_tool.delete"))
                .clicked()
            {
                out = Some(PasskeyRowAction::Delete);
            }
            if ui
                .add(
                    egui::ImageButton::new(icons::EDIT.image(
                        th.icon_glyph_size_row_action.value(),
                        th.text_muted().into(),
                    ))
                    .frame(false),
                )
                .on_hover_text(t("remote_tool.edit"))
                .clicked()
            {
                out = Some(PasskeyRowAction::Edit);
            }
            // revealed 면 eye-off + active(밝은) tint, 아니면 eye + muted.
            let (reveal_icon, reveal_tint) = if revealed {
                (icons::EYE_OFF, th.text_primary())
            } else {
                (icons::EYE, th.text_muted())
            };
            if ui
                .add(
                    egui::ImageButton::new(
                        reveal_icon
                            .image(th.icon_glyph_size_row_action.value(), reveal_tint.into()),
                    )
                    .frame(false),
                )
                .on_hover_text(t("remote_tool.reveal_tooltip"))
                .clicked()
            {
                out = Some(PasskeyRowAction::Reveal);
            }
        });
    });
    hsep(ui, th);
    out
}

/// 로컬 GUI 전용 값 노출. path kind 는 경로, inline kind 는 관리 파일 내용을 읽는다.
fn reveal_value(k: &Passkey) -> String {
    if k.kind == "inline" {
        std::fs::read_to_string(&k.path).unwrap_or_else(|_| "(unreadable)".into())
    } else {
        k.path.clone()
    }
}

fn draw_passkey_form(ui: &mut egui::Ui, th: &Theme, st: &mut UiState) {
    // 프로필 폼과 동일한 디자인 rtScrollPad + rtFooter 구조(하단 고정 footer, 전체폭
    // borderTop, 좌우 space-lg 패딩). 외곽 content Frame margin 은 폼일 때 0(상위 draw 분기).
    let full_x = ui.clip_rect().x_range();
    let sep = egui::Stroke::new(th.border_width.value(), th.border_strong());
    let pad_lg = th.spacing_lg.value() as i8;
    let pad_md = th.spacing_md.value() as i8;

    let mut do_save = false;
    let mut do_cancel = false;
    let footer = egui::TopBottomPanel::bottom("remote_tool.passkey_footer")
        .resizable(false)
        .show_separator_line(false)
        .frame(egui::Frame::NONE.inner_margin(egui::Margin {
            left: pad_lg,
            right: pad_lg,
            top: pad_md,
            bottom: pad_md,
        }))
        .show_inside(ui, |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if primary_button(ui, th, t("remote_tool.save")).clicked() {
                    do_save = true;
                }
                ui.add_space(th.spacing_sm.value());
                if ghost_button(ui, th, t("remote_tool.cancel")).clicked() {
                    do_cancel = true;
                }
            });
        });
    ui.painter()
        .hline(full_x, footer.response.rect.top() + 0.5, sep);

    egui::CentralPanel::default()
        .frame(egui::Frame::NONE)
        .show_inside(ui, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    egui::Frame::NONE
                        .inner_margin(egui::Margin {
                            left: pad_lg,
                            right: pad_lg,
                            top: pad_md,
                            bottom: pad_md,
                        })
                        .show(ui, |ui| {
                            let editing = st.kform.editing_original.is_some();
                            selectable_label(
                                ui,
                                if editing {
                                    t("remote_tool.passkey_form_edit")
                                } else {
                                    t("remote_tool.passkey_form_add")
                                },
                                th.text_primary(),
                                th.font_size_body.value(),
                                false,
                            );
                            ui.add_space(th.spacing_md.value());

                            let f = &mut st.kform;
                            // 행 간 세로 간격 = 디자인 rowGap(space-sm 8).
                            ui.spacing_mut().item_spacing.y = th.spacing_sm.value();
                            text_row(ui, th, t("remote_tool.field_name"), &mut f.name, "", false);
                            form_row(ui, th, t("remote_tool.field_kind"), |ui| {
                                for opt in KNOWN_PASSKEY_KINDS {
                                    ui.selectable_value(&mut f.kind, (*opt).to_string(), *opt);
                                }
                            });
                            form_row(ui, th, t("remote_tool.field_value"), |ui| {
                                if f.kind == "inline" {
                                    ui.add(
                                        egui::TextEdit::multiline(&mut f.value)
                                            .desired_rows(3)
                                            .hint_text(t("remote_tool.value_inline_hint")),
                                    );
                                } else {
                                    ui.add(
                                        egui::TextEdit::singleline(&mut f.value)
                                            .desired_width(f32::INFINITY)
                                            .hint_text("~/.ssh/id_ed25519"),
                                    );
                                }
                            });
                            selectable_label(
                                ui,
                                t("remote_tool.passkey_value_note"),
                                th.text_muted(),
                                th.font_size_caption.value(),
                                false,
                            );
                            if let Some(err) = &st.kerr {
                                selectable_label(
                                    ui,
                                    err,
                                    th.accent_danger(),
                                    th.font_size_caption.value(),
                                    false,
                                );
                            }
                        });
                });
        });

    if do_cancel {
        st.kerr = None;
        st.passkey_view = Sub::List;
        return;
    }
    if do_save {
        match save_passkey(st) {
            Ok(()) => {
                st.kerr = None;
                st.passkey_view = Sub::List;
            }
            Err(e) => st.kerr = Some(e),
        }
    }
}

fn save_passkey(st: &mut UiState) -> Result<(), String> {
    let f = st.kform.clone();
    let name = f.name.trim();
    if name.is_empty() {
        return Err(t("remote_tool.err_name_empty").to_string());
    }
    if !is_valid_passkey_name(name) {
        return Err(t("remote_tool.err_name_format").to_string());
    }
    if f.value.trim().is_empty() {
        return Err(t("remote_tool.err_value_empty").to_string());
    }
    let mut pk = Passkeys::load();
    if pk
        .passkeys
        .iter()
        .any(|k| k.name == name && Some(k.name.as_str()) != f.editing_original.as_deref())
    {
        return Err(t("remote_tool.err_name_dup").to_string());
    }
    // rename: 옛 이름(+관리 파일) 제거.
    if let Some(orig) = &f.editing_original
        && orig != name
    {
        pk.remove(orig);
    }
    let res = if f.kind == "inline" {
        pk.upsert_inline(name, &f.value)
    } else {
        pk.upsert_path(name, f.value.trim().to_string())
    };
    res.map_err(|e| format!("{e}"))?;
    pk.save().map_err(|e| format!("save: {e}"))?;
    Ok(())
}

// ── 공통 ─────────────────────────────────────────────────────────────────
fn draw_confirm_delete(
    ui: &mut egui::Ui,
    th: &Theme,
    noun: &str,
    name: &str,
    hint: Option<&str>,
) -> Option<bool> {
    let mut out = None;
    ui.add_space(th.spacing_sm.value());
    selectable_label(
        ui,
        &format!("{}: \"{name}\"?", noun),
        th.text_primary(),
        th.font_size_body.value(),
        false,
    );
    if let Some(h) = hint {
        selectable_label(ui, h, th.text_muted(), th.font_size_caption.value(), false);
    }
    ui.add_space(th.spacing_sm.value());
    ui.horizontal(|ui| {
        if ui
            .button(egui::RichText::new(t("remote_tool.delete")).color(th.accent_danger()))
            .clicked()
        {
            out = Some(true);
        }
        if ui.button(t("remote_tool.cancel")).clicked() {
            out = Some(false);
        }
    });
    out
}

/// 폼 한 행 — 디자인 ProfileForm grid `[112px 1fr]` 의 수동 2컬럼 전사.
/// `egui::Grid` 는 2열 입력의 무한폭(`desired_width(INFINITY)`)이 1열(라벨) 폭 협상을
/// 붕괴시켜 112px 를 확보하지 못하고 라벨이 `.truncate()` 로 잘렸다. Type 행이 이미 쓰던
/// 수동 `ui.horizontal` 2컬럼(고정 112 라벨 + columnGap + 입력)으로 전 행을 통일한다.
/// columnGap = space-md(12). 세로 rowGap(8) 은 호출부의 `item_spacing.y = spacing_sm` 로 일괄.
fn form_row(ui: &mut egui::Ui, th: &Theme, label: &str, add_input: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = th.spacing_md.value();
        field_label(ui, th, label);
        add_input(ui);
    });
}

/// 폼 텍스트 입력 행. `placeholder` 는 빈 입력 시 보일 예시값(기술 예시라 번역 비대상
/// — i18n 하드코딩 예외), `mono` 면 입력을 monospace 폰트로 그린다(host/port/remote-tasty
/// 처럼 식별자/경로 성격 필드). 입력은 `INFINITY` 로 1fr(잔여폭) 을 채운다.
fn text_row(
    ui: &mut egui::Ui,
    th: &Theme,
    label: &str,
    value: &mut String,
    placeholder: &str,
    mono: bool,
) {
    form_row(ui, th, label, |ui| {
        let mut edit = egui::TextEdit::singleline(value)
            .desired_width(f32::INFINITY)
            .hint_text(placeholder);
        if mono {
            edit = edit.font(egui::TextStyle::Monospace);
        }
        ui.add(edit);
    });
}

/// 디자인 ProfileForm 의 라벨 컬럼 폭 (LogicalPx). grid `gridTemplateColumns: 112px 1fr`.
/// 화면전용 고정값이라 `Theme` 상수가 아니지만 길이이므로 `LogicalPx` 로 둔다.
const LABEL_COL_WIDTH: LogicalPx = LogicalPx(112.0);
/// hint/error 문구를 입력 컬럼에 맞춰 들여쓸 폭 = 라벨 컬럼(112) + columnGap(12).
/// 디자인 `marginLeft: 124px`.
const HINT_INDENT: LogicalPx = LABEL_COL_WIDTH.plus(LogicalPx(12.0));

// ── selectable 텍스트 ──────────────────────────────────────────────────
// egui `Label` 의 드래그 선택은 내장 `LabelSelectionState::cursor_for()` 가 처리하는데,
// 드래그 중 포인터가 위젯 rect 밖으로 세로(y)로 나가는 경우만 처리하고 가로(x) 이탈은
// 그 프레임에 selection cursor 갱신이 안 돼 선택이 멈춘다(egui 이슈 #3816 — "top-down
// 레이아웃부터 지원, 좌우는 나중"이라는 의도적으로 축소된 설계 범위, upstream 이 자체
// 수정할 근거 없음이 egui 0.35.0 dev 최신 커밋까지 확인됨). 반면 `TextEdit` 의 커서
// 갱신은 `Galley::cursor_from_pos` 가 가로/세로 모두 위젯 범위 밖 좌표를 자동 clamp
// 해서 이 버그가 없는 별개 코드 경로다 — 그래서 selectable 텍스트를 `TextEdit`
// (read-only 취급) 기반으로 렌더링해 우회한다.
//
// `egui::RichText` 는 필드가 전부 private 라 이미 만들어진 값에서 색/크기 등을
// introspect 하는 API가 없어, 스타일을 개별 파라미터로 받는다.
//
// `TextEdit::interactive(false)` 는 편집뿐 아니라 선택 자체도 막아버려 쓸 수 없다 —
// 대신 매 프레임 지역 변수로 clone 한 버퍼를 넘겨, 사용자가 타이핑해도 다음 프레임에
// 원래 텍스트로 되돌아가는 방식(편집 결과를 버림)으로 read-only 를 흉내낸다.

/// `selectable_text` 의 줄바꿈 모드.
#[derive(Clone, Copy)]
enum TextWrap {
    /// 줄바꿈 없음, 콘텐츠 폭만큼만 차지(`egui::Label` 기본값과 동형).
    None,
    /// 가용 폭에서 여러 줄로 줄바꿈(`Label::wrap()` 과 동형).
    Wrap,
    /// 지정 폭에서 한 줄로 말줄임(`Label::truncate()` 과 동형) — `…` 로 elide.
    ///
    /// 폭이 `LogicalPx` 가 아닌 이유: 이 값은 곧장 egui `LayoutJob::wrap.max_width` 로
    /// 들어간다. 타입을 붙이면 만드는 자리 하나에서 벗기던 것을 쓰는 자리 하나에서
    /// 벗기게 될 뿐이라 총수가 그대로다.
    Truncate(f32),
}

/// selectable 텍스트 렌더 — 위 모듈 주석 참고.
fn selectable_text(
    ui: &mut egui::Ui,
    text: &str,
    color: impl Into<egui::Color32>,
    size: f32,
    monospace: bool,
    italic: bool,
    wrap: TextWrap,
) -> egui::Response {
    let color = color.into();
    let font_id = if monospace {
        egui::FontId::monospace(size)
    } else {
        egui::FontId::proportional(size)
    };
    let mut buffer = text.to_string();
    let mut layouter = move |ui: &egui::Ui, text: &str, wrap_width: f32| {
        let mut job = egui::text::LayoutJob::default();
        job.append(
            text,
            0.0,
            egui::TextFormat {
                font_id: font_id.clone(),
                color,
                italics: italic,
                ..Default::default()
            },
        );
        match wrap {
            TextWrap::None => job.wrap.max_width = f32::INFINITY,
            TextWrap::Wrap => {
                job.wrap.max_width = wrap_width;
                job.break_on_newline = true;
            }
            TextWrap::Truncate(w) => {
                job.wrap.max_width = w;
                job.wrap.max_rows = 1;
                job.wrap.break_anywhere = true;
            }
        }
        ui.fonts(|f| f.layout_job(job))
    };
    // Wrap 은 가용 폭까지 확장해야 실제로 줄바꿈된다. None/Truncate 는 콘텐츠(또는
    // truncate 결과) 폭만큼만 차지해야 `Label` 과 동일하게 부모 레이아웃(가로 나열,
    // 우측 정렬용 RTL 트릭 등)에 자연스럽게 맞물린다 — desired_width 를 고정폭으로
    // 주면 짧은 텍스트도 그 폭을 다 차지해 정렬이 깨진다.
    let desired_width = if matches!(wrap, TextWrap::Wrap) {
        f32::INFINITY
    } else {
        0.0
    };
    ui.add(
        egui::TextEdit::multiline(&mut buffer)
            .frame(false)
            .desired_rows(1)
            .desired_width(desired_width)
            .layouter(&mut layouter),
    )
}

/// `selectable_text` 축약형 — 줄바꿈 없음·italic 아님(가장 흔한 경우).
fn selectable_label(
    ui: &mut egui::Ui,
    text: &str,
    color: impl Into<egui::Color32>,
    size: f32,
    monospace: bool,
) -> egui::Response {
    selectable_text(ui, text, color, size, monospace, false, TextWrap::None)
}

/// 폼 라벨 — 112px 고정폭 컬럼 + 우측 정렬(디자인 `rtLabel`). Grid 첫 컬럼과
/// Type 행/passkey 행이 모두 같은 컬럼 폭으로 정렬되도록 폭을 강제한다.
fn field_label(ui: &mut egui::Ui, th: &Theme, label: &str) {
    ui.allocate_ui_with_layout(
        egui::vec2(LABEL_COL_WIDTH.value(), ui.spacing().interact_size.y),
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            selectable_text(
                ui,
                label,
                th.text_muted(),
                th.font_size_body.value(),
                false,
                false,
                TextWrap::Truncate(LABEL_COL_WIDTH.value()),
            );
        },
    );
}

/// hint/error 문구를 입력 컬럼(124px)에 맞춰 들여써서 출력. 디자인의
/// `marginLeft: 124px` 정합 — 라벨 컬럼 아래가 아니라 입력 칸에 맞춘다.
fn indented_hint(
    ui: &mut egui::Ui,
    th: &Theme,
    text: &str,
    color: impl Into<egui::Color32>,
    italic: bool,
) {
    ui.horizontal(|ui| {
        ui.add_space(HINT_INDENT.value());
        selectable_text(
            ui,
            text,
            color,
            th.font_size_caption.value(),
            false,
            italic,
            TextWrap::Wrap,
        );
    });
}

// ── detect 워커 ───────────────────────────────────

/// detect 슬롯 poison 을 보고했는가(첫 1 회만 — 폴링이 프레임마다 돈다).
///
/// 임계구역은 `Option<Result<_, _>>` 한 칸이라 패닉이 나도 불변식이 성립하고, 폴링은
/// **메인(렌더) 스레드**라 패닉하면 모든 창이 죽는다 — 복구가 맞다. 조용히 버리던
/// 종전 형태는 두 방향 모두 사용자에게 원인을 남기지 않았다: 워커 쓰기를 버리면 완료
/// 신호가 영영 안 와 **"Detecting…" 이 영구 표시**되고(이 워커에는 상한이 없다),
/// 폴링의 `unwrap_or(true)` 는 끝나지 않은 detect 를 끝난 것으로 처리한다.
/// 근거 `docs/dev-guide/error-handling.md` "락 poison".
static DETECT_SLOT_POISONED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
const DETECT_SLOT_WHAT: &str = "remote tool detect slot";
fn spawn_detect(ctx: &egui::Context, name: String) -> DetectJob {
    let slot: Arc<Mutex<Option<Result<String, String>>>> = Arc::new(Mutex::new(None));
    let slot_w = Arc::clone(&slot);
    let ctx_w = ctx.clone();
    let name_w = name.clone();
    std::thread::spawn(move || {
        let res = tasty_ssh::detect_and_persist(&name_w)
            .map(|m| m.as_str().to_string())
            .map_err(|e| e.to_string());
        *crate::poison::recover_mutex(slot_w.lock(), DETECT_SLOT_WHAT, &DETECT_SLOT_POISONED) =
            Some(res);
        ctx_w.request_repaint();
    });
    DetectJob { name, slot }
}

fn poll_detect(st: &mut UiState) -> bool {
    let done = match &st.detecting {
        Some(job) => {
            crate::poison::recover_mutex(job.slot.lock(), DETECT_SLOT_WHAT, &DETECT_SLOT_POISONED)
                .is_some()
        }
        None => false,
    };
    if done {
        st.detecting = None;
    }
    done
}

#[cfg(test)]
// 테스트 본문은 `let _ =` 사유 주석 정책의 범위 밖이다(전수 가드가 제외한다) —
// 여기 경고는 조치 대상이 될 수 없어 프로덕션 신호만 가린다. error-handling.md.
#[allow(clippy::let_underscore_must_use)]
mod tests {
    use super::*;

    /// detect 슬롯이 poison 돼도 "아직 안 끝났다" 를 그대로 답한다.
    ///
    /// 조용히 버리는 구현(`unwrap_or(true)`)이면 여기서 완료로 판정해 진행 중인
    /// detect 를 끝난 것처럼 지운다 — 이 워커에는 상한이 없어 되돌릴 지점도 없다.
    ///
    /// **자동 실행 채널이 하나뿐이다.** 이 테스트는 `src/adapters/mod.rs` 의
    /// `#[cfg(feature = "gui")] pub mod ui;` 안에 있어 `--no-default-features` 조합에서는
    /// 컴파일 단계에 통째로 사라진다 — 헤드리스 잡의 초록은 이 테스트가 돌았다는 뜻이
    /// 아니다(없는 테스트는 실패하지 못한다). 실측: 두 자동 잡의 명령을 워크플로에서
    /// 그대로 읽어 `-- --list` 이름을 대조하면 기본 조합에만 뜬다. 팝업 상태를 직접
    /// 쥐고 도는 테스트라 gui 밖으로 옮길 대상이 없어 고칠 수 있는 결함이 아니고,
    /// 사실을 적어 두는 것이 맞는 처리다.
    #[test]
    fn a_poisoned_detect_slot_does_not_look_finished() {
        let slot: Arc<Mutex<Option<Result<String, String>>>> = Arc::new(Mutex::new(None));
        let poisoner = Arc::clone(&slot);
        // 이유: 이 스레드는 패닉하는 것이 목적이라 join 결과는 항상 Err 다 — 버린다.
        let _ = std::thread::spawn(move || {
            let _guard = poisoner.lock().expect("fresh lock");
            panic!("poison the detect slot on purpose");
        })
        .join();
        assert!(slot.is_poisoned(), "전제: 락이 poison 이다");

        let mut st = UiState {
            detecting: Some(DetectJob {
                name: "loopback".into(),
                slot: Arc::clone(&slot),
            }),
            ..Default::default()
        };
        assert!(!poll_detect(&mut st), "빈 슬롯은 아직 끝나지 않은 것이다");
        assert!(st.detecting.is_some(), "진행 중인 detect 를 지우면 안 된다");

        *slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(Ok("ssh".into()));
        assert!(poll_detect(&mut st), "채워지면 완료로 판정한다");
        assert!(st.detecting.is_none());
    }

    fn prof(name: &str, kind: &str) -> RemoteProfile {
        RemoteProfile::new(name, kind)
    }

    /// `UiState`(폼 버퍼·detect 워커)는 훅 한 번으로 drop 되지만, `FILTER_MEMORY_ID`
    /// (적용된 프로토콜 필터)는 session-only 로 popup 재오픈에도 유지돼야 하므로
    /// 건드리지 않는다.
    #[test]
    fn on_close_clears_ui_state_but_preserves_filter() {
        let ctx = egui::Context::default();
        write_ui(
            &ctx,
            UiState {
                tab: Tab::Attach,
                ..Default::default()
            },
        );
        write_filter(&ctx, ["ssh".to_string()].into_iter().collect());

        let (mut state, mut engine) = crate::state::tests::test_state();
        on_close_remote_tool_popup(&ctx, &mut state, &mut engine);

        assert!(
            ctx.memory(|m| m.data.get_temp::<UiState>(egui::Id::new(UI_MEMORY_ID)))
                .is_none()
        );
        assert_eq!(read_filter(&ctx), ["ssh".to_string()].into_iter().collect());
    }

    #[test]
    fn protocol_set_known_first_then_alpha() {
        let ps = vec![
            prof("a", "http"),
            prof("b", "zeta"),
            prof("c", "ssh"),
            prof("d", "smb"),
            prof("e", "ssh"), // 중복
            prof("f", "alpha"),
        ];
        // KNOWN_TYPES = [ssh, smb, http] 순서 우선, 나머지(alpha, zeta)는 알파벳.
        assert_eq!(
            protocol_set(&ps),
            vec!["ssh", "smb", "http", "alpha", "zeta"]
        );
    }

    #[test]
    fn protocol_set_excludes_attach_kind() {
        // tasty-attach 는 Attach 탭 전담 — Profiles 탭 프로토콜 집합에 안 낀다.
        let ps = vec![prof("a", "ssh"), prof("b", "tasty-attach")];
        assert_eq!(protocol_set(&ps), vec!["ssh"]);
    }

    #[test]
    fn protocol_set_dedup_and_skip_blank() {
        let ps = vec![prof("a", "ssh"), prof("b", "ssh"), prof("c", "  ")];
        assert_eq!(protocol_set(&ps), vec!["ssh"]);
    }

    #[test]
    fn filter_excludes_hidden_kinds() {
        let ps = [prof("a", "ssh"), prof("b", "smb"), prof("c", "http")];
        // excluded = {} → 전체.
        let none: HashSet<String> = HashSet::new();
        let all: Vec<&str> = ps
            .iter()
            .filter(|p| !none.contains(p.kind.trim()))
            .map(|p| p.name.as_str())
            .collect();
        assert_eq!(all, vec!["a", "b", "c"]);
        // excluded = {ssh} → ssh 제외.
        let hidden: HashSet<String> = ["ssh".to_string()].into_iter().collect();
        let vis: Vec<&str> = ps
            .iter()
            .filter(|p| !hidden.contains(p.kind.trim()))
            .map(|p| p.name.as_str())
            .collect();
        assert_eq!(vis, vec!["b", "c"]);
    }

    #[test]
    fn new_kind_visible_by_default() {
        // exclude-set 에 없는 새 kind(ftp)는 기본 표시(가정 4 자동 충족).
        let p = prof("x", "ftp");
        let hidden: HashSet<String> = ["ssh".to_string()].into_iter().collect();
        assert!(!hidden.contains(p.kind.trim()));
    }

    // ── 로컬 ssh config 섹션 ─────────────────────────────────────────

    fn host(alias: &str, hostname: Option<&str>, port: Option<u16>) -> SshConfigHost {
        SshConfigHost {
            alias: alias.into(),
            source: std::path::PathBuf::from("/home/u/.ssh/config"),
            hostname: hostname.map(str::to_string),
            user: Some("zilhak".into()),
            port,
        }
    }

    fn cache(hosts: Vec<SshConfigHost>) -> LocalSshCache {
        LocalSshCache {
            hosts,
            path: "~/.ssh/config".into(),
            exists: true,
            unreadable: false,
        }
    }

    /// 한 프레임을 헤드리스로 돌리고 그려진 텍스트를 모은다. "그 섹션이 실제로
    /// 렌더됐는가" 를 판정하는 유일하게 정직한 관찰점이다.
    fn painted_text(profiles: &RemoteProfiles, st: &mut UiState, hidden: &[&str]) -> Vec<String> {
        let ctx = egui::Context::default();
        write_filter(
            &ctx,
            hidden.iter().map(|s| s.to_string()).collect::<HashSet<_>>(),
        );
        let th = theme::theme();
        let passkeys = Passkeys::default();
        let mut out = Vec::new();
        // 첫 프레임은 폰트/레이아웃이 확정되지 않아 galley 가 비는 경우가 있어 두 번 돈다.
        for _ in 0..2 {
            out.clear();
            let full = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    draw_profile_list(ui, &th, st, profiles, &passkeys);
                });
            });
            for shape in full.shapes.iter() {
                collect_text(&shape.shape, &mut out);
            }
        }
        out
    }

    fn collect_text(shape: &egui::epaint::Shape, out: &mut Vec<String>) {
        match shape {
            egui::epaint::Shape::Text(t) => out.push(t.galley.text().to_string()),
            egui::epaint::Shape::Vec(v) => {
                for s in v {
                    collect_text(s, out);
                }
            }
            _ => {}
        }
    }

    /// 헤드리스로 목록 스크롤 영역을 한 프레임 그리고, 칠해진 Mesh 개수를 센다.
    /// 가장자리 페이드는 이 파일에서 Mesh 를 쓰는 유일한 지점이라 개수가 곧 페이드 수다.
    fn painted_fades(view_h: f32, rows: usize) -> usize {
        let ctx = egui::Context::default();
        let th = theme::theme();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, view_h),
            )),
            ..Default::default()
        };
        let full = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                scroll_list_with_fade(ui, &th, |ui| {
                    for i in 0..rows {
                        ui.label(format!("row {i}"));
                    }
                });
            });
        });
        let mut n = 0;
        for shape in full.shapes.iter() {
            count_mesh(&shape.shape, &mut n);
        }
        n
    }

    fn count_mesh(shape: &egui::epaint::Shape, n: &mut usize) {
        match shape {
            egui::epaint::Shape::Mesh(_) => *n += 1,
            egui::epaint::Shape::Vec(v) => {
                for s in v {
                    count_mesh(s, n);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn fade_edges_none_when_content_fits() {
        assert_eq!(fade_edges(0.0, 100.0, 200.0), (false, false));
    }

    #[test]
    fn fade_edges_bottom_only_at_top_of_overflowing_list() {
        assert_eq!(fade_edges(0.0, 900.0, 200.0), (false, true));
    }

    #[test]
    fn fade_edges_both_when_scrolled_to_middle() {
        assert_eq!(fade_edges(350.0, 900.0, 200.0), (true, true));
    }

    #[test]
    fn fade_edges_top_only_at_bottom_of_list() {
        assert_eq!(fade_edges(700.0, 900.0, 200.0), (true, false));
    }

    #[test]
    fn fade_edges_keeps_both_just_inside_the_epsilon_band() {
        // 엡실론 **상한** 고정: 끝에서 1.5px 떨어진 지점은 아직 "더 있다" 다.
        // (위쪽도 같은 이유로 1.5px 만 스크롤된 상태를 함께 잠근다.)
        assert_eq!(fade_edges(698.5, 900.0, 200.0), (true, true));
        assert_eq!(fade_edges(1.5, 900.0, 200.0), (true, true));
    }

    #[test]
    fn fade_height_is_capped_at_half_the_viewport() {
        // 넉넉한 뷰포트 — 토큰 값 그대로.
        assert_eq!(fade_height(24.0, 400.0), 24.0);
        // 2×토큰보다 낮은 뷰포트 — 위/아래가 만나되 겹치지는 않는다.
        assert_eq!(fade_height(24.0, 40.0), 20.0);
        assert_eq!(fade_height(24.0, 0.0), 0.0);
    }

    #[test]
    fn fade_edges_tolerates_subpixel_overshoot() {
        // 끝까지 스크롤했는데 0.4px 가 남는 경우 — 아래쪽 페이드를 남기지 않는다.
        assert_eq!(fade_edges(699.6, 900.0, 200.0), (true, false));
    }

    #[test]
    fn short_list_paints_no_fade() {
        assert_eq!(painted_fades(400.0, 2), 0);
    }

    #[test]
    fn overflowing_list_paints_bottom_fade() {
        // 맨 위에서 시작하므로 아래쪽 하나만.
        assert_eq!(painted_fades(120.0, 40), 1);
    }

    #[test]
    fn import_prefill_puts_alias_in_host_and_leaves_user_port_empty() {
        let f = import_prefill("gx10");
        assert_eq!(f.kind, "ssh");
        assert_eq!(f.name, "gx10"); // 기본값 = alias (폼에서 바꿀 수 있다)
        assert_eq!(f.host, "gx10"); // alias 그대로 — 값 펼치기 없음
        assert_eq!(f.shell, "auto");
        // user/port 를 채우면 ssh config 위임이 깨진다.
        assert!(f.user.is_empty());
        assert!(f.port.is_empty());
        assert!(f.label.is_empty());
        assert!(f.passkey_ref.is_empty());
        assert!(f.editing_original.is_none());
    }

    #[test]
    fn already_imported_alias_is_detected_by_host_field() {
        let mut profiles = RemoteProfiles::default();
        profiles.upsert(RemoteProfile::new("my-gpu", "ssh").with_field("host", "gx10"));
        profiles.upsert(RemoteProfile::new("other", "smb").with_field("host", "bastion"));
        assert_eq!(imported_as(&profiles, "gx10"), Some("my-gpu"));
        // ssh kind 가 아니면 "가져옴" 이 아니다 — 가져오기는 ssh 프로필만 만든다.
        assert_eq!(imported_as(&profiles, "bastion"), None);
    }

    #[test]
    fn local_target_hint_falls_back_to_alias_and_dash() {
        assert_eq!(
            local_target_hint(&host("gx10", Some("10.0.0.5"), Some(2200))),
            "10.0.0.5:2200"
        );
        assert_eq!(
            local_target_hint(&host("gx10", Some("10.0.0.5"), None)),
            "10.0.0.5"
        );
        // HostName 이 없으면 ssh 가 alias 를 호스트로 쓴다.
        assert_eq!(local_target_hint(&host("gx10", None, Some(22))), "gx10:22");
        assert_eq!(local_target_hint(&host("gx10", None, None)), "—");
    }

    #[test]
    fn profile_empty_key_distinguishes_no_profiles_from_filtered_out() {
        assert_eq!(
            profile_empty_key(false, false),
            Some("remote_tool.profile_empty")
        );
        assert_eq!(
            profile_empty_key(true, false),
            Some("remote_tool.profile_filter_empty")
        );
        assert_eq!(profile_empty_key(true, true), None);
    }

    #[test]
    fn protocol_filter_does_not_hide_local_section() {
        let mut profiles = RemoteProfiles::default();
        profiles.upsert(RemoteProfile::new("my-gpu", "ssh").with_field("host", "other"));
        let mut st = UiState {
            local: Some(cache(vec![host("gx10", Some("10.0.0.5"), Some(2200))])),
            ..Default::default()
        };
        // ssh 를 필터로 가려 프로필 목록이 통째로 빈 상태가 되게 한다.
        let texts = painted_text(&profiles, &mut st, &["ssh"]);
        assert!(
            texts.iter().any(|s| s.contains("gx10")),
            "필터가 로컬 섹션까지 가렸다: {texts:?}"
        );
        assert!(
            texts
                .iter()
                .any(|s| s == t("remote_tool.profile_filter_empty")),
            "필터 빈 상태 문구가 없다: {texts:?}"
        );
    }

    #[test]
    fn empty_profiles_still_renders_local_section() {
        let profiles = RemoteProfiles::default();
        let mut st = UiState {
            local: Some(cache(vec![host("gx10", Some("10.0.0.5"), Some(2200))])),
            ..Default::default()
        };
        let texts = painted_text(&profiles, &mut st, &[]);
        assert!(
            texts.iter().any(|s| s.contains("gx10")),
            "프로필 0 건에서 early return 해 로컬 섹션이 사라졌다: {texts:?}"
        );
        assert!(
            texts
                .iter()
                .any(|s| s == t("remote_tool.local_ssh_heading")),
            "섹션 헤더가 없다: {texts:?}"
        );
    }

    #[test]
    fn local_section_shows_empty_note_instead_of_hiding() {
        let profiles = RemoteProfiles::default();
        let mut st = UiState {
            local: Some(LocalSshCache {
                hosts: Vec::new(),
                path: "~/.ssh/config".into(),
                exists: false,
                unreadable: false,
            }),
            ..Default::default()
        };
        let texts = painted_text(&profiles, &mut st, &[]);
        // 파일이 없을 때와 있는데 0 건일 때의 문구가 다르다.
        assert!(
            texts
                .iter()
                .any(|s| s == t("remote_tool.local_ssh_missing")),
            "{texts:?}"
        );
    }

    #[test]
    fn local_section_distinguishes_unreadable_config_from_empty_one() {
        let profiles = RemoteProfiles::default();
        let mut st = UiState {
            local: Some(LocalSshCache {
                hosts: Vec::new(),
                path: "~/.ssh/config".into(),
                // 파일은 있는데 못 읽었다 — "빈 설정" 과 다른 문구가 나와야 한다.
                exists: true,
                unreadable: true,
            }),
            ..Default::default()
        };
        let texts = painted_text(&profiles, &mut st, &[]);
        assert!(
            texts
                .iter()
                .any(|s| s == t("remote_tool.local_ssh_unreadable")),
            "{texts:?}"
        );
        assert!(
            !texts.iter().any(|s| s == t("remote_tool.local_ssh_empty")),
            "빈 설정 문구가 함께 떴다: {texts:?}"
        );
    }

    #[test]
    fn empty_key_covers_three_causes() {
        let base = LocalSshCache {
            hosts: Vec::new(),
            path: "~/.ssh/config".into(),
            exists: true,
            unreadable: false,
        };
        assert_eq!(
            local_ssh_empty_key(&LocalSshCache {
                exists: false,
                ..base.clone()
            }),
            "remote_tool.local_ssh_missing"
        );
        assert_eq!(
            local_ssh_empty_key(&LocalSshCache {
                unreadable: true,
                ..base.clone()
            }),
            "remote_tool.local_ssh_unreadable"
        );
        assert_eq!(local_ssh_empty_key(&base), "remote_tool.local_ssh_empty");
    }

    /// 권한 없는 실제 파일(mode 000)에서 캐시가 "있는데 못 읽음" 으로 떨어지는지.
    /// root 는 권한을 무시하므로 그 환경에서는 판정 자체가 성립하지 않아 건너뛴다.
    #[test]
    #[cfg(unix)]
    fn cache_marks_mode_000_config_unreadable() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("config");
        std::fs::write(&path, "Host a\n").expect("write fixture");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).expect("chmod 000");

        if std::fs::File::open(&path).is_ok() {
            // root(또는 CAP_DAC_OVERRIDE) — mode 000 이어도 열린다.
            return;
        }
        let cache = local_ssh_cache_at(Some(path.clone()));
        assert!(cache.exists);
        assert!(cache.unreadable);
        assert_eq!(
            local_ssh_empty_key(&cache),
            "remote_tool.local_ssh_unreadable"
        );

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).expect("restore");
        let cache = local_ssh_cache_at(Some(path));
        assert!(!cache.unreadable);
        assert_eq!(local_ssh_empty_key(&cache), "remote_tool.local_ssh_empty");
    }

    /// 파일 없음 / 경로 미확정은 "권한 문제" 가 아니다 — 별도 문구가 담당하므로
    /// 캐시의 `unreadable` 이 서면 안 된다.
    #[test]
    fn cache_marks_absent_config_missing_not_unreadable() {
        // 부재는 "권한 문제" 가 아니다 — 코어가 부재를 readable:false 로 표현하므로
        // 이 구분이 실제로 유지되는지 고정한다.
        let dir = tempfile::tempdir().expect("temp dir");
        let cache = local_ssh_cache_at(Some(dir.path().join("nope")));
        assert!(!cache.exists);
        assert!(!cache.unreadable);
        assert_eq!(local_ssh_empty_key(&cache), "remote_tool.local_ssh_missing");

        // 홈 자체를 못 구한 경우도 같은 갈래다.
        let cache = local_ssh_cache_at(None);
        assert!(!cache.exists);
        assert!(!cache.unreadable);
        assert_eq!(local_ssh_empty_key(&cache), "remote_tool.local_ssh_missing");
    }

    /// `~/.ssh/config` 자리에 디렉토리가 있는 경우. linux 는 디렉토리에도
    /// `File::open` 을 허용하므로 코어가 `is_file()` 을 겹쳐 막는데, 그 수정이
    /// **GUI 문구까지** 닿는지는 여기서만 확인된다 — 경로 주입 seam 이 없으면
    /// 쓸 수 없는 테스트다.
    #[test]
    fn cache_marks_directory_config_unreadable() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("config");
        std::fs::create_dir(&path).expect("디렉토리 픽스처");

        let cache = local_ssh_cache_at(Some(path));
        assert!(cache.exists, "디렉토리도 존재는 한다");
        assert!(
            cache.unreadable,
            "디렉토리를 '읽히는 빈 설정' 으로 오독하면 안 된다"
        );
        assert_eq!(
            local_ssh_empty_key(&cache),
            "remote_tool.local_ssh_unreadable"
        );
    }

    #[test]
    fn unknown_kind_detection() {
        assert!(is_unknown_kind("ftp")); // 코어/known 모두 모름
        assert!(!is_unknown_kind("ssh")); // builtin
        assert!(!is_unknown_kind("smb")); // builtin
        assert!(!is_unknown_kind("http")); // KNOWN_TYPES
    }
}
