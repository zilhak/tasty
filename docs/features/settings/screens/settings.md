# 설정 창 화면

- **부모 기획**: [../index.md](../index.md)
- **트리거 위치**: [사이드바](../../sidebar/screens/sidebar.md) 하단 **설정 버튼**
- **시각 소스**: `design-system/ui_kits/terminal/overlays/settings_window.jsx` — claude design, vendor 예정

## 트리거

사이드바 하단 **설정 버튼** 클릭 → 설정 모달 창이 열린다 (전역 1개, 활성 시 입력 차단).

## 레이아웃 (2-level IA)

```
┌──────────────────────────────────────────────┐
│ [General] [Appearance] [Keybindings] [Plugins]│  L1 탭바
├───────────────┬──────────────────────────────┤
│ 🔍 필터        │                              │
│ ▸ General     │   (선택된 L2 섹션의 설정 항목)  │  콘텐츠
│   Terminal    │                              │
│   Clipboard   │                              │
│   …(L2 섹션)   │                              │
├───────────────┴──────────────────────────────┤
│                          [ Cancel ] [ Save ] │
└──────────────────────────────────────────────┘
```

## UI 요소 인벤토리

- **L1 탭바** (상단): General / Appearance / Keybindings / Plugins.
- **L2 섹션 목록** (좌측): 현재 L1 의 하위 섹션 + **필터 검색**. (L1 전환 시 필터 클리어.)
- **콘텐츠** (중앙): 선택된 L2 섹션의 설정 항목. 도메인별 내용은 해당 기능 문서로 위임 (연결 개념):
  - Keybindings → `features/keybindings/` *(재작성 예정)* / [`design/policies/key-mapping`](../../../design/policies/key-mapping.md)
  - Theme(Appearance) → [`design/systems/theme`](../../../design/systems/theme.md)
  - Clipboard / Notifications / Updates / FileHandler → 각 feature *(재작성 예정)*
  - Plugins → [`features/plugin-system/`](../../plugin-system/index.md)
- **Save / Cancel** (하단): draft 커밋 / 폐기.

## 상태별 시각

- **plugin page 유무**: Plugins 탭 / Appearance plugin sub-tab 은 등록된 plugin page 가 있을 때만.
- **Keybindings 녹화/충돌**: 키 녹화 중 표시 + 충돌 시 확인 팝업.

## 시각 소스

`design-system/ui_kits/terminal/overlays/settings_window.jsx` — 창 치수·탭바·L2 목록·콘텐츠 배치의 단일 출처. 스크린샷: `design-system/assets/screens/settings_window-tabs.png`, `settings_window-clipboard.png`. (design-system vendor 후 resolve.)
