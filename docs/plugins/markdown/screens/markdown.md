# Markdown surface 화면

- **부모 기획**: [../index.md](../index.md)
- **시각 소스**: plugin 이 생성한 sanitize HTML 문서 — host native OS WebView 가 렌더. `design-system/` 의 마크다운 surface 디자인(있으면), vendor 예정.

[작업 영역](../../../features/work-area/screens/work-area.md) 타일 안에 열리는 마크다운 렌더 surface. plugin 이 `pulldown-cmark`+`ammonia` 로 만든 HTML 문서를 host 의 native OS WebView overlay(WebKitGTK/WKWebView/WebView2)에 올려 렌더한다(webview, [ADR-0065](../../../adr/0065-markdown-webview-render-channel.md)) — host 는 문서의 픽셀에 관여하지 않는다.

## 트리거

마크다운 파일 열기 또는 `markdown` surface 생성/전환.

## UI 요소 인벤토리

- **렌더된 마크다운 본문** — 제목/문단/목록/코드블록/링크 등.
- **주소창** — 문서 자체에 내장된 `<input>`+`<button>`(host egui 위젯 아님).
- 탭 표시명은 파일명.

## 상태별 시각

- 파일 없음/로드 실패 등은 문서 내 `.tasty-state` div 로 표시.

## Mermaid 다이어그램

펜스드 코드블록 언어가 `mermaid` 인 블록(` ```mermaid `)은 `code.language-mermaid` 로 살아남아
[mermaid.js](https://github.com/mermaid-js/mermaid)(MIT, 오프라인 vendor —
`crates/tasty-plugin-markdown/assets/mermaid.min.js` + `NOTICE.md`)로 렌더된다. 문서에 mermaid
블록이 하나도 없으면 이 스크립트는 아예 삽입되지 않는다(수 MB 번들이라 불필요한 문서에 매번
인라인하는 낭비를 피함). 테마는 `Theme.is_light` 에 따라 mermaid 내장 `default`/`dark` 팔레트로
매핑되며, 테마 전환 시 문서 전체가 재생성되므로(`reload_all_webviews`) 별도 런타임 재테마
로직 없이 자동 반영된다. 문법이 깨진 다이어그램은 그 블록만 원본 코드 텍스트로 남고, 나머지
콘텐츠 렌더에는 영향을 주지 않는다.

## 디자인 토큰 매핑

`crates/tasty-plugin-markdown/src/render.rs::render_document` 가 완전한 HTML5 문서 하나를 만든다
— `<style>` 안에 Theme 토큰을 CSS custom property 로 주입한 뒤(`theme_css`), 문서 전체(주소창 +
본문)가 그 property 를 참조한다. host 는 이 문서를 통째로 `webview.set_url` 로 받아 native
WebView 에 올릴 뿐, 개별 요소를 픽셀 단위로 그리지 않는다 — 아래 표는 CSS custom property ↔
Theme 토큰 매핑이다.

| UI 요소 | CSS custom property | 토큰 / 비례 | 비고 |
|---|---|---|---|
| 문서 배경/전경 | `--md-bg` / `--md-fg` | `bg-app`(=crust) · `text-secondary` | webview 렌더 경로엔 focus 신호가 없어 `bg-app` 이 문서의 유일한 배경(`surfaces.markdown.focused_bg` 설정값은 이 경로에서 쓰지 않음) |
| 주소창 바 | `#tasty-addr-bar` | `bg-sidebar` · 40px sticky top | `<input list>`+native `<datalist>`(최근목록)+Go `<button>` — 전부 문서 HTML |
| 강조 텍스트 | `--md-strong` | `text-primary` | heading, `<strong>` |
| 링크 | `--md-link` | `accent-primary` | nav-fragment 로 rewrite 된 `href` |
| 코드 배경/보더 | `--md-code-bg` / `--md-code-border` | `surface-raised` / `separator` | 인라인 `<code>` + `<pre>` |
| 인용구 좌측 바/본문 | `--md-quote-bar` / (본문은 `--md-fg`) | `border-strong` | `blockquote` |
| 구분선 | `--md-rule` | `separator` | `hr` |
| 헤딩 크기 | `--md-h1`..`--md-h6` | `heading_sizes_px` — `font-size-prose-h1`(h1)↔`font-size-body`(h6) 5단계 선형보간 | CSS 라 per-level override 가능(현재는 선형보간을 디자인으로 채택) |
| 표(GFM) 격자선 | `--md-border` | `md-table-border`(=`border-strong`) | 실제 `<table>` border-collapse — 문서 코드/인용 보더와는 별개 토큰(값은 같지만 의미상 독립) |
| 표 zebra | `--md-zebra` | `md-table-row-bg-zebra` | `tr:nth-child(even)` |
| 상태(에러) 제목 | inline hex(`danger`) | `accent-danger` | `.tasty-state-title` |
| 상태(에러/빈 문서) 본문 | inline hex(`muted`) | `text-muted` | `.tasty-state-detail` |
| 코너 반경/보더 굵기 | `--md-radius` / `--md-border-w` | `corner-radius` / `border-width` | 주소창 입력·코드 블록·표 공용 |

## 갤러리 specimen

`crates/tasty-gallery/src/catalog/components/markdown_viewer.rs` — Layouts › `Content viewers` ›
`Markdown surface`. 갤러리는 live webview 를 띄우지 않으므로, 위 CSS 출력을 egui `Frame`/`Label`
로 **손으로 근사**한다(픽셀 동일성은 비목표) — 헤딩/문단/링크/리스트/코드블록/표(격자+zebra)/캡션
대표 문서 + 주소창 chrome 의 정적 근사. 3자 매핑: [design-gallery-mapping.md](../../../design/systems/design-gallery-mapping.md#surface-viewers-layouts).

## 시각 소스

plugin 이 host `theme.query` IPC 로 조회한 Theme 토큰을 CSS 로 문서에 주입해 자가 렌더(host 는
native WebView 로 그 문서를 표시만 함). design-system 에 마크다운 디자인이 vendor 되면 링크로 교체.
