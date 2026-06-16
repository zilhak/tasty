# 명령 팔레트 화면

- **부모 기획**: [../index.md](../index.md)
- **트리거 위치**: `toggle_command_palette` 단축키 · [도구 메뉴](../../tools-menu/screens/tools-menu.md) `Command palette`
- **시각 소스**: `design-system/ui_kits/terminal/overlays/command_palette.jsx` — claude design, vendor 예정

## 트리거

단축키(`toggle_command_palette`) 또는 도구 메뉴 항목 → 화면 중앙에 팔레트 popup.

## 레이아웃

```
┌──────────────────────────────────┐
│ 🔍 (검색 입력)                     │
├──────────────────────────────────┤
│ ▸ New workspace          ctrl+…   │  후보 행: 아이콘 + 라벨 + 첫 바인딩
│   Split pane             ctrl+…   │
│   Toggle settings                 │
│   …                               │
└──────────────────────────────────┘
```

## UI 요소 인벤토리

- **검색 입력** (상단): 쿼리 입력. 즉시 필터.
- **후보 리스트**: 각 행 = leading 아이콘(디자인 명시 명령은 전용 아이콘, 나머지는 fallback) + 명령 라벨 + 우측 첫 바인딩(회색). 선택 행 강조, `↑/↓` 이동.

## 상태별 시각

- **빈 쿼리**: 전체 후보. **검색 중**: 점수순 필터. **결과 0**: 빈 목록.

## 시각 소스

`design-system/ui_kits/terminal/overlays/command_palette.jsx` — 팔레트 치수·행·아이콘·바인딩 표기의 단일 출처. 스크린샷: `design-system/assets/screens/statusbar-palette-chip.png`. (vendor 후 resolve.)
