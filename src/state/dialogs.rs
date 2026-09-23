//! GUI 가 소유하는 dialog·popup 입력 상태 — `AppState::dialogs` 와 그 필드의 자료형.
//!
//! `state.rs` 에서 그대로 옮겨 온 것이다 — 동작 변경이 없다. 한 파일로 모은 이유는
//! 소유자가 하나이기 때문이다: 여기 있는 값은 전부 popup·메뉴·드래그가 열려 있는 동안만
//! 사는 사용자 view 상태이고, 세우는 쪽도 비우는 쪽도 GUI 다.

use std::collections::VecDeque;

use crate::adapters::ui::info_modal::InfoModal;
use crate::adapters::ui::popup::transfer::{TransferError, TransferProgress};

use super::selection;

/// 링크 우클릭 메뉴의 대상 — 우클릭 시점에 hover 링크에서 찍은 스냅샷.
///
/// 메뉴가 열려 있는 동안 화면이 바뀔 수 있어(새 출력·scrollback 트림·resize) 좌표만
/// 들고 있지 않고 **표시 문자열**(`text`)도 함께 담는다. 좌표는 렌더러의 `LinkSegment`
/// 대신 선택 좌표(`SelectionPoint`)로 담는다 — 메뉴 대상이 렌더 캐시의 수명에 묶이지 않게.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalLinkMenu {
    pub(crate) surface_id: u32,
    /// 첫 세그먼트의 시작 셀(포함).
    pub(crate) start: selection::SelectionPoint,
    /// 마지막 세그먼트의 끝 셀(포함).
    pub(crate) end: selection::SelectionPoint,
    /// 우클릭 시점에 그 범위에서 추출한 화면 텍스트. OSC 8 이면 URI 가 아니라 라벨이다.
    pub(crate) text: String,
    /// "연결 동작" 이 picker 에 넘길 대상. `None` 이면 항목을 노출하지 않는다
    /// (mailto 등 핸들러로 열 곳이 없는 scheme).
    pub(crate) open_with: Option<crate::file::dispatch::DispatchTarget>,
    /// 원격(mirror) surface 의 경로 링크 — 로컬 핸들러로 열 수 없어 빈 picker 만 띄운다.
    pub(crate) remote_path: bool,
}

/// A pending native context menu request.
#[derive(Clone)]
pub enum PendingNativeMenu {
    /// Tab right-click: Rename / Close
    Tab {
        pane_id: u32,
        tab_index: usize,
        x: f32,
        y: f32,
    },
    /// Pane/empty area right-click: Open Markdown... / Open Explorer / Open HTML...
    Pane { pane_id: u32, x: f32, y: f32 },
    /// Workspace right-click in sidebar: Rename title / Rename subtitle
    Workspace { ws_idx: usize, x: f32, y: f32 },
    /// Terminal surface right-click: Copy surface id (좌표는 logical px 기준)
    TerminalSurface { surface_id: u32, x: f32, y: f32 },
    /// 수식키 hover 링크 위 terminal 우클릭: 선택 / 복사 / 연결 동작. 좌표는 logical px.
    TerminalLink {
        link: TerminalLinkMenu,
        x: f32,
        y: f32,
    },
    /// 비-terminal surface (markdown/image/explorer/html 등) right-click (T9).
    /// 전용 항목(현재 copy surface id) + 구분선 + 잘라내기/여기로 이동. 좌표는
    /// logical px 기준. terminal 은 selection-copy 가 있어 `TerminalSurface` 로 분리.
    Surface { surface_id: u32, x: f32, y: f32 },
    /// Explorer surface 내부 우클릭 (T11): 엔트리/다중선택/빈 영역 대상 파일 메뉴.
    /// `paths` 가 비면 빈 영역(=cwd) 대상. `single_is_dir` 는 `paths.len()==1` 일 때만
    /// 유효(폴더 전용 항목 게이팅). 좌표는 logical px.
    Explorer {
        surface_id: u32,
        paths: Vec<std::path::PathBuf>,
        cwd: std::path::PathBuf,
        single_is_dir: bool,
        x: f32,
        y: f32,
    },
    /// Explorer 사이드바 즐겨찾기 항목 우클릭 → "새 탭으로 열기"/"이 폴더로 루트
    /// 설정"/"즐겨찾기에서 제거". 제거는 전역 경로만으로 되지만, "루트 설정" 이
    /// 특정 explorer surface 의 cwd 를 바꾸므로 `surface_id` 가 필요하다.
    ExplorerFavorite {
        surface_id: u32,
        path: std::path::PathBuf,
        x: f32,
        y: f32,
    },
    /// "New workspace" 버튼 우클릭 (full/collapsed sidebar 공통): 프리셋으로 새 워크스페이스 생성
    NewWorkspaceButton { x: f32, y: f32 },
    /// 확장 사이드바 카테고리 헤더 우클릭 (토글 on): 비-normal 은 이름변경/삭제 +
    /// 새 카테고리, normal 은 새 카테고리만(additive).
    WorkspaceCategoryHeader {
        cat_id: crate::model::WorkspaceCategoryId,
        x: f32,
        y: f32,
    },
    /// 확장 사이드바 빈 배경 우클릭 (카테고리 토글 on/off 공통): 새 카테고리 · 원격 워크스페이스 추가.
    SidebarBackground { x: f32, y: f32 },
    /// 탭 "+" 버튼 우클릭: 프리셋으로 탭/페인 생성
    NewTabButton { pane_id: u32, x: f32, y: f32 },
}

