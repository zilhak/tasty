# 레이아웃 프리셋 (Layout presets)

- **Status**: Implemented
- **주체**: 로컬 사용자 · AI Agent (`preset.*`)
- **ADR**: 없음
- **코드**: `tasty-presets` 크레이트, `~/.tasty/presets/{workspace,tab,pane}/<name>.toml`, `preset.*` 핸들러
- **화면**: PresetView (EditorView 계열, modeless)

## 목적

Workspace/Tab/Pane 레이아웃과 각 leaf surface 의 초기화 파라미터(kind·cwd·시작 명령어·kind 별 params)를 미리 저장해 재사용한다. [닫힌 항목 복원](../closed-tab-restore/index.md)(인메모리 LIFO)과 달리 **디스크 영구 저장**, 반복 사용 목적.

## 내부 동작

### 세 종류

WorkspacePreset(전체: 상위 레이아웃 + 모든 pane/tab/surface) · TabPreset(단일 탭) · PanePreset(단일 페인: 탭 목록 + 활성 탭). 셋 다 `LayoutPreset` trait 구현(`tasty-presets`).

### 저장 / 편집

저장: 사이드바 워크스페이스 카드 우클릭 · 탭 타이틀/탭바 빈 공간 우클릭 · 도구 메뉴 "프리셋". 위치 `~/.tasty/presets/{kind}/<name>.toml`(파일명 = 정본, 같은 kind 내 중복 불가 — 충돌 시 `-N` suffix).

캡처 시 **deferred(미복원) 터미널 탭** — PTY 가 아직 spawn 되지 않아 트리에서 `EmptySurface { deferred_spawn: Some(..) }` placeholder 로 있는 비활성 탭 — 도 `kind="terminal"` + `cwd`(`DeferredSpawn.working_dir`)로 캡처된다. (`EmptySurface::kind()` 는 항상 `"empty"` 라, 캡처 경로가 `is_deferred()` 가드로 가로채 layout 영속화(`SavedSurface::capture_surface`)와 동형으로 처리한다.) 적용 시 빈 패널이 아니라 해당 cwd 의 터미널로 복원된다. PTY 가 한 번도 안 뜬 placeholder 는 세션 데이터(restore_command·scrollback)가 없으므로 cwd 만 옮긴다. convert 버튼만 보이는 진짜 빈 패널(비-deferred `EmptySurface`)은 그대로 `kind="empty"` 로 캡처된다.

**PresetView**(EditorView 계열, modeless, 종류별 1 인스턴스 — [hierarchy](../../concepts/hierarchy.md))는 L1 scope 탭(Workspace/Tab/Pane) 아래 2-depth list→detail 본문이다:

- **좌측 리스트**(196px): 현재 scope 의 저장된 preset 목록. row = 이름 + mono subtitle(workspace 는 저장된 subtitle, 없으면 pane/tab 개수 / tab·pane 은 surface·tab 개수). 선택 row 는 `surface-active` 채움 + 2px accent 좌측 bar. 헤더에 `N presets` + New preset(`+`) 버튼(현재 레이아웃 capture 가 아니라 terminal 1개짜리 최소 preset 생성 — 본문 capture 경로는 컨텍스트 메뉴 저장이 담당). 빈 scope → "저장된 프리셋이 없습니다.".
- **우측 detail**: 44px 툴바(좌: preset 이름+subtitle / 우: rename·duplicate·delete 아이콘 + Edit 버튼) 위에 선택 preset 의 **데모 레이아웃 미리보기**(상위 pane split = 카드+gap, 하위 surface split = hairline, leaf = kind 라벨, mini-tab 클릭 전환 — 구조만, 내용 렌더 없음). 툴바 rename·duplicate·delete 는 store 에 직결돼 즉시 동작(rename 은 인라인 입력).

#### WYSIWYG 편집 모드 (Edit 버튼)

툴바 **Edit** 버튼을 누르면 같은 미리보기 영역이 그 자리에서 편집 가능한 WYSIWYG 모드로 전환된다(별도 화면·모달 없음). Edit 는 primary **Done** 으로 바뀌고, 옆에 "자동 저장됨" 안내가 표시된다 — **별도 Save 버튼 없음**. 모든 변경은 `PresetStore::save_*_overwrite` 로 즉시 디스크에 write-through 된다(기존 preset 의 메타데이터는 보존하고 레이아웃 트리만 교체).

- **surface 선택**: 편집 모드에서는 모든 surface 가 1px hairline 윤곽을 얻고, 클릭으로 한 surface 를 선택하면 2px accent inset 윤곽 + 핸들 클러스터(우측 split `dir=row` · 하단 split `dir=col` · 제거 danger)가 붙는다. 마지막 한 장 남은 surface 제거는 무효(트리에 0-surface 탭을 쓰지 않음).
- **leaf 인라인 폼**: 선택한 leaf 의 중앙 라벨이 인라인 폼으로 바뀐다 — kind 드롭다운(Select) + 작업 디렉터리 Input(mono) + 시작 명령어 Input(mono, **kind=`terminal` 일 때만** 노출). kind 를 바꾸면 라벨이 즉시 갱신되고 시작 명령어 필드가 토글된다.
  - **kind 드롭다운은 `SurfaceKindRegistry` 를 진실 소스로 삼는다** — 편집기(`PresetView`)가 main engine 의 공유 `surface_registry` Arc 를 받아 프레임마다 스냅샷(`KindCatalog`)을 파생한다. 후보 목록은 런타임 등록 kind(플러그인 on/off)를 즉시 반영하고, 표시명은 registry 의 `display_name_i18n_key` 로 해석한다. `empty`/`attached` 는 사용자가 직접 만들 수 없는 내부 kind 라 후보에서 제외한다. 편집 중인 leaf 의 현재 kind 가 목록에 없으면(비활성 플러그인 등) 유실 방지로 덧붙는다. registry 미주입(main window 부재 등)이면 정적 fallback 목록(`terminal`/`markdown`/`image`/`explorer`/`html`)으로 graceful 하게 떨어진다.
