# 토스트 시스템 (Toast System)

> **상태: 설계.** ToastManager 기반 휘발성 인앱 알림 프레임워크.

## 개요

**Toast**는 윈도우 내부에 잠깐 떠올랐다가 자동으로 사라지는 **휘발성 피드백 UI**다. "복사됨", "북마크 추가됨" 같이 사용자의 즉각적인 동작 결과를 알릴 때 쓴다.

Popup과 비교하면 다음이 다르다.

| 항목 | Popup | Toast |
|------|-------|-------|
| 수명 | 사용자가 닫을 때까지 | 일정 시간 후 자동 소멸 (기본 2초) |
| 포커스 | 클릭 시 키보드 포커스 보유 가능 | **절대 포커스를 받지 않는다** |
| 입력 | 클릭/드래그/X버튼 동작 | 입력 이벤트를 소비하지 않는다 (마우스 통과) |
| 타이틀바 | 28px 타이틀 + X 버튼 + 드래그 | 없음. 본문만 |
| 위치 | 자유 이동 | 스코프별 고정 위치(스택). 사용자가 옮기지 않는다 |
| z-order | 클릭 순서 | 항상 최상단(전경 레이어). 토스트 간에는 새것이 위 |
| 트리거 주체 | 사용자 또는 에이전트 | **사용자 행동만**. CLI/IPC는 토스트를 띄우지 않는다 |

## 트리거 정책 (CRITICAL)

**Toast는 사용자 행동(키보드 단축키 / 마우스)에서만 발사된다.** CLI/IPC를 통한 에이전트 동작은 사용자의 시각 상태에 영향을 주면 안 되므로 토스트를 띄우지 않는다. `docs/design/popup-system.md`의 "사용자 행동과 에이전트 행동의 분리" 원칙을 그대로 따른다.

복사를 예로 들면:
- 터미널 선택 후 `Ctrl+C` → 토스트 ✅
- Explorer에서 `Shift+Ctrl+C`로 경로 복사 → 토스트 ✅
- 클립보드 뷰어에서 항목 클릭하여 복사 → 토스트 ✅
- IPC `clipboard.set` → 토스트 ❌
- OSC 52(터미널 프로그램이 보낸 클립보드 시퀀스) → 토스트 ❌ (사용자가 직접 누른 동작이 아님)

## 스코프 (Scope)

Toast도 Popup과 동일한 **스코프**를 가지지만, 의미가 단순하다. 스코프는 **어느 영역에 떠오를지를 결정하는 위치 앵커**일 뿐, 가시성 필터의 역할은 거의 없다(토스트는 어차피 짧게 떴다 사라지므로).

| 스코프 | 앵커 | 예시 |
|--------|------|------|
| Window | 윈도우 우측 하단 | 전역 알림 |
| Workspace | 워크스페이스 영역 우측 하단 | 워크스페이스 단위 작업 결과 |
| Pane | 해당 Pane 우측 하단 | Pane 단위 작업 결과 |
| Surface | 해당 Surface 우측 하단 | "복사됨" 등 surface에서 일어난 동작 |

기본 스코프는 `Surface`. 어느 surface에서 일어난 일인지 모르면 `Window`로 넘어간다. 같은 스코프 내에서 여러 토스트가 동시에 살아 있으면 **아래에서 위로 쌓는다** (가장 새 것이 가장 아래).

스코프가 화면에서 사라지면(다른 워크스페이스로 전환 등) 해당 토스트는 즉시 제거된다 — 어차피 짧게 살다 갈 운명이다.

## 시각/레이아웃

- 배경: `theme.surface0` (Popup과 동일한 칩셋)
- 보더: 1px `theme.surface1`
- 코너 라운드: `theme.corner_radius`
- 안쪽 여백: 좌우 12px / 상하 8px
- 폰트: `theme.font_size_body`
- 가로폭: 본문 길이에 맞춰 자동, 단 스코프 영역 폭의 80%를 넘지 않는다
- 세로폭: 한 줄 또는 본문에 맞춤
- 위치: 스코프 우측 하단에서 안쪽으로 12px 여백, 토스트 사이 간격 6px
- 페이드: 등장 80ms / 소멸 160ms (애니메이션은 알파만, 위치 이동 없음 — `docs/design/theme-system.md`의 "터미널 콘텐츠 애니메이션은 0ms" 규칙은 비-터미널 알림 UI에는 적용되지 않음)