/// All transient UI dialog/popup state, grouped to avoid AppState bloat.
/// New dialogs should be added here, not as top-level AppState fields.
pub struct DialogState {
    /// Unified rename dialog: target + edit buffer.
    pub(crate) rename: Option<(RenameTarget, String)>,
    /// 마우스 캡처 배너 "더보기" 컨텍스트 메뉴 대상 surface_id. 메뉴가 어느
    /// surface 의 배너에 대한 것인지 `mouse_capture_menu` draw_fn 에 전달한다
    /// (`RenameTarget` 과 동일 패턴 — popup 이 대상 정보를 직접 갖지 않아서).
    pub(crate) mouse_capture_banner_menu_target: Option<u32>,
    /// Surface convert popup: target surface_id (None = closed)
    pub(crate) convert_popup: Option<u32>,
    /// Keyboard-selected index in the convert popup menu
    pub(crate) convert_popup_selected: Option<usize>,
    /// Pending native context menu
    pub(crate) pending_native_menu: Option<PendingNativeMenu>,
    /// Pending file drag request (paths to drag to external apps).
    pub(crate) pending_file_drag: Option<Vec<String>>,
    /// Tab drag-and-drop state.
    pub(crate) tab_drag: Option<TabDragState>,
    /// Workspace drag-and-drop state.
    pub(crate) ws_drag: Option<WsDragState>,
    /// 부팅 시점 정보/에러 알림용 modal 큐. 큐 head를 [확인] 버튼으로 처리한다.
    /// `crate::adapters::ui::info_modal::show_info_modal()`로 push.
    pub(crate) info_modal_queue: VecDeque<InfoModal>,
    /// 휴먼 핸드오프 — 응답 대기 중인 approval 큐. popup 의 head 가 현재 화면.
    /// `approval.request` IPC 가 push하고, 선택지 클릭 시 pop.
    pub(crate) pending_approval_ids: VecDeque<tasty_approval::ApprovalId>,
    /// approval popup 의 코멘트 입력 버퍼 (현재 head용 임시 상태).
    pub(crate) approval_comment_buffer: String,
    /// file_handler_picker popup 의 입력/선택 상태. `None` 이면 popup 미오픈.
    pub(crate) file_handler_picker: Option<FileHandlerPickerData>,
    /// 네이티브 파일 피커(docs/features/native-file-picker/index.md) popup 의 내비게이션/로딩/선택 상태. `None` 이면 popup 미오픈.
    pub(crate) file_picker: Option<FilePickerData>,
    /// DAG 목록 popup 의 전 상태(검색/필터/열린 DAG/그래프 뷰). `on_close` 가
    /// 통째로 기본값으로 되돌린다 — popup 은 surface 가 아니라 snapshot/restore
    /// 대상이 아니고, 닫힌 뒤에도 남는 상태는 다음 open 을 오염시킨다.
    pub(crate) dag_list: crate::adapters::ui::popup::dag_list::DagListState,
    /// 도구 메뉴 클릭 / preset save 후속 — PresetView 를 열어달라는 요청.
    /// `selection` 이 `Some` 이면 PresetView 가 열린 뒤 해당 preset 을 선택한다.
    /// App 메인 루프 `process_pending_open_preset_window` 가 drain.
    pub(crate) pending_open_preset_window: bool,
    /// PresetView 가 열린 뒤 자동 선택할 preset. `pending_open_preset_window` 와 함께 사용.
    pub(crate) pending_preset_window_selection: Option<(tasty_presets::PresetKind, String)>,
    /// 프리셋 적용 picker popup 의 현재 하이라이트 (preset name). popup 닫힘 시 None.
    pub(crate) preset_picker_selected: Option<String>,
    /// `enter_copy_mode` 단축키 트리거 신호. MainView 가 다음 frame 에 소비.
    pub(crate) pending_enter_copy_mode: bool,
    /// 축소 레일 카테고리 팝업(`rail_category`)이 대상으로 하는 카테고리 id.
    /// `---` 버튼 클릭 시 set, popup 닫힘 시 None.
    pub(crate) rail_category_popup: Option<crate::model::WorkspaceCategoryId>,
    /// 카테고리 헤더 우클릭 메뉴의 "프리셋으로부터 워크스페이스 생성" 이 연 프리셋
    /// 적용 팝업(`APPLY_WORKSPACE_POPUP_ID`)이 대상으로 하는 카테고리 id. Apply 시
    /// `take()` 로 소비해 `Intent::ApplyPreset.category` 에 실어 보낸다. 어떤 경로로
    /// 닫히든 `on_close_apply_preset_popup` 훅(`popup/preset_apply.rs`)이 `None` 으로
    /// 정리하므로, 다음 open 시점에는 방어적으로 다시 리셋할 필요가 없다.
    pub(crate) preset_apply_target_category: Option<crate::model::WorkspaceCategoryId>,
    /// 카테고리 삭제 확인 다이얼로그(`confirm_delete_category`)의 대상 카테고리 id.
    /// Delete 액션 시 set, 확인/취소/닫힘 시 None.
    pub(crate) pending_category_delete: Option<crate::model::WorkspaceCategoryId>,
    /// 워크스페이스 강제 끊기 확인 다이얼로그(`confirm_force_detach_workspace`)의 대상
    /// **워크스페이스 id**. 사이드바 우클릭 메뉴가 set, 확인/취소/닫힘 시 None.
    ///
    /// 인덱스가 아니라 id 인 이유: 이 팝업은 메뉴가 닫힌 뒤에도 열려 있어 목록이 밀릴
    /// 창이 길다(기존 메뉴 콜백은 `ws_idx` 를 잡고 그 위험을 주석으로 인정한다).
    pub(crate) pending_force_detach_workspace: Option<crate::model::WorkspaceId>,
    /// Lua 스크립트 TOFU 변경 확인(`script_changed_confirm`) 팝업의 보류 상태 (ADR-0627).
    /// 등록 해시와 현재 파일 해시가 다르면 단축키 발화가 실행을 보류하고 이 값을 채운다 —
    /// 사용자가 [실행] 하면 `App::dispatch_pending_script_confirm` 이 해시를 갱신·영속하고
    /// 워커에서 실행하며, [취소]/Esc 면 슬롯을 폐기한다.
    pub(crate) pending_script_confirm: Option<PendingScriptConfirm>,
    /// 원격 전송 진행 팝업(`transfer_progress`) 상태. 진행 중인 파일 행들을 담고,
    /// 이미지 붙여넣기 업로드 워커의 진행 이벤트가 갱신한다. 모든 행이 끝나면 `None` + 팝업 self-close.
    pub(crate) transfer_progress: Option<TransferProgress>,
    /// 원격 전송 실패 팝업(`transfer_error`) 큐. 전송 실패/거부 시 push, Dismiss 시
    /// pop(큐가 비면 팝업 닫힘 — info_modal 큐 패턴). head 가 현재 화면.
    pub(crate) transfer_error: VecDeque<TransferError>,
}

