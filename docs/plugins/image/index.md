# Image (`com.tasty.image`)

- **Status**: Implemented (bundled plugin)
- **주체**: 로컬 사용자 (GUI surface) · AI Agent (`tasty image` CLI)
- **배포/통합**: bundled · surface_kind(egui-mesh) · 파일 핸들러 — [plugins 개념](../../concepts/plugins.md)
- **코드**: `crates/tasty-plugin-image/`(`main.rs`/`doc.rs`/`render.rs`), 등록 `src/engine/surface_registry/egui_mesh.rs`(화이트리스트)
- **권한**: 매니페스트 `permissions`
- **결정**: [ADR-0028](../../adr/0028-plugin-egui-mesh-render-channel.md)(egui-mesh 채널) · [ADR-0030](../../adr/0030-image-egui-mesh-bitmap-texture.md)(image mesh-only 개정)
- **화면**: [screens/image.md](screens/image.md)

> **예제로서**: egui-mesh surface 가 **비트맵 텍스처 + chrome 을 함께** 그리는 예제 — plugin 이 자기 egui `Context` 에서 tessellate 한 mesh 를 host 가 합성한다(markdown 은 순수 위젯, image 는 텍스처 포함). 새 egui-mesh surface 시작점 → [plugin-development](../../dev-guide/plugin-development.md#surface-kind--rendering-3-종).

## 목적

이미지를 보고 간단히 그리는 **`image` surface 종류**(뷰어 + 그림판)를 제공한다. `rendering = "egui-mesh"` — plugin 이 비트맵을 자기 egui `Context` 의 텍스처로 올려(폰트 atlas 와 동일 `TexturesDelta` 채널) chrome 과 함께 mesh 로 tessellate 하고, host 가 합성한다. 별도 Canvas 레이어는 없다([ADR-0030](../../adr/0030-image-egui-mesh-bitmap-texture.md)).

## 내부 동작

- **surface_kind `image` (egui-mesh)** — plugin(`ImageDoc`)이 픽셀·편집 상태·zoom/pan 을 소유하고, 원본 이미지 + 편집 오버레이 + floating selection 을 텍스처로 올려 viewer/paint chrome(control bar·paint bar·8 handles·zoom)과 함께 그린다. host `EguiMeshSurface` stand-in 은 파일·display_name·영속화만. 파일 로드 또는 빈 캔버스(그림판 모드 진입).
- **파일 핸들러** — `detector "image"`(확장자 규칙) + `handler` `open_surface{surface_kind:"image"}`. 이미지 파일 열기 시 이 surface.
- **cli / IPC** — `image.save`/`export_png`/`paste`/`next`/`prev` 는 plugin 이 직접 처리(픽셀·편집·네비 상태 소유), `image.open`(surface 변환)·`image.list`(host surface 열거)는 host 로 trampoline.

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
