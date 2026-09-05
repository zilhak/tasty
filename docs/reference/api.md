# IPC / CLI API 레퍼런스

tasty 를 조작하는 전체 IPC/CLI 표면의 네임스페이스별 목록. **메서드·권한의 정답은 코드** — `crates/tasty-ipc/src/method_meta.rs`(`METHOD_TABLE`/`DEBUG_METHODS`)와 `src/adapters/ipc/handler.rs` 라우터다. 이 문서는 사람이 읽는 지도이며, 각 메서드의 *동작* 은 해당 feature 문서로 위임한다.

## 접속

```python
import socket, json, os
port = int(open(os.path.expanduser("~/.tasty/tasty.port")).read().strip())  # 동적 포트
s = socket.socket(); s.connect(("127.0.0.1", port))
def call(method, params=None):
    s.sendall((json.dumps({"jsonrpc":"2.0","id":1,"method":method,"params":params or {}}) + "\n").encode())
    return json.loads(s.recv(1<<16).decode())
```

- 전송: loopback TCP, 동적 포트(`~/.tasty/tasty.port`), 줄단위 JSON-RPC 2.0. 근거 [ADR-0004](../adr/0004-ipc-transport-tcp.md).
- 대부분의 CLI 서브커맨드가 동명 IPC 를 감싼다. **모든 명령은 포커스 비의존** — 대상을 ID 로 직접 지정([focus 정책](../design/policies/focus.md)).
- **포커스 독립**: release 표면엔 포커스 변경 API 가 없다. 사용자 입력 재현(키/마우스 주입, popup 강제 open, `window.focus`)은 debug 전용 → [debug-ipc](../dev-guide/debug-ipc.md).

## 권한

plugin caller 는 메서드별 권한 토큰이 필요하다(`method_meta`). Local(CLI/사용자)은 무제한. 토큰 목록·게이트는 [concepts/plugins](../concepts/plugins.md) · [dev-guide/plugin-permissions](../dev-guide/plugin-permissions.md). 런타임 권한 상승(capability elevation)·audit 은 [features/capability-elevation](../features/capability-elevation/index.md).

## 네임스페이스

### 구조 — workspace / pane / tab / surface / split / tree
`workspace.{list,create,update,move}` · `pane.{list,close}` · `split` · `tab.{list,create,close,move}` · `tree`. 도메인은 [work-area](../features/work-area/index.md).

### Surface 상호작용
- 입력: `surface.{send,send_key,send_combo,send_to,send_wait_idle,wake,respawn_terminal}`
- 읽기/마크: `surface.{set_mark,read_since_mark,parse_since_mark,screen_text,cursor_position,foreground_process,is_typing,locate}`. `screen_text`(및 `pty.read`)는 dim(ghost-suggestion, 예: Claude Code 자동완성 제안) 셀을 기본 제외 — `show_dim:true`(CLI `--show-dim`)로 포함.
  - `lines:N`(CLI `--lines N`) 생략 시 보이는 화면 전체. 지정하면 **내용 기준 마지막 N 줄** — 내용 아래의 공백 행은 건너뛰고, 화면 내용이 N 에 모자라면 스크롤백에서 채운다(합쳐도 모자라면 있는 만큼). 내용 *중간* 의 빈 줄은 출력의 일부이므로 보존하고 줄 수에도 포함한다. 빈 행 판정은 `show_dim` 과 같은 값으로 하므로 `--show-dim` 유무가 반환 줄 수를 바꾸지 않는다. alternate screen(TUI)이 떠 있을 때도 부족분은 primary 스크롤백으로 채운다 — alt screen 은 자체 스크롤백이 없기 때문이다.
  - **N 보다 적게 왔을 때 왜인지 물을 수 있다.** 응답에 `is_terminal`(그 surface 뒤에 터미널이 있는가) · `scrollback_len`(현재 스크롤백 줄 수) · `alt_screen`(대체 화면인가)이 함께 실린다. `scrollback_len: 0` 이면 **받은 것이 가진 전부**이고(정상 포화), 0 이 아닌데 N 보다 적게 왔다면 그건 결함이다. 터미널이 아닌 surface(markdown/html/explorer/image)나 없는 surface 는 `is_terminal: false` 와 두 필드 `null` 로 나온다 — **`0` 이 아니다**(0 은 "스크롤백이 비었다" 라는 다른 사실이다). 같은 필드가 `pty.read` 응답에도 실린다.
