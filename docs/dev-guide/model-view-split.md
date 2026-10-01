# Model + Host View 분리 패턴

`tasty-model` 의 surface 모델은 GUI에 의존하지 않는다(egui/wgpu 직접 사용 금지). 휘발성 GUI 상태(텍스처·캐시·편집 세션·스크롤·팝업 버퍼)는 호스트 측 **View** 구조체에 둔다. host 가 egui 로 직접 그리는 내장 surface 에 뷰 상태를 더할 때 이 패턴을 따른다.

`tasty-model` 이 의존하는 type-\* crate 는 `tasty-type-geometry`(LogicalPx/PhysicalPx)와 `tasty-type-appearance` 다 — 둘 다 GUI 타입을 들이지 않는다.


공통 창 상태는 `src/view/state.rs`의 `ViewState`, OS 창과 GPU는 `ViewBase`, MainView 전용 선택·viewport·popup 데이터는 `MainViewState`에 둔다. 비동기 구조 완료가 화면 선택을 바꿀 때는 originating View identity와 NavigationState의 선택 세대를 확인한다. EngineSession의 kind 인스턴스는 EngineRuntime의 단일 컬렉션에서 빌리며, 구조 트리의 SurfaceDescriptor를 실행 객체로 downcast하지 않는다.

## 왜 분리하나

- **플러그인 호환성** — 모델은 직렬화 가능한 식별 정보만 보유 → plugin 프로세스가 같은 모델을 그대로 쓸 수 있다.
- **테스트 용이성** — 모델 단위 테스트가 GUI 컨텍스트 없이 가능.
- **정리 일관성** — View 가 store 에 모이면 surface 닫힘 시 한 곳에서 일괄 해제.

> 적용 대상은 **host 내장 surface**(host 가 egui 로 그리는 surface, 현재 explorer·dag_graph·empty)다. `image` 같은 **egui-mesh plugin surface** 와 `html`/`markdown`([ADR-0029](../adr/0029-webview-host-integration.md)) 같은 **webview plugin surface** 는 plugin 프로세스가 자기 상태를 들고 그리므로 이 패턴 밖이다 (→ [concepts/plugins](../concepts/plugins.md)).

## 어디에 무엇을 두나

| 종류 | 위치 | 예 |
|------|------|-----|
| 식별 정보 | model | `id`, `tabs`, `active`, `dag_id` |
| 직렬화 영속 상태 | model | mtime, 트리 구조 |
| egui 타입 | view | `egui::ColorImage`, `TextureHandle` |
| 편집 세션 머신 | view | `EditState`, `DragState`, `ActionHistory` |
| 휘발성 UI 버퍼 | view | popup 텍스트 버퍼, scroll offset, brush 설정 |

판단 기준: **"이 상태를 plugin 프로세스가 들고 있을 이유가 있는가?"** 없으면 view, 있으면 model.

## 패턴

<a id="1-model--슬림-식별탐색-정보-cratestasty-modelsrc"></a>

### 1. Model — 식별·탐색 정보 (`crates/tasty-model/src/`)

```rust
pub struct FooPanel { pub id: u32, pub file_path: String, last_mtime: Option<SystemTime> }
impl FooPanel {
    pub fn poll_reload(&mut self) -> Option<String> { /* 외부 변경 감지 시 새 콘텐츠 */ }
}
impl Surface for FooPanel {        // crates/tasty-model/src/surface_trait.rs
    fn kind(&self) -> &'static str { "foo" }
    /* ... */
}
```

### 2. View + Store — 호스트 측 (`src/adapters/ui/surface/<foo>/view.rs`)

```rust
pub struct FooView { pub content: String, pub texture: Option<egui::TextureHandle> }

#[derive(Default)]
pub struct FooViewStore { views: HashMap<SurfaceId, FooView> }
impl FooViewStore {
    pub fn get_or_init(&mut self, panel: &mut FooPanel) -> &mut FooView {
        let view = self.views.entry(panel.id).or_insert_with(|| FooView::new(panel));
        if let Some(c) = panel.poll_reload() { view.replace_content(c); }
        view
    }
    pub fn drop_view(&mut self, sid: SurfaceId) { self.views.remove(&sid); }
}
```

### 3. MainViewState 등록 + 정리 (`src/state.rs`)

```rust
pub struct MainViewState { /* ... */ pub(crate) foo_views: FooViewStore }

pub(crate) fn release_surface_views(&mut self, surface_id: u32) {
    /* ... */
    self.foo_views.drop_view(surface_id);  // ← 누락하면 close/reopen 시 텍스처/캐시 누수
}
```

### 4. 렌더 호출 — `mem::take` 패턴

디스패치 루프에서 `&mut FooPanel`(engine.workspaces 경로)와 `&mut FooView`(state.foo_views 경로)를 같은 상위 상태를 통해 동시에 빌리기 어려운 경우, 루프 직전에 store를 잠시 꺼낸다:

```rust
let mut foo_views = std::mem::take(&mut state.foo_views);
for info in &infos {
    if let Some(panel) = surface.as_foo_mut() {
        let view = foo_views.get_or_init(panel);
        draw_foo(ui, panel, view);
    }
}
state.foo_views = foo_views;   // 반드시 복원 (이후 state 접근 전에)
```

