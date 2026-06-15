# Plugin 시스템

- **Status**: Implemented

외부 plugin 프로세스를 별도 OS 프로세스로 띄워 surface 종류를 확장한다.
릴리스 에셋의 `plugins.md` 참조.

### 기본 제공 plugin (built-in)
> 분류 정책 전체 (host-native / bundled / user 3 카테고리): [architecture/plugin-categories.md](architecture/plugin-categories.md). 본 절은 *카테고리 2 (bundled plugin)* 의 현재 구현 상태.

- Tasty 바이너리에 함께 묶여 배포되는 plugin은 첫 실행 시 `~/.tasty/plugins/<id>/`에 자동 설치된다 (`builtin::BUILTINS` 목록)
- 현재 기본 제공:
  - `com.tasty.explorer` (파일 탐색기 surface)
  - `com.tasty.claude` (Claude Code 통합 — `tasty claude launch|spawn|children|parent|broadcast|wait|kill|respawn|install|uninstall|hook`)
  - `com.tasty.codex` (Codex CLI 통합 — `tasty codex launch|spawn|children|parent|tell|wait|broadcast|kill|respawn|install|uninstall|hook`. claude plugin과 동일 사용법, `--surface` 미지정 시 호출자 surface의 `TASTY_SURFACE_ID` env로 fallback. `tasty codex spawn` / `tell` 도 claude 와 동일한 자동 wait chain 지원 — terminal_states 에 `untrusted` 추가됨. IPC 메서드: `codex.wait_by_surface` 도 함께 노출)
- 사용자가 plugin 메뉴에서 "제거"를 선택하면 `removed_builtins`에 기록되어 다음 실행에서 자동 재설치되지 않는다 — 외부 plugin과 완전히 동일한 라이프사이클 적용
- 번들 위치 탐색 순서: `TASTY_BUILTIN_PLUGINS_DIR` env > 실행 파일 옆 `plugins/` > dev 빌드 시 `target/<profile>/builtin-plugins/` (workspace 자동 부트스트랩, 등록된 모든 builtin 동기화)
- **권한 자동 복구**: builtin plugin이 사용자 디렉터리에는 있지만 `plugins.toml`에 grant 엔트리가 없는 경우(예: 이전 버전에서 builtin으로 인식되지 않은 채 외부 plugin처럼 설치됨), 부팅 시 매니페스트의 모든 권한을 자동 grant. `granted = []`로 명시 비워둔 경우는 entry가 있으니 건드리지 않음

### Plugin 관리 모달
- 사이드바 좌측 메뉴의 🧩 버튼으로 PluginsView 모달 진입 (Settings 모달과 동일 패턴)
- 상단 탭: **Installed**(설치된 플러그인 목록 + 상세) / **Add plugin**(외부 디렉터리에서 import)
- Installed 탭: 좌측 plugin 목록 + 우측 상세 — 이름/버전/설명/저자/홈페이지, 활성 토글, 등록 surface kinds, 매니페스트 권한 / grant 상태, 설치 경로(폴더 열기 버튼 포함), 로그 파일 경로
- **Error 상태 표시**: spawn 반복 실패로 자동 비활성화된(`auto_disabled`) plugin 은 좌측 목록 행 우측에 빨간 status dot(`accent_danger`), 우측 상세에 빨간 경고 박스("연결 실패 — Settings 에서 구성 확인")로 표시. enable 상태인 plugin 에만 나타난다 (사용자가 끈 plugin 은 정상 종료이므로 제외)
- 권한 grant/revoke 버튼으로 즉시 반영 (process 재시작 없이)
- "제거" 버튼은 사전 확인 다이얼로그를 거친 뒤 plugin 실행 종료 + 디스크 삭제. built-in plugin인 경우 추가 경고 표시
- Add plugin 탭: 경로 입력 + "확인" 버튼, 하단 "찾기" 버튼(rfd 네이티브 폴더 선택). 검증 시 매니페스트 정보(id/name/version/설명/권한/surface kinds/원본 경로) 미리보기 + 추가/취소. 이미 같은 id가 설치되어 있으면 추가 버튼 비활성화. 추가 시 `~/.tasty/plugins/<id>/`로 복사 + discovery 재실행 + 매니페스트 권한 자동 grant + auto-enable. 결과는 모달 윈도우 영역의 toast로 통지

