# 상태바 화면

- **부모 기획**: [../index.md](../index.md)
- **시각 소스**: `design-system/ui_kits/terminal/work.jsx` (`StatusBar` 컴포넌트) — claude design, vendor 예정

[작업 영역](../../work-area/screens/work-area.md) 하단의 24px 바. 동작은 부모 기획, 여기선 시각.

> **표시 항목 미확정** — 아래 인벤토리의 *항목들*(브랜치/sid/셸·그리드/팔레트/테마)은 현재 소스의 잠정 구성이며 교체될 수 있다. 확정된 것은 바의 위치·크기·좌우 클러스터 구조다.

## 트리거

작업 영역이 있으면 항상 하단에 표시(`bottom_inset`).

## UI 요소 인벤토리

```
┌ 상태바 (높이 status_bar_height, 상단 1px separator) ───────────────┐
│ ● main   42   zsh · 120×40                  <Cmd+K> palette  ● Mocha │
│ └브랜치  └sid └셸·그리드                      └팔레트 칩   └테마토글  │
└──────────────────────────────────────────────────────────────────┘
```

- **좌측 클러스터**(표시 전용): 브랜치 점(`accent_success`)+이름 · surfaceId · `<셸> · <cols>×<rows>`.
- **우측 클러스터**(clickable): 팔레트 칩(`<단축키> palette`) · 테마 토글(점 + 테마명).
- **상단 1px separator** + `bg_app` 배경.

## 상태별 시각

- **repo / 비-repo** — 브랜치 클러스터 표시/숨김.
- **terminal / 비-terminal** — 셸·그리드는 terminal 한정.
- **테마 light / dark** — 토글 점 색(yellow / mauve).
- **hover** — 우측 칩·토글 텍스트 색 전환.

## 시각 소스

`design-system/ui_kits/terminal/work.jsx` 의 `StatusBar` — 바 높이·셀 패딩·점 크기·색 토큰의 단일 출처. (design-system vendor 후 링크 resolve.)
</content>
