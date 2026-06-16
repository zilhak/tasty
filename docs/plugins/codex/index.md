# Codex (`com.tasty.codex`)

- **Status**: Implemented (bundled plugin)
- **주체**: AI Agent / 로컬 사용자 (`tasty codex` CLI · IPC)
- **배포/통합**: bundled · cli + ipc_namespace + 멀티에이전트 — [plugins 개념](../../concepts/plugins.md)
- **코드**: `crates/tasty-plugin-codex/`
- **권한**: `process.spawn` 등 (매니페스트 `permissions`)
- **화면**: 없음 — CLI/IPC 오케스트레이션 플러그인 (headless).

## 목적

**Codex CLI 를 tasty 안에서 실행·오케스트레이션**하는 통합. [claude](../claude/index.md) 플러그인과 동형이며, 주로 작성한 코드/판단을 Codex 에게 교차 검증시키는 용도.

## 내부 동작

- **cli `codex`** (`tasty codex …`) — 서브커맨드: `launch` · `spawn`(자식, 패인 분할) · `children`/`parent` · `tell`(메시지 전송, 줄바꿈 보존·자동 제출) · `broadcast` · `wait` · `kill`/`respawn` · `hook`(stop/notification/prompt-submit/session-start). `install`/`uninstall`(Tasty 훅을 Codex CLI 설정에 설치 — wait 동작에 필요).
- **ipc_namespace `codex`** — 위 동작의 IPC 표면.
- **event_subscribe** `surface.closed` — 인스턴스 상태 정리.

## 인터페이스

- **AI Agent / 사용자**: `tasty codex launch|spawn|tell|wait|broadcast|kill|respawn|children|parent|hook|install …`.
- 일반 흐름: `spawn --prompt "…"` → (선택) `tell` → `wait --child <idx>` → 출력 확인.

## 비-목표

- Codex 자체 기능 — 외부 CLI. 이 플러그인은 *실행·배치·관계 관리*.
- 터미널/PTY 내부 — host.

## Acceptance Criteria

- [ ] Given 플러그인 활성 When `tasty codex spawn --prompt "…"` Then 자식 Codex 가 패인 분할로 생성된다.
- [ ] Given 자식 When `tasty codex tell <msg>` Then 줄바꿈 보존하며 메시지가 전송·제출된다.
- [ ] Given `tasty codex wait --child <id>` Then idle/needs_input/exited 까지 대기한다.
</content>