/// Lua 스크립트 TOFU 변경 확인 팝업의 보류 상태 (ADR-0627).
///
/// 단축키 발화 시 등록 해시(`ScriptRegistry`)와 현재 파일 해시가 다르면 실행 대신 이 값을 채우고
/// 확인 팝업을 띄운다. [실행] 확정 시 `new_hash` 로 레지스트리를 갱신·영속하고 워커에서 실행.
#[derive(Debug, Clone)]
pub struct PendingScriptConfirm {
    /// 대상 스크립트 id (레지스트리 참조).
    pub(crate) script_id: String,
    /// 표시 이름(파일명 fallback 은 게이트가 미리 해소).
    pub(crate) name: String,
    /// 이미 읽은 현재 파일 소스 — 승인 시 그대로 실행(재읽기 없음, TOCTOU 축소).
    pub(crate) source: String,
    /// 현재 파일 소스의 SHA256 — 승인 시 레지스트리 해시를 이 값으로 갱신.
    pub(crate) new_hash: String,
    /// 팝업 wrapper 의 사용자 결정: `Some(true)`=실행, `Some(false)`=취소.
    pub(crate) result: Option<bool>,
}

/// Tab drag-and-drop state (UI-only, not persisted).
#[derive(Clone)]
pub struct TabDragState {
    pub(crate) pane_id: u32,
    pub(crate) tab_index: usize,
    /// Current mouse x in logical pixels (for insert position calculation).
    pub(crate) current_x: f32,
}

