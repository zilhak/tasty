# 원격 attach 자식 (claude·codex 공통)

attach 로 연결한 원격 워크스페이스(mirror)에 `tasty claude spawn` / `tasty codex spawn` 으로 자식 에이전트를 만들고, 그 자식을 이 Tasty 의 자식처럼 tell·children·state·kill 로 다룬다. 자식의 완료는 부모가 있는 Tasty 의 기존 수신처(완료 로그·Codex 대화)에 기록된다.

- C: 부모가 있는 Tasty(attach client, mirror 보유). L: C 의 mirror surface. S: 자식이 실제로 도는 Tasty(attach 서버). R: S 의 surface.
- host 쪽 전달(출처 토큰·holder 검사·통지 outbox·ACK·tombstone)은 [attach 동작](../dev-guide/attach-behavior.md)과 [원격 attach](../features/remote-attach/index.md)가 정본이다. 이 문서는 provider plugin 이 그 위에서 하는 일만 적는다.
- 공용 코드: `crates/tasty-plugin-agent-common/src/remote_attach.rs`. claude 는 `crates/tasty-plugin-claude/src/remote.rs`·`crates/tasty-plugin-claude/src/codex_return/attach.rs`. codex 는 `crates/tasty-plugin-codex/src/handlers/remote.rs`·`crates/tasty-plugin-codex/src/delivery/attach_out.rs`.

## 매니페스트 선언

두 plugin 모두 `[contributes.remote_attach]` 를 선언한다. host 는 provider 목록을 갖지 않고 이 선언으로만 허용한다.

| plugin | `methods`(S 에서 holder 대신 실행) | `receive`(C 에서 원격 결과 수신) | `relay_meta`(S→C 로 옮기는 메타) |
|---|---|---|---|
| claude | `spawn`·`tell`·`kill` | `remote_receive` | `claude-last-stop-failure`·`claude-background-wait`·`claude-background-quiet` |
| codex | `spawn`·`tell`·`kill` | `remote_receive` | 없음 |

`relay_meta` 는 C 의 완료 알림 문구(`notify-done`·`notify-stop-failure`)가 L 에서 읽는 메타다. 중계되는 훅 이벤트는 각 plugin 의 `[[contributes.hook_events]]` 와 host 가 만드는 `process-exit` 뿐이다.

## 누가 무엇을 하나

| 일 | 어디서 |
|---|---|
| 부모 해석(물리 surface / Codex 대화), Codex 결과 수신처, 완료 알림 훅 | C plugin. 기존 경로 그대로 L 에 묶는다 |
| 자식 실행 명령·세션 토큰·settings·프로필·권한 모드·CLI·cwd | S plugin. S 에 설치된 CLI·설정·토큰으로 실행한다 |
| 훅 처리·턴 확정 | S plugin. 자식은 S 의 `TASTY_SURFACE_ID` 로 S 에 보고한다. 훅 명령·install 절차는 바뀌지 않는다 |
| 상태·이벤트·메타 중계 | host. plugin 은 할 일이 없다 |
| 턴 결과(부모가 Codex 대화일 때만) | S plugin 이 `attach.child_notice` 로 맡기고, C plugin 이 `remote_receive` 로 받아 기존 종점에 기록한다 |

훅에는 부모 Tasty 주소나 비밀이 들어가지 않는다. 결과는 기존 attach holder 연결로만 오간다.

## client(C) plugin

