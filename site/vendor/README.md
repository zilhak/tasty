# 디자인 시스템 vendor

Claude Design 프로젝트 **Tasty Design System** 에서 받아온 사본이다. 사이트가 앱 UI 를
그림이 아니라 **실제 컴포넌트**로 보여주기 위해 쓴다 — 랜딩의 제품 창, 가이드의 UI 설명이
모두 여기서 온다.

사이트가 정본이 아니다. 킷이 바뀌면 여기를 갱신하고, 반대 방향으로는 흐르지 않는다.

## 구성

| 경로 | 내용 |
|------|------|
| `styles.css` | 토큰 진입점. 소비자는 이 파일만 링크한다 |
| `tokens/` | 3티어 토큰 — `primitives` → `semantic` → `components` + `base` |
| `components/` | 공용 위젯 (Button · Tab · Kbd · Badge · Table …) |
| `ui_kits/terminal/` | 앱 셸 (`app` · `chrome` · `work` · `titlebar/`) 과 오버레이 |
| `icons/` | 아이콘 SVG |
| `screens/` | UI 스크린샷 (mocha 전용 — 라이트 테마 대응본 없음) |
| `guidelines/` | 디자인 가이드라인 문서. 각 파일이 독립 HTML 이고 첫 줄 `@dsCard` 주석이 그 문서의 그룹·이름·뷰포트를 들고 있다 |
| `gallery/` | 레퍼런스 갤러리. 킷과 같은 방식(전역으로 쓴 모듈 그래프)이라 같은 변환을 거쳐 생성 트리로 나온다. `gallery.css` 는 갤러리 자체의 크롬 스타일이라 갤러리 라우트에서만 로드한다 |

## 원본에서 제외한 것

- **웹폰트 (D2Coding, 8.3MB)** — `styles.css` 의 `@import url("tokens/fonts.css")` 를 뺐다.
  sans 는 원래 시스템 폰트 스택이고 mono 도 폴백 체인(`"SF Mono"` · `"Cascadia Code"` …)이
  있어 빠져도 렌더가 깨지지 않는다. 브랜드 통일이 필요해지면 woff2 서브셋으로 되돌린다.
- **`_ds_bundle.js`** — 브라우저 직접 실행용 전역 번들. 사이트는 `components/` 를 직접
  번들하므로 필요 없다.
- **프리뷰 `index.html`** — 위 번들에 의존하는 킷 자체 미리보기. 원본에만 둔다.

## 통합 시 주의

`ui_kits/terminal/app.jsx` 는 마운트할 때 `document.documentElement.dataset.theme` 을
자기 state 로 덮어쓴다. 사이트의 테마 토글과 충돌하므로, 사이트에 얹을 때는 테마를
props 로 주입받도록 감싼다.