/// Workspace drag-and-drop state (UI-only, not persisted).
#[derive(Clone)]
pub struct WsDragState {
    pub(crate) ws_idx: usize,
    /// Current mouse y in logical pixels (for insert position calculation).
    pub(crate) current_y: f32,
}

impl DialogState {
    pub fn new() -> Self {
        Self {
            rename: None,
            mouse_capture_banner_menu_target: None,
            convert_popup: None,
            convert_popup_selected: None,
            pending_native_menu: None,
            pending_file_drag: None,
            tab_drag: None,
            ws_drag: None,
            info_modal_queue: VecDeque::new(),
            pending_approval_ids: VecDeque::new(),
            approval_comment_buffer: String::new(),
            file_handler_picker: None,
            file_picker: None,
            dag_list: Default::default(),
            pending_preset_window_selection: None,
            pending_open_preset_window: false,
            preset_picker_selected: None,
            pending_enter_copy_mode: false,
            rail_category_popup: None,
            preset_apply_target_category: None,
            pending_category_delete: None,
            pending_force_detach_workspace: None,
            pending_script_confirm: None,
            transfer_progress: None,
            transfer_error: VecDeque::new(),
        }
    }

    /// Returns true if any dialog with text input is open.
    pub fn has_text_input_open(&self) -> bool {
        self.rename.is_some()
    }

    /// Returns true if any dialog/popup overlay is open.
    pub fn has_any_overlay(&self) -> bool {
        self.has_text_input_open()
    }
}

/// file_handler picker popup 의 한 행 — handler 요약.
///
/// 행의 글리프와 이름은 handler 모델에 **없다** — 둘 다 도출한다. 그래서 요약이 도출에
/// 필요한 것만 들고 간다: 선언된 표시명(없으면 id 조각이 이름이 된다) · 출처 · action 이
/// 여는 surface kind · Recent 의 마지막 사용 시각.
#[derive(Debug, Clone)]
pub struct PickerHandlerSummary {
    pub(crate) id: crate::file::handler::HandlerId,
    /// 선언된 표시명(i18n key 가 풀린 값). `None` 이면 화면이 id 의 마지막 `/` 뒤 조각을
    /// mono 로 쓴다 — "선언된 이름이 없다" 가 눈에 보이게.
    pub(crate) display_name: Option<String>,
    /// 출처 — 둘째 줄 낱말(built-in / you / plugin)과 plugin mauve 판정.
    pub(crate) owner: crate::file::handler::HandlerOwner,
    /// `OpenSurface` 가 여는 surface kind. 행 글리프의 출처이고, `Ipc`/`System` 은 `None`.
    pub(crate) surface_kind: Option<String>,
    /// Recent 행의 마지막 사용 시각(unix epoch 초). 후보 행은 `None`.
    pub(crate) last_used_at: Option<i64>,
}