- **spawn**: `--workspace`(숫자 id)나 `--pane` 이 mirror 면 `attach.agent_target` 으로 확인한 뒤 `attach.agent_call{method: spawn}` 을 보낸다. mirror 가 아니면 기존 로컬 spawn 이다. workspace 이름으로는 mirror 를 고르지 않는다.
  - 서버가 기능을 모르면(`capable=false`) 아무것도 만들지 않고 오류로 끝난다.
  - 보내는 인자는 허용 목록뿐이다. claude: `cwd`·`prompt`·`profile`·`profile_file`·`permission_mode`·`model`·`effort`. codex: `cwd`·`prompt`·spawn 옵션·승인 정책. 둘 다 `return_turns`(부모가 Codex 대화인가)를 붙인다. `_tasty_context`·부모·수신처·`caller_surface` 는 보내지 않는다.
  - 사용자가 명시한 `--cwd`·`--profile-file` 경로와 `--profile` 이름은 서버가 해석하고 확인한다. 서버에 그 파일이 없으면 서버 plugin 이 터미널을 만들기 전에 거절한다. C 에서 해석한 설정 경로(기본 프로필·plugin 설정)를 대신 넣어 보내지는 않는다.
  - 응답을 받으면 L 에 기존 경로를 묶는다(물리 부모: 완료 알림 훅, Codex 부모: 결과 경로). 묶기에 성공한 뒤에만 `attach.agent_commit` 을 부른다. commit 전에는 서버가 그 자식의 통지를 보내지 않으므로 첫 완료가 수신처 없이 지나가지 않는다. 묶기에 실패하면 commit 하지 않고 그 자식을 원격 kill 로 닫은 뒤 오류로 끝난다.
  - 응답의 `child_surface_id`·`child_index` 는 C 기준(L)이다. `remote: true`, `remote_child_surface_id`(R)를 덧붙인다.
  - 결과를 알 수 없으면(-32010, 35초 상한·재연결 실패·서버 재시작) 성공이나 실패로 단정하지 않고 결과불명으로 알린다. 그 call 의 부모는 plugin 메모리에 15분 남긴다. host 가 늦은 결과를 `remote_receive{kind: late_spawn}` 으로 주면 그 부모로 경로를 묶고 `recorded` 로 답한다(host 가 commit). 기다리는 부모가 없거나(plugin 재시작·만료) 서버가 오류를 냈으면 그 자식을 원격 kill 로 닫고 `rejected` 로 답한다.
- **tell**: 대상이 C 에 등록된 원격 자식이면 결과 경로를 먼저 정하고(Codex 호출자: 경로 묶기, 그 밖: 이전 Codex 경로 해제) 입력은 `attach.agent_call{method: tell}` 로 보낸다. `--caller-surface` 의 완료 알림 훅은 L 에 다시 건다.
- **kill**: index 로 고른 자식이 원격 자식이면 `attach.agent_call{method: kill}` 로 닫는다. 결과 경로는 바로 지우지 않고 끝난 경로로 남겨(24시간) 서버에 남은 결과가 늦게 와도 기록한다.
- **children·state·parent**: host child registry 가 L 을 일반 자식처럼 답한다. claude children 은 host 의 `remote`·`liveness` 필드를 그대로 싣는다. 연결이 끊긴 동안 liveness 는 `unknown` 이다.
- **remote_receive**(`kind: notice`): payload 의 결과를 L 의 결과 경로 수신처로 기록한다. 완료 키는 서버 키에 `r.` 를 붙인다. 재전송은 같은 키라 Codex 원장이 `duplicate` 로 거른다. 경로가 없으면 `rejected`, 일시 실패는 JSON-RPC 오류(host 가 다시 보낸다)다.
  - claude 는 `codex.deliver` 를 부른다(claude→codex 단방향). codex 는 자기 namespace 를 다시 부르지 않고 내부 `delivery::deliver` 를 직접 부른다(같은 worker 재진입 교착 방지).
  - 수신처는 결과가 도착한 때의 L 경로 수신처다. 서버는 C 의 수신처를 모른다.

## 서버(S) plugin

host 는 원격 출처 호출에 `parent_external = attach:o<n>`·`attach_context`·`workspace`·`pane`(spawn), `attach_target_surface`(tell·kill)를 넣는다. plugin 은 `attach:` 부모와 `attach_context` 가 함께 있을 때만 원격 출처로 본다.

