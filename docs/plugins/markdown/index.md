# Markdown Viewer (`com.tasty.markdown`)

- **Status**: Implemented (bundled plugin)
- **주체**: 로컬 사용자 (GUI surface) · AI Agent (`tasty markdown` CLI)
- **배포/통합**: bundled · surface_kind(egui-mesh) · 파일 핸들러 — [plugins 개념](../../concepts/plugins.md)
- **코드**: `crates/tasty-plugin-markdown/`(`src/render.rs` 파싱+egui 렌더, `src/main.rs` surface/입력/링크) — [egui-mesh 채널](../../dev-guide/egui-mesh-channel.md)
- **권한**: `surface.read/write`, `fs.read`(파일 읽기 + 링크 dispatch), `file_handler.*`, `ui.settings_page` (매니페스트 `permissions`)
- **화면**: [screens/markdown.md](screens/markdown.md)

> **예제로서**: egui-mesh surface(plugin 이 자기 프로세스 egui 로 렌더) + **파일 detector/handler** + cli + settings_page 를 한 플러그인에 모은 예제 → [plugin-development](../../dev-guide/plugin-development.md#파일-핸들러-detector--handler).

## 목적

마크다운 파일을 렌더해 보는 **`markdown` surface 종류**를 제공한다. `rendering = "egui-mesh"`(ADR-0028) — 플러그인이 **자기 프로세스에서 markdown 을 egui 로 파싱·레이아웃·tessellate** 하고, host 가 그 mesh 를 surface 영역에 합성한다. host 는 Theme 토큰(색/간격/폰트크기)을 `set_context` 로 매 frame 전달해 plugin 이 디자인 토큰대로 그린다.

## 내부 동작

- **surface_kind `markdown` (egui-mesh)** — plugin 이 `MdDoc`(파일 경로·내용·base_dir·mtime)를 소유하고, 헤딩 6단계/인라인 서식/리스트/테이블/blockquote/코드/rule 을 egui 로 그린다. display_name 은 파일명.
- **콘텐츠 전달** — surface 생성 시 host 가 `surface.create{file}` 를 plugin 에 보낸다(첫 set_context bootstrap 직전, 같은 채널 FIFO 로 순서 보장). plugin 이 파일을 직접 읽는다(`fs.read`).
- **Theme parity** — host 가 resolved Theme 스냅샷(색 집합+is_light+UI zoom)을 `set_context.theme` 로 전달 → plugin 이 `Theme::with_colors_and_zoom` 으로 동일 Theme 재구성. 테마 변경 시 host 가 재forward 한다.
- **폰트** — B1 은 본문 폰트-패밀리를 egui 기본(Proportional)으로 두고 **CJK fallback 만 plugin 이 설치**(한글/일문 tofu 방지). 사용자 커스텀 markdown 폰트-패밀리 정합은 후속.
- **인라인 이미지** — B1 은 alt/대상 텍스트로 대체(비트맵 텍스처 로딩은 후속).
- **파일 핸들러** — `detector "markdown"`(확장자 매핑) + `handler` action `open_surface{surface_kind:"markdown"}`. 마크다운 파일 열기 시 이 surface 로 뜬다.
- **링크 클릭** — 본문 링크 클릭(forward 된 실제 사용자 입력)은 plugin 이 분류·해석한 뒤 host `file_handler.dispatch` 로 보낸다(Explorer "파일 열기" 와 동일한 `DispatchFile` 경로 — 그 surface 가 속한 **Pane 의 새 탭**, 포커스 전환 없음). 경로 해석 기준:
  - **상대 경로**(`docs/index.md`, `../sibling.md`)는 **현재 마크다운 파일의 폴더(base_dir) 기준**으로 절대화한다(프로세스 cwd 가 아님). 절대 경로는 그대로.
  - **외부 URL**(`http(s)://`·`mailto:`·`data:`)만 plugin 이 OS 핸들러(`webbrowser`)로 위임한다.
  - **`#anchor`** 는 무시(문서 내 위치 — 열 대상 없음). base_dir 을 모르는 상대 경로, 존재하지 않는 경로도 무반응.
  - 이미지의 상대 경로도 동일하게 base_dir 기준으로 해석한다.
- **cli** — `tasty markdown …`(reload 등).
- **settings_page** — `markdown` 페이지.

## 인터페이스

- **사용자**: 마크다운 파일 열기 → markdown surface, 또는 surface 종류 전환.
- **AI Agent**: `tasty markdown …` CLI / `markdown.*` IPC. surface 생성은 [work-area](../../features/work-area/index.md) (`--type markdown`).

## 비-목표

- surface 배치/생성 도메인 — [work-area](../../features/work-area/index.md).
- 픽셀/타이포 — design-system.

## Acceptance Criteria

- [ ] Given markdown 플러그인 활성 When 마크다운 파일 열기 Then markdown surface 로 렌더된다.
- [ ] Given `tasty new tab --type markdown --file <f>` Then 그 파일이 렌더된다.
- [ ] Given `tasty list surfaces` Then 해당 surface 가 `kind:"markdown"` 으로 보고된다.

## 화면

- [screens/markdown.md](screens/markdown.md) — 마크다운 렌더 surface.
</content>
