# Image (`com.tasty.image`)

- **Status**: Implemented (bundled plugin)
- **주체**: 로컬 사용자 (GUI surface) · AI Agent (`tasty image` CLI)
- **배포/통합**: bundled · surface_kind(host-rendered) · 파일 핸들러 — [plugins 개념](../../concepts/plugins.md)
- **코드**: `crates/tasty-plugin-image/`, host 렌더 `crates/tasty-model/src/image_panel.rs` + `src/engine/surface_registry/builtins.rs`(`register_image`)
- **권한**: 매니페스트 `permissions`
- **화면**: [screens/image.md](screens/image.md)

> **예제로서**: host-rendered surface 의 **최소 예제**(61줄 단일 main.rs). 새 host-rendered surface 의 시작점 → [plugin-development](../../dev-guide/plugin-development.md#surface-kind--rendering-3-종).

## 목적

이미지를 보고 간단히 그리는 **`image` surface 종류**(뷰어 + 그림판)를 제공한다. `rendering = "host"` — host 가 egui + 텍스처로 직접 그린다.

## 내부 동작

- **surface_kind `image` (host-rendered)** — host 가 `ImagePanel` 렌더. 파일 로드 또는 빈 캔버스(`new_blank`). display_name 은 파일명.
- **파일 핸들러** — `detector "image"`(확장자 규칙) + `handler` `open_surface{surface_kind:"image"}`. 이미지 파일 열기 시 이 surface.
- **cli** — `tasty image open …`(surface 를 image kind 로 전환 + 파일 로드) 등. `image.*` IPC.

## 인터페이스

- **사용자**: 이미지 파일 열기 → image surface. 빈 캔버스로 그림판 사용.
- **AI Agent**: `tasty image …` CLI / `image.*` IPC. surface 생성은 [work-area](../../features/work-area/index.md) (`--type image`).

## 비-목표

- surface 배치/생성 도메인 — [work-area](../../features/work-area/index.md).
- 그림판 편집 도구 상세 — design-system / 구현.

## Acceptance Criteria

- [ ] Given image 플러그인 활성 When 이미지 파일 열기 Then image surface 로 표시된다.
- [ ] Given `tasty image open --file <f>` Then 활성 surface 가 image kind 로 전환되어 파일을 로드한다.
- [ ] Given 빈 캔버스 Then 그림판으로 그릴 수 있다.

## 화면

- [screens/image.md](screens/image.md) — 이미지 뷰어 / 그림판 surface.
</content>