/// file_handler picker popup 의 상태.
///
/// 호출자가 popup 을 띄울 때 채워 넣는다. picker 는 직접 dispatch 하지 않고
/// 선택 결과를 [`FileHandlerPickerData::result`] 로 남긴다. host 본체 layer 가
/// frame 끝에 result 를 확인해 실제 핸들러 실행 + RecentPicks 기록을 수행한다.
#[derive(Debug, Clone)]
pub struct FileHandlerPickerData {
    /// Explicit dispatch owner, retained through picker selection and cancellation.
    pub(crate) origin_surface_id: Option<u32>,
    /// 원본 dispatch 의 출처. picker 왕복을 통과해 선택 후 `execute_handler_action`
    /// 까지 전달된다 — 사용자가 고른 결과는 선택하고 에이전트 것은 선택하지 않는다.
    pub(crate) dispatch_origin: crate::file::dispatch::FileDispatchOrigin,
    /// 원본 dispatch target. picker 가 닫힌 뒤 host 가 handler 를 실행할 때
    /// 사용한다 — `target_display` 는 화면용이라 escape/축약이 들어갈 수 있다.
    pub(crate) target: crate::file::dispatch::DispatchTarget,
    /// 표시용 — picker 헤더에 보일 대상 (예: 파일 경로).
    pub(crate) target_display: String,
    /// 탐지된 detector — 없을 수도 있음 ($unknown 등 unmatched).
    pub(crate) detector: Option<crate::file::format::DetectorId>,
    /// 목록의 첫 그룹(`Suggested` / fallback 이면 전체 핸들러) — handler id 사전순.
    pub(crate) candidates: Vec<PickerHandlerSummary>,
    /// `candidates` 가 detector 매칭 결과가 아니라 `FileHandlerRegistry::all_handlers()`
    /// fallback(이 포맷엔 매칭 핸들러가 없어 전체 핸들러를 대신 보여주는 경우)이면
    /// true. draw wrapper 가 그 그룹의 heading 을 `suggested_heading` 대신
    /// `fallback_heading` + caption 으로 바꾸고 attention 톤을 켜는 데 사용 —
    /// 1회성 dispatch 후보일 뿐 이 detector 에 영구 연결되지 않음.
    pub(crate) candidates_are_fallback: bool,
    /// 같은 목록의 둘째 그룹(`Recent`) — 현재 등록된 것만, 저장 파일 순서.
    pub(crate) recent: Vec<PickerHandlerSummary>,
    /// 이 형식에서 자동으로 실행됐을 handler — `default` Tag 가 붙는 행. fallback
    /// 후보(이 형식에 매칭되는 핸들러가 없음)에는 기본이 없으므로 `None` 이다.
    pub(crate) default_handler: Option<crate::file::handler::HandlerId>,
    /// 현재 선택된 handler. 더블클릭/[열기]로 dispatch.
    pub(crate) selected: Option<crate::file::handler::HandlerId>,
    /// dispatch 결과. host 본체 layer 가 frame 끝에서 소비.
    pub(crate) result: Option<FileHandlerPickerResult>,
    /// 원본 dispatch 의 크기제한 bypass 플래그. picker 왕복을 통과해 선택 후
    /// `execute_handler_action` 까지 전달된다(대용량 markdown 게이트 존중).
    pub(crate) ignore_size_limit: bool,
}

/// picker 의 닫기 사유.
#[derive(Debug, Clone)]
pub enum FileHandlerPickerResult {
    /// 사용자가 handler 선택. host 본체 layer 가 실행 + recent 기록.
    Selected(crate::file::handler::HandlerId),
    /// 취소 또는 ESC — dispatch 없음.
    Cancelled,
    /// 시스템 전체 handler 가 0개(빈 상태)일 때 "설정에서 핸들러 등록" 클릭.
    /// App 레이어(`dispatch_pending_picker_results`)가 `file::dispatch::apply_file_picker_result`
    /// 로 내려보내지 않고 직접 가로채 Settings 모달을 FileHandler 탭으로 연다 —
    /// Core 는 winit `ActiveEventLoop` 에 접근할 수 없다.
    OpenSettings,
}

