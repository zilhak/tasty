# Image surface 화면

- **부모 기획**: [../index.md](../index.md)
- **시각 소스**: plugin egui-mesh 자가 렌더 (비트맵=egui 텍스처) — `design-system/` 의 image surface 디자인(있으면), vendor 예정.

[작업 영역](../../../features/work-area/screens/work-area.md) 타일 안에 열리는 이미지 뷰어 / 그림판 surface.

## 트리거

이미지 파일 열기, `image` surface 생성/전환, 또는 빈 캔버스.

## UI 요소 인벤토리

- **이미지 뷰** — 로드된 이미지 표시(맞춤/확대 등).
- **빈 캔버스** — 파일 없이 시작한 그림판.
- 탭 표시명은 파일명(빈 캔버스면 기본 "Image").

## 상태별 시각

- 로드됨 / 빈 캔버스 / 로드 실패.

## 디자인 토큰 매핑

`crates/tasty-plugin-image/src/render.rs::draw` 가 상단 control bar + 이미지 영역을 자기 egui
`Context` 에서 그려 mesh 로 host 가 합성한다([ADR-0030](../../../adr/0030-image-egui-mesh-bitmap-texture.md)).
툴바 버튼 아이콘은 `tasty-icons` 빌드타임 베이크 벡터([ADR-0036](../../../adr/0036-plugin-icon-buildtime-bake-tasty-icons-single-source.md)),
zoom 은 텍스트 버튼:

| UI 요소 | 토큰 | 비고 |
|---|---|---|
| 캔버스 배경 | `bg-sidebar` | host `mantle` |
| 툴바 아이콘 버튼 | `surface-raised` + `border-default` · 글리프 tint `text-primary`(disabled=`text-muted`) | chevron-left/right(prev/next) · refresh · edit · plus(new) — `tasty-icons` 베이크 벡터, 24×20 고정. no-image 상태는 refresh/new 만 |
| zoom 컨트롤 | `surface-raised` + `border-default` | `Fit`/`+`/`-` **텍스트** 버튼(30×20·24×20) + `%` 라벨 |
| 파일명 라벨 | `text-muted` · `font-size-caption` | `subtext0` |
| zoom 퍼센트 | `text-muted` · `font-size-caption` | 우측 정렬 |
| 로드된 그림 프레임 | `bg-panel` + `border-default` | fit-to-window |
| fallback / 빈 안내 glyph | `IMAGE` glyph · `text-muted`/`text-disabled` | `tasty-icons` SURFACES |
| "No image" 텍스트 | `text-muted` | plugin `no_image` |

## 갤러리 specimen

`crates/tasty-gallery/src/catalog/components/image_viewer.rs` — Layouts › `Content viewers` ›
`Image surface / canvas`. viewer(그림 fit) / no-image(fallback glyph) 두 상태를 토큰으로 전사.
3자 매핑: [design-gallery-mapping.md](../../../design/systems/design-gallery-mapping.md#surface-viewers-layouts).

## 시각 소스

plugin 이 host 가 forward 한 Theme 토큰으로 자가 렌더. design-system vendor 후 링크로 교체.