- 명령(OSC 133): `surface.{commands,last_command,command_at}`
- 메타: `surface.meta.{set,get,unset,list}` · `surface.set_cwd`
- 주의 환기(attention): `surface.completion`(발동) · `surface.attention.{get,clear}`(조회·해제). 해제는 `kind` 선택 필터를 받고, 하드 점유 중인 surface 와 mirror surface 는 거절한다(그 상태의 소유자가 다른 인스턴스다) — [surface-highlight](../features/surface-highlight/index.md)
- 출력 옵저버: `output.observe_{start,stop,list,info}`
- 동작·파서는 [terminal-output](../features/terminal-output/index.md), 파서 카탈로그 [output-parsers](output-parsers.md). (IME `surface.ime_*` 는 debug 빌드 전용 local-only — release 표면에 없다.)

### 메모리 (`memory.*` / `memory.secret.*`)
regular(`put/get/delete/list/exists/count/scopes/stats/query/export/import`) · secret(동일 verb) · `gc` · blackboard(`bb_*`) · plan(`plan_*`) · cache(`cache_*`) · goal(`goal_*` — surface 스코프 단일 목표 문장, `surface_id` 명시 필수). 모델·권한은 [design/systems/memory](../design/systems/memory.md).

### 에이전트 협업 (`agent.*`)
`task_{create,list,get,cancel,retry,graph,reduce,run,delete,purge}` · `dag_{list,get}` · `barrier_*` · `semaphore_*` · `lease_*` · `rate_limit_*`. 전부 `agent`(AgentManage) 권한 — `task_run`(workspace runner thread start/stop/status)도 포함(호스트가 재시작 시 runner 를 자동으로 다시 켜지 않으므로, plugin 이 자기 workspace 의 runner 를 스스로 되살릴 수 있어야 한다). **local caller 전용**(plugin 호출 거부)은 `task_await`(진짜 blocking — `approval.await` 와 대칭, plugin SDK 단일 워커 스레드가 막히는 걸 막기 위함. 기본 timeout 10분, `timeout_ms:0` 은 무한 대기)와 `task_set_result`(외부 task 완료 신호 — 러너가 Custom task 생명주기를 단독 소유하므로 plugin 이 별도로 전이시키면 쓰기 주체가 이중화된다. plugin 은 완료 판정 전략 선언으로 우회). 둘 다 [method_meta.rs](../../crates/tasty-ipc/src/method_meta.rs)의 `METHOD_TABLE`에 `local_only()` 로 **명시 등재**돼 있다 — 미등재(`UnknownMethod` 거부)는 정책과 누락이 구분되지 않으므로, 라우터 분기가 있는 모든 메서드는 표에 등재한다(`tests/ipc_router_table_parity.rs` 가 강제). `task_delete`/`task_purge` 는 참조(`depends_on`/`Fallback.task`/`Reduce.inputs`) 안전 검사를 거친다 — 기본 거부+참조자 목록, `--cascade`(연쇄 삭제)/`--force`(참조 검사만 우회, `running` 상태 제약은 못 뚫음). `task_command.kind = "run"`(Surface 없는 bare subprocess)의 결과는 `task_get`/`task_await` 의 `result.output` 에 stdout/stderr 캡처(각 마지막 64KiB tail + `truncated`/`dropped_bytes`)를 싣는다 — 실패(비0 exit)는 `result.error` 문자열에 같은 내용이 포함된다. `semaphore_set_permits` 는 세마포어 한도를 제자리에서 바꾼다(delete→create 우회의 "세마포어가 없는 순간" 을 없앤다) — 축소는 drain 이라 기존 홀더를 강제 회수하지 않고 새 acquire 만 거절한다. `semaphore_acquire` 의 `ttl_ms` 는 선택이며, 준 경우에만 그 홀더가 만료돼 회수된다(기본은 만료 없음, lease 와 같은 메커니즘 — [ADR-0119](../adr/0119-agent-semaphore-resize-and-holder-expiry.md)). `dag_{list,get}` 은 workspace 안의 flat 한 task 를 **DAG 단위로 쪼갠 조회 표면**이다 — DAG 는 영속 레코드가 아니라 `metadata.dag`(explicit) 또는 그래프 연결성(derived)에서 도출된다. `dag_list` 는 `workspace_id` 를 생략하면 살아있는 전 workspace 를 순회하고(응답 `scope: "live_workspaces"`), `dag_get` 은 그 DAG 부분집합만으로 `task_graph` 와 동일한 `nodes`/`edges`(또는 dot)를 낸다. [agent-collaboration](../features/agent-collaboration/index.md).