### 매니페스트 + 디스커버리
- `~/.tasty/plugins/<id>/tasty-plugin.toml` 형식 (manifest_version=1, api_version=1)
- 부팅 시 자동 스캔, 매니페스트 검증 실패한 plugin은 warn 로그 후 스킵
- `~/.tasty/plugins.toml`로 활성/비활성 + `removed_builtins` 영속화

### 매니페스트 schema 확장 (F.H)
- `[surface_kinds.default_colors]` — plugin 이 자기 surface 의 권장 색
  (`focused_bg/fg`, `unfocused_bg/fg`) 을 직접 노출. hello 시점에 host 가
  `Theme.surface_themes` 에 머지하며, 사용자 theme TOML 의 `[surfaces.<kind>]`
  가 정의돼 있으면 *그쪽이 우선* (priority: 사용자 TOML > plugin default >
  FALLBACK_SURFACE). `crates/tasty-themes/src/plugin_defaults.rs` 가 누적 +
  user-defined 보호 invariant 유지.
- `[[contributes.window]]` + `permissions = ["window.spawn"]` — plugin 이
  OS-level 별도 윈도우를 contribute. 1.0 schema-only — host 가 hello 시
  `tracing::info!` 로그 + `plugin.window_declared` host event 발화. 실 spawn
  handler / multi-window 라우팅은 별도 영역.
- `crates/tasty-plugin-markdown/tasty-plugin.toml` 에 schema 사용 예시 +
  실 plugin binary. `BUILTINS` 등록 — 첫 부팅 시 자동 install 되며 markdown
  surface kind / detector / handler / cli 를 contribute (image plugin 과 동일
  host-rendered 패턴).
- `[[contributes.cli]]` 의 subcommand 엔트리에 `[polling]` 옵션 — `state_field`
  / `terminal_states` (예: `["idle", "needs_input", "exited"]`) / `interval_ms`
  (default 500) / `timeout_field`. 호스트가 plugin CLI 응답을 polling 하여
  terminal state 도달까지 *반복 IPC 호출* 을 manifest 기반으로 일반화. `tasty
  claude wait` 가 이 메커니즘 위에 동작 (Phase F 후속). `polling` 미설정 시는
  옛 *1 회 응답 + 즉시 종료* 동작.

### 프로세스 생명주기
- 호스트가 `127.0.0.1:0` 으로 listen, plugin이 token 들고 connect 하는 인증 방식
- stdout/stderr 자동 redirect → `~/.tasty/plugins-logs/<id>.log`
- 15초 ping / 60초 timeout 헬스체크, 비응답 시 자동 재시작
- 10초 내 spawn 실패 3회 시 자동 비활성화 (사용자 수동 enable까지 정지)
- 종료 시 모든 plugin에 graceful shutdown 송신 후 2초 timeout, 그 후 kill
- **개발용 자동 reload**: 환경변수 `TASTY_PLUGIN_AUTO_RELOAD` 가 비어있지 않고 `"0"` 도 아니면 활성. 매 pump tick 2초 간격으로 실행 중인 plugin 의 *entry binary mtime* + *manifest version* 을 baseline 과 비교, 변화 감지 시 `--restart-running` 과 동일한 graceful swap (`swap_shutdown_internal` → `swap_respawn_internal`) 으로 새 binary 부팅. `plugins.toml::disabled` 미수정. production 기본 off — flag off 시 polling cost 0. 상세: [`docs/dev-guide/plugin-ecosystem.md` §6.6](dev-guide/plugin-ecosystem.md)

