# 전체화면 무대(Fullscreen stage) 시스템

무대는 **창 전체를 독점하는 독립 표면**이다. 기존 Workspace/Pane/Tab/Surface 트리와
**병렬로** 존재하며, 기존 요소를 확대한 것이 아니다. 용어 구분은
[concepts/ubiquitous-language](../../concepts/ubiquitous-language.md), 결정의 근거·대안은
[ADR-0082](../../adr/0082-fullscreen-independent-stage.md). 이 문서는 시스템 *동작 모델* 이다.

구현: `src/adapters/ui/fullscreen.rs`(무대 셸·닫힘 훅 drain) + `.../fullscreen/defs.rs`(정적
테이블) + `AppState`(상태) + `Gpu::render`(렌더 분기).

## 모델

1. **별개 데이터** — 무대 콘텐츠는 뒤의 tasty 개체와 내부 로직상 연관이 없다. "이 popup 을
   전체화면으로" 는 그 popup 을 확대하는 것이 아니라 **같은 형상의 별개 인스턴스**를 무대에
   구성하는 것이다.
2. **원본은 그대로** — 무대가 유지되는 동안 뒤는 가려져 있으므로 **redraw 하지 않는다.**
   나올 때 그때 화면에 보이는 것을 다시 그린다.
3. **창당 하나** — 무대 상태는 `AppState`(= `MainView` 당 하나)의 `Option` 필드다. 한 창에
   최대 1 개, 창이 여럿이면 창마다 독립적으로 가질 수 있다.
4. **선언된 것만** — 무대에 올릴 수 있는 것은 정적 테이블(`fullscreen::defs::all_defs()`)에
   등록된 `StageDef` 뿐이다.
5. **휘발성** — 영속화하지 않는다. `src/core/layout_persistence/` 는 무대를 모른다(재시작이
   전체화면 상태로 부팅되면 창을 조작할 수 없다).

## 구조 — popup 시스템과의 대칭

| popup | 무대 |
|-------|------|
| `PopupDef` + `popup::defs::all_defs()` | `StageDef` + `fullscreen::defs::all_defs()` |
| `PopupManager`(다중 + z-order) | `Option<StageState>`(하나뿐이라 관리자 불필요) |
| `PopupManager::close` → `closed_queue` → `on_close` | `AppState::close_fullscreen_stage` → `stage_closed_queue` → `drain_on_close_hooks` |
| `draw_popup_layer` | `draw_fullscreen_stage` |

`StageDef` 필드: `id`(debug IPC 가 지정하는 이름) · `title_key`(i18n) · `draw_fn` · `on_close`.
무대 콘텐츠의 **자체 상태**는 popup 관례를 따른다 — 무대 id 로 키를 만든 egui temp memory 에
두고 `on_close` 에서 지운다. `StageState` 에는 무대 수명 그 자체에 속하는 것만 담는다.

## 진입 / 종료

- 진입 `AppState::open_fullscreen_stage(id)` — 테이블에 없는 id 는 `false` 로 거부. 다른 무대가
  올라와 있으면 **그 무대를 닫고**(훅 경유) 교체한다. 같은 id 재진입은 no-op(콘텐츠 상태가
  날아가지 않게).
- 종료 `AppState::close_fullscreen_stage()` — **닫는 경로 전부가 지나는 유일한 지점**
  ([ADR-0063](../../adr/0063-popup-close-hook-single-choke-point.md) 과 같은 패턴). 닫힌 id 를
  훅 대기열에 넣고, draw 경로가 `on_close` 를 정확히 1 회 발화한다.
- 훅 drain 은 **무대 프레임과 일반 프레임 양쪽**에서 돈다. 무대를 나오면 다음 프레임은 일반
  프레임이라 무대 draw 경로가 아예 돌지 않기 때문이다.
- 진입·종료 뒤에는 프레임이 필요하다. IPC 경로는 라우팅이 이미 `dirty` 를 세운다.

## 레이어

무대는 등록된 `egui::Area`(`Order::Foreground`)로 그린다 — 미등록 raw layer(그 tier 안에서
항상 최상단이 되는 [함정](../../architecture/input-layer.md))를 **의도적으로 쓰지 않는다.**
무대 프레임에는 경쟁할 레이어가 없다(host chrome·popup·오버레이가 이 프레임에 그려지지
않는다). "항상 최상단" 을 얻으려고 함정에 기댈 이유가 없고, 등록해 두면 `Areas::order` 추적과
입력 라우팅이 정상 경로를 탄다.

셸은 창 전체 scrim(`theme.scrim()`, 마커 오버레이와 같은 토큰) + 제목을 그리고, 그 안쪽을
`StageDef::draw_fn` 이 채운다.

## 렌더 파이프라인 — 무대 분기의 **위치가 계약이다**

