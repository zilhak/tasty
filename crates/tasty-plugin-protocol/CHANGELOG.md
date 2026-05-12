# Plugin Protocol Changelog

이 문서는 외부 plugin 작성자가 의존하는 wire schema의 변경 이력을 기록한다.

## 정책

- `HOST_API_VERSION`은 메이저 버전 단위로 호스트와 plugin 사이에 매치된다 (`src/plugin/manifest.rs::HOST_API_VERSION`).
- minor 추가는 같은 `api_version` 내에서 호환되어야 한다. 새 필드는 **optional + default**, 새 enum variant는 `#[serde(other)]`로 fallback 가능한 형태로만 허용한다.
- major 증가가 필요한 변경은 별도 RFC가 동반되어야 한다 (필드 의미 변경/제거, required 필드 추가, 에러 코드 의미 변경 등).
- 자세한 break 분류와 deprecation 절차는 [docs/dev-guide/plugin-ecosystem.md §4](../../docs/dev-guide/plugin-ecosystem.md) + (예정) `docs/dev-guide/ipc-stability.md` 참조.

본 changelog는 [Keep a Changelog 1.1](https://keepachangelog.com/en/1.1.0/) 형식을 따른다.

## [Unreleased]

### Added
- IPC alias 정규화 layer — 옛 메서드 이름이 새 이름과 같은 핸들러로 라우팅된다.

### Changed
- `surface.meta_set` / `meta_get` / `meta_unset` / `meta_list`이 `surface.meta.set` / `meta.get` / `meta.unset` / `meta.list`(점 표기)로 정규화됨. 핸들러/method_meta는 새 이름만 등록.

### Deprecated
- `surface.meta_set` / `surface.meta_get` / `surface.meta_unset` / `surface.meta_list` — 1.0 tag 직전에 alias가 제거된다. 새 호출자는 점 표기 사용.

### Removed
- (없음)

### Fixed
- (없음)

## [api_version = 1, baseline] — 2026-05-12

baseline 시점의 schema를 기록한다. 이후 모든 변경은 `[Unreleased]`에 적은 뒤 릴리스 시 버전 헤더로 옮긴다.

### 핵심 메시지

- `AuthMessage { plugin_id, token }` — 호스트 listener에 첫 줄로 송신
- `PluginEvent` — plugin → 호스트 알림 (`Hello`, `Log`, `IpcCall`, surface event 등)
- `PluginRequest { id, method, params }` — 호스트 → plugin 요청 (`ping`, `surface.*`, `command.invoke`, `ipc.invoke`, `ipc.result`, `shutdown`)
- `PluginResponse { id, result, error, error_code }` — plugin → 호스트 응답 (JSON-RPC 형식)
- `IpcCallResult { call_id, result, error }` — 호스트가 `ipc.result` 요청 안에 담아 plugin에 전달
- `IpcInvokeParams { method, params, caller_plugin_id }` — `ipc.invoke` 요청 본문

### UI 트리

- `UiNode` (텍스트/버튼/Tree/Splitter 등)
- `UiEvent` (사용자 입력 노티)
- `ButtonStyle`, `LabelStyle`, `SelectionMode`, `SplitDir`

### 메서드 상수

- `METHOD_PING` / `METHOD_SURFACE_CREATE` / `METHOD_SURFACE_EVENT` / `METHOD_SURFACE_RESTORE` / `METHOD_SURFACE_SNAPSHOT` / `METHOD_SURFACE_DESTROY` / `METHOD_COMMAND_INVOKE` / `METHOD_IPC_INVOKE` / `METHOD_IPC_RESULT` / `METHOD_SHUTDOWN`

이 baseline은 phase1 plugin extension 완료 시점의 schema에 해당한다.
