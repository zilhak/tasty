# 설정 창 화면

- **부모 기획**: [../index.md](../index.md)
- **트리거 위치**: [사이드바](../../sidebar/screens/sidebar.md) 하단 **설정 버튼**
- **시각 소스**: `design-system/ui_kits/terminal/overlays/settings_window.jsx` — claude design, vendor 예정

## 트리거

사이드바 하단 **설정 버튼** 클릭 → 설정 모달 창이 열린다 (전역 1개, 활성 시 입력 차단).

## 레이아웃 (2-level IA)

```
┌──────────────────────────────────────────────────────────────────────┐
│ [General][Terminal][Appearance][Keybindings][FileHandler][Misc][Plugins]│  L1 탭바 (7탭, 폭 넘치면 화살표 스크롤)
├───────────────┬────────────────────────────────────────────────────────┤
│ 🔍 필터        │                                                        │
│ ▸ General     │   (선택된 L2 섹션의 설정 항목)                            │  콘텐츠
│   Clipboard   │                                                        │
│   Notifications│                                                       │
│   …(L2 섹션)   │                                                        │
├───────────────┴────────────────────────────────────────────────────────┤
│                                              [ Cancel ] [ Save ]        │
└────────────────────────────────────────────────────────────────────────┘
```

## UI 요소 인벤토리

- **L1 탭바** (상단, 7탭, 이 순서): General / Terminal / Appearance / Keybindings / FileHandler / Misc / Plugins.
- **L2 섹션 목록** (좌측): 현재 L1 의 하위 섹션 + **필터 검색**. (L1 전환 시 필터 클리어.) L1 별 L2:
  - **General**: General / Clipboard / Notifications / Accessibility / Updates
  - **Terminal**: General(터미널 동작 설정) / Performance
  - **Appearance**: Theme / Colors(프리셋 색 개별 override picker) / General / Display(UI 스케일 전용) / Terminal / (플러그인 기여 페이지 동적) / HTML
  - **Keybindings**: General / Workspace / Pane / Tab / Surface / Clipboard / Zoom / Image / Preset / Plugins
  - **FileHandler**: Extension Mapping / Detectors / Handlers
  - **Misc**: Tastyrc (Windows 전용; 비-Windows 는 섹션 0개 → empty state)
  - **Plugins**: 플러그인 기여 설정 페이지 (동적)
- **콘텐츠** (중앙): 선택된 L2 섹션의 설정 항목. 도메인별 내용은 해당 기능 문서로 위임 (연결 개념):
  - Keybindings → [`features/keybindings/`](../../keybindings/index.md) / [`design/policies/key-mapping`](../../../design/policies/key-mapping.md)
  - Theme(Appearance) → [`design/systems/theme`](../../../design/systems/theme.md)
  - Clipboard → [`features/clipboard/`](../../clipboard/index.md) · Notifications → [`features/notifications/`](../../notifications/index.md) · Updates → [`features/auto-update/`](../../auto-update/index.md) · FileHandler → [`features/file-handler/`](../../file-handler/index.md)
  - Plugins → [`features/plugin-system/`](../../plugin-system/index.md)
- **Save / Cancel** (하단): draft 커밋 / 폐기.

## 상태별 시각

- **plugin page 유무**: Plugins 탭 / Appearance plugin sub-tab 은 등록된 plugin page 가 있을 때만.
- **Keybindings 녹화/충돌**: 키 녹화 중 표시 + 충돌 시 확인 팝업.

## 시각 소스

`design-system/ui_kits/terminal/overlays/settings_window.jsx` — 창 치수·탭바·L2 목록·콘텐츠 배치의 단일 출처. 스크린샷: `design-system/assets/screens/settings_window-tabs.png`, `settings_window-clipboard.png`. (design-system vendor 후 resolve.)
