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

`StageDef` 필드: `id`(진입 경로가 지정하는 이름) · `title_key`(i18n) · `draw_fn` · `on_close`.
무대 콘텐츠의 **자체 상태**는 popup 관례를 따른다 — 무대 id 로 키를 만든 egui temp memory 에
두고 `on_close` 에서 지운다. `StageState` 에는 무대 수명 그 자체에 속하는 것만 담는다.

## 셸이 그리는 것 / 콘텐츠가 그리는 것

셸(`draw_fullscreen_stage`)은 **창 전체 scrim + 상단 제목 + 종료 버튼**을 그리고, 그 아래를
`content_rect`(제목 띠 아래 · 바깥 여백 `space-xl`)로 잘라 `draw_fn` 에 child `Ui` 로 넘긴다.
콘텐츠는 chrome 위치를 알 필요도, 침범할 수도 없다.

**종료 버튼은 셸이 공통 제공한다** — 콘텐츠 구현자가 빠뜨릴 수 없어야 하기 때문이다. 무대
프레임은 host chrome 을 통째로 건너뛰어 창 닫기·최소화 버튼(CSD 타이틀바)까지 사라지므로,
종료 수단이 없는 무대는 창을 빠져나갈 방법이 없는 상태가 된다. `PopupManager` 가 X 버튼을
중앙 관리하는 것과 같은 구조다.

따라서 popup 형상을 옮겨오는 콘텐츠도 **popup 타이틀바는 다시 그리지 않는다** — 제목과 닫기는
셸 chrome 의 몫이라 역할이 겹친다. 재사용하는 "형상" 은 프레임(배경/보더)과 콘텐츠다.

## 진입 경로 — popup 타이틀바의 전체화면 버튼

무대를 선언한 popup(`PopupDef.fullscreen_stage = Some(<stage id>)`)은 타이틀바 X 왼쪽에
전체화면 버튼을 얻는다([popup.md 규칙 2·8](popup.md)). 누르면 그 무대가 뜨고 **원본 popup 은
열린 채 남는다** — 무대가 덮을 뿐이다. 두 정적 테이블(popup / 무대)의 id 정합은 단위 테스트가
강제한다.

첫 소비자는 **알림 무대**(`notifications`)다: popup 의 형상 함수
(`notification::draw_notification_content_inner`)가 `PopupState` 기하에 의존하지 않아 무대가
그대로 호출할 수 있고, 목록형이라 넓은 화면에서 실익이 있다. 무대의 자체 상태는 목록 스크롤
위치뿐이며(셸이 콘텐츠 Ui 를 무대 id 로 salt 하므로 popup 쪽과 다른 egui state 를 쓴다),
`on_close` 가 그것을 지운다.

## 진입 / 종료

- 진입 `AppState::open_fullscreen_stage(id)` — 테이블에 없는 id 는 `false` 로 거부. 다른 무대가
  올라와 있으면 **그 무대를 닫고**(훅 경유) 교체한다. 같은 id 재진입은 no-op(콘텐츠 상태가
  날아가지 않게).
- 종료 `AppState::close_fullscreen_stage()` — **닫는 경로 전부가 지나는 유일한 지점**
  ([ADR-0063](../../adr/0063-popup-close-hook-single-choke-point.md) 과 같은 패턴). 닫힌 id 를
  훅 대기열에 넣고, draw 경로가 `on_close` 를 정확히 1 회 발화한다.
- 훅 drain 은 **무대 프레임과 일반 프레임 양쪽**에서 돈다. 무대를 나오면 다음 프레임은 일반
  프레임이라 무대 draw 경로가 아예 돌지 않기 때문이다.
- 진입·종료 뒤에는 프레임이 필요하다. IPC 경로는 라우팅이 이미 `dirty` 를 세운다. **draw 중에
  일어나는 진입/종료**(popup 버튼 클릭 · 무대 종료 버튼)는 그 프레임의 dirty 를 이미 소비한
  뒤라 `ctx.request_repaint()` 로 다음 프레임을 직접 유도한다 — 빠뜨리면 다음 입력이 올
  때까지 화면이 바뀌지 않는다.

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

## OS 창 전환

