# MainView 화면 (전체 레이아웃)

- **부모 기획**: [../index.md](../index.md)
- **시각 소스**: `design-system/ui_kits/terminal/` (`chrome.jsx`, `work.jsx`, `index.html`) — claude design, vendor 예정

이 화면은 **합성 화면** 이다 — 각 영역은 *자기 위치/역할만* 적고, 내용은 하위 feature 문서로 **링크만** 한다 (연결 개념).

## 레이아웃

```
┌──────────────────────────────────────────────┐
│ 타이틀바 (CSD, OS별)                            │  → window-chrome
├──────────┬───────────────────────────────────┤
│          │ 탭 스트립                            │  → workspace-tabs
│ 사이드바  ├───────────────────────────────────┤
│          │ 작업 영역 (Pane/Tab/Surface)        │  → hierarchy (상위/하위 레이아웃)
│          │                                    │
│          ├───────────────────────────────────┤
│          │ 상태바                              │  → workspace-status-bar
└──────────┴───────────────────────────────────┘
```

## 영역

- **타이틀바** (최상단, OS별 CSD) → `features/window-chrome/` 참조 *(재작성 예정)*
- **사이드바** (좌측, 전체 높이) → [`features/sidebar/`](../../sidebar/index.md)
- **작업 영역** (중앙) — 상단 탭 스트립 + 그 아래 surface 배치.
  - 탭 스트립 → `features/workspace-tabs/` 참조 *(재작성 예정)*
  - surface 배치(상위 레이아웃 Pane / 하위 레이아웃 Surface)와 surface 종류 → [구조 계층](../../../concepts/hierarchy.md)
- **상태바** (하단) → `features/workspace-status-bar/` 참조 *(재작성 예정)*

## 상태별 시각

- Workspace 0개/전환 등 상태는 각 하위 feature(사이드바·탭) 문서에서 다룬다. MainView 화면 자체는 영역 배치만 정의한다.

## 시각 소스

`design-system/ui_kits/terminal/` — 전체 셸의 치수·색·영역 배치 단일 출처. 스크린샷: `design-system/assets/screens/sidebar-full.png`, `terminal-surface.png` 등. (design-system vendor 후 링크 resolve.)
