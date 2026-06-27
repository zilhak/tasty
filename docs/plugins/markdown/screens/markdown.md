# Markdown surface 화면

- **부모 기획**: [../index.md](../index.md)
- **시각 소스**: host-rendered (egui) — `design-system/` 의 마크다운 surface 디자인(있으면), vendor 예정.

[작업 영역](../../../features/work-area/screens/work-area.md) 타일 안에 열리는 마크다운 렌더 surface. host 가 egui 로 그린다.

## 트리거

마크다운 파일 열기 또는 `markdown` surface 생성/전환.

## UI 요소 인벤토리

- **렌더된 마크다운 본문** — 제목/문단/목록/코드블록/링크 등.
- 탭 표시명은 파일명.

## 상태별 시각

- 파일 없음/로드 실패 등은 surface 내 표시.

## 디자인 토큰 매핑

`src/adapters/ui/surface/markdown.rs::draw_markdown` 가 `ScrollArea::vertical` 안에서
`egui_commonmark` 로 그리며, theme 색을 commonmark visuals 에 주입한다. 폰트는 본문 기준 비례:

| UI 요소 | 토큰 / 비례 | 비고 |
|---|---|---|
| surface 배경 | `bg-panel` | toolbar 없음, 본문이 타일 채움 |
| 본문/헤딩 텍스트 | `text-secondary` | `override_text_color = subtext1`(헤딩 포함) |
| 헤딩 크기 | body × 1.5 | `TextStyle::Heading` |
| small 캡션 | body × 0.85 · `text-muted` | `TextStyle::Small` |
| 링크 | `accent-primary` | `hyperlink_color` |
| 코드블록 배경 | `surface-raised` | `code_bg_color = surface0` |
| 코드 텍스트 | mono · `text-secondary` | `TextStyle::Monospace` |

## 갤러리 specimen

`crates/tasty-gallery/src/catalog/components/markdown_viewer.rs` — Layouts › `Content viewers` ›
`Markdown surface`. 헤딩/문단/링크/리스트/코드블록/캡션 대표 문서를 같은 토큰·비례로 전사. 3자
매핑: [design-gallery-mapping.md](../../../design/systems/design-gallery-mapping.md#surface-viewers-layouts).

## 시각 소스

host-rendered 이므로 픽셀은 host 렌더 + 테마 토큰. design-system 에 마크다운 디자인이 vendor 되면 링크로 교체.