### 휴먼 핸드오프 (`approval.*`)
`request,respond,await,cancel,get,list,history,summary.{set,get}`. [human-handoff](../features/human-handoff/index.md).

### 텔레메트리 (`telemetry.*`)
`record,record_batch,summary,timeseries,top` · `cap.{set,list,remove,status,reset}` · `anomaly.list` · `session_summary`. [telemetry](../features/telemetry/index.md).

### 세션 / attach
`session.{issue,revoke,list}` · `attach.{acquire,release,force_detach,force_detach_workspace,into_gui,list}`. attach 는 [remote-attach](../features/remote-attach/index.md), 신원 토큰은 [capability-elevation](../features/capability-elevation/index.md).

### 기타 호스트
- 알림: `notification.{list,create}` — [notifications](../features/notifications/index.md)
- 훅: `hook.{set,list,unset}` · `global_hook.{set,list,unset}` · `surface.fire_hook`
- 웹훅(인바운드 HTTP): `webhook.{register,list,info,unregister,sweep,config}` (local-only) — [webhook](../features/webhook/index.md)
- 메시지 패싱: `message.{send,read,count,clear}`
- 파일 핸들러: `file_handler.{reload,dispatch}` — [file-handler](../features/file-handler/index.md)
- 훅 핸들러: `hook_handler.{list,reload,dispatch}` (local-only) — 훅/웹훅 공유 핸들러 레지스트리 조회(비활성 포함)·user config(`~/.tasty/hook-handlers.toml`) 재로드·id 로 수동 발화(IpcSequence/ShellCommand). dispatch 는 fire-and-forget(응답은 accepted ACK 만)
- 완료 판정 전략: `completion_strategy.list` (local-only) — `agent.task_create` 의 `Custom.poll` 이름 참조가 가리키는 완료 판정 전략 레지스트리 조회(비활성 포함). reload/dispatch 대응물 없음(판정 함수이지 발화 대상 아님) — [agent-runner](../dev-guide/agent-runner.md)
- 이미지: `image.{open,save,export_png,next,prev,paste,list}` — [image plugin](../plugins/image/index.md)
- 원격 연결 프로필: `remote.profile.{list,get,add,detect,remove,list_local,import}`(`list_local`=로컬 `~/.ssh/config` alias 열거·읽기 전용, `import`=그 alias 를 ssh 프로필로 등록·셸 감지 없음)(구 `tool.ssh.*`/`ssh.profile.*`는 alias로 한시 호환) — [remote-profiles](../features/remote-profiles/index.md)
- webview: `webview.set_url`
- 스크린샷: `ui.screenshot {path, surface_id?, window_id?}` (local-only, focus 독립 — 대상을 ID 로 지정) — [screenshot-methods](../ai-verification/screenshot-methods.md)
- 시스템: `system.info` · `system.gpu_stats` (local-only, GPU 리소스 카운트 스냅샷 — wgpu 전역 리포트 + 창별 렌더러 카운트, 메모리 누수 soak 검증용. CLI `tasty list gpu-stats`) — [memory-leak-soak](../dev-guide/memory-leak-soak.md)
- 타이머 관측: `timer.list` (local-only, 조회 전용 — 등록된 주기 작업의 키/주기/다음 데드라인/precision 스냅샷 + 지금 인스턴스를 깨우고 있는 hard deadline 요약. 본체 허브 + plugin manager 허브 합산. CLI `tasty list timers`) — [timer-hub](../dev-guide/timer-hub.md)
- Plugin 설정 read-back: `settings.get_plugin_setting {storage_key}` — 자기 자신의 `plugin_settings` 값만 조회(caller 로 스코프 강제)
- 원격 전송 저장 정책: `settings.get_remote_transfer` / `settings.set_remote_transfer {dir?, max_mb?}` (local-only, focus 독립 — 전역 설정) — 원격 bulk 파일 전송 수신측 저장 폴더(`dir`, 빈 값=기본 `~/.tasty/transfers/`)와 폴더 최대 용량(`max_mb`, MiB). set 은 지정 필드만 현재 설정 위에 덮어써 저장한다. CLI `tasty settings {get-remote-transfer,set-remote-transfer}`. — [remote-attach](../features/remote-attach/index.md)