/// 네이티브 파일 피커의 디렉토리 로드 상태. gallery specimen `FpState` 와 1:1
/// 대응 — `ErrorConn` 은 원격(mirror) 전용, 로컬은 `ErrorPerm`/`Loaded`/`Empty` 만 쓴다.
#[derive(Debug, Clone)]
pub(crate) enum FpLoadState {
    /// 원격 조회 요청을 보내고 응답 대기 중. `sent_at` 기준 soft timeout 이 지나면
    /// wrapper 가 `ErrorConn` 으로 전이시킨다(응답 없는 "서버는 살아있는데 응답이
    /// 안 오는" 케이스 — 세션의 `disconnected` 플래그만으론 못 잡는다).
    Loading {
        request_id: u64,
        sent_at: std::time::Instant,
    },
    /// 엔트리 로드 완료(`FilePickerData::entries` 참조, 비어있지 않음).
    Loaded,
    /// 로드 완료했으나 디렉토리가 비어있음.
    Empty,
    /// 권한 거부(로컬/원격 공통 — 파일시스템 자체의 read 실패).
    ErrorPerm(String),
    /// 원격 연결 끊김/타임아웃(원격 전용 — mirror 세션의 `disconnected` 플래그 또는
    /// soft timeout).
    ErrorConn(String),
}

/// `file_picker.trigger` IPC(ADR-0636)로 popup 을 연 plugin 의 요청자 정보.
/// Tools 메뉴가 연 경우(`requester: None`)와 구분해, 확정/취소 시
/// `"file_picker.result"` 이벤트를 이 plugin 에만 unicast 하는 데 쓴다.
#[derive(Debug, Clone)]
pub(crate) struct FilePickerRequester {
    pub(crate) plugin_id: String,
    /// `next_file_picker_trigger_request_id()` 발급 값 — `FpLoadState::Loading` 의
    /// 내부 `request_id` 와는 별개 네임스페이스(`core::mod` 문서 참고).
    pub(crate) request_id: u64,
    /// 이 피커를 연 plugin popup instance(= 부모). `file_picker.trigger` 의
    /// `owner_popup_instance` 파라미터로 plugin 이 자진 신고한 값이다 — host 는
    /// popup 밖(surface 위젯 등)에서 호출한 경우를 구분할 수 없으므로 `Option`.
    ///
    /// 소유 관계의 **유일한 보관처**다(ADR-0636). 별도 레지스트리를 두면 피커
    /// 수명과 어긋날 수 있어, 피커 자신이 들고 있게 했다 — 피커가 사라지면 관계도
    /// 같이 사라진다.
    pub(crate) owner_popup_instance: Option<u64>,
}

/// 네이티브 파일 피커 popup 의 상태. `file_handler_picker` 와 동일하게 popup
/// 은 직접 dispatch 하지 않고 결과를 [`FilePickerData::result`] 에 남긴다 — host
/// 본체 layer(`app::dispatch::file_picker`)가 frame 끝에 소비한다.
pub(crate) struct FilePickerData {
    /// `Some(local mirror workspace id)` 면 원격 브라우징(호스트 배지 렌더), `None`
    /// 이면 로컬. 트리거 시점의 **활성 workspace**(`AppState::active_workspace`) 기준으로
    /// 1회 판별해 고정한다(트리거 이후 활성 workspace 가 바뀌어도 popup 대상은 흔들리지
    /// 않음).
    pub(crate) mirror_ws_id: Option<u32>,
    /// 헤더 host 배지 문자열(`mirror_ws_id.is_some()` 일 때만 유효).
    pub(crate) remote_host: Option<String>,
    /// 현재 표시 중인 디렉토리(로컬: 절대경로, 원격: 원격 경로 문자열).
    pub(crate) current_dir: String,
    pub(crate) load: FpLoadState,
    /// 현재 디렉토리의 엔트리(`load` 가 `Loaded`/`Empty` 일 때만 최신).
    pub(crate) entries: Vec<crate::core::fs_list::DirEntryInfo>,
    /// 선택된 엔트리 이름(현재 `current_dir` 기준). 현재는 단일 선택만 지원 —
    /// `Select` action 이 매번 통째로 교체한다(멀티 셀렉트 토글 UI 는 스코프 밖).
    pub(crate) selected: Vec<String>,
    /// Confirm/Cancel 결과. host 본체 layer 가 frame 끝에서 소비.
    pub(crate) result: Option<FilePickerResult>,
    /// `Some` 이면 `file_picker.trigger` IPC 로 이 popup 을 연 plugin.
    /// Tools 메뉴 트리거는 `None`.
    pub(crate) requester: Option<FilePickerRequester>,
    /// `file_picker.trigger` 의 `filters?: string[]`(확장자, 점 없이) — 비어 있으면
    /// 필터 없음(모든 엔트리 표시). Tools 메뉴 트리거는 항상 비어 있음. 디렉토리는
    /// 필터와 무관하게 항상 표시한다(내비게이션 대상이라 필터로 숨기면 하위로 못 감).
    pub(crate) filters: Vec<String>,
}

