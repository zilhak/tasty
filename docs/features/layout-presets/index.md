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

### surface 영속 id

각 `PresetSurface` 는 **preset 파일 내에서만 고유한** 영속 식별자 `id`(`Option<u32>`, TOML `id = N`)를 갖는다. load→편집→save→재load 를 관통해 같은 surface 를 안정적으로 지목하기 위한 것으로, 향후 surface 단위 복구 커맨드의 타겟(= "preset 이름 + surface id")이 된다.

- **preset-local**: 전역 고유성은 요구하지 않는다(uuid 불요). `duplicate_preset` 복제본은 같은 id 집합을 그대로 갖는 것이 옳다.
- **하위호환·마이그레이션**: 구버전 TOML 에는 `id` 가 없다. `serde(default)` 로 결손을 허용하고, `LayoutPreset::normalize_surface_ids` 가 로드/저장 시 결손·중복 id 를 high-water mark 이후 번호로 **파일 전체 단위**(Workspace 는 모든 pane·tab 통합)로 결정적 재부여한다. 로드 시 정규화가 무언가 바꾸면 디스크에 되써 마이그레이션을 영속화한다(RO 파일시스템 등 되쓰기 실패는 로그만 남기고 메모리 정규화는 유지 — 멱등).
- **런타임 id 와 무관**: apply 는 적용 시 런타임 surface id 를 새로 발급하며 이 영속 id 를 쓰지 않는다. 편집기(`DemoLayout`)는 leaf 에 영속 id 를 그대로 채택하고(세션 재부여 없음), 신규 leaf 만 새 id 를 받는다 — split/remove/탭 추가 후에도 기존 surface 의 id 는 불변.

### 저장 / 편집

저장: 사이드바 워크스페이스 카드 우클릭 · 탭 타이틀/탭바 빈 공간 우클릭 · 도구 메뉴 "프리셋". 위치 `~/.tasty/presets/{kind}/<name>.toml`(파일명 = 정본, 같은 kind 내 중복 불가 — 충돌 시 `-N` suffix).

캡처 시 **deferred(미복원) 터미널 탭** — PTY 가 아직 spawn 되지 않아 트리에서 `EmptySurface { deferred_spawn: Some(..) }` placeholder 로 있는 비활성 탭 — 도 `kind="terminal"` + `cwd`(`DeferredSpawn.working_dir`)로 캡처된다. (`EmptySurface::kind()` 는 항상 `"empty"` 라, 캡처 경로가 `is_deferred()` 가드로 가로채 layout 영속화(`SavedSurface::capture_surface`)와 동형으로 처리한다.) 적용 시 빈 패널이 아니라 해당 cwd 의 터미널로 복원된다. PTY 가 한 번도 안 뜬 placeholder 는 세션 데이터(restore_command·scrollback)가 없으므로 cwd 만 옮긴다. convert 버튼만 보이는 진짜 빈 패널(비-deferred `EmptySurface`)은 그대로 `kind="empty"` 로 캡처된다.

**PresetView**(EditorView 계열, modeless, 종류별 1 인스턴스 — [hierarchy](../../concepts/hierarchy.md))는 L1 scope 탭(Workspace/Tab/Pane) 아래 2-depth list→detail 본문이다:

- **좌측 리스트**(196px): 현재 scope 의 저장된 preset 목록. row = 이름 + mono subtitle(workspace 는 저장된 subtitle, 없으면 pane/tab 개수 / tab·pane 은 surface·tab 개수). 선택 row 는 `surface-active` 채움 + 2px accent 좌측 bar. 헤더에 `N presets` + New preset(`+`) 버튼(현재 레이아웃 capture 가 아니라 terminal 1개짜리 최소 preset 생성 — 본문 capture 경로는 컨텍스트 메뉴 저장이 담당). 빈 scope → "저장된 프리셋이 없습니다.".
- **우측 detail**: 44px 툴바(좌: preset 이름+subtitle / 우: rename·duplicate·delete 아이콘 + Edit 버튼) 위에 선택 preset 의 **데모 레이아웃 미리보기**(상위 pane split = 카드+gap, 하위 surface split = hairline, leaf = kind 라벨, mini-tab 클릭 전환 — 구조만, 내용 렌더 없음). 툴바 rename·duplicate·delete 는 store 에 직결돼 즉시 동작(rename 은 인라인 입력).

#### WYSIWYG 편집 모드 (Edit 버튼)

툴바 **Edit** 버튼을 누르면 같은 미리보기 영역이 그 자리에서 편집 가능한 WYSIWYG 모드로 전환된다(별도 화면·모달 없음). Edit 는 primary **Done** 으로 바뀌고, 옆에 "자동 저장됨" 안내가 표시된다 — **별도 Save 버튼 없음**. 모든 변경은 `PresetStore::save_*_overwrite` 로 즉시 디스크에 write-through 된다(기존 preset 의 메타데이터는 보존하고 레이아웃 트리만 교체).

