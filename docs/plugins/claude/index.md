# Claude Code (`com.tasty.claude`)

- **Status**: Implemented (bundled plugin)
- **주체**: AI Agent / 로컬 사용자 (`tasty claude` CLI · IPC)
- **배포/통합**: bundled · cli + ipc_namespace + 멀티에이전트 — [plugins 개념](../../concepts/plugins.md)
- **코드**: `crates/tasty-plugin-claude/`
- **권한**: `process.spawn` 등 (매니페스트 `permissions`)
- **화면**: 없음 — CLI/IPC 로 터미널 surface 를 조작하는 오케스트레이션 플러그인 (headless).
- **플로우**: 멀티에이전트 오케스트레이션 다이어그램 (spawn·tell·wait·hook·상태머신) — [Figma · Flows & IA](https://www.figma.com/design/ct3uPefwY2uk6i1i9wYpkU/Untitled?node-id=33-915).

> **예제로서**: **최대 통합 레퍼런스**(~3.5k줄) — cli + ipc namespace + 멀티에이전트 + **훅** + event_subscribe + 외부 설치. state/handlers/install/hook/error_scan 모듈 분리의 본보기 → [plugin-development](../../dev-guide/plugin-development.md#cli--ipc-namespace).

## 목적

**Claude Code CLI 를 tasty 안에서 실행·오케스트레이션**하는 통합. 새 워크스페이스/패인에 Claude 인스턴스를 띄우고, 부모-자식 관계로 여러 인스턴스를 spawn·제어한다 (멀티에이전트).

## 내부 동작

- **cli `claude`** (`tasty claude …`) — 서브커맨드: `launch`(새 워크스페이스에서 실행) · `spawn`(자식 인스턴스, 패인 분할) · `children`/`parent`(관계 조회) · `tell`/`broadcast`(메시지 전송) · `kill`/`respawn` · `reboot`(같은 세션 resume 재시작, 아래) · `hook`(Claude Code 훅 통합: stop/notification/session-end/prompt-submit/session-start/subagent-stop) · `notify-done`(내부용: spawn/tell 상태 전환 시 caller 에게 알림 전달 + 형제 hook 정리·재무장, 아래).
- `spawn`/`tell`은 **동기 블록 없이 즉시 반환**한다. 대상(child 또는 tell 대상 surface)이 idle/needs_input 에 도달할 때마다, 그리고 최종적으로 exited 에 도달했을 때 caller surface(spawn/tell을 호출한 surface)에 완료 메시지가 자동으로 주입된다 — `claude-idle`/`needs-input`/`process-exit` 3개의 once(1회성) surface hook을 등록해 구현하며, 그중 하나가 fire되면 `notify-done`이 알림 전송 + 나머지 형제 hook 정리 후, target surface 가 아직 살아있으면(=이번 fire 가 process-exit 가 아니었으면) `surface.locate` 로 확인해 3개 hook 을 다시 등록한다(자기재무장). 이 덕분에 needs-input(되묻기) 같은 일시적 상태 전환을 거쳐도 그 뒤 진짜 완료 시 알림을 놓치지 않는다 — "spawn/tell 당 알림 1회"가 아니라 "child 가 살아있는 동안 상태 전환마다 알림"이다.
- **ipc_namespace `claude`** — 위 동작의 IPC 표면.
- **event_subscribe** `surface.closed` — surface 종료를 받아 인스턴스 상태 정리.
- 실제 Claude 프로세스는 터미널 surface 안에서 돌고(`process.spawn`), 플러그인은 그 생명주기·관계를 관리한다.
- **`reboot`** (`tasty claude reboot [--surface <id>] [--delay <초>] [--prompt <추가문구>]`) — surface 안의 Claude 를 종료하고 **같은 세션으로 재시작**한다. Claude 는 스스로 자기 TUI 를 껐다 켤 수 없으므로 에이전트가 이 명령을 자기 surface 에 호출한다(설정/훅/버전 변경 반영용). 동작: 즉시 응답 반환 → `--delay`(기본 5s) 후 Ctrl+C ×4(0.5s 간격) → 전경 프로세스가 Claude 에서 이탈했는지 확인 후 셸에 `claude -r <session_id>` 전송(session id 는 요청 시점에 surface meta `claude-session-id` 에서 캡처) → Claude 복귀 확인 후 재시작 안내 프롬프트를 `terminal.tell` 로 제출(화면 검증·재시도 + 별도 Enter 로 결정적 제출). 안전 가드: 전경이 여전히 Claude 면 텍스트 미전송·중단, resume 후 미복귀면 안내 미전송(셸 오염 방지), 같은 surface 중복 reboot 거부. **턴의 마지막 행동으로 호출할 것** — delay 이후 진행 중이던 턴은 잘린다.
- spawn 시 parent 의 살아있는 child 수가 설정 임계치를 넘으면 응답에 `warning` 필드가 실린다 — Settings › Plugin › Claude Code 에서 임계치 조정.

## 인터페이스

- **AI Agent / 사용자**: `tasty claude launch|spawn|tell|broadcast|kill|respawn|children|parent|hook …`.
- surface/패인 생성 자체는 [work-area](../../features/work-area/index.md) 도메인을 사용.

## 비-목표

- Claude Code 자체 기능 — 외부 CLI. 이 플러그인은 *실행·배치·관계 관리*.
- 터미널/PTY 내부 — host.

## Acceptance Criteria

- [ ] Given 플러그인 활성 When `tasty claude launch` Then 새 워크스페이스에서 Claude 가 실행된다.
- [ ] Given 부모 인스턴스 When `tasty claude spawn` Then 자식 인스턴스가 패인 분할로 생성되고 `children` 에 보인다.
- [ ] Given 자식 When `tasty claude spawn`(또는 `tell`) 후 자식이 idle/needs_input/exited 에 도달 Then caller surface 에 완료 알림이 주입되고 형제 hook 이 함께 정리된다. 자식이 exited 가 아닌 상태(idle/needs_input)로 도달한 경우엔 형제 hook 이 재등록돼 그 뒤 상태 전환에도 계속 알림이 온다.
</content>
