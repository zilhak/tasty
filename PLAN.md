# 클립보드 히스토리 & 도구 시스템 설계 계획

## 1. 배경

Tasty가 실행 중일 때 시스템 클립보드를 모니터링하여 히스토리를 저장하고,
Popup과 Surface 양쪽에서 동일한 클립보드 뷰어를 제공한다.

이를 위해 Popup 시스템 리팩토링과 도구(Tool) 프레임워크를 함께 설계한다.

## 2. Popup 시스템 리팩토링

### 현재 문제

- Popup 컨테이너(`PopupContent` trait 구현체)가 거의 보일러플레이트
- 각 popup마다 struct + impl 5개 메서드를 반복 작성
- 실제로 다른 것은 `id`, `title`, `size`, `scope`, `close_on_outside_click`, `draw 함수` 뿐

### 방향: 컨테이너를 데이터화

PopupContent trait 구현체를 매번 만들지 않고, 설정 데이터 + draw 함수 포인터로 팝업을 정의한다.

```rust
// 팝업 = 설정 데이터 + 렌더링 함수
PopupDef {
    id: "clipboard_viewer",
    title_key: "tool.clipboard.title",  // i18n 키
    default_size: vec2(400, 500),
    scope: Window,
    close_on_outside_click: false,
    draw_fn: draw_clipboard_viewer,  // fn(&mut Ui, &mut AppState) -> PopupAction
}
```

기존 popup들(Notification, Convert, FileOpen, Bookmark)도 이 방식으로 마이그레이션한다.

### 기존 PopupContent trait 구현체와의 관계

- 기존 `PopupContent` trait은 `PopupDef` 내부에서 자동 구현하거나 제거
- 개별 struct (NotificationPopup, BookmarkNamePopup 등)는 제거하고 `PopupDef` 인스턴스로 대체

## 3. Surface/Window 렌더링 소스 분리

### 현재 문제

- Explorer만 `explorer_ui.rs`로 렌더링 분리됨
- Markdown, Html은 `egui_panels.rs`에 인라인
- 일관성 없음

### 방향: 모든 surface의 컨테이너와 렌더링을 파일 분리

| Surface | 모델 (컨테이너) | 렌더링 (UI) |
|---------|----------------|-------------|
| Explorer | `model/explorer_panel.rs` | `explorer_ui.rs` (이미 분리됨) |
| Markdown | `model/markdown_panel.rs` | `markdown_ui.rs` (인라인 → 분리) |
| Html | `model/html_panel.rs` | `html_ui.rs` (인라인 → 분리) |
| Empty | `model/empty_surface.rs` | `empty_ui.rs` (인라인 → 분리) |
| ClipboardViewer | `model/clipboard_viewer_panel.rs` (신규) | `clipboard_viewer_ui.rs` (신규) |

Surface/Window는 popup과 달리 각각 고유한 생명주기를 가지므로 (Terminal=PTY, Explorer=파일트리, Markdown=파일감시 등)
trait 추상화 없이 **파일 분리만** 한다. 컨테이너와 렌더링의 의존성은 유지.

`egui_panels.rs`는 surface 타입 판별 → 해당 `_ui.rs` 호출하는 디스패치 역할만 담당.

## 4. 클립보드 히스토리 시스템

### 저장소: EngineState

```rust
// engine_state.rs
pub struct EngineState {
    // ... 기존 필드 ...
    pub clipboard_history: ClipboardHistory,
}
```

- `EngineState`에서 관리하므로 윈도우가 없어도 (최소화 상태) 모니터링 지속
- `arboard` 크레이트로 시스템 클립보드 접근 (크로스 플랫폼)
- 500ms~1초 간격 폴링으로 변경 감지
- 최근 N개 저장 (설정 가능), 중복 연속 제거
- 텍스트만 지원 (1차)

### 클립보드 뷰어

동일한 렌더링 함수 `draw_clipboard_viewer(ui, state) -> PopupAction`를 사용하여:

- **Popup**: PopupDef로 등록, 단축키로 토글
- **Surface**: `ClipboardViewerPanel` (impl Surface), 탭에 들어가는 독립 surface
  - `SurfaceType::ClipboardViewer` 추가

두 형태 모두 `EngineState.clipboard_history`를 읽어서 최신순 목록 렌더링.

## 5. 사이드바 "도구" 버튼

### 위치

- collapsed sidebar: Expand 버튼 위에 도구 아이콘 버튼
- expanded sidebar: Collapse 버튼 위에 "도구" 텍스트 버튼

### 동작

클릭 시 컨텍스트 메뉴(네이티브 메뉴) 표시:
- 클립보드 뷰어 (popup 열기)
- (향후 다른 도구 추가 가능)

## 6. CLI: `tasty tool`

```
tasty tool clipboard list              # 히스토리 조회 (최신순)
tasty tool clipboard list --limit 20   # 최근 20개
tasty tool clipboard get --index 3     # 특정 항목 가져오기
tasty tool clipboard paste --index 3   # 시스템 클립보드에 다시 넣기
tasty tool clipboard clear             # 히스토리 초기화
```

IPC 핸들러에서 `EngineState.clipboard_history` 직접 접근.

## 7. 단축키

- 클립보드 뷰어 popup 토글: 기본 바인딩 설정 (예: `Ctrl+Shift+V`)

## 8. 구현 순서 (안)

1. Popup 시스템 리팩토링 (PopupDef 데이터화)
2. Surface 렌더링 소스 분리 (Markdown, Html, Empty)
3. 클립보드 히스토리 저장소 (EngineState + 폴링)
4. 클립보드 뷰어 렌더링 함수 (공유)
5. 클립보드 뷰어 Popup (PopupDef 등록)
6. 클립보드 뷰어 Surface (ClipboardViewerPanel + SurfaceType)
7. 사이드바 도구 버튼
8. CLI `tasty tool clipboard`
9. 단축키 바인딩
