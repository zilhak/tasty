# HTML surface 화면

- **부모 기획**: [../index.md](../index.md)
- **시각 소스**: 네이티브 WebView 오버레이 — 콘텐츠는 OS WebView 가 그린다(디자인 토큰 무관).

[작업 영역](../../../features/work-area/screens/work-area.md) 타일 위치에 WebView 오버레이로 그려지는 HTML surface.

## 트리거

HTML 파일 열기 또는 `html` surface 생성(`--url`).

## UI 요소 인벤토리

- **WebView 콘텐츠** — URL/파일의 웹 렌더. 타일 rect 에 오버레이로 정렬.
- 탭 표시명은 파일명/URL.

## 상태별 시각

surface 는 트리에선 `RemoteSurface` marker. 네이티브 WebView 의 navigation 생명주기
(start/finish/fail)를 3 backend(WebView2 / WKNavigationDelegate / WebKitGTK)가 `NavState`
(Idle/Loading/Done/Failed)로 host 에 전달하고, host 가 그 상태에 따라 chrome 을 그린다:

- **Idle** — URL 미지정. placeholder(`GLOBE` · "No page loaded").
- **Loading** — 탐색 중. WebView overlay 를 숨기고 `Spinner` + "Loading…" chrome.
- **Done** — 성공. WebView overlay 가 페이지를 그린다(메뉴/팝업으로 overlay 가 일시
  숨겨질 때만 boundary chrome backdrop 노출).
- **Failed** — 실패. overlay 를 숨긴 채 `ALERT_CIRCLE`(`accent-danger`) + "Failed to load"
  + URL chrome. 실패 사유는 화면 대신 `tracing::warn!` 로그로만 남긴다.

## 디자인 토큰 매핑

콘텐츠 픽셀은 OS WebView 가 그려 **토큰 무관**이다. tasty 가 토큰으로 책임지는 건 overlay 가
붙기 전/실패 시의 *chrome* 뿐 — 얇게만 정의한다:

| chrome 요소 | 토큰 | 비고 |
|---|---|---|
| 타일 경계 | `bg-panel` + `border-default` | overlay 마운트 영역 |
| boundary / placeholder glyph | `GLOBE` glyph · `text-muted`/`text-disabled` | gallery `icons.rs` SURFACES |
| 안내 캡션 / URL | `text-muted` · `text-disabled` | "No page loaded" 등 |
| 로딩 | `Spinner`(ui-widgets) · `text-muted` | attach·탐색 중 |
| 에러 | `ALERT_CIRCLE` glyph + `accent-danger` | 로드 실패 |
| 콘텐츠 영역 | (비움) | 네이티브 overlay 가 덮음 |

## 갤러리 specimen

`crates/tasty-gallery/src/catalog/components/html_chrome.rs` — Layouts › `Content viewers` ›
`HTML (webview) chrome`. boundary / placeholder / loading / error 4 chrome 상태만 전사(콘텐츠는
overlay). 3자 매핑: [design-gallery-mapping.md](../../../design/systems/design-gallery-mapping.md#surface-viewers-plugins).

## 시각 소스

콘텐츠는 OS 네이티브 WebView 가 렌더하므로 design-system 토큰이 적용되지 않는다(웹 페이지 자체 스타일). 타일 정렬/경계만 작업영역 레이아웃을 따른다.
