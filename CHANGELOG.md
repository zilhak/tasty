# Changelog

본 문서는 사용자(AI 에이전트 포함)가 의존하는 표면 — CLI 명령, IPC 메서드, 매니페스트 스키마, plugin 인터페이스 — 의 변경만 기록한다. 내부 refactor·테스트·문서는 `git log`를 참조.

형식: [Keep a Changelog 1.1](https://keepachangelog.com/en/1.1.0/). 버전: [SemVer](https://semver.org/lang/ko/).

각 변경은 다음 카테고리 중 하나에 속한다:

- `Added` — 새 기능, 새 메서드/명령
- `Changed` — 동작 변경 (BREAK는 머리에 `(BREAK)` 표기)
- `Deprecated` — 폐기 예정, 아직 동작은 함
- `Removed` — 제거된 기능
- `Fixed` — 버그 수정

자세한 안정성 정책·break 분류·deprecation 절차는 [`docs/dev-guide/api-conventions.md`](docs/dev-guide/api-conventions.md) 의 "안정성 정책" 절 참조.

## [Unreleased]

### Removed

- (BREAK) `tasty design *` CLI 서브커맨드 11종(`login`/`logout`/`import-session`/`status`/`projects`/`detect`/`probe`/`chat`/`chat-status`/`turn-status`/`protocol`) 및 그 IPC(`design.*`) 전체 제거 — `claude-design` 플러그인이 tasty 본체에서 완전히 빠지며 별도 프로젝트로 분리된다. 대체/alias 없음. 상세: [ADR-0057](docs/adr/0057-remove-claude-design-plugin.md).

### Fixed

- `tasty remote attach --raw`(및 `tasty attach --raw`): 서버/터널 연결이 끊겨도 `--reconnect`(기본 ON) 백오프 재연결이 전혀 동작하지 않던 결함 수정. raw 브리지가 종료 사유와 무관하게 `process::exit(0)` 으로 프로세스를 죽여 재연결 판단 지점(`AttachExit::Disconnected`)에 도달하지 못했다 — 이제 mirror-dump 와 동일하게 채널 기반으로 종료 사유를 구분해 정상 반환한다.

## [0.9.7] - 2026-07-15

많은 변경이 있었음(누적된 릴리스 갭).

## [0.9.6] - 2026-07-15

많은 변경이 있었음(누적된 릴리스 갭).

## [0.9.4] - 2026-07-14

많은 변경이 있었음(누적된 릴리스 갭).