### Surface 렌더링 (UI tree DSL)
- plugin이 JSON UI tree를 보내면 호스트가 egui로 렌더 (vbox/hbox/scroll/splitter/label/icon/button/tree/addressbar/text_preview/spacer)
- 호스트가 사용자 이벤트를 모아 `surface.event`로 plugin에 송신 (click/key/tree_*/addressbar_*/scroll/splitter_drag/focus_changed/resize)
- `RemoteSurface` 어댑터가 layout tree에 끼워지므로 본체 surface와 동등하게 split/tab 가능
- **Draggable splitter**: `UiNode::Splitter`의 `id: Option<String>`이 `Some`이면 호스트가 divider에 6px hit-test 영역 + 1px 중앙선을 그리고, 드래그하면 `UiEvent::SplitterDrag { node_id, ratio }`를 plugin에 송신. 부드러운 시각 피드백을 위해 egui memory에 사용자 ratio를 저장하며 plugin이 다른 값으로 응답 시 동기화. 양쪽 pane은 최소 40px 보장. SDK 헬퍼: `ui::splitter_id(id, dir, ratio, first, second)`
- **Canvas (픽셀 출력)**: `UiNode::Canvas { buffer_id, width, height, format, filter }` — plugin이 SharedBuffer에 RGBA 픽셀을 직접 쓰면 호스트가 wgpu 텍스처에 dirty rect만 부분 업로드해 egui로 합성. `tasty-shm` footer 8B `AtomicU64` generation으로 tear-free 동기화 (plugin Release-fetch_add → host Acquire-load). 포맷은 `Rgba8`/`Bgra8` (sRGB 해석), 샘플링 필터는 `Linear`/`Nearest`. `id`가 있으면 마우스 입력이 `UiEvent::CanvasPointer { node_id, x, y, phase, button }`로 라우팅 (phase: Move/Down/Up/Drag/Leave). SDK 헬퍼: `ui::canvas`, `ui::canvas_with_id`, `ui::canvas_full`

### 권한 모델
- 매니페스트의 `permissions = [...]`에 권한 토큰 (surface.read/write, notification, clipboard.read/write, fs.read/write, process.spawn, terminal.*, network, 그리고 `ipc.invoke:<plugin-prefix>`로 다른 plugin의 namespace 호출 권한) 선언
- IPC 라우터 진입에서 plugin caller일 때 `method_meta` 테이블과 매니페스트 권한 대조 — 권한 미선언 시 -32001 거부
- `plugin.*`, `window.*`, `surface.ime_*`, debug.* 메서드는 항상 plugin 호출 불가 (local-only)
- `~/.tasty/plugins.toml`의 `[grants."<id>"].granted = [...]`로 grant 영속화. CLI install은 매니페스트 권한 자동 grant (사용자 의도적 명령으로 간주)
- 권한 변경은 plugin process 재시작 없이 즉시 반영

### 격리 디렉터리
- 호스트가 spawn 시 환경변수 주입: `TASTY_PLUGIN_DIR / DATA_DIR / CONFIG_PATH / LOG_PATH`
- `~/.tasty/plugin-data/<id>/` 런타임 데이터 (업그레이드 시 보존), `~/.tasty/plugin-config/<id>.toml` 사용자 설정

### 관리 IPC/CLI
- `plugin.list / install / remove / enable / disable / permissions / grant / revoke` IPC
- `tasty plugin list / install <path> / remove <id> / enable <id> / disable <id> / logs <id> [--follow] / permissions <id> / grant <id> <perm> / revoke <id> <perm>`
- `logs`는 호스트 IPC 무관 — 파일 직접 출력 (호스트 죽었을 때도 동작)

### plugin → 호스트 IPC 호출
- plugin이 `PluginEvent::IpcCall {call_id, method, params}` 송신
- 호스트가 권한 게이트 통과 시 라우터로 디스패치, 결과를 `ipc.result` 요청으로 회신 (`call_id`로 매칭)

### Plugin SDK
- `crates/tasty-plugin-protocol`: 호스트와 plugin이 공유하는 wire 타입 (UI tree DSL, JSON-RPC envelope). serde 외 의존성 없음
- `crates/tasty-plugin-sdk`: plugin 작성용 SDK — `Plugin` trait 구현 후 `tasty_plugin_sdk::run(plugin)` 호출이면 핸드셰이크/메시지 루프 자동 처리
- `ui::*` 빌더 헬퍼 (vbox/hbox/scroll/splitter/label/button/tree/addressbar)
- `env::PluginEnv`로 호스트 환경변수 일괄 로딩
- `crates/tasty-plugin-sdk-wasm` (POC, main workspace 외부 격리): wasi-preview2 component 형식 plugin 의 host-side runtime. clipboard-history 변환 결과는 [evaluations/wasm-poc.md](evaluations/wasm-poc.md) 참조