- 이 부모에게는 완료 알림 훅을 걸지 않고 `codex.parent`·Codex 호출 문맥도 쓰지 않는다. 상태·이벤트·메타는 host 가 L 로 중계한다.
- spawn 은 `terminal.spawn` 에 받은 부모·위치·문맥을 그대로 싣고, 기존 로컬 spawn 과 같은 기동 경로(세션 토큰·settings·프로필·권한 모드·오류 감시)를 쓴다. claude 는 S 의 Claude Code 설정에 Tasty Stop·StopFailure 훅이 없으면 응답에 경고를 싣는다.
- 서버의 공개 호출(서버 사용자의 CLI·다른 도구)은 attach 원격 자식을 바꾸지 못한다. `tell`·`reboot` 의 대상 surface 부모가 `attach:` 이거나, `kill`·`respawn`·`child_profile`(claude)·`broadcast` 가 `attach:` 부모 키를 지정하면 plugin 이 거절한다. host 는 그 자식의 attach 점유 예외를 생성 plugin 단위로 주므로, 이 검사가 없으면 서버 로컬 호출이 같은 plugin 을 거쳐 점유를 우회한다. plugin 의 내부 동작(auto_resume·훅 처리)은 공개 호출이 아니어서 그대로다.
- 원격 출처 호출의 대상은 `attach_target_surface` 가 지금도 그 출처 부모(`terminal.parent` 의 `parent_external`)의 자식일 때만 쓴다. 공개 호출이 출처 키를 꾸며 넣어도 다른 surface 에는 닿지 않는다. 같은 부모의 자식인지까지 꾸민 호출을 가려내는 것은 host 의 문맥 토큰 확인 몫이다.
- tell 은 `attach_target_surface` 에만 보낸다. kill 은 `terminal.children{parent_external}` 에서 그 surface 의 index 를 찾아 `terminal.kill` 한다.
- `return_turns` 가 켜진 자식은 결과 경로를 attach 부모로 둔다.
  - claude: 경로의 `attach` 표지. 확정된 턴은 기존처럼 claim 해 영속 요청으로 남기고, 송신 스레드가 `codex.deliver` 대신 `attach.child_notice{surface_id: R, key: completion_key, payload: {completion}}` 를 부른다.
  - codex: 반환 기록의 `attach_parent`. 소유 서버로 턴을 확정한 뒤 원장 대신 `attach-out/` 영속 기록으로 옮기고 `attach.child_notice` 를 부른다.
  - `pending` 이면 영속 기록을 두고 같은 키로 늘어나는 간격을 두어 다시 묻는다. `delivered`(C 가 recorded·duplicate)나 `rejected` 를 받은 뒤에만 지운다. S host 가 다시 시작하면 같은 키로 다시 쌓으므로 C 에 같은 결과가 다시 갈 수 있고, C 원장이 같은 키로 거른다(장애 재전달 허용).

## 한계

- `--cwd`·`--profile-file` 은 CLI 가 C 의 파일시스템에서 먼저 확인하고 절대 경로로 바꾼다. 그래서 같은 경로가 C 에도 있어야 하며, 서버에서는 그 경로를 다시 확인한다. 서버에만 있는 경로는 아직 쓸 수 없다. `--cwd` 를 생략하면 서버의 기본 위치에서 시작한다.
- respawn·broadcast·launch 는 원격 자식에 대해 지원하지 않는다.
- 결과불명 call 의 부모 기록은 plugin 메모리에만 있다. plugin 이 다시 시작한 뒤 온 늦은 자식은 닫는다.
- Codex 결과 수신처는 결과 도착 시점의 L 경로를 따른다. 확정을 기다리는 동안 다른 Codex 대화가 tell 하면 앞 턴의 결과도 새 대화로 간다.
- `remote_receive` 는 `codex.deliver` 와 같이 공개 IPC 로도 부를 수 있다(별도 권한 경계가 아니다).
