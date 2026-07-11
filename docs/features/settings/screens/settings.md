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
│   Notifications│                                                       │
│   Accessibility│                                                       │
│   …(L2 섹션)   │                                                        │
├───────────────┴────────────────────────────────────────────────────────┤
│                                              [ Cancel ] [ Save ]        │
└────────────────────────────────────────────────────────────────────────┘
```

## UI 요소 인벤토리

- **L1 탭바** (상단, 7탭, 이 순서): General / Terminal / Appearance / Keybindings / FileHandler(표시 라벨 **Handler**) / Misc / Plugins.
- **L2 섹션 목록** (좌측): 현재 L1 의 하위 섹션 + **필터 검색**. (L1 전환 시 필터 클리어.) L1 별 L2:
  - **General**: General / Notifications / Accessibility
  - **Terminal**: General(터미널 동작 설정) / Mouse Capture(마우스 캡처 안내 배너 토글 + Shift 우회 Note + 캡처 비활성화 블랙리스트) / TUI(OSC 52 클립보드 읽기 허용 토글 + bordered warning callout) / Performance
  - **Appearance**: Theme / Colors(프리셋 색 개별 override picker) / General / Display(UI 스케일 전용) / Terminal / (플러그인 기여 페이지 동적) / HTML
  - **Keybindings**: General / Workspace / Pane / Tab / Surface / Clipboard / Zoom / Image / Preset / Plugins
  - **FileHandler**(표시 "Handler"): File Extension Mapping / File Detectors / File Handlers / Hook Handlers(공유 훅 핸들러 레지스트리 편집 — 리스너 설정은 CLI 전용, 여기 미노출)
  - **Misc**: Tastyrc (Windows 전용; 비-Windows 는 섹션 0개 → empty state).
  - **Plugins**: 플러그인 기여 설정 페이지 (동적)
- **콘텐츠** (중앙): 선택된 L2 섹션의 설정 항목. 도메인별 내용은 해당 기능 문서로 위임 (연결 개념):
  - Keybindings → [`features/keybindings/`](../../keybindings/index.md) / [`design/policies/key-mapping`](../../../design/policies/key-mapping.md)
  - Theme(Appearance) → [`design/systems/theme`](../../../design/systems/theme.md)
  - Notifications → [`features/notifications/`](../../notifications/index.md) · FileHandler(파일 서브탭) → [`features/file-handler/`](../../file-handler/index.md) · Hook Handlers → [`features/webhook/`](../../webhook/index.md)·[`features/hooks/`](../../hooks/index.md)
  - Plugins → [`features/plugin-system/`](../../plugin-system/index.md)
- **Save / Cancel** (하단): draft 커밋 / 폐기. 헤더 밴드에 close ✕ 는 없다 — 닫기/취소 진입점은 footer **Cancel** + OS 타이틀바 close 뿐.
- **Keybindings › Preset**: 이 서브탭만 표준 패딩/스크롤 래퍼 없이 **full-bleed** drill-down(목록⇄상세 content-swap)으로 그려진다. 상세: [`features/keybindings/`](../../keybindings/index.md#프리셋).

## 상태별 시각

- **plugin page 유무**: Plugins 탭 / Appearance plugin sub-tab 은 등록된 plugin page 가 있을 때만.
- **Keybindings 녹화/충돌**: 키 녹화 중 표시 + 충돌 시 확인 팝업.

## 시각 소스

`design-system/ui_kits/terminal/overlays/settings_window.jsx` — 창 치수·탭바·L2 목록·콘텐츠 배치의 단일 출처. 스크린샷: `design-system/assets/screens/settings_window-tabs.png`. (design-system vendor 후 resolve.)
