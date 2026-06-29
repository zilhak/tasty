# Markdown Viewer (`com.tasty.markdown`)

- **Status**: Implemented (bundled plugin)
- **주체**: 로컬 사용자 (GUI surface) · AI Agent (`tasty markdown` CLI)
- **배포/통합**: bundled · surface_kind(host-rendered) · 파일 핸들러 — [plugins 개념](../../concepts/plugins.md)
- **코드**: `crates/tasty-plugin-markdown/`, host 렌더 `crates/tasty-model/src/markdown_panel.rs` + `src/engine/surface_registry/builtins.rs`(`register_markdown`)
- **권한**: surface 읽기 등 (매니페스트 `permissions`)
- **화면**: [screens/markdown.md](screens/markdown.md)

> **예제로서**: host-rendered surface + **파일 detector/handler** + cli + settings_page 를 한 플러그인에 모은 예제 → [plugin-development](../../dev-guide/plugin-development.md#파일-핸들러-detector--handler).

## 목적

마크다운 파일을 렌더해 보는 **`markdown` surface 종류**를 제공한다. `rendering = "host"` — 플러그인은 kind 를 선언만 하고 **host 가 egui 로 직접 그린다**(host-rendered whitelist).

## 내부 동작

- **surface_kind `markdown` (host-rendered)** — host 가 `MarkdownPanel`(파일 경로 보유)을 egui 로 렌더. display_name 은 파일명.
- **파일 핸들러** — `detector "markdown"`(확장자 매핑) + `handler` action `open_surface{surface_kind:"markdown"}`. 마크다운 파일 열기 시 이 surface 로 뜬다.
- **링크 클릭** — 본문 링크 클릭은 **tasty 파일 핸들러로 dispatch** 된다(Explorer "파일 열기" 와 동일한 `DispatchFile` 경로 — 그 surface 가 속한 **Pane 의 새 탭**, 포커스 전환 없음). 경로 해석 기준:
  - **상대 경로**(`docs/index.md`, `../sibling.md`)는 **현재 마크다운 파일의 폴더(base_dir) 기준**으로 절대화한다(프로세스 cwd 가 아님). 절대 경로는 그대로.
  - **외부 URL**(`http(s)://`·`mailto:`·`data:`)만 OS 로 위임한다.
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
