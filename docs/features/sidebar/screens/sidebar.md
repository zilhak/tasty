# 사이드바 화면

- **부모 기획**: [../index.md](../index.md)
- **상위 화면**: [MainView 전체 레이아웃](../../main-view/screens/main-view.md) 의 좌측 영역
- **시각 소스**: `design-system/ui_kits/terminal/chrome.jsx` — claude design, vendor 예정

각 영역은 자기 위치/역할만 적고, 다른 기능으로 위임되는 버튼은 그 문서를 **링크만** 한다 (연결 개념).

## 레이아웃 (full)

```
┌──────────────────┐
│ tasty.   [접기]   │  헤더 — 워드마크 + 수박 로고 + 접기 버튼
├──────────────────┤
│ Workspaces       │  ← 섹션 heading
│ ┌──────────────┐ │
│ │ workspace A  │ │  워크스페이스 카드 (클릭 전환 / 드래그 재정렬 /
│ │ workspace B ●│ │   점유 시 인디케이터 ●)
│ └──────────────┘ │
│ [+ New workspace]│
│        ⋮         │  (남는 높이 전부 워크스페이스 영역)
├──────────────────┤
│ ⚙ 도구            │  → features/tools-menu/ 참조
│ 🔌 플러그인        │  → features/plugin-system/ 참조
│ ⚙ 설정            │  → features/settings/ 참조
└──────────────────┘
```

## UI 요소 인벤토리

- **헤더**: 워드마크 `tasty.` + 수박 로고(`icon_256.png`) + **접기 버튼**(full↔collapsed 토글).
- **워크스페이스 영역** (남는 높이): `Workspaces` heading → 워크스페이스 카드 목록 → `New workspace` 버튼.
  - 카드: 클릭=전환, 드래그=재정렬, 점유 중이면 인디케이터.
- **하단 버튼** (각 버튼이 뭘 하는지 한 줄 + 상세는 해당 문서로 링크 — 연결 개념):
  - **도구** (`icons::TOOLS`) — 클릭 시 **도구 메뉴**를 연다 (리스닝 포트 등 빌트인 진단/유틸 항목 모음). → `features/tools-menu/` *(재작성 예정)*
  - **플러그인** (`icons::PLUG`) — **플러그인 관리 창**을 연다 (설치/활성·비활성/설정). → `features/plugin-system/` *(재작성 예정)*
  - **설정** (`icons::SETTINGS`) — **설정 창**을 연다 (탭별 환경설정). → `features/settings/` *(재작성 예정)*

## 상태별 시각

- **full / collapsed**: collapsed 는 아이콘만 남긴 좁은 형태 (워드마크/라벨 숨김, 카드→정사각 슬롯).
- **워크스페이스 점유**: 다른 client 가 점유 중인 카드에 인디케이터 표시.
- **드래그 중**: 카드 재정렬 드래그 스냅샷.

## 시각 소스

`design-system/ui_kits/terminal/chrome.jsx` — 사이드바 치수·색·로고·버튼 배치의 단일 출처. 스크린샷: `design-system/assets/screens/sidebar-full.png`, `sidebar-collapsed.png`, `sidebar-workspaces.png`. (design-system vendor 후 링크 resolve.)