### Plugin 관리 (`plugin.*`, local-only)
`list,show,install,remove,enable,disable,upgrade_builtins,permissions,grant,revoke` · `grant_agent_permission`/`revoke_agent_permission`/`list_agent_permissions` · `request_permission` · `audit_{query,summary,follow,clear}`(**deny 만 기록된다** — allow 는 저장하지 않으므로 평시 조회 결과는 비어 있는 것이 정상이다, [ADR-0085](../adr/0085-ipc-log-retention-bounded.md)) · `extension.list`. [plugin-system](../features/plugin-system/index.md) · [capability-elevation](../features/capability-elevation/index.md).

### Lua 스크립트
release IPC 없음 — 스크립트는 등록 목록 + 단축키 트리거로만 실행된다(ADR-0031). 임의 Lua 주입은 debug 빌드 전용 `debug.lua.eval`. [lua-hooks](../features/lua-hooks/index.md).

### Plugin 확장 네임스페이스
plugin 이 `[[contributes.ipc_namespace]]` 로 prefix 를 선언하면 `<prefix>.<method>` 가 그 plugin 으로 forward 된다(예: `claude.*`, `codex.*`). [plugins/](../plugins/index.md).

### Debug 전용 (debug 빌드만, `DEBUG_METHODS`)
`ui.state` · `debug.{info,cell_info,screen_attrs,glyph_color,feed_bytes,inject_mouse,inject_key,tool.*,popup.*,event_bus.*,extension.invoke_hook}` · `window.focus`/`view.focus` · `system.shutdown`. release 미노출 → [debug-ipc](../dev-guide/debug-ipc.md).

(`ui.screenshot` 은 focus-독립 정식 기능으로 승격 — 위 "기타 호스트" 참조. [screenshot-methods](../ai-verification/screenshot-methods.md).)

## CLI 매핑

CLI 는 위 IPC 를 감싼다(`tasty list workspaces`, `tasty send text`, `tasty memory put`, `tasty agent task-create`, `tasty approval request`, `tasty plugin list`, `tasty screenshot --path …` …). debug 서브커맨드는 debug 빌드만. 환경별 접속/실행/종료 패턴은 [environments](environments.md).

## 관련

- [event-catalog](event-catalog.md) — IPC 와 별개 채널인 Event Bus
- [identity](../identity.md) — 사용자/에이전트 행동 분리(이 표면의 설계 축)