### 동봉 plugin 예시
- `tasty-plugin-explorer`: 외부 binary로 작성된 파일 탐색기. SDK만 의존하며 호스트 코드 의존 없음
- 레이아웃: 상단 주소바 + ★+ 즐겨찾기 추가 버튼 / 좌측(트리+즐겨찾기 vertical split, drag로 비율 조절) ↔ 우측(미리보기) horizontal split, drag 가능. 비율은 `tree_ratio` / `left_inner_ratio` 두 splitter ID로 추적되어 layout snapshot에 영속화
- 즐겨찾기: `TASTY_PLUGIN_DATA_DIR/bookmarks.json`에 `{entries: [{name, path}]}` JSON으로 저장. 선택된 항목이 있고 미등록 상태일 때만 ★+ 버튼 활성. 즐겨찾기 항목 클릭 시 디렉터리면 root 이동, 파일이면 select+preview, ✕ 버튼으로 삭제
- 매니페스트 `[[contributes.commands]]`: `explorer.refresh` (F5), `explorer.go_up` (alt+up)
- 호스트의 빌트인 `ExplorerPanel`은 단계 08D에서 제거되어 plugin으로 일원화

### Plugin 단축키
- 매니페스트 `[[contributes.commands]]`로 plugin이 자기 surface에서 받을 단축키를 선언. `id`, `title_i18n_key`, `default_keybinding`, `binding_mode` 필드
- `binding_mode`:
  - `"independent"` (기본): plugin 자체 키. 호스트 키와 무관
  - `"inherit:<host_action>"`: 호스트의 의미론적 액션을 따라감. 화이트리스트는 `clipboard.copy`, `clipboard.paste`, `clipboard.cut`, `select_all`
- 매칭 우선순위: focused surface가 plugin RemoteSurface일 때 plugin 키가 호스트 액션보다 먼저 매칭. 매칭 시 이벤트 소모 → 호스트 액션은 트리거되지 않음 (`src/plugin/key_dispatch.rs`)
- 호스트 → plugin: `command.invoke` IPC 메시지 (`{ surface_id, command_id }`). SDK는 `Plugin::handle_command(CommandInvokeCtx)` 콜백으로 전달
- 사용자 오버라이드 영속화: `~/.tasty/plugins.toml`의 `[keybindings."<plugin-id>"]` 섹션. 형태: `mode = "key" | "inherit" | "none"` + 부속 필드
- 설정 → 단축키 → **Plugins** 탭: 좌측 카테고리에서 `Plugins` 선택 → 상단 드롭다운으로 plugin 선택 → 각 command별로 Mode 콤보(Inherit/Custom/None) + Inherit source 콤보(화이트리스트 4종) 또는 Custom 키 텍스트 입력 + Reset 버튼. 변경은 모달 close 시점에 plugins.toml에 기록

### Plugin CLI / IPC namespace
- 매니페스트 `[[contributes.cli]]` + `[[contributes.ipc_namespace]]`로 plugin이 자기 CLI 서브커맨드와 IPC prefix를 선언하면, `tasty <name> <sub>` 명령과 `<prefix>.<method>` IPC가 plugin에 직접 라우팅된다
- 호스트 CLI는 부팅 시 `~/.tasty/plugins/*/tasty-plugin.toml`을 스캔하여 매니페스트 기반 clap 서브커맨드를 정적 명령에 이어 동적으로 합친다. 정적 명령이 항상 우선 매칭되어 plugin이 호스트 명령을 가릴 수 없다
- CLI 인자는 매니페스트의 `arg_groups` 스키마대로 JSON-RPC params 객체로 직렬화되어 plugin에 전달된다
- IPC dispatcher는 정적 라우팅에 매칭되지 않은 메서드를 namespace registry에서 lookup하여 owner plugin에 `ipc.invoke` 메시지로 forward한다. 응답은 비동기로 도착하며, 호스트가 client 응답 채널에 연결해둔다
- 예약 prefix(`system`, `surface`, `tab`, `pane`, `workspace`, `plugin`, `hook`, `global_hook`, `message`, `tool`, `notification`, `window`, `debug`, `ui`, `ime`, `ipc`, `split`, `tree`)는 사용 불가. 같은 prefix를 두 plugin이 동시에 선언하면 나중에 로드된 쪽은 거부 (참고: `claude`/`codex` 등 번들 plugin이 점유한 prefix는 외부 plugin이 중복 선언 시 동일하게 거부됨)
- 다른 plugin namespace 호출은 `ipc.invoke:<prefix>` 권한이 필요하다. 자기 자신을 호출하는 무한 forward는 호스트가 `-32001`로 거부
- SDK: plugin은 `Plugin::handle_ipc_method(IpcMethodCtx) -> Result<Value, IpcMethodError>` 콜백 하나로 namespace 전체 dispatch를 처리. `ctx.caller_plugin_id`로 호출자가 plugin인지 사용자(CLI/IPC)인지 구분

