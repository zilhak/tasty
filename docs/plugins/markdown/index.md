# Markdown Viewer (`com.tasty.markdown`)

- **Status**: Implemented (bundled plugin)
- **주체**: 로컬 사용자 (GUI surface) · AI Agent (`tasty markdown` CLI)
- **배포/통합**: bundled · surface_kind(egui-mesh) · 파일 핸들러 — [plugins 개념](../../concepts/plugins.md)
- **코드**: `crates/tasty-plugin-markdown/`(`src/render.rs` = Theme→`Visuals` 주입 + `egui_commonmark` 호출, `src/main.rs` surface/입력/링크) — [egui-mesh 채널](../../dev-guide/egui-mesh-channel.md) · egui 버전 lockstep은 [dep-issues](../../dev-guide/dep-issues.md)
- **권한**: `surface.read/write`, `fs.read`(파일 읽기 + 링크 dispatch), `file_handler.*`, `ui.settings_page` (매니페스트 `permissions`)
- **화면**: [screens/markdown.md](screens/markdown.md)

> **예제로서**: egui-mesh surface(plugin 이 자기 프로세스 egui 로 렌더) + **파일 detector/handler** + cli + settings_page 를 한 플러그인에 모은 예제 → [plugin-development](../../dev-guide/plugin-development.md#파일-핸들러-detector--handler).

## 목적

마크다운 파일을 렌더해 보는 **`markdown` surface 종류**를 제공한다. `rendering = "egui-mesh"`(ADR-0028) — 플러그인이 **자기 프로세스에서 `egui_commonmark` 라이브러리로 markdown 을 렌더·tessellate** 하고, host 가 그 mesh 를 surface 영역에 합성한다. `egui_commonmark` 는 색을 전부 `egui::Visuals` 에서 읽으므로, host 가 `set_context` 로 매 frame 전달하는 Theme 토큰을 plugin 이 `Visuals`/text-style 에 주입해 디자인 토큰대로 그린다.

## 내부 동작

- **surface_kind `markdown` (egui-mesh)** — plugin 이 `MdDoc`(파일 경로·내용·base_dir·mtime)를 소유하고, `egui_commonmark` 로 헤딩/인라인 서식/리스트/테이블/blockquote/코드/rule 을 그린다. 헤딩 사다리는 라이브러리가 `Heading`(prose-h1 앵커)↔`Body` 사이를 보간하고(per-H2 픽셀 지정 불가·본문 leading override 불가 — 정본 확정 예외, `tokens/semantic.css:137-138,152`), 표는 `Frame::group`+`Grid::striped` 로 grid border(md-table-border, 불투명)·zebra·cell fg 를 노출한다(header 밴드/불투명 base fill 은 라이브러리 Grid 제약). 코드블록은 `egui_extras` 내장 하이라이트(syntect 미도입). display_name 은 파일명.
- **콘텐츠 전달** — surface 생성 시 host 가 `surface.create{file}` 를 plugin 에 보낸다(첫 set_context bootstrap 직전, 같은 채널 FIFO 로 순서 보장). plugin 이 파일을 직접 읽는다(`fs.read`).
- **Theme parity** — host 가 resolved Theme 스냅샷(색 집합+is_light+UI zoom)을 `set_context.theme` 로 전달 → plugin 이 `Theme::with_colors_and_zoom` 으로 동일 Theme 재구성. 테마 변경 시 host 가 재forward 한다.
- **폰트** — B1 은 본문 폰트-패밀리를 egui 기본(Proportional)으로 두고 **CJK fallback 만 plugin 이 설치**(한글/일문 tofu 방지). 사용자 커스텀 markdown 폰트-패밀리 정합은 후속.
- **인라인 이미지** — 후속 단계. `egui_commonmark` 의 `load-images` feature 를 꺼둔 상태라(빌드·바이너리 절감) 현재 이미지는 로더 미설치로 렌더되지 않는다. 활성화 시 `egui_extras` 이미지 로더 + mesh 비트맵 경로(ADR-0030)로 흐른다.
- **파일 핸들러** — `detector "markdown"`(확장자 매핑) + `handler` action `open_surface{surface_kind:"markdown"}`. 마크다운 파일 열기 시 이 surface 로 뜬다.
- **파일열기 팝업** — plugin 매니페스트 `[[contributes.popup]] id="file-open"`(egui-mesh). 경로 입력 필드 + **찾아보기** + 열기/취소로 구성한다. **두 경로로 열린다**: (1) surface_kind 매니페스트 `convert_input_popup = "file-open"` capability — host 가 convert 팝업/`open_markdown`·`convert_to_markdown` 단축키/context menu 진입점에서 이 팝업을 `open_popup_instance` 로 직접 연다([ADR-0043](../../adr/0043-convert-input-popup-capability.md)), (2) event trigger `com.tasty.markdown.file_open`. **찾아보기**는 host generic **`fs.pick_file {filters,start_dir?} → {path?}`** IPC 로 native 파일 다이얼로그를 위임한다(plugin 프로세스는 native 다이얼로그를 못 연다 — host 가 winit main 스레드에서 rfd 모달을 열고 경로를 회신, [ADR-0042](../../adr/0042-fs-pick-file-native-dialog-host-delegation.md)). filters 는 caller(이 plugin)가 markdown 확장자로 채운다(host 무지). **열기 확정 시 open context 의 `surface_id` 유무로 분기**한다: 있으면 그 surface 를 제자리 변환(host `markdown.navigate {surface_id,path}` — convert-to-markdown), 없으면 새 탭으로 연다(host `file_handler.dispatch {path,depth:"deep"}` — open-markdown). `fs.pick_file` 은 임의 경로를 고르는 read 관심사라 **`fs.read`** 권한으로 게이트. large-file 확인 팝업(`id="large-file-confirm"`)도 같은 egui-mesh 팝업 계열이다.
- **링크 클릭** — 본문 링크 클릭(forward 된 실제 사용자 입력)은 plugin 이 분류·해석한 뒤 host `file_handler.dispatch` 로 보낸다(Explorer "파일 열기" 와 동일한 `DispatchFile` 경로 — 그 surface 가 속한 **Pane 의 새 탭**, 포커스 전환 없음). 경로 해석 기준:
  - **상대 경로**(`docs/index.md`, `../sibling.md`)는 **현재 마크다운 파일의 폴더(base_dir) 기준**으로 절대화한다(프로세스 cwd 가 아님). 절대 경로는 그대로.
  - **외부 URL**(`http(s)://`·`mailto:`·`data:`)만 plugin 이 OS 핸들러(`webbrowser`)로 위임한다.
  - **`#anchor`** 는 무시(문서 내 위치 — 열 대상 없음). base_dir 을 모르는 상대 경로, 존재하지 않는 경로도 무반응.
  - 이미지의 상대 경로도 동일하게 base_dir 기준으로 해석한다.
- **주소창 편집 + 히스토리 드롭다운(03)** — 상단 주소창 경로 필드는 explorer 와 공유하는 `PathField` 위젯(`crates/tasty-ui-widgets/src/path_field.rs` — `AutoComplete` 트리거 `Input` + 후보 드롭다운 + 우측 arrow-right Go `IconButton`)으로 배선된다. 비편집 시 현재 경로를 `text-secondary` 로 표시하고, 클릭해 포커스하면 편집모드로 들어가며 그 순간 generic `recent.query {kind:"markdown"}` 로 최근목록을 캐시하고 **버퍼를 비워(clear-on-focus)** 필드 아래 floating 드롭다운(최신순 최대 10개)을 필터 없이 전부 펼친다. 이후 타이핑은 `MatchMode::Substring` typeahead 필터 + 매치 구간 highlight 로 후보를 좁힌다. 키보드/포인터: **↑/↓** keyboard-active 행 이동(wrap, 필터된 가시 목록 기준), **Enter** = active 행 있으면 그 경로로·없으면 현재 버퍼로 `navigate`, **Go 클릭** = 현재 버퍼로 navigate, **Esc** = 드롭다운 닫고 버퍼 원복, 행 클릭 = 그 경로로 navigate. 확정 이동 경로는 위젯이 반환하는 확정 문자열(필터된 가시 후보/버퍼)이다 — 원본 recent 인덱스를 역참조하지 않는다. 스페이스/한글 입력은 키 forward + IME 라이브 preedit(작업1) 위에서 필드에 그대로 들어간다. 편집·이동 부수효과는 forward 된 실제 사용자 입력에서만 발생 — identity 경계 준수.
- **최근목록 조회** — host 의 generic **`recent.query {kind}`** IPC 가 그 kind 의 최근 연 파일을 **최신순 최대 10개** 반환한다(`{recent:[{path,file_name}]}`). markdown plugin 은 `kind:"markdown"` 을 채워 호출한다(host 는 특정 kind 를 모른다 — generic per-kind). 위 주소창 드롭다운의 데이터 공급원. **읽기 전용** — host `AppState.recent_files` 캐시를 필터 없이 조회할 뿐 사용자 상태(포커스/선택/히스토리)를 바꾸지 않는다(불가침 원칙). 임의 파일 read 가 아니라 이미 열었던 목록 반환뿐이라 `navigate`(FsRead)보다 약한 **`surface.read`** 권한. recent 기록 대상 여부는 매니페스트 `records_recent` capability 로 판정한다.
- **cli** — `tasty markdown recent`(최근목록 조회 — plugin CLI 서브커맨드가 `recent.query` 로 trampoline). reload 등도 plugin CLI.
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
