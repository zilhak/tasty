# 내부 팝업 시스템

> **상태: 구현 완료.** PopupManager 기반 공통 팝업 프레임워크 적용됨.

## 개요

내부 팝업(Popup)은 윈도우 내부에 존재하는 가상 창이다. 터미널과 공존하며 포커스를 독점하지 않는다. 모든 내부 팝업은 `PopupManager`를 통해 관리되며, 아래 7가지 규칙을 따른다.

## 규칙

### 1. 타이틀 + 콘텐츠 구조

모든 내부 팝업은 **상단 타이틀 영역**과 **하단 콘텐츠 영역**으로 구성된다. 타이틀 영역 높이는 28px 고정.

### 2. 타이틀바 구성

- 타이틀 텍스트는 **중앙 정렬**
- 타이틀 영역의 가장 우측에 **X(닫기) 버튼** 배치
- 닫기 버튼 호버 시 빨간색으로 하이라이트

### 3. 타이틀바 드래그 이동

타이틀 영역을 클릭 및 드래그하면 팝업 전체가 이동한다.

### 4. 타이틀바 커서

타이틀 영역에 마우스를 올리면 **grab(이동) 커서**로 변경된다.

### 5. 배경색 구분

팝업 배경색은 `theme.surface0`을 사용한다. 이는 터미널의 focused 배경(`#000000`)과 unfocused 배경(`theme.terminal_bg`)과 모두 다르므로, 팝업이 터미널 위에 있음을 시각적으로 구분할 수 있다. 타이틀바는 `theme.mantle`을 사용한다.

### 6. 윈도우 경계 제한

팝업의 어떤 부분도 윈도우 밖으로 빠져나가지 못한다. 윈도우 크기가 변경되면 팝업이 밀려나며 자동으로 위치가 조정된다.

### 7. 다중 팝업과 z-order

- 여러 팝업이 동시에 열릴 수 있다
- 나중에 열리거나 클릭된 팝업이 더 앞에 표시된다
- 팝업의 아무 부분을 클릭하면 해당 팝업이 최상단으로 올라온다
- 겹친 영역에서의 마우스 이벤트는 최상단 팝업만 받는다

## 구현

### PopupContent trait (`src/ui/popup.rs`)

모든 팝업은 `PopupContent` trait을 구현하여 자신의 고유 동작을 정의한다.

```rust
pub trait PopupContent {
    fn id(&self) -> PopupId;
    fn title(&self) -> String;
    fn default_size(&self) -> egui::Vec2;
    fn scope(&self) -> PopupScope { PopupScope::Window }
    fn close_on_outside_click(&self) -> bool { false }
    fn draw(&mut self, ui: &mut egui::Ui, state: &mut AppState) -> PopupAction;
}
```

### PopupManager (`src/ui/popup.rs`)

팝업의 공통 동작(z-order, 드래그, 타이틀바, clamp, 포커스)을 관리하는 중앙 매니저.

- `register_content()`: PopupContent에서 PopupState 자동 생성 및 등록
- `open()` / `close()` / `toggle()`: 팝업 열기/닫기
- `open_with_scope()`: 동적 스코프 지정 + 센터링 (Surface 스코프 팝업에 사용)
- `draw()`: 모든 열린 팝업을 z-order 순으로 렌더링

### PopupState

개별 팝업의 공통 상태를 저장 (PopupManager 내부).

- `id`: 고유 식별자 (`&'static str`)
- `title`: 타이틀바에 표시될 텍스트 (PopupContent에서 매 프레임 갱신)
- `pos`: 위치 (논리 픽셀)
- `size`: 크기 (논리 픽셀, PopupContent에서 매 프레임 갱신)
- `open`: 열림 여부
- `focused`: 키보드 포커스 보유 여부
- `scope`: 스코프 (open_with_scope로 동적 변경 가능)

### 팝업 추가 방법

1. `PopupContent` trait을 구현하는 struct 생성 (예: `src/ui/my_popup.rs`)
2. `AppState::new()`에서:
   - `popup_contents`에 `Box::new(MyPopup::new())` 추가
   - `popups.register_content(&MyPopup::new())` 호출
