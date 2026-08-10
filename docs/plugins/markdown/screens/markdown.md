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

## Frontmatter 숨김

문서 최상단(첫 줄)에 오는 YAML(`---\n...\n---`) 또는 TOML(`+++\n...\n+++`) frontmatter 는
Jekyll/Hugo/Obsidian/Zettlr 등에서 흔히 붙이는 메타데이터 블록이다. `pulldown-cmark` 의
`ENABLE_YAML_STYLE_METADATA_BLOCKS`/`ENABLE_PLUSES_DELIMITED_METADATA_BLOCKS` 옵션으로 파싱 단계에서
`Tag::MetadataBlock` 이벤트로 인식되고, HTML 라이터가 이 블록을 항상 비출력(non-writing) 처리하므로
렌더된 본문에서 완전히 사라진다 — 별도의 메타데이터 표시 패널은 두지 않는다(파싱 의존성·i18n·
깨진 YAML 폴백 처리 비용을 늘리지 않기 위한 선택). 이 규칙은 CommonMark frontmatter 확장 사양대로
**문서의 맨 처음에 올 때만** 적용된다 — 문서 중간의 `---` 는 지금까지와 동일하게 `<hr>` 로 렌더된다.

## Smart punctuation

`pulldown-cmark` 의 `ENABLE_SMART_PUNCTUATION` 옵션이 항상 켜져 있다(설정 토글 없음). 직선따옴표
`"`/`'` → 곡선따옴표, `--`/`---` → en/em dash, `...` → ellipsis 로 자동 치환된다. Obsidian 등
대다수 문서 뷰어의 기본값과 맞춘 선택이며, GitHub 의 raw 렌더링과 완전히 동일하지 않다는 점은
감수한 트레이드오프다. 인라인 코드(`` `--` ``)와 펜스드 코드블록 내부 텍스트, 그리고 백슬래시로
이스케이프한 문장부호(`\"`, `\-\-`)는 이 치환의 영향을 받지 않고 원문 그대로 남는다.

## GFM alert blockquote

`> [!NOTE]`/`[!TIP]`/`[!IMPORTANT]`/`[!WARNING]`/`[!CAUTION]`(blockquote 의 첫 줄에 태그만 있어야
인식, 대소문자 무관 — `pulldown-cmark` `Options::ENABLE_GFM`)은 각각 고유 accent 색·아이콘·헤더
레이블을 가진 alert 로 렌더된다. 헤더 레이블은 `render.rs::ALERT_KINDS` 가 plugin 자신의
`Translator` 로 UI 언어에 맞게 조회한 뒤(`markdown.alert.{note,tip,important,warning,caution}`,
`lang/{en,ko,ja}.toml`) `data-label` 속성으로 문서에 심는다 — CSS 는 언어를 분기할 수 없으므로
`content: attr(data-label)` 로 그 값을 그대로 반영한다. 아이콘은 `tasty-icons` 의 canonical
글리프(note=`ALERT_CIRCLE`, tip=`STAR_FILL`, important=`BELL`, warning=`ALERT_TRIANGLE`,
caution=`CLOSE`)를 각 kind 의 accent 색으로 구운 SVG data URI 로 `background-image` 에 심는다
(`render.rs::alert_icon_data_uri`) — `tasty_icons::Icon` 원본은 `stroke="white"`/`fill="white"`
고정이라 egui 텍스처 tint 대신 색을 직접 구운 사본을 만든다. 배경은 그 accent 색의 저알파(≈12%,
`drop_overlay.rs` 관례와 동일 비율) 버전.

일반 blockquote(태그 없는 `>`)는 영향받지 않는다 — pulldown-cmark 는 그 경우 `class` 자체를
emit 하지 않는다. `data-label` 은 실제 `Tag::BlockQuote(Some(kind))` AST 이벤트에서만 심어지므로,
문서 본문에 raw HTML 로 `<blockquote class="markdown-alert-note">` 같은 리터럴을 직접 써넣어도
가짜 alert 로 오인되지 않는다(완성된 HTML 문자열을 매칭하는 방식이 아니라 파서 이벤트 자체를
가로채는 방식이기 때문).

## Bare URL autolink

본문에 `<...>` 꺾쇠나 `[text](url)` 마크다운 문법 없이 그냥 텍스트로 적은 `http://`/`https://` URL 도
클릭 가능한 링크로 변환된다. `pulldown-cmark` 0.12.2 는 CommonMark 코어의 꺾쇠 autolink 만 지원하고
GFM 의 확장 autolink(스킴 없는 bare URL 인식 포함)는 구현하지 않으므로(`Options::ENABLE_GFM` 는 alert
blockquote 태그만 켠다), `render.rs::autolink_bare_urls`/`split_bare_urls` 가 이벤트 스트림에서 직접
스캔해 `Tag::Link` 로 쪼갠다. **이번 스코프는 `http(s)://` 스킴만이다** — `www.`-prefix(스킴 없는
호스트)나 이메일 자동링크는 제외(필요성이 확인되면 별도 TODO).

