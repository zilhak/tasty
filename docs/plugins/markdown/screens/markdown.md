# Markdown surface 화면

- **부모 기획**: [../index.md](../index.md)
- **시각 소스**: plugin egui-mesh 자가 렌더 — `design-system/` 의 마크다운 surface 디자인(있으면), vendor 예정.

[작업 영역](../../../features/work-area/screens/work-area.md) 타일 안에 열리는 마크다운 렌더 surface. host 가 egui 로 그린다.

## 트리거

마크다운 파일 열기 또는 `markdown` surface 생성/전환.

## UI 요소 인벤토리

- **렌더된 마크다운 본문** — 제목/문단/목록/코드블록/링크 등.
- 탭 표시명은 파일명.

## 상태별 시각

- 파일 없음/로드 실패 등은 surface 내 표시.

## 디자인 토큰 매핑

`crates/tasty-plugin-markdown/src/render.rs::render` 가 `ScrollArea::vertical`(host 측)
안에서 `pulldown-cmark` 이벤트 스트림을 직접 egui 위젯으로 그리는 토큰 기반 6단계 prose
렌더러다. 색·크기·간격은 전부 `MdStyle` 이 `Theme` 에서 캡처한 토큰에서 온다:

본문 위에는 얇은 주소창 chrome(`main.rs`)이 있다 — 경로 AutoComplete + Go 버튼. 경로 필드는
공유 `AutoComplete` 위젯(`tasty-ui-widgets` — 트리거 `Input` + 후보 드롭다운)을 직접 쓴다.
아이콘은 `tasty-icons` 빌드타임 베이크 벡터다([ADR-0036](../../../adr/0036-plugin-icon-buildtime-bake-tasty-icons-single-source.md)).

| UI 요소 | 토큰 / 비례 | 비고 |
|---|---|---|
| surface 배경 | `bg-panel` | 상단 주소창 바 + 본문이 타일 채움 |
| 주소창 바 | `bg-sidebar` · 40px | 경로 AutoComplete + Go |
| 경로 필드 선두 글리프 | `FILE` glyph · `text-muted` | `AutoComplete` 트리거 leading 아이콘(베이크 벡터) |
| 경로 필드(트리거) | `Input` — `surface-raised` + 테두리(idle `border-default` / 편집 `border-focus` + focus ring) · 28px · mono caption(11) | 비편집=`text-secondary`, 편집=`text-primary` |
| 히스토리 드롭다운 | `menu container`(surface-raised · border-default 1px · `shadow-popover` lift) · 필드 폭 · 필드 하단 `space-xs` 오프셋 floating | 편집 진입 시 `recent.query {kind:"markdown"}` 최신순 최대 10개 |
| 드롭다운 후보 행 | `MenuItem` 언어 · 28px · middle-ellipsis 경로 · hover=`overlay-hover` / keyboard-active=`surface-active`(2단계 분리) | 행 선두 `FILE` 아이콘 · empty="No recent files"(`text-muted`) |
| Go 버튼 | `ARROW_RIGHT` glyph · `text-secondary`(hover `text-primary`) · `font-size-body` | `tasty-icons` 베이크 벡터(raw `→` 제거) |
| 본문 텍스트 | `text-secondary` · 본문 leading 은 egui_commonmark 소유 | 헤딩 색은 단계별 차별화. `line-height-prose` override 미노출 → 은퇴(retire-pending) |
| 헤딩 크기 | `font-size-prose-h1`(20) 을 `Heading` 앵커로, H2~H6 은 egui_commonmark 이 `Heading`↔`Body` 사이 보간 | per-H2 픽셀 토큰(`prose-h2`) 미노출 → 은퇴(retire-pending). 6단계 prose 위계는 라이브러리 보간 |
| small 캡션 | body × 0.85 · `text-muted` | |
| 링크 | `accent-primary` | |
| 코드블록 배경 | `surface-raised` | |
| 코드 텍스트 | mono · `text-secondary` | markdown 폰트 패밀리 |
| 표(GFM) 격자선 | `md-table-border`(→ `border-strong`) | 외곽 + 가로 + 세로 컬럼 격자선(세로선은 vline 수동 draw) |
| 표 헤더 밴드 | `md-table-header-bg`(→ `surface-raised`) · `md-table-header-fg`(→ `text-primary`) | 헤더 신호는 weight 아닌 색+배경 |
| 표 행 채움 | `md-table-row-bg`(→ `bg-panel`, 불투명 base) · zebra `md-table-row-bg-zebra`(→ `bg-sidebar`) | 첫 본문행=base, 2행째부터 짝수행=zebra |
| 표 셀 | `md-table-cell-fg`(→ `text-secondary`) · 패딩 `md-table-cell-padding-{x,y}`(8/4) | 값 사다리 mantle<base<surface0<surface1 |

## 갤러리 specimen

`crates/tasty-gallery/src/catalog/components/markdown_viewer.rs` — Layouts › `Content viewers` ›
`Markdown surface`. 헤딩/문단/링크/리스트/코드블록/표(격자+zebra)/캡션 대표 문서를 같은 토큰·비례로 전사. 3자
매핑: [design-gallery-mapping.md](../../../design/systems/design-gallery-mapping.md#surface-viewers-layouts).

## 시각 소스

plugin 이 host 가 forward 한 Theme 토큰으로 자가 렌더. design-system 에 마크다운 디자인이 vendor 되면 링크로 교체.