- **이름/subtitle 인라인 편집**: 편집 모드에서 툴바의 preset 이름은 텍스트 입력으로, subtitle 은 (Workspace 한정 실제 필드일 때) 입력으로 바뀌어 포커스 해제 시 store 에 commit 된다.
- **트리 변형**: 편집 모델(`DemoLayout`)은 3계층 전부를 변형한다 — surface split(우측/하단) · surface 제거 · 탭 추가(+) · **탭 삭제** · **pane split** · **pane 제거**. surface 변형과 탭 추가는 마우스 핸들/`+` 버튼으로도 트리거되고, 전부(탭 삭제·pane split·pane 제거 포함)는 아래 **표준 단축키**로도 발화한다. 모든 변형은 기존 leaf/pane id 를 보존하며 자동 저장된다. 무효 가드: 마지막 surface 제거·마지막 탭 삭제(pane 은 항상 탭 ≥1)·루트 단일 pane 제거는 no-op. pane split 은 **Workspace scope 에서만** 유효(Pane/Tab scope 는 pane 트리가 없어 no-op).
- **표준 단축키 (focus 기반)**: 편집 모드에서 본체와 동일한 `KeybindingSettings` 단축키로 편집을 조작한다 — 코드에 키를 하드코딩하지 않고 설정 필드를 그대로 매칭한다(§단축키 정책). 대상은 **현재 선택된 surface(leaf)** 와 그 leaf 가 속한 pane 이다. 선택이 없으면 전부 no-op(임의 대상 조작 금지). 텍스트 입력(이름/subtitle/cwd/시작 명령어) 포커스 중에는 문자 키가 입력으로 가도록 단축키 매칭을 차단한다.

  | 단축키 액션 (`KeybindingSettings`) | 대상 | 동작 |
  |-----|------|------|
  | `split_surface_vertical` / `split_surface_horizontal` | 선택 surface | 좌우 / 상하 분할 |
  | `close_surface` | 선택 surface | 제거(마지막 1장이면 no-op) |
  | `new_tab` | 소속 pane | terminal 탭 추가 |
  | `close_active` | 소속 pane | active 탭 삭제 → **마지막 탭이면 pane 제거로 폴백**(라이브 close_active 의 탭→pane 체인과 동형) |
  | `split_pane_vertical` / `split_pane_horizontal` | 소속 pane | 좌우 / 상하 pane 분할(**Workspace scope 한정**) |
  | `close_pane` | 소속 pane | pane 제거(루트 단일 pane 이면 no-op) |

  구현 위치는 `Act` enum 이 `demo_layout.rs` private 이고 편집 대상 `DemoLayout` 이 egui temp 캐시에만 살기 때문에 winit `handle_event` 가 아니라 egui 렌더 경로(`draw_preview` → `DemoLayout::apply_shortcut`)다. **제약**: double-tap 바인딩(`shift+shift`/`ctrl+ctrl`/`alt+alt`)은 `parse_binding` 이 거부하므로 편집기에서 지원하지 않는다 — 해당 액션에 double-tap 바인딩만 지정한 사용자는 일반 조합 바인딩을 추가로 지정해야 한다. 또 `KeybindingSettings` 스냅샷은 편집 창을 **열 때** 캡처되므로(appearance 주입과 동일), 설정 변경은 창을 다시 열어야 반영된다.

mini-tab strip 은 `tab_bar.rs`, split 라인은 `divider.rs` 위젯을 재사용한다.

### 적용 — 포커스 규칙

- 단축키(`apply_workspace_preset`/`apply_tab_preset`/`apply_pane_preset`, 기본 빈 칸): 적용 popup → 선택 → 새 인스턴스 생성 + **포커스 이동**.
- **CLI/IPC `preset.apply` 는 항상 `focus: false`** — 포커스 독립 원칙. 단축키 호출만 포커스 이동.

terminal 시작 명령어는 PTY ready 직후 stdin 에 한 줄 자동 입력.

## 인터페이스

`preset.{list,get,save,delete,rename,capture,apply}`(`SurfaceRead`/`SurfaceWrite`) — `tasty preset {list,get,save,delete,rename,capture,apply}`. 표 → [reference/api](../../reference/api.md#구조--workspace--pane--tab--surface--split--tree).

## 관련

- [closed-tab-restore](../closed-tab-restore/index.md) · [layout-persistence](../layout-persistence/index.md) · [work-area](../work-area/index.md)
