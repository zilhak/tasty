# Explorer (내장 파일 관리자 surface)

- **Status**: Implemented
- **주체**: 로컬 사용자 · AI Agent ([주체](../../concepts/actors.md))
- **ADR**: 없음
- **코드**: surface kind 등록 `register_explorer` (`src/engine/surface_registry/builtins.rs`), 모델 `ExplorerPanel`/`ExplorerTab` (`crates/tasty-model/src/explorer_panel.rs`), 뷰 스토어 `ExplorerView`/`ExplorerViewStore` (`src/adapters/ui/surface/explorer/view.rs`), 렌더 `draw_explorer` (`src/adapters/ui/surface/explorer.rs`), deferred action 적용 `apply_explorer_action` (`src/adapters/ui/egui_panels.rs`)
- **화면**: host-rendered egui surface (터미널/마크다운/이미지와 동궤의 surface 타입)

## 목적

OS 파일 관리자에 의존하지 않고 tasty surface 안에서 디렉토리를 탐색하고 파일을 열기 위한 내장 파일 관리자다. 다른 host surface(terminal/markdown/image)와 동일하게 pane/tab 레이아웃에 들어가고, surface 변환·이동·레이아웃 영속화 대상이 된다. (과거 `com.tasty.explorer` plugin 이 제공하던 기능을 본체 host builtin surface 로 승격한 것 — surface kind `"explorer"` 는 부팅 시 `register_builtin_kinds` 가 등록한다.)

## 내부 동작 (headless-valid)

### 모델 (`ExplorerPanel`)

- `ExplorerPanel` 은 식별(`id`)과 내비게이션 상태만 보유한다 — 내부 탭 목록(`tabs`)·활성 탭 인덱스(`active`). 각 `ExplorerTab` 은 **cwd(고정 루트)** 와 **current(현재 폴더, 필드명 `root`)** 를 분리해 보유하고, 히스토리(back/forward 스택), 뷰 모드(`view_mode`), 정렬 컬럼/방향(`sort_column`/`sort_dir`)을 가진다.
- **cwd ↔ current 분리** (VS Code 식 "고정 프로젝트 + 자유 탐색"): `cwd()` 는 explorer 를 연 프로젝트 루트로 **좌측 사이드바 트리 루트**·**스폰 cwd**(`source_cwd()`)·**surface/탭 표시명**의 기준이며 내비게이션에 불변. `current()`(=`current_root()`) 는 **우측 목록**·**상단 breadcrumb** 이 따라가는 탐색 폴더로, back/forward/go_up 이 이것만 움직인다. current 는 cwd 하위로 제한되지 않고 파일시스템 어디로든 자유 이동한다.
- 내비게이션: `navigate_to(dir)` / `go_back` / `go_forward` / `go_up` — 모두 **current 에만** 작용. `can_go_up` 은 current 의 파일시스템 부모 존재만 본다(cwd 경계로 clamp 안 함). `set_cwd(folder)` 는 cwd·current 를 folder 로 재설정하고 히스토리를 비운다(explorer-03 "이 폴더로 루트 설정"). 히스토리는 탭별로 독립.
- **`..` 상위 이동**: current 에 부모가 있으면(파일시스템 루트 아님) 우측 목록 최상단에 `..` 특수 행을 그려 상위 폴더로 이동한다. `..` 는 **렌더 전용**이라 `view.entries`/선택/상태줄/컨텍스트 메뉴 대상이 아니며 더블클릭 시 `Navigate(parent)` 만 emit 한다.
- 내부 탭: `add_tab`(활성 탭 cwd 복제, current=cwd) / `close_tab(idx)` / `active_tab[_mut]`. surface 하나 안에 여러 디렉토리 탭을 둔다 (상위 pane 탭과 별개). 탭별 cwd 는 독립(per-tab).

### 뷰 상태 (`ExplorerView`, surface id 로 keying)

디렉토리 엔트리 캐시·선택 집합·트리 펼침 같은 무거운 GUI 상태는 모델이 아니라 per-surface 뷰 스토어에 둔다 (markdown/image 뷰 스토어와 동형).