/// 네이티브 파일 피커의 닫기 사유.
#[derive(Debug, Clone)]
pub(crate) enum FilePickerResult {
    /// 취소 또는 ESC — dispatch 없음.
    Cancelled,
    /// 사용자가 [열기] — 선택된 절대/원격 경로들과 원격 여부.
    Confirmed { paths: Vec<String>, is_remote: bool },
}

/// What is being renamed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenameTarget {
    /// Workspace name.
    WorkspaceName { ws_idx: usize },
    /// Workspace subtitle.
    WorkspaceSubtitle { ws_idx: usize },
    /// Tab name.
    TabName { pane_id: u32, tab_index: usize },
    /// Explorer 파일/폴더 이름 변경 (T11). 대상 surface + 현재 경로.
    ExplorerEntry {
        surface_id: u32,
        path: std::path::PathBuf,
    },
    /// Explorer 즐겨찾기 추가 (T11). rename 팝업과 동일 골격 — buffer = 표시 라벨.
    /// 대상 경로를 그 라벨로 전역 즐겨찾기에 등록한다.
    ExplorerAddFavorite { path: std::path::PathBuf },
    /// 새 워크스페이스 카테고리 생성 — buffer 빈 문자열로 시작, 확인 시 인라인 검증.
    NewCategory,
    /// 카테고리 이름 변경 — 대상 카테고리 id. buffer 초기값 = 현재 이름.
    CategoryName {
        cat_id: crate::model::WorkspaceCategoryId,
    },
}

impl RenameTarget {
    /// i18n key for the dialog heading.
    pub fn heading_key(&self) -> &'static str {
        match self {
            Self::WorkspaceName { .. } => "rename_dialog.title_heading",
            Self::WorkspaceSubtitle { .. } => "rename_dialog.subtitle_heading",
            Self::TabName { .. } => "rename_dialog.tab_heading",
            Self::ExplorerEntry { .. } => "explorer.popup.rename.title",
            Self::ExplorerAddFavorite { .. } => "explorer.popup.add_favorite.title",
            Self::NewCategory => "rename_dialog.new_category_heading",
            Self::CategoryName { .. } => "rename_dialog.category_heading",
        }
    }

    /// Popup scope matching the rename target.
    pub fn popup_scope(&self) -> crate::model::popup_kind::PopupScope {
        match self {
            Self::WorkspaceName { ws_idx } => {
                crate::model::popup_kind::PopupScope::Workspace(*ws_idx)
            }
            Self::WorkspaceSubtitle { ws_idx } => {
                crate::model::popup_kind::PopupScope::Workspace(*ws_idx)
            }
            Self::TabName { pane_id, tab_index } => {
                crate::model::popup_kind::PopupScope::Tab(*pane_id, *tab_index)
            }
            Self::ExplorerEntry { surface_id, .. } => {
                crate::model::popup_kind::PopupScope::Surface(*surface_id)
            }
            // 즐겨찾기는 전역이라 윈도우 스코프(특정 surface 에 묶이지 않음).
            Self::ExplorerAddFavorite { .. } => crate::model::popup_kind::PopupScope::Window,
            // 카테고리는 특정 워크스페이스/surface 에 묶이지 않으므로 윈도우 스코프.
            Self::NewCategory | Self::CategoryName { .. } => {
                crate::model::popup_kind::PopupScope::Window
            }
        }
    }
}
