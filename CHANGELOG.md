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
- IPC 메서드 별칭 정규화 layer (`src/ipc/alias.rs`). 옛 이름은 호스트가 새 이름으로 자동 매핑하면서 `tracing::warn`을 출력.
- 명명 규칙 자동 검증 테스트 (`src/ipc/method_meta.rs::all_registered_methods_match_naming_policy`).
- Plugin SDK에 `PluginError` 도메인 에러 + `From<PluginError> for IpcMethodError`.

### Changed
- `surface.meta_set` / `meta_get` / `meta_unset` / `meta_list` → `surface.meta.set` / `meta.get` / `meta.unset` / `meta.list` (점 표기). 옛 이름은 alias로 동작하지만 deprecated.
- Plugin SDK `HostHandle::call` 반환 타입이 `Result<Value, HostCallError>` → `Result<Value, PluginError>`. `HostCallError`는 `PluginError`의 `#[deprecated]` alias로 유지.

### Deprecated
- `surface.meta_set` / `surface.meta_get` / `surface.meta_unset` / `surface.meta_list` (underscore 합성). 1.0 tag 직전에 alias 제거.
- `tasty_plugin_sdk::HostCallError` type alias. 새 코드는 `PluginError` 사용.

### Removed
- (없음)

### Fixed
- (없음)