- **surface 선택**: 편집 모드에서는 모든 surface 가 1px hairline 윤곽을 얻고, 클릭으로 한 surface 를 선택하면 2px accent inset 윤곽 + **remove(제거) 핸들 1개**가 붙는다(우측 상단). split-right/split-down 핸들은 아래 **경계 hover-split 존**이 대체해 제거됐다. 마지막 한 장 남은 surface 제거는 무효(트리에 0-surface 탭을 쓰지 않음).
- **경계 hover-split 존 (마우스)**: 선택되지 않은 surface 의 4변 바깥 30% 밴드를 hover 하면 accent 22% 밴드 + 안쪽 변 2px accent 55% 분할선 overlay 가 뜨고 커서가 crosshair 로 바뀐다. 클릭하면 그 변으로 split 된다 — **좌/우 존 = 좌우(row) split, 상/하 존 = 상하(column) split**, **좌·상 존은 새 surface 가 first(좌/상)**, 우·하는 second. 축 길이가 46px 미만이면 그 축 밴드는 소멸(중앙 선택은 항상 가능). 선택된 surface 에서는 존이 뜨지 않는다(배경 클릭으로 선택 해제 후 가능). 기존 surface id 는 보존되고 새 surface 만 새 id 를 받는다.
- **탭 삭제 `×` (마우스)**: 편집 모드에서 탭이 2개 이상인 pane 의 active/hover 탭 우측에 14×14 close `×` 가 노출된다(탭 1개면 숨김 + no-op — pane 은 항상 탭 ≥1). 클릭하면 그 탭이 삭제되고 active 인덱스가 재클램프된다.
- **leaf 인라인 폼**: 선택한 leaf 의 중앙 라벨이 인라인 폼으로 바뀐다 — kind 드롭다운(Select) + 작업 디렉터리 Input(mono) + 시작 명령어 Input(mono, **kind=`terminal` 일 때만** 노출). kind 를 바꾸면 라벨이 즉시 갱신되고 시작 명령어 필드가 토글된다.
  - **kind 드롭다운은 `SurfaceKindRegistry` 를 진실 소스로 삼는다** — 편집기(`PresetView`)가 main engine 의 공유 `surface_registry` Arc 를 받아 프레임마다 스냅샷(`KindCatalog`)을 파생한다. 후보 목록은 런타임 등록 kind(플러그인 on/off)를 즉시 반영하고, 표시명은 registry 의 `display_name_i18n_key` 로 해석한다. `empty`/`attached` 는 사용자가 직접 만들 수 없는 내부 kind 라 후보에서 제외한다. 편집 중인 leaf 의 현재 kind 가 목록에 없으면(비활성 플러그인 등) 유실 방지로 덧붙는다. registry 미주입(main window 부재 등)이면 정적 fallback 목록(`terminal`/`markdown`/`image`/`explorer`/`html`)으로 graceful 하게 떨어진다.
- **이름/subtitle 인라인 편집**: 편집 모드에서 툴바의 preset 이름은 텍스트 입력으로, subtitle 은 (Workspace 한정 실제 필드일 때) 입력으로 바뀌어 포커스 해제 시 store 에 commit 된다.
- **트리 변형**: 편집 모델(`DemoLayout`)은 3계층 전부를 변형한다 — surface split · surface 제거 · 탭 추가(+) · **탭 삭제(×)** · **pane split** · **pane 제거**. 마우스로는 경계 hover-split 존(surface split·4방향·before/after)·remove 핸들(surface 제거)·`+` 버튼(탭 추가)·`×`(탭 삭제)로 트리거되고, 전부(pane split·pane 제거 포함)는 아래 **표준 단축키**로도 발화한다. 모든 변형은 기존 leaf/pane id 를 보존하며 자동 저장된다. 무효 가드: 마지막 surface 제거·마지막 탭 삭제(pane 은 항상 탭 ≥1)·루트 단일 pane 제거는 no-op. pane split 은 **Workspace scope 에서만** 유효(Pane/Tab scope 는 pane 트리가 없어 no-op).
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

`preset.get`/`preset.save` 는 preset 을 JSON 으로 그대로 직렬화/역직렬화하므로 각 surface 의 영속 `id`(위 [surface 영속 id](#surface-영속-id))가 공개 스키마에 자동 노출·왕복된다 — 향후 surface 단위 타겟팅(`--surface-id N`)의 토대다. `save` 로 들어온 결손·중복 id 는 저장 시 정규화된다.

## 관련

- [closed-tab-restore](../closed-tab-restore/index.md) · [layout-persistence](../layout-persistence/index.md) · [work-area](../work-area/index.md)
