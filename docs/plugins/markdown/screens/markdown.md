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

`src/adapters/ui/surface/markdown/render.rs::render` 가 `ScrollArea::vertical` 안에서
`pulldown-cmark` 이벤트 스트림을 직접 egui 위젯으로 그리는 토큰 기반 6단계 prose 렌더러다.
색·크기·간격은 전부 `MdStyle` 이 `Theme` 에서 캡처한 토큰에서 온다:

| UI 요소 | 토큰 / 비례 | 비고 |
|---|---|---|
| surface 배경 | `bg-panel` | toolbar 없음, 본문이 타일 채움 |
| 본문 텍스트 | `text-secondary` · `line-height-prose` 행간 | 헤딩 색은 단계별 차별화 |
| 헤딩 크기 | `font-size-prose-h1`(20, h1) / `font-size-prose-h2`(14, h2·h3) / body(h4~h6) | 6단계 prose 위계 |
| small 캡션 | body × 0.85 · `text-muted` | |
| 링크 | `accent-primary` | |
| 코드블록 배경 | `surface-raised` | |
| 코드 텍스트 | mono · `text-secondary` | markdown 폰트 패밀리 |

## 갤러리 specimen

`crates/tasty-gallery/src/catalog/components/markdown_viewer.rs` — Layouts › `Content viewers` ›
`Markdown surface`. 헤딩/문단/링크/리스트/코드블록/캡션 대표 문서를 같은 토큰·비례로 전사. 3자
매핑: [design-gallery-mapping.md](../../../design/systems/design-gallery-mapping.md#surface-viewers-layouts).

## 시각 소스

host-rendered 이므로 픽셀은 host 렌더 + 테마 토큰. design-system 에 마크다운 디자인이 vendor 되면 링크로 교체.