- **엔트리 캐시**: `sync(panel)` 이 활성 탭의 `(root, sort_column, sort_dir)` 키를 보고 디렉토리/정렬이 바뀌었거나 새로고침이 요청됐을 때만 디스크에서 다시 읽는다. 디렉토리가 바뀌면 선택을 초기화한다. 읽기 실패는 `LoadState::NoPermission`(권한 거부) / `LoadState::Error(msg)` 로 분류해 콘텐츠 중앙 상태 텍스트로 표현한다.
- **선택**: `selected: HashSet<PathBuf>` + `anchor`(shift 범위 기준). `select_all()` 은 현재 디렉토리 전체를 선택, `selected_paths_text()` 는 선택 경로를 정렬·개행 결합한 클립보드 페이로드를 만든다.
- **사이드바 트리**: `expanded` 펼침 집합 + `tree_children` lazy 하위 디렉토리 캐시.

### 뷰 모드 / 정렬

- 뷰 모드 3 종(grid / list / detail)을 toolbar 의 공용 `segmented` 위젯으로 토글한다. detail 뷰는 정렬 컬럼 헤더를 클릭하면 해당 컬럼으로 정렬(같은 컬럼 재클릭 시 방향 토글).
- list/detail 데이터 행은 공용 `Table`(selectable)을, 사이드바 디렉토리 행은 공용 `tree_row` 를 재사용한다.

### deferred action 적용

렌더 중 발생한 사용자 상호작용은 `ExplorerAction`(OpenFile / Navigate / GoBack / GoForward / GoUp / Refresh / SetViewMode / SetSort / NewTab / CloseTab / SelectTab / ContextMenu) 으로 모았다가 `apply_explorer_action(state, engine, sid, act)` 에서 적용한다. 파일 열기/새로고침은 뷰 스토어만, 내비게이션·뷰모드·탭 조작은 **origin surface id 로 직접 지정**한 `ExplorerPanel` 을 가변 차용해 처리한다(포커스 독립). 경로가 바뀌면 `ExplorerView` 가 다음 draw 에서 자동 감지해 재로드한다.

- 파일 열기는 `DomainIntent::DispatchFile { origin_surface_id: Some(sid) }` 로 [file-handler](../file-handler/index.md) 에 위임한다 — explorer 자신은 파일 식별/디스패치 정책을 모른다.

### 컨텍스트 메뉴 · 파일 조작

우클릭 컨텍스트 메뉴는 **2-단계 네이티브 메뉴 패턴**([context-menu](../../dev-guide/context-menu.md))을 따른다: 렌더 중 우클릭을 감지하면 `ExplorerAction::ContextMenu { target, cwd, x, y }` 를 모으고, `apply_explorer_action` 이 이를 `PendingNativeMenu::Explorer`/`ExplorerFavorite` 슬롯에 선점한다(generic surface fallback 보다 먼저 실행돼 explorer 전용 메뉴가 이긴다). 이후 `MainView::process_pending_native_menu` 가 OS 네이티브 메뉴(`platform::native_menu::show_context_menu`)를 띄우고 선택 id 를 조작으로 번역한다.

대상(target)은 우클릭 위치/선택 상태로 결정한다(design §3.3 target rule): 선택 안의 항목 → 선택 전체, 선택 밖 → 그 항목으로 선택 리셋, 빈 영역 → cwd. variant 4종(빈 영역 / 파일 / 폴더 / 다중):

- **경로 복사** (`copy_path`, 다중은 개행 결합) → OS 텍스트 클립보드.
- **복사 / 잘라내기 / 붙여넣기** — explorer 내부 파일 클립보드(`CoreState::explorer_clipboard`, 단일 슬롯·세션 휘발)에 경로+cut 플래그를 담고, 붙여넣기에서 소비한다. 실제 파일 이동은 `explorer/ops.rs`(순수 fs 헬퍼 — 충돌 시 `(copy)` 접미사, 자기 자신/하위로 붙여넣기 거부, cut 의 cross-volume 은 copy+remove 폴백). 잘라내기는 이동 성공 시 클립보드를 비운다.
- **휴지통으로 이동** (`delete`) — `trash` 크레이트로 OS 휴지통에 보낸다(가역적이라 확인 모달 없음).
- **이름 변경** (`rename`, 단일만) — 공용 rename 팝업(`PopupDef`)을 재사용해 `std::fs::rename`.
- **OS 기본 앱으로 열기** (`open_in_system`, 단일 폴더만) — `platform::reveal::open_path`(Windows `explorer` / macOS `open` / Linux `xdg-open`).
- **즐겨찾기 추가** (`add_to_favorites`, 단일 폴더 또는 빈 영역) — 아래 참조.