이 pass 는 상태를 가진 스캔이다(단순 무상태 `map()` 이 아님) — 다음을 명시적으로 제외한다:

- 이미 `Tag::Link` 안의 텍스트(`[https://x](https://x)`처럼 이미 명시적으로 링크된 경우) — 중복 링크
  방지.
- `Tag::CodeBlock`(펜스드/들여쓰기 코드블록) 내부 텍스트.
- 인라인 코드는 별도 추적이 필요 없다 — pulldown-cmark 는 인라인 코드 내용을 애초에 `Event::Text`
  가 아니라 별개의 `Event::Code` variant 로 표현하므로 구조적으로 이미 제외된다.

실측 확인 결과, 하나의 시각적 URL 이 여러 `Event::Text` 조각으로 쪼개져 들어오는 경우가 실제로
있다 — URL 안에서 진짜 강조(emphasis)로 짝이 맞지 않는 `*`/`_` 하나가 단독 1글자 `Event::Text` 로
따로 토큰화된다(예: `.../foo*bar` → `Text("...foo")` + `Text("*")` + `Text("bar...")`). 그래서 이
pass 는 실제 마크업(다른 태그 시작/`SoftBreak` 등)이 끼어들기 전까지 연속된 `Event::Text` 를 하나의
버퍼로 병합한 뒤 그 위에서 URL 을 스캔한다 — 진짜 마크업 경계는 절대 이어붙이지 않는다.

경계 처리:

- **문장부호 트리밍**: URL 끝의 `.`/`,`/`;`/`:`/`!`/`?`/따옴표/단독 `*`/`_`/`~` 는 한 글자씩 제거해
  링크 밖으로 뺀다(`https://example.com.` → `.` 는 링크 밖).
- **괄호 균형**: 매치된 URL **자기 자신 안의** `(`/`)` 개수만 비교한다 — 위키 URL처럼 URL 내부에서
  괄호가 짝이 맞으면(`Rust_(programming_language)`) 끝의 `)` 를 유지하고, 문장이 URL 을 감싸는
  괄호(`(https://example.com)`)처럼 URL 자체엔 없던 `)` 만 남으면 링크 밖으로 뺀다.
- **HTML entity**: `&amp;` 같은 entity 는 pulldown-cmark 파서가 `Event::Text` 에 도달하기 전에 이미
  실제 문자로 디코드해두므로, autolink 여부와 무관하게 `push_html` 이 나가는 길에 동일하게
  재이스케이프한다 — 이 pass 에 별도 entity 처리가 필요 없다.

새로 만들어진 `Tag::Link` 의 `dest_url` 은 명시적 마크다운 링크와 동일한 규칙으로
`#tasty-nav:link:<enc>` 프래그먼트로 재작성된다 — `autolink_bare_urls` 는 원본 URL 을 그대로
`dest_url` 에 넣은 `Tag::Link` 만 만들고, 이벤트 스트림 전체를 마지막에 한 번 통과하는
`rewrite_link_event` 가 명시적 링크와 자동 생성 링크를 구분 없이 동일하게 nav-fragment 로 바꾼다
(순서는 `unsafe_content_html` 의 주석 참조).

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
| GFM alert 5종 | inline hex(kind 별 accent) | note=`accent-primary` · tip=`accent-success` · important=`accent-agent` · warning=`accent-warning` · caution=`accent-danger` | 전용 alert 토큰 없음 — 기존 semantic accent 재사용(위 "GFM alert blockquote" 절) |

## 갤러리 specimen

`crates/tasty-gallery/src/catalog/components/markdown_viewer.rs` — Layouts › `Content viewers` ›
`Markdown surface`. 갤러리는 live webview 를 띄우지 않으므로, 위 CSS 출력을 egui `Frame`/`Label`
로 **손으로 근사**한다(픽셀 동일성은 비목표) — 헤딩/문단/링크/리스트/코드블록/표(격자+zebra)/캡션
대표 문서 + 주소창 chrome 의 정적 근사 + GFM alert 5종(accent 배경/보더 + 아이콘 + 굵은 레이블,
실제 CSS 는 좌측 바만 쓰지만 specimen 은 egui `Frame` 표준 paint-order 를 살려 4변 보더로 근사).
3자 매핑: [design-gallery-mapping.md](../../../design/systems/design-gallery-mapping.md#surface-viewers-layouts).

## 시각 소스

plugin 이 host `theme.query` IPC 로 조회한 Theme 토큰을 CSS 로 문서에 주입해 자가 렌더(host 는
native WebView 로 그 문서를 표시만 함). design-system 에 마크다운 디자인이 vendor 되면 링크로 교체.