무대의 경계는 작업영역이 아니라 **OS 창까지**다 — 무대가 서면 창 자체가 모니터를 덮는다.
브라우저 Fullscreen API 와 같은 모델이다: `requestFullscreen()` 은 새 창을 만들지 않고 **같은
창**을 OS fullscreen 으로 전환한 뒤 크롬 UI 를 숨긴다. 무대도 새 `View`(별개 OS 창)를 만들지
않고 그 `MainView` 의 winit 창을 전환한다. 구현은 `src/view/main/fullscreen_window.rs`.

**리컨실러다, 상태 머신이 아니다.** 무대 상태의 단일 수렴점(`open_fullscreen_stage` /
`close_fullscreen_stage`)은 `AppState` 위에 있고 `AppState` 는 headless 빌드에도 있어 winit
핸들을 들고 있지 않다. 그래서 전환 호출을 그 두 함수에 박는 대신 `MainView` 가 매 프레임
`fullscreen_stage_active()` 를 창에 반영한다 — WebView 노출을 `has_egui_overlay_open()` 에
맞추는 `sync_webviews` 와 같은 관례다. 여닫는 경로가 몇 개로 늘어나도 각 경로가 전환을 기억할
필요가 없고, 상태와 창이 어긋나면 다음 프레임에 수렴한다. 호출 위치는 `handle_redraw` 의
**render 뒤**다 — 무대는 자기 draw 안에서 닫힐 수 있어(`StageAction::Close`), 앞에 두면 그
프레임에 닫힌 무대의 창 복원이 다음 프레임으로 밀린다.

**`Borderless` 를 쓴다.** `Exclusive(VideoMode)` 는 모니터 해상도 자체를 바꾸는 게임용 모드라
다른 창들의 배치를 흐트러뜨리고 복귀 시 원래 배치가 돌아오지 않는다. 터미널은 해상도를 바꿀
이유가 없다.

**모니터는 `current_monitor()` 로 명시 지정한다** — 창이 있는 그 모니터를 덮는다. `Borderless(None)`
의 "현재 모니터" 판정은 winit 이 플랫폼 백엔드에 위임하므로 DE/컴포지터마다 해석이 다를 수
있다. `current_monitor()` 가 `None` 을 돌려주면 그 값이 그대로 `Borderless(None)` = 백엔드 판정
폴백이 되므로 별도 분기가 없다.

### 창 상태 복원

진입 **직전**의 창 상태(`was_fullscreen` / `was_maximized`)를 기록해 두고 종료 시 그대로 되돌린다.
그 기록의 존재 자체가 "이 fullscreen 은 무대가 만든 것" 의 마커를 겸한다.

| 진입 시점 창 | 무대 중 | 종료 후 |
|---|---|---|
| 일반 창 | fullscreen | 일반 창(진입 전 크기·위치) |
| maximize | fullscreen | maximize |
| **이미 OS fullscreen** | 그대로(재설정하지 않음) | **fullscreen 유지** |

셋째 줄이 핵심이다. macOS 신호등의 풀스크린 버튼처럼 **사용자가 직접 만든** 창 상태를 무대가
해제하면 "무대를 한 번 열었다 닫았더니 내가 만든 전체화면이 풀렸다" 가 된다. 무대는 자기가
만든 전환만 되돌린다. 같은 이유로 이미 fullscreen 인 창에는 `set_fullscreen` 을 다시 걸지도
않는다 — macOS 는 fullscreen 전환이 별도 Space 이동 애니메이션이라 중복 호출이 눈에 보이는
깜빡임이 된다.

### 리사이즈 잠금은 maximize 만이 아니다

리사이즈 엣지 hit-test(`view/main/mouse.rs`)는 `is_maximized()` 가 아니라 "maximize **또는** OS
fullscreen" 을 본다(`fullscreen_window::window_size_is_locked`). 무대 중에는 입력 게이트가
막아주지만 **무대 없이 fullscreen 인 창**(macOS 신호등, 또는 WM 단축키)이 가능하므로 게이트에
기대지 않는다.

### grid 는 두 전환 모두에서 불변이다

