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

### PopupDef (`src/ui/popup.rs`) — 데이터 지향 정의

모든 팝업은 **`PopupDef` 구조체**로 정의된다. 과거 trait 기반 구현은 제거되었다 (2026-04).

```rust
pub struct PopupDef {
    pub id: PopupId,
    pub title_key: &'static str,       // i18n 키 (draw 시점에 번역)
    pub title_fn: Option<fn(&AppState) -> String>,  // 동적 타이틀(선택, title_key 대신 사용)
    pub default_size: egui::Vec2,
    pub sizer: Option<fn(&AppState) -> egui::Vec2>,  // 동적 크기(선택)
    pub default_scope: PopupScope,
    pub close_on_outside_click: bool,
    pub headless: bool,                // 타이틀바·닫기 버튼 없이 콘텐츠만 렌더링
    pub sticky_focus: bool,            // 바깥 클릭해도 키보드 포커스 유지
    pub draw_fn: fn(&mut egui::Ui, &mut AppState) -> PopupAction,
}
```

모든 팝업 정의는 **`src/ui/popup_defs.rs`의 `all_defs()`**에 집중 모아 놓는다. 새 팝업 1개 추가 = 테이블 항목 1개 + draw 함수 하나.

### PopupManager (`src/ui/popup.rs`)

팝업의 공통 동작(z-order, 드래그, 타이틀바, clamp, 포커스)을 관리하는 중앙 매니저.

- `register_def(&PopupDef)`: PopupDef에서 PopupState 자동 생성 및 등록
- `open()` / `close()` / `toggle()`: 팝업 열기/닫기
- `open_with_scope()`: 동적 스코프 지정 + 센터링 (Surface 스코프 팝업에 사용)
- `draw()`: 모든 열린 팝업을 z-order 순으로 렌더링

### PopupState

개별 팝업의 공통 상태를 저장 (PopupManager 내부).

- `id`: 고유 식별자 (`&'static str`)
- `title`: 타이틀바에 표시될 텍스트 (매 프레임 `PopupDef.title_key`로 번역)
- `pos`: 위치 (논리 픽셀)
- `size`: 크기 (논리 픽셀, open 시점에 `sizer`로 1회 갱신)
- `open`: 열림 여부
- `focused`: 키보드 포커스 보유 여부
- `scope`: 스코프 (open_with_scope로 동적 변경 가능)

### 팝업 추가 방법

1. `src/ui/my_popup.rs`에 `pub fn draw_my_popup(ui, state) -> PopupAction` 함수를 정의.
2. `src/ui/popup_defs.rs`의 `all_defs()`에 `PopupDef { id: "my_popup", title_key: ..., default_size: ..., draw_fn: draw_my_popup, ... }` 항목 추가.
3. **열기**: Intent 발화 — `state.dispatch_intent(...)`. 직접 `state.popups.open*` 호출 금지.
   ```rust
   state.dispatch_intent(
       Intent::OpenPopup {
           id: "my_popup",
           mode: OpenPopupMode::CenteredFocused,
       }
       .from_user_menu("source_id"),
   );
   ```
4. **닫기**: 동일하게 Intent 발화 또는 popup draw 함수에서 `PopupAction::Close` 반환.
   ```rust
   state.dispatch_intent(
       Intent::ClosePopup { id: "my_popup" }.from_user_shortcut("escape"),
   );
   ```

추가적으로 `popup_defs`가 재빌드되지 않도록 ID는 **유일**해야 한다.

#### 직접 호출 금지

`state.popups.open*` / `state.popups.close*` / `state.popups.toggle*` 직접 호출은
금지된다 (clippy custom lint 또는 grep CI 로 강제). 예외 케이스는 `// intent-exempt: <사유>`
주석을 달아 명시 — 현재 예외:
- popup 도메인 핸들러 본문 (`src/intent/popup.rs`)
- popup 시스템 자체의 draw-prep / self-close cleanup (`src/ui/notification.rs`)
- Settings 윈도우 내부의 별도 `PopupManager` (`src/settings_ui/mod.rs`)

#### OpenPopupMode 종류

| Mode | 용도 |
|------|------|
| `Default` | 위치 자유, focus 없음. 일반 알림 패널 등. |
| `CenteredFocused` | 화면 중앙 + focus. 모달성 다이얼로그 (info_modal, command_palette 등). |
| `WithScope(scope)` | scope rect 기준 센터링. Surface/Window 종속 popup. |
| `AtTopOfScope(scope)` | scope 상단 정렬. search bar 등. |
| `AtFocused(pos)` | 지정 위치. context menu 등. |

#### Origin 메타데이터 선택