종류별 강조 색은 좌측 4px 컬러 바로 표현한다.

| 종류 | 좌측 바 | 용도 |
|------|---------|------|
| Info | `theme.blue` | 일반 정보 ("복사됨" 등 기본값) |
| Success | `theme.green` | 성공적인 완료 ("저장됨" 등) |
| Warning | `theme.yellow` | 주의 |
| Error | `theme.red` | 실패 |

## 입력 동작

Toast는 **입력 이벤트를 소비하지 않는다**. 토스트 위에서 마우스를 클릭/드래그해도 토스트는 무시되고 이벤트는 그 아래 레이어(터미널/popup/divider 등)로 그대로 전달된다. `popup_hovered`도 토스트 영역에서는 false 그대로다.

키보드 포커스도 받지 않는다. `PopupManager::has_focused()`와 무관하게 토스트는 늘 0표를 행사한다.

## 합치기/제한

같은 스코프에서 같은 메시지가 짧은 시간(기본 500ms) 내에 다시 발사되면 새 토스트를 만들지 않고 **기존 토스트의 수명을 갱신**한다(연속으로 Ctrl+C를 눌렀을 때 토스트가 깜빡이지 않게).

스코프당 최대 동시 표시 개수는 5개. 초과하면 가장 오래된 것을 즉시 제거한다.

## 구현 (`src/ui/toast.rs`)

### Toast 종류

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Success,
    Warning,
    Error,
}
```

### ToastScope

`PopupScope`와 같은 enum이지만 **위치 앵커 용도로만** 쓴다.

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum ToastScope {
    Window,
    Workspace(usize),
    Pane(u32),
    Surface(u32),
}
```

### ToastState

개별 토스트 인스턴스 상태.

```rust
pub struct ToastState {
    pub id: u64,
    pub message: String,
    pub kind: ToastKind,
    pub scope: ToastScope,
    pub spawned_at: Instant,
    pub lifetime: Duration,   // 기본 2s
}
```

### ToastManager

```rust
pub struct ToastManager {
    toasts: Vec<ToastState>,
    next_id: u64,
}

impl ToastManager {
    pub fn push(&mut self, message: impl Into<String>, kind: ToastKind, scope: ToastScope);
    pub fn push_info(&mut self, message: impl Into<String>, scope: ToastScope);
    /// 매 프레임 호출. 만료된 토스트를 제거하고 살아있는 것을 그린다.
    pub fn draw(&mut self, ctx: &egui::Context, draw_ctx: &LayoutContext);
}
```

`draw()`는 `LayoutContext`를 받아 스코프별 rect를 얻는다. PopupManager와 같은 컨텍스트를 공유하므로 별도 계산이 필요 없다.

### AppState 통합

```rust
pub struct AppState {
    // ...
    pub toasts: crate::ui::ToastManager,
}
```

draw 호출은 `ui::draw_popups` 직후(= popup의 위)에서 한다.

### 발사 헬퍼

`AppState`에 thin helper를 둔다:

```rust
impl AppState {
    pub fn toast_copied(&mut self, surface_id: Option<u32>) {
        let scope = surface_id
            .map(ToastScope::Surface)
            .unwrap_or(ToastScope::Window);
        self.toasts.push_info(t("toast.copied"), scope);
    }
}
```

## i18n

모든 토스트 문자열은 `t("toast.*")` 키를 사용한다. `lang/{en,ko,ja}.toml` 세 파일에 동시 추가.

## Popup과의 관계

Toast는 Popup의 한 변종이 **아니다**. PopupManager에 얹지 않고 별도 `ToastManager`로 둔다. Popup 7대 규칙(타이틀바, X 버튼, 드래그, z-order 클릭 승격, close_on_outside_click 등)은 토스트와 정면으로 충돌하기 때문이다.

다만 스코프 정의(`ToastScope`)와 스코프-rect 계산은 `LayoutContext`를 재사용해 일관성을 유지한다.