### 즐겨찾기 (favorites)

전역(surface 무관)·영속 즐겨찾기. `~/.tasty/explorer-favorites.toml`(`[[favorite]]` 배열, label+path)에 저장되며 부팅 시 `CoreState::explorer_favorites`(`ExplorerFavorites`)로 로드된다. 메모리 mutator(`add`/`remove`)는 순수하고 디스크 반영은 호출처가 `save()` 로 한다(테스트가 디스크를 건드리지 않게 분리).

- **추가**: 컨텍스트 메뉴 "Add to favorites" → rename 팝업과 동일 골격의 입력 팝업(`RenameTarget::ExplorerAddFavorite`, 확정 버튼 라벨만 "Add")으로 라벨을 받아 등록(같은 경로 재등록 시 라벨만 갱신).
- **표시/이동**: 사이드바 상단 "Favorites" 섹션(비어있지 않을 때만)에 STAR 아이콘 행으로 나열, 클릭 시 해당 경로로 이동.
- **제거**: 즐겨찾기 행 우클릭 → `PendingNativeMenu::ExplorerFavorite` → "Remove from favorites". 즐겨찾기는 전역이라 경로만으로 제거한다.
- 즐겨찾기 목록은 프레임당 1회 스냅샷으로 `draw_explorer` 에 전달된다(렌더 루프에서 `engine` 가변 차용 충돌 회피).

## 인터페이스

### AI Agent (IPC/CLI)

explorer 는 일반 surface 생성 메커니즘으로 다룬다 (전용 IPC 추가 없이 generic 경로):

- 생성: `tasty new tab --type explorer [--path <dir>]` / `tasty new workspace --type explorer [--path <dir>]`. `--path` 미지정 시 inherit cwd → home 순으로 결정. (IPC: `DomainIntent::CreateTab { kind: "explorer", surface_params }`.)
- 조회/닫기: `tasty list surfaces` 에 `foreground_process`/`pane_id`/`workspace_id` 와 함께 나타나고, `tasty close ...` 로 닫는다 — 전 워크스페이스 순회·ID 직접 지정(포커스 독립).
- 변환: 다른 surface 를 explorer 로 in-place 변환 — `Intent::ConvertSurface { kind: "explorer" }`. cwd 미지정 시 source surface 에서 carry. [convert-surface](../convert-surface/index.md) 의 generic convert popup 도 registry kind 열거로 explorer 를 노출한다.

### 사용자 트리거 (단축키 — [KeybindingSettings](../keybindings/index.md))

모든 단축키는 `KeybindingSettings` 로 노출되며 하드코딩하지 않는다. explorer 포커스에서만 동작:

| 액션 | 필드 | Tasty 프리셋 기본 |
|------|------|------|
| 새로고침 | `explorer_refresh` | `F5` |
| 상위 폴더로 | `explorer_go_up` | `Alt+Up` |
| 전체 선택 | `select_all` | `Ctrl+A` / `Alt+A` |
| 경로 복사 | `copy_path` | `Alt+Shift+C` |
| explorer 로 변환 | `convert_to_explorer` | (기본 미할당) |

세 진입점(직접 키 매칭 `keybinding.rs`, 더블탭 `double_tap.rs`, action-id/Command Palette `dispatch.rs`)이 동일 효과를 낸다. 설정 UI 는 Keybindings 탭의 **Explorer** 서브탭.

### 폰트

Appearance → **Explorer** 서브탭에서 surface 폰트를 오버라이드한다 (`appearance.plugin_font_overrides["explorer"]`, `effective_font_for_kind("explorer")` 가 읽음).

## 비-목표 (Out of scope)

- 파일 식별/렌더 정책 — explorer 는 열기를 [file-handler](../file-handler/index.md) 에 위임한다.
- 컨텍스트 메뉴 파일 조작(복사/잘라내기/붙여넣기/이름변경)은 **에이전트(IPC/CLI) 노출 대상이 아니다** — 사용자 우클릭 조작 전용. surface 단위 이동은 [surface-move](../surface-move/index.md) 가 별도 제공한다.

## 관련

- [work-area](../work-area/index.md)(Surface/Tab/Pane 계층) · [file-handler](../file-handler/index.md)(파일 열기 위임) · [convert-surface](../convert-surface/index.md)(explorer 로/에서 변환) · [keybindings](../keybindings/index.md) · [settings](../settings/index.md)(폰트/단축키 탭)