3. 열기: `state.popups.open("my_popup")` 또는 `state.popups.open_with_scope("my_popup", scope)`
4. 닫기: `state.popups.close("my_popup")` (또는 draw()에서 `PopupAction::Close` 반환)

### 등록된 팝업

| ID | 구현체 | 스코프 | 외부 클릭 닫기 |
|---|---|---|---|
| `notifications` | `NotificationPopup` | Window | No |
| `convert_surface` | `ConvertSurfacePopup` | Surface (동적) | Yes |

## 외부 클릭 닫기

`PopupState`의 `close_on_outside_click: bool` 필드로 팝업별 외부 클릭 동작을 제어한다.

- `false` (기본값): 팝업 바깥 클릭 시 **언포커스만** 수행. 팝업은 열려 있음. (예: 알림 패널)
- `true`: 팝업 바깥 클릭 시 **팝업을 닫음**. (예: Surface 타입 전환 팝업)

## 팝업 포커스

팝업은 **포커스 상태**를 가진다. 포커스된 팝업이 있으면 키보드 입력이 터미널로 전파되지 않는다.

- 팝업 클릭 → 해당 팝업에 포커스 (다른 팝업은 언포커스)
- 팝업 바깥 클릭 → 모든 팝업 언포커스 (터미널로 포커스 복귀)
- 팝업 닫기 → 자동 언포커스
- `PopupManager::has_focused()`: 포커스된 팝업 존재 여부 확인

이는 Modal의 전역 입력 독점과 다르다. 팝업 포커스는 **키보드만 차단**한다. 마우스 이벤트는 입력 계층(Input Layer)에 따라 팝업이 소비한다. 상세는 `input-layer.md` 참조.

## 팝업 스코프

팝업은 **소속 범위(scope)**를 가진다. 스코프에 따라 가시성과 경계 제약이 결정된다.

| 스코프 | 소속 대상 | 가시성 규칙 | 경계 제약 | 예시 |
|--------|----------|------------|----------|------|
| Window | 윈도우 | 항상 보임 (워크스페이스 전환과 무관) | 윈도우 경계 내 | 알림 패널 |
| Workspace | 워크스페이스 | 해당 워크스페이스 활성 시만 보임 | 워크스페이스 영역 내 | (향후 확장) |
| Pane | 페인 | 해당 페인 영역 내에 존재, 페인이 보일 때만 보임 | 페인 경계 내 | (향후 확장) |
| Tab | 탭 | 해당 탭이 활성 탭일 때만 보임 | 탭 소속 페인 경계 내 | (향후 확장) |
| Surface | 서피스 | 해당 서피스 영역 내에 존재, 서피스가 보일 때만 보임 | 서피스 경계 내 | Surface 타입 전환 팝업 |

### 스코프별 동작

- **Window 스코프**: 기존 PopupManager 동작과 동일. `screen_rect` 기준 clamp.
- **Workspace 스코프**: 다른 워크스페이스로 전환하면 숨겨짐. 돌아오면 다시 보임 (닫히는 게 아님).
- **Pane 스코프**: 해당 pane rect 기준 clamp. 드래그도 pane 경계 내에서만.
- **Tab 스코프**: pane 내에서 다른 탭으로 전환하면 숨겨짐.
- **Surface 스코프**: 해당 surface rect 기준 clamp.

### 구현

`PopupState`에 `scope: PopupScope` 필드. `PopupScope` enum: `Window`, `Workspace(usize)`, `Pane(u32)`, `Tab(u32, usize)`, `Surface(u32)`. `PopupManager::draw()`에 `PopupDrawContext`를 받아 스코프별 가시성 필터링 및 clamp rect 결정.

## Modal과의 차이

| 항목 | Popup | Modal |
|---|---|---|
| 입력 차단 | 키보드: 포커스 시 차단. 마우스: 입력 계층에 따라 소비 | O (전역 입력 독점) |
| 동시 열기 | 여러 개 가능 | 최대 1개 |
| 위치 | 자유 이동 | 화면 중앙 고정 |
| z-order | 클릭 순서 | 항상 최상단 |
| 구현 | PopupManager | 별도 OS 윈도우 |