### Plugin Surface lifecycle (Event Bus)
- 매니페스트 `event_subscribe = ["surface.closed"]`로 구독한 plugin은 다른 surface가 닫혔을 때 Event Bus를 통해 `surface.closed` envelope을 받는다
- 알림 payload: `{ surface_id, kind, reason }`. reason은 `"user"`(PTY 종료/단축키/탭 우클릭) / `"ipc"`(IPC `surface.close` / `surface.close_self`) / `"crash"`(plugin 프로세스 크래시 cascade)
- 호스트가 fire-and-forget으로 fan-out 하며 plugin 응답은 무시. SDK는 `Plugin::on_event(EventDispatchCtx)` 트레이트 메서드로 dispatch
- cascade 닫힘(탭/팬/워크스페이스 전체 삭제로 따라가는 surface)은 envelope 대상이 아니며, 명시적으로 close된 surface_id만 발사된다

### Plugin extension (다른 plugin 확장)
- 매니페스트 `[extends]` 블록으로 특정 target plugin의 IPC 호출/이벤트 발화를 가로채는 extension plugin을 작성 가능. 단일 A+ 제약: target 1개당 활성 extension은 최대 1개 (lexicographic 우선순위로 winner 결정, 나머지는 `Conflict` 상태)
- hook 종류: `pre_event` / `post_event` / `pre_ipc` / `post_ipc`. mode: `transform`(payload 교체) / `filter`(`pass: bool` 차단) / `observe`(관찰만)
- 권한: `[extends]` 선언 plugin은 `permissions[]`에 `ext:<target_id>` 토큰 필수
- 상태: `Active` / `Pending(reason)` (대상 미설치/버전 불일치 등) / `Disabled` / `Conflict`. 대상 install/enable 시 자동 승격
- Fail-open: hook timeout/에러 시 원래 값 사용, 흐름은 계속. `(extension_id, target_key)` 연속 3회 실패 → 60초 backoff. self-loop(caller == extension_id) skip
- SDK: `Plugin::handle_extension_hook(ctx) -> ExtensionHookOutcome` (`pass()` / `block()` / `transformed(v)`). 작성 가이드: `docs/dev-guide/plugin-development.md` §6-1
- CLI: `tasty plugin extension list`로 전체 extension 상태 조회 (`plugin.extension.list` IPC)

### Plugin i18n
- 매니페스트 `lang_dir` (기본 `"lang"`): plugin 디렉터리 내 lang 파일들이 위치
- 호스트는 plugin 디스커버리 시 `<lang_dir>/en.toml`(fallback) + `<lang_dir>/<active>.toml`을 읽어 namespace overlay로 호스트 i18n registry에 머지
- lookup 순서: 호스트 base → plugin namespaces. base에 동일 키가 있으면 plugin은 호스트 키를 덮어쓸 수 없음
- plugin install 시 `register_namespace`, remove 시 `unregister_namespace` (`src/i18n.rs` — 본 바이너리 잔존, GUI-free 도메인 crate 후보)

### 한계
- IPC 게이트는 plugin이 호스트를 통한 호출만 막음. plugin이 직접 fs를 쓰면 호스트가 알 수 없음 — 향후 OS-level 샌드박스/WASM으로 보강 ([평가](evaluations/plugin-sandbox.md))
- 호스트의 빌트인 ExplorerPanel은 단계 08D에서 외부 plugin으로 일원화 예정 (1300+ 줄 침습적 refactor라 별도 작업으로 분리)