무대 분기는 `Gpu::render` 안, **offscreen surface 스크린샷 처리 뒤 · 레이아웃/렌더 패스 앞**에
있다. 이것은 "조기 반환" 이 아니라 background live-frame 과 stage frame 의 **분리**다:

1. offscreen surface 캡처 — 무대와 무관하게 항상
2. 비시각 relay(egui-mesh forward · attach mesh relay) — 무대와 무관하게 항상
3. **분기** — 무대면 clear + 무대 콘텐츠만, 아니면 기존 합성 전부
4. window 스크린샷 캡처 + `present` — **양쪽 경로 모두**

| 잘못된 위치 | 죽는 것 |
|-------------|---------|
| `MainView::render_if_dirty` 조기 반환 | attach mesh relay. 로컬 전체화면이 원격 사용자 화면을 멈춘다(주체 간 비침범 위반) |
| `Gpu::render` 최상단 | `ui.screenshot --surface <id>`(offscreen)가 영구 대기 |
| capture+present 를 건너뜀 | `ui.screenshot`(window)이 영구 대기 — 무대 검증 수단 자체가 사라진다 |
| 레이아웃/`resize_all` 뒤 | 무대 중 PTY grid 재계산 → "원본 그대로" 계약 파괴 |

무대 중에도 **`dirty` 를 억제하지 않는다.** relay 전체가 로컬 `dirty` 프레임에 종속돼 있어,
"어차피 안 보이니 프레임을 아끼자" 는 최적화가 곧 원격 구독자 굶김이다. 이 네 제약은
`tests/fullscreen_stage_render_gate.rs` 가 구조 가드로 고정한다.

**PTY drain 은 계속 돈다.** drain 은 `AppEvent::TerminalOutput` 핸들러 몫이고 redraw 경로와
분리돼 있어 무대가 건드리지 않는다 — 무대 중에도 스크롤백이 정상적으로 쌓인다.

## 무대 중 동결되는 것 / 동결되지 않는 것

| 대상 | 무대 중 |
|------|---------|
| PTY grid(cols/rows) | **동결** — `handle_redraw` 의 `resize_all_terminals` 스킵. 창 크기가 바뀌어도 진입 시점 값 유지 |
| 신규 터미널 기본 grid | DPI 변경 시 갱신을 **보류**했다가 무대를 나온 첫 프레임에 1 회 적용 |
| swapchain 크기 | 따라간다(창을 안 따라가면 렌더가 깨진다) |
| PTY 출력 처리 | 계속 |
| attach mesh relay | 계속 |
| offscreen/window 스크린샷 | 계속 |
| WebView | **숨김** — 아래 |

### WebView 는 "스킵" 이 아니라 "숨김"

WebView 는 OS 네이티브 자식 뷰(macOS `WKWebView` / Windows WebView2 / Linux WebKitGTK)이고
wgpu 렌더 표면 **위**에 있다. 그리지 않아도 화면에 남으므로 반드시 `set_visible(false)` 가
필요하다. 무대는 `AppState::has_egui_overlay_open()` 에 참여하고, `MainView::sync_webviews` 가
그 값으로 reveal 을 결정한다 — popup 이 열렸을 때와 **같은 게이트**를 그대로 쓴다.

## 아직 없는 것 — 무대 위에 얹을 때 전제할 경계

무대 코어는 **화면**만 갈아끼운다. 다음 둘은 코어에 **없고**, 무대 위에 기능을 얹는 쪽이
직접 세워야 한다.

- **입력은 게이트되지 않는다.** 무대가 참여하는 `AppState::has_egui_overlay_open()` 의 소비처는
  **WebView 표시 여부 하나뿐**이다(`MainView::sync_webviews`). 키보드 경로
  (`view/main/keyboard.rs`)와 마우스 경로(`view/main/mouse.rs`)는 각자 `settings_open` 기반의
  **별도 `overlay_open` 식**을 쓰고 있어 무대를 아예 모른다 — 무대가 떠 있어도 키는 그대로
  터미널로 가고 클릭은 뒤의(보이지 않는) 위젯 좌표로 판정된다. 무대에 입력을 주려면 그
  게이트를 처음부터 새로 세워야 하고, 기존 두 식에 무대 조건을 얹을지는 그 작업의 판단이다.
- **무대 프레임에는 CSD 타이틀바도 없다.** 무대 프레임은 host chrome 을 통째로 건너뛰므로
  창 닫기·최소화 버튼까지 사라진다. **종료 수단(단축키 등)이 없으면 창을 빠져나올 방법이
  없다** — 무대에 진입 경로를 붙이는 작업은 종료 경로를 같은 범위에서 함께 붙여야 한다.

## headless

무대는 화면 투영이라 headless 에 대응 도메인이 없다(`docs/identity.md` §2.2). 상태 필드와
API 는 `#[cfg(feature = "gui")]` 안에 있고, `AppState::fullscreen_stage_active()` 만 두 빌드
공통으로 존재해 headless 에서는 항상 `false` 를 돌려준다.
