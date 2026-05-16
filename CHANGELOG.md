# Changelog

본 문서는 사용자(AI 에이전트 포함)가 의존하는 표면 — CLI 명령, IPC 메서드, 매니페스트 스키마, plugin 인터페이스 — 의 변경만 기록한다. 내부 refactor·테스트·문서는 `git log`를 참조.

형식: [Keep a Changelog 1.1](https://keepachangelog.com/en/1.1.0/). 버전: [SemVer](https://semver.org/lang/ko/).

각 변경은 다음 카테고리 중 하나에 속한다:

- `Added` — 새 기능, 새 메서드/명령
- `Changed` — 동작 변경 (BREAK는 머리에 `(BREAK)` 표기)
- `Deprecated` — 폐기 예정, 아직 동작은 함
- `Removed` — 제거된 기능
- `Fixed` — 버그 수정

자세한 안정성 정책·break 분류·deprecation 절차는 [`docs/dev-guide/ipc-stability.md`](docs/dev-guide/ipc-stability.md) 참조.

## [Unreleased]

### Added
- `tasty-output` 크레이트 — surface 출력 시멘틱 파서 골격. 빌트인 4종(기본 활성): `path` (파일 경로 + line/col), `url` (http/https/ftp/ssh/file), `prompt_boundary` (OSC 133 A/B/C/D), `exit_code` (OSC 133 D 페이로드). 옵션 6종 (명시적 opt-in): `compile_error` (rustc/gcc/clang/tsc, 멀티라인), `stack_trace` (python/rust/node/java, 멀티라인), `test_result` (cargo/pytest/jest), `progress` (bar/size/percent), `osc_link` (OSC 8), `osc_notification` (OSC 9/777). `Parser` trait 에 `parse_block` default impl 신설 — 단일 라인 파서는 변동 없이 `parse_line` 만 구현하면 되고, 멀티라인 파서는 `parse_block` override 로 block 컨텍스트를 본다. 옵저버 스트리밍은 라인 단위 dispatch 만 사용하므로 멀티라인 파서는 `surface.parse_since_mark` (batch) 경로에서만 발화한다. 카탈로그: [`docs/agent-guide/output-parsers.md`](docs/agent-guide/output-parsers.md).
- IPC: `surface.parse_since_mark { surface_id, parsers? }` — read_since_mark 결과를 파서들로 분해해 `items: [{ kind, line, byte_start, byte_end, data }]` 반환. CLI: `tasty read parse-since-mark`.
- OSC 133 명령 인덱싱 — 셸 통합이 보내는 `\e]133;{A|B|C|D};...` 시퀀스를 추적해 surface 별로 `{ prompt_started_at, command_started_at, ended_at, exit_code, command }` JSON 레코드를 `tasty-memory` `scope=surface:<id>` 위에 `tasty.commands.<unix-ms>` 키로 영속화. 새 IPC: `surface.commands { surface_id, limit?, since? }`, `surface.last_command { surface_id }`, `surface.command_at { surface_id, index }` (음수 인덱스 지원, 모두 `TerminalRead` 권한). CLI: `tasty read commands`, `tasty read last-command`, `tasty read command-at --index N`. terminal 엔진에 `TerminalEventKind::PromptBoundary { phase, payload }` 이벤트 신설.
- 출력 옵저버 — PTY 라인 → 빌트인 파서 → sink fan-out 인프라. terminal 엔진에 `TerminalEventKind::OutputAppended { text }` 신설 (Print/PrintString/LineFeed 에서 emit), 호스트에 `ObserverRouter` (per-surface 라인 버퍼 + 파서 dispatch + per-observer bounded channel) + sink worker thread. sink 2 종: `memory` (`scope=global` 위 `tasty.observer.<id>.<ms>` 키로 누적, `max_records` ring buffer), `file` (JSONL append; 기본 경로 `~/.tasty/observers/<id>.jsonl`). Backpressure 정책: bounded channel 가득 차면 새 item drop + `dropped` counter 증가 (PTY freeze 방지). 새 IPC (`TerminalRead`): `output.observe_start { surface_id?, parsers?, kinds?, sink: { type, ... } }`, `output.observe_stop`, `output.observe_list`, `output.observe_info`. CLI: `tasty output observe {start|stop|list|info}`. Surface 가 닫히면 그 surface 에 매인 옵저버 자동 정리 (wildcard `surface_id=None` 옵저버는 유지). socket/fifo sink + 옵저버 spec persistence 는 후속 phase.
- IPC 메서드 별칭 정규화 layer (`src/ipc/alias.rs`). 옛 이름은 호스트가 새 이름으로 자동 매핑하면서 `tracing::warn`을 출력.
- 명명 규칙 자동 검증 테스트 (`src/ipc/method_meta.rs::all_registered_methods_match_naming_policy`).
- Plugin SDK에 `PluginError` 도메인 에러 + `From<PluginError> for IpcMethodError`.
- Plugin surface lifecycle observer — 매니페스트 `[[contributes.surface_observer]] event = "closed"`(`surface.read` 권한 필수)로 구독하면 다른 surface가 닫혔을 때 `Plugin::on_surface_lifecycle(SurfaceLifecycleCtx { event, surface_id, kind, reason })` 콜백을 받는다. fire-and-forget. reason은 `UserClose`(PTY 종료/단축키/탭 우클릭) 또는 `AgentClose`(IPC `surface.close*`). SDK에 `SurfaceLifecycleCtx` / `SurfaceLifecycleEvent` / `SurfaceCloseReason` 노출.
- Plugin manifest 의 예약 IPC prefix 에 `memory`, `output` 추가 — 호스트 메서드와 충돌하지 않도록 plugin 이 해당 namespace 를 점유하면 매니페스트 검증 단계에서 거부된다.

### Changed
- `surface.meta_set` / `meta_get` / `meta_unset` / `meta_list` → `surface.meta.set` / `meta.get` / `meta.unset` / `meta.list` (점 표기). 옛 이름은 alias로 동작하지만 deprecated.
- Plugin SDK `HostHandle::call` 반환 타입이 `Result<Value, HostCallError>` → `Result<Value, PluginError>`. `HostCallError`는 `PluginError`의 `#[deprecated]` alias로 유지.
- `surface.meta.*` 가 파일 기반 (`~/.tasty/surfaces/<id>/meta.json` 풍의 임시 디렉터리) 에서 `tasty-memory` 위 `scope=surface:<id>` text/plain entry 로 통합 (응답 형태 동일). 같은 row 가 `memory.*` API 로도 보이며 `memory.changed` 이벤트로 변경이 전파된다. 키 형식 검증 (`[a-z0-9._-]+`, 1..=256) 이 새로 강제되므로 대문자/공백 키는 거부된다.

### Deprecated
- `surface.meta_set` / `surface.meta_get` / `surface.meta_unset` / `surface.meta_list` (underscore 합성). 1.0 tag 직전에 alias 제거.
- `tasty_plugin_sdk::HostCallError` type alias. 새 코드는 `PluginError` 사용.

### Removed
- (없음)

### Fixed
- (없음)