| Builder | 사용 시점 |
|---------|-----------|
| `.from_user_shortcut(name)` | 키보드 단축키 발화 |
| `.from_user_menu(name)` | 메뉴/사이드바 버튼/사이드 메뉴 발화 |
| `.from_user_context_menu()` | 우클릭 컨텍스트 메뉴 발화 |
| `.from_agent_ipc()` | **debug 빌드 한정** (`debug.popup.*` 의 사용자 입력 재현) |
| `.from_agent_plugin(id)` | **debug 빌드 한정** |
| `.from_agent_cli()` | **debug 빌드 한정** |
| `.cascaded_from(parent)` | 직전 Intent 의 origin 전파 (cascade 처리) |

#### Popup 발화 정책 (CRITICAL)

**Popup 은 사용자 행동 (키보드 단축키 / 마우스 / 메뉴) 에서만 발사된다.**
release 빌드의 시스템 / 에이전트 / 도메인 cascade 어느 경로도 popup 을 자동으로
띄울 수 없다. `toast-system.md` 의 "트리거 정책 (CRITICAL)" 과 동일한 원칙이
popup 에도 적용된다.

- ✅ 단축키 / 마우스 / 메뉴 → `OpenPopup` 발화
- ✅ popup A 의 *사용자 액션* 결과 cascade → popup B (origin 전파)
- ❌ IPC / CLI / Plugin 의 release 표면에서 popup 발화 — **금지**
- ❌ 시스템 조건 (PTY 종료, 시간 경과 등) 으로 자동 popup — **금지**.
  필요하면 *Domain Intent 로 데이터만 변경* (NotificationStore push 등) 하고
  UI 는 그 데이터를 *수동으로* 표시.
- ✅ Debug 빌드의 `debug.popup.*` IPC — *사용자 입력 재현* 용도로 한정
  (CLAUDE.md "사용자 입력 재현은 debug 한정" 원칙).

이 정책은 *타입 차원에서 강제* 한다 — Core / Domain handler 는 `UiIntent`
(즉 `OpenPopup` / `ClosePopup` / `TogglePopup`) 를 발화하는 메서드를 갖지 않는다.
GUI adapter (단축키 핸들러, 메뉴 콜백, popup draw 함수) 에서만 발화 가능.
상세: [`../plans/archived/phase-d/intent-ui-vs-domain.md`](../../.claude-workspace/plans/archived/phase-d/intent-ui-vs-domain.md).

상세 설계: [action-dispatch.md](action-dispatch.md)

### 등록된 팝업

| ID | draw_fn | 스코프 | 외부 클릭 닫기 | 포커스 고정 |
|---|---|---|---|---|
| `notifications` | `notification_popup::draw_notification_popup` | Window | No | No |
| `convert_surface` | `convert_popup::draw_convert_popup` | Surface (동적) | Yes | No |
| `markdown_open` | `file_open_popup::draw_markdown_open_popup` | Window | No | No |
| `html_open` | `file_open_popup::draw_html_open_popup` | Window | No | No |
| `bookmark_name` | `bookmark_popup::draw_bookmark_popup` | Window | No | No |
| `rename` | `dialog::draw_rename_popup` | Tab/Workspace (동적) | No | No |

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

### 포커스 고정 (sticky focus)

`PopupDef`에 `sticky_focus: bool` 속성이 있다. 기본값은 `false`.

- `false` (기본): 팝업 바깥 클릭 시 언포커스됨. 키보드 입력이 터미널로 돌아간다.
- `true`: 팝업 바깥을 클릭해도 **키보드 포커스가 팝업에 유지**된다. 마우스 이벤트는 정상적으로 터미널에 전달된다. 포커스 해제는 **팝업 닫기(Escape 등)**로만 가능.

용도: 검색 바처럼 열려있는 동안 키보드 입력을 항상 자신이 받아야 하면서, 터미널에서의 마우스 조작(스크롤, 텍스트 선택 등)은 허용해야 하는 오버레이.

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

`PopupState`에 `scope: PopupScope` 필드. `PopupScope` enum: `Window`, `Workspace(usize)`, `Pane(u32)`, `Tab(u32, usize)`, `Surface(u32)`. `PopupManager::draw()`에 `LayoutContext`를 받아 스코프별 가시성 필터링 및 clamp rect 결정.

## Modal과의 차이

| 항목 | Popup | Modal |
|---|---|---|
| 입력 차단 | 키보드: 포커스 시 차단. 마우스: 입력 계층에 따라 소비 | O (전역 입력 독점) |
| 동시 열기 | 여러 개 가능 | 최대 1개 |
| 위치 | 자유 이동 | 화면 중앙 고정 |
| z-order | 클릭 순서 | 항상 최상단 |
| 구현 | PopupManager | 별도 OS 윈도우 |
