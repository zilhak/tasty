# Claude Code (`com.tasty.claude`)

- **Status**: Implemented (bundled plugin)
- **주체**: AI Agent / 로컬 사용자 (`tasty claude` CLI · IPC)
- **배포/통합**: bundled · cli + ipc_namespace + 멀티에이전트 — [plugins 개념](../../concepts/plugins.md)
- **코드**: `crates/tasty-plugin-claude/`
- **권한**: `process.spawn` 등 (매니페스트 `permissions`)
- **화면**: 없음 — CLI/IPC 로 터미널 surface 를 조작하는 오케스트레이션 플러그인 (headless).

> **예제로서**: **최대 통합 레퍼런스**(~3.5k줄) — cli + ipc namespace + 멀티에이전트 + **훅** + event_subscribe + 외부 설치. state/handlers/install/hook/error_scan 모듈 분리의 본보기 → [plugin-development](../../dev-guide/plugin-development.md#cli--ipc-namespace).

## 목적

**Claude Code CLI 를 tasty 안에서 실행·오케스트레이션**하는 통합. 새 워크스페이스/패인에 Claude 인스턴스를 띄우고, 부모-자식 관계로 여러 인스턴스를 spawn·제어한다 (멀티에이전트).

## 내부 동작

- **cli `claude`** (`tasty claude …`) — 서브커맨드: `launch`(새 워크스페이스에서 실행) · `spawn`(자식 인스턴스, 패인 분할) · `children`/`parent`(관계 조회) · `tell`/`broadcast`(메시지 전송) · `wait`/`wait-any`(상태 대기) · `kill`/`respawn` · `hook`(Claude Code 훅 통합: stop/notification/session-end/prompt-submit/session-start/subagent-stop).
- **ipc_namespace `claude`** — 위 동작의 IPC 표면.
- **event_subscribe** `surface.closed` — surface 종료를 받아 인스턴스 상태 정리.
- 실제 Claude 프로세스는 터미널 surface 안에서 돌고(`process.spawn`), 플러그인은 그 생명주기·관계를 관리한다.

## 인터페이스

- **AI Agent / 사용자**: `tasty claude launch|spawn|tell|wait|broadcast|kill|respawn|children|parent|hook …`.
- surface/패인 생성 자체는 [work-area](../../features/work-area/index.md) 도메인을 사용.

## 비-목표

- Claude Code 자체 기능 — 외부 CLI. 이 플러그인은 *실행·배치·관계 관리*.
- 터미널/PTY 내부 — host.

## Acceptance Criteria

- [ ] Given 플러그인 활성 When `tasty claude launch` Then 새 워크스페이스에서 Claude 가 실행된다.
- [ ] Given 부모 인스턴스 When `tasty claude spawn` Then 자식 인스턴스가 패인 분할로 생성되고 `children` 에 보인다.
- [ ] Given 자식 When `tasty claude wait --child <id>` Then idle/needs_input/exited 까지 대기한다.
</content>
