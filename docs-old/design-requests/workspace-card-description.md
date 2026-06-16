# Design Request — Workspace Card Description

## 배경

- 코드의 `WorkspaceEntryView` (sidebar view 의 owned snapshot) 에는 `description: String` 필드가 정의되어 있다 (`src/adapters/ui/sidebar/view.rs:23`).
- `description` 은 workspace 의 메타데이터로, preset 적용 시 / 사용자 입력으로 채워진다 (`src/state/preset_apply.rs:101`, `src/core/mod.rs:1795`). IPC 응답에서도 함께 노출된다 (`src/adapters/ipc/handler/workspace.rs:67,199,302`).
- 디자인 시스템 (`.claude/Tasty Design System/ui_kits/terminal/chrome.jsx::WorkspaceRow`) 에는 description 슬롯이 정의되어 있지 않다 (title + subtitle 2 행 구조만).
- 본 문서는 design 측에 "워크스페이스 카드에서 description 을 어떻게 표시할지" 에 대한 디자인 가이드를 요청한다.

## 현재 코드 동작

- 위치: `src/adapters/ui/sidebar/view.rs::draw_workspace_card` (라인 618~).
- **현재 카드에 description 이 렌더링된다** (3번째 줄, subtitle 과 동일한 들여쓰기):
  - 1행: status dot (16px slot) + title (active 면 `RichText::strong()`)
  - 2행 (subtitle 있을 때만): 20px 들여쓰기 + `RichText::small().color(subtext0)`
  - 3행 (description 있을 때만): 20px 들여쓰기 + `RichText::small().color(overlay0)` — subtitle 보다 한 단계 더 dim 한 톤
- 단, 디자인 시스템의 `WorkspaceRow` 컴포넌트엔 description 슬롯 자체가 정의되어 있지 않으므로, 본 표시는 **본체 측 임시 결정**이며 디자인 가이드가 부재한 상태이다.
- 즉, **"현재 표시 중인 description 라인을 어떻게 시각 스펙으로 확정할지" + "표시 정책 자체 (현행 유지 / 조건부 노출 / 제거 후 다른 위치) 가 결정 사항이다.**

## 요청 사항

다음 항목에 대한 디자인 가이드를 요청한다.

### 1. 표시 정책

- 카드 안 3번째 줄로 항상 표시할지 (subtitle 과 동일한 들여쓰기)
- hover / active 상태에서만 표시할지
- 카드 안에 두지 않고 tooltip / hover popup 으로 분리할지
- workspace 의 "메모" 성격이라면 settings panel 같은 별도 위치에 두고 사이드바에는 노출하지 않을지

### 2. 시각 스펙 (카드 내 표시를 채택할 경우)

- 폰트 크기 / weight / 색 토큰 (현재 후보: `--text-muted` / catppuccin `overlay0` 또는 `subtext0`)
- 줄 수 — 1줄 ellipsis vs 다중 라인 wrap
- title / subtitle 과의 위계 (간격, 행간)
- 좌측 들여쓰기 — subtitle 과 동일하게 20px (status dot slot + 4px gap) 인지

### 3. 유즈케이스

- description 의 일반적 길이 (한 줄 메모 vs 문단)
- 길어질 때의 처리 (ellipsis / tooltip hover 확장 / 별도 패널 이동)
- 다국어 (영/한/일) 에서 길이가 늘어났을 때의 대응

## 제안 옵션 (디자인 측이 채택 가능한 후보)

| 옵션 | 설명 | 장점 | 단점 |
|------|------|------|------|
| A | 3번째 줄로 항상 표시 (**현행 동작**) | 정보 즉시 노출, 구조 단순 | 카드 높이 가변, description 비어있을 때와 있을 때 시각 점프 |
| B | hover 시에만 expand | 평상시 카드가 깔끔, description 없는 워크스페이스와 시각 통일 | hover discoverability 낮음, 키보드/스크린리더에 불리 |
| C | tooltip 으로 이동 | 카드 레이아웃 안정, 긴 텍스트도 수용 가능 | 클릭 가능한 인터랙션과 tooltip 충돌, 모바일/터치에선 부재 |
| D | settings panel 등 별도 위치로 이동 | 사이드바 정보 밀도 낮춤, 긴 텍스트 자유 | 사이드바에서는 인식 불가, 메타데이터 가치 약화 |
| E | 1줄 ellipsis + tooltip 으로 확장 | 평상시 일정한 카드 높이, 긴 텍스트도 수용 | tooltip 동작 일관성 신경 써야 함 |

## 참고

- `.claude/Tasty Design System/ui_kits/terminal/chrome.jsx::WorkspaceRow` 정의 (title + subtitle 2 행)
- `src/adapters/ui/sidebar/view.rs::WorkspaceEntryView` — 필드 정의
- `src/adapters/ui/sidebar/view.rs::draw_workspace_card` — 현재 렌더링 (description 3행으로 표시)
- `src/state/preset_apply.rs:101`, `src/core/mod.rs:1795` — description 이 설정되는 경로
- `src/adapters/ipc/handler/workspace.rs:67,199,302` — IPC 응답에 포함되는 형태