fullscreen 전환은 창 크기를 바꾸므로 `WindowEvent::Resized` 를 두 번 발생시킨다(진입 시 커짐,
종료 시 작아짐). 위 "무대 중 동결되는 것" 의 grid 동결이 **이 전환에 대해서도** 성립해야
한다 — 안 그러면 무대 진입만으로 뒤 터미널이 전부 리플로우된다. 종료 쪽은 리컨실러가 무대
상태를 이미 지운 뒤에 창을 되돌리므로 그 `Resized` 는 정상 경로로 흘러 진입 전 크기 = 진입 전
grid 로 돌아온다.

### 발화 정책

`set_fullscreen` 은 사용자가 보는 창을 바꾸므로 `docs/identity.md` 원칙 1 의 사용자 상태다.
release IPC/CLI 에 창 전환 API 를 노출하지 않는다 — 전환은 무대 상태를 따라갈 뿐이고, 무대
진입 경로 자체가 debug 격리이므로 자연히 그 경로만 탄다. 터미널 이스케이프로 창을 조작하는
경로도 없다([ADR-0011](../../adr/0011-xtwinops-window-ops-unsupported.md) 의 거부 결정 그대로).

### 플랫폼별 확인 결과

| 플랫폼 | 상태 | 내용 |
|---|---|---|
| **Linux / X11 / GNOME** | 확인(1회 수동) | 진입·종료·maximize 복원·사용자 fullscreen 유지·창 2 개 독립성 전부 실측. 전환 시 검은 프레임·잔상 없음. 무대 없이 fullscreen 인 창에서 CSD 타이틀바/캡션 버튼이 정상 렌더. 단 임시(비커밋) 훅을 통한 1 회 수동 실측이며 회귀 커버리지는 없다 — 진입 경로(todo/28) 확보 후 통합 검증으로 승격 필요 |
| **Linux / Wayland** | **미확인** | 검증 환경이 X11 세션이라 컴포지터 차이를 재현할 수 없다 |
| **Windows** | **미확인** | undecorated + `undecorated_shadow` 조합의 섀도/보더 잔상, 작업 표시줄 가림 여부. 해당 OS 없음 |
| **macOS** | **미확인** | 신호등 풀스크린과의 상태 어긋남, 별도 Space 이동 애니메이션, fullscreen 창 위 모달 z-order. 해당 OS 없음. 코드는 `fullscreen()` 조회 분기로 대응해 두었고 위 표의 셋째 줄이 그 계약이다 |
| **멀티 모니터** | **미확인** | 단일 모니터 환경이라 "창이 있는 모니터를 덮는가"·"두 모니터를 동시에 덮는가" 를 재현할 수 없다. 판정 수단으로 `MainView::fullscreen_window_report()` 가 덮고 있는 모니터의 이름·위치·크기·배율을 돌려준다 — 무대 조회 IPC 가 이 값을 실으면 출력만 보고 판정할 수 있다 |
| **DPI 가 다른 모니터로 전환** | **미확인** | 위와 같은 이유. `resync_scale_factor` → `update_grid_size` 경로는 무대 중 보류하도록 이미 배선돼 있다(위 "무대 중 동결되는 것") |

**무대 중 사용자가 창 fullscreen 을 직접 해제하면**(macOS 신호등 등) 무대는 그대로 남고 창만
작아진다. 무대 rect 는 창 크기를 추종하므로 작아진 창을 그대로 채운다 — 의도한 동작이다.
무대는 "창을 덮는 표면" 이지 "창을 fullscreen 으로 붙잡아 두는 잠금" 이 아니다.

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
  창 닫기·최소화 버튼까지 사라진다. 마우스 탈출 수단은 셸이 공통 제공하는 종료 버튼 하나뿐이며,
  **키보드 탈출 수단(단축키)은 아직 없다** — 별도 트랙이 붙인다.

## headless

무대는 화면 투영이라 headless 에 대응 도메인이 없다(`docs/identity.md` §2.2). 상태 필드와
API 는 `#[cfg(feature = "gui")]` 안에 있고, `AppState::fullscreen_stage_active()` 만 두 빌드
공통으로 존재해 headless 에서는 항상 `false` 를 돌려준다.