이 패턴은 `src/adapters/ui/egui_panels.rs`(메인 디스패치)에서 쓰인다.

## 안티패턴

- **Model 에 `egui::*` 필드** — plugin 호환성을 깬다. View 로.
- **store `drop_view` 누락** — close/reopen 누수. 모든 닫기 경로가 부르는 `release_surface_views` 로 강제.
- **`mem::take` 후 복원 누락** — 다음 프레임 빈 store → 전 view 재생성 → flicker/상태 손실.
- **panel↔view 양방향 의존** — view 는 panel 을 읽지만 panel 은 view 를 모른다.

## 현재 적용된 host surface

| Model | View | Store |
|-------|------|-------|
| `ExplorerPanel` (id, tabs, active) | `ExplorerView` (entries, …) | `MainViewState::explorer_views` |
| `DagGraphSurface` (id, dag_id, workspace_id, direction) | `DagGraphView` (data, layout cache, …) | `MainViewState::dag_graph_views` |
| `TerminalSurface` / `EmptySurface` | (없음 — GPU 렌더 또는 id-only) | — |

신규 host surface 추가 시 이 표에 줄을 더한다. plugin surface(`image`/`html`/`markdown`)는 여기 들어오지 않는다.

## 구조 선택의 projection

Workspace·Pane·Tab의 구조에는 현재 사용자 선택을 저장하지 않는다. View의 navigation은 workspace·pane·tab·surface ID를 선택하고, 현재 구조와 대조해 삭제된 선택만 보정한다. 인덱스는 UI 입력과 기존 IPC·저장 형식의 경계에서 변환한다. 이전 순서는 삭제 시 이웃을 찾는 보정 자료이며 별도의 선택 원본이 아니다.

모델의 `StructurePresentation`은 구조를 기존 조회·attach·복원 DTO로 만드는 읽기 전용 입력이다. 직렬화와 복원 사본 캡처가 이 입력을 명시적으로 받으며, 모델이 View 선택을 변경하지 않는다. `Tab::surface`와 `surface_mut`는 명시 surface ID를 찾고 없는 ID에는 `None`을 반환한다. 구조 자체의 대표 surface가 필요한 호출자는 `first_surface_id`라는 기준을 명시한다.


## 요청 문맥과 headless

GUI의 `MainViewState`는 `src/state/main.rs`에, GUI 없는 `CommandContext`는 `src/state/command.rs`에 별도 구조체로 정의한다. headless는 MainViewState를 생성하지 않는다. CommandContext의 navigation 값은 로컬 사용자 포커스가 아니라 기존 생략 대상 해소와 응답 호환에 필요한 기본 문맥이다. popup·sidebar·OS/GPU 자원과 설정창 열림 상태는 소유하지 않는다.

공통 App adapter의 `RequestContext`는 빌드에 맞는 수신 타입을 재노출하는 이름이다. 두 원본을 공유하거나 동기화하는 wrapper가 아니며 Core 명령은 이 타입을 받지 않는다. 구조 실행과 결과 처리는 `src/app/structural_exec.rs`와 `structural_cascade.rs`에서 수행한다. 삭제/이동 결과를 받은 뒤 선택 ID와 표시 map을 보정하고, 사용자 생성 continuation만 새 대상을 선택한다.

분할 트리의 `SplitNodeId`는 프로세스 내부에서만 사용하는 node identity다. 기존 `focus_second` wire bool은 navigation의 별도 split-hint map에서 합성한다. 노드 이동·재결합은 ID를 유지하고 새 노드는 새 ID를 받는다. split 생성의 hint 기본값은 true, layout/preset/undo 복원은 false이며 remote 입력은 받은 값을 보존한다. 이 ID는 wire나 journal의 영속 식별자가 아니다.

Mirror는 원격 pane/tab ID에 안정된 로컬 ID를 대응시킨다. snapshot 재구성에서 생존한 로컬 선택을 유지하고, 선택 ID가 사라지면 원격 기본값을 적용한다. 사용자 닫기의 인접 후보가 있으면 그 후보를 우선한다. 삭제된 pane/tab의 대응 ID는 매 재구성 후 회수한다. 연결·재연결·delta 결과에서 View map도 정리하므로 parked 상태에서도 redraw를 기다리지 않고 삭제된 선택·스크롤·hint 자료를 회수한다.

사용자 탭 생성 후속 처리는 공통 App 실행 helper가 결과 ID에 적용한다. 호출자가 확인한 로컬 사용자 origin만 이 continuation을 허용하며, 원격 사용자 요청은 서버의 로컬 선택을 바꾸지 않는다. 원격 layout 파서는 immutable source 참조를 한 문맥으로 빌려 쓰고 선택/hint 적용과 리소스 데이터를 복제하지 않는다.

닫힌 구조를 복원하면 새 객체의 내부 선택과 legacy split hint는 origin과 무관하게 복원 자료로 초기화한다. 이미 존재하는 선택은 보존하며, 복원된 탭·pane·workspace로 사용자를 옮기는 후속 선택만 User origin에 제한한다. attach 전송 projection은 구조 결과의 보정값을 반영하고, 원격 사용자 생성의 일회성 active 값은 서버 View 선택으로 역수입하지 않는다. 새 구독은 그 시점 서버 View/명령 기본값으로 다시 캡처한다.
