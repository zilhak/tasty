# 상태바 화면

- **부모 기획**: [../index.md](../index.md)
- **시각 소스**: Claude Design `gallery/layouts.jsx` 의 **Workspace status bar** 섹션 — 항목·축소 순서·변종 여섯의 정본

[작업 영역](../../work-area/screens/work-area.md) 하단의 24px 바. 동작은 부모 기획, 여기선 시각.

## 트리거

작업 영역이 있으면 항상 하단에 표시(`bottom_inset`).

## UI 요소 인벤토리

```
┌ 상태바 (높이 status_bar_height, 상단 1px separator) ─────────────────┐
│ ⑂ main   s3·p1   zsh   120×32                     [Ctrl]+[K]    ☀ │
│ └브랜치  └surface id └shell └grid                  └팔레트 키캡  └테마 │
└────────────────────────────────────────────────────────────────┘
```

- **바깥 여백 10 · 항목 사이 gap 10**. 셀마다 패딩을 주는 형태가 아니라, 인접한 두 항목 사이는 10 이다.
- **좌측 클러스터**(표시 전용, 순서 고정): `git-branch` 글리프(잉크 `statusbar-glyph` · 크기 `statusbar-glyph-size` → `icon-size-xs` 12) + 이름 · surface id · shell · grid. 바의 인라인 글리프는 **한 자리**에서 크기를 받는다 — 글리프마다·테마마다 갈리지 않는다.
  - **mono 는 둘뿐이다** — surface id(`s3·p1`)와 grid(`120×32`). 브랜치 이름과 shell 은 UI 글꼴이다. 크기는 넷 다 `font-size-caption`.
  - 브랜치 항목은 **전체 폭 160 상한**이고, 넘으면 이름을 말줄임(`…`)한다.
  - `<shell>` 셀은 정확히는 해당 surface 의 **foreground 프로세스명**이다(셸 idle 시엔 셸 이름). Windows 에선 *가장 얕은 non-shell 자손*(사용자가 띄운 바깥쪽 앱, 예: `node`)을 표시한다 — 안쪽 단명 helper 가 아니라. 매 프레임 OS 조회가 아니라 1Hz busy-poll 캐시(`CoreState::foreground_name`)에서 읽으므로 표시는 최대 1초 지연될 수 있다. 판정·플랫폼별 메커니즘은 [busy-indicator](../../../design/policies/busy-indicator.md).
- **우측 클러스터**(clickable): 팔레트 단축키 **Kbd 키캡**(라벨 단어 없음) · 테마 **글리프**(이름 없음, 잉크 `statusbar-theme-glyph` · 크기는 좌측과 같은 `statusbar-glyph-size`).
- **상단 1px separator** + `bg_app` 배경.

## 상태별 시각

- **repo / 비-repo** — 브랜치 항목이 통째로 있거나 없다. 없을 때 dash 를 그리지 않는다.
- **detached HEAD** — 브랜치 자리에 `@ <short sha>`.
- **terminal / 비-terminal** — shell 과 grid 는 terminal 한정이고 서로 독립이다.
- **팔레트 바인딩 없음** — 키캡 항목이 자리째 없다.
- **테마 light / dark** — 글리프(해 / 테마). 색은 두 쪽이 같다.
- **hover** — 우측 두 항목만 반응한다(테마 글리프는 잉크가 밝아진다).
- **좁은 윈도우** — grid → shell → surface id → 팔레트 키캡 → 브랜치 텍스트 순으로 빠지고, **테마 글리프는 안 빠진다**. 갤러리 `statusbar` specimen 이 변종 여섯으로 전시한다.

## 시각 소스

Claude Design `gallery/layouts.jsx` 의 **Workspace status bar** 섹션 — 항목 목록·축소 순서·여백·글꼴 구분·색 토큰의 단일 출처.
</content>
