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
- 읽기/마크: `surface.{set_mark,read_since_mark,parse_since_mark,screen_text,cursor_position,foreground_process,is_typing,locate}`
- 명령(OSC 133): `surface.{commands,last_command,command_at}`
- 메타: `surface.meta.{set,get,unset,list}` · `surface.set_cwd`
- 출력 옵저버: `output.observe_{start,stop,list,info}`
- 동작·파서는 [terminal-output](../features/terminal-output/index.md), 파서 카탈로그 [output-parsers](output-parsers.md). (IME `surface.ime_*` 는 local-only.)

### 메모리 (`memory.*` / `memory.secret.*`)
regular(`put/get/delete/list/exists/count/scopes/stats/query/export/import`) · secret(동일 verb) · `gc` · blackboard(`bb_*`) · plan(`plan_*`) · cache(`cache_*`). 모델·권한은 [design/systems/memory](../design/systems/memory.md).

### 에이전트 협업 (`agent.*`)
`task_{create,list,get,await,cancel,retry,graph,set_result,run,reduce}` · `barrier_*` · `semaphore_*` · `lease_*` · `rate_limit_*`. 전부 `agent`(AgentManage) 권한. [agent-collaboration](../features/agent-collaboration/index.md).

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
- 이미지: `image.{open,save,export_png,next,prev,paste,list}` — [image plugin](../plugins/image/index.md)
- SSH 프로필: `tool.ssh.{list,get,add,detect,remove}` — [ssh-tool](../features/remote-profiles/index.md)
- webview: `webview.set_url`
- 스크린샷: `ui.screenshot {path, surface_id?, window_id?}` (local-only, focus 독립 — 대상을 ID 로 지정) — [screenshot-methods](../ai-verification/screenshot-methods.md)
- 시스템: `system.info`

### Plugin 관리 (`plugin.*`, local-only)
`list,show,install,remove,enable,disable,upgrade_builtins,permissions,grant,revoke` · `grant_agent_permission`/`revoke_agent_permission`/`list_agent_permissions` · `request_permission` · `audit_{query,summary,follow,clear}` · `extension.list`. [plugin-system](../features/plugin-system/index.md) · [capability-elevation](../features/capability-elevation/index.md).

### Lua 스크립트
release IPC 없음 — 스크립트는 등록 목록 + 단축키 트리거로만 실행된다(ADR-0031). 임의 Lua 주입은 debug 빌드 전용 `debug.lua.eval`. [lua-hooks](../features/lua-hooks/index.md).

### Plugin 확장 네임스페이스
plugin 이 `[[contributes.ipc_namespace]]` 로 prefix 를 선언하면 `<prefix>.<method>` 가 그 plugin 으로 forward 된다(예: `claude.*`, `codex.*`). [plugins/](../plugins/index.md).

### Debug 전용 (debug 빌드만, `DEBUG_METHODS`)
`ui.state` · `debug.{info,cell_info,screen_attrs,glyph_color,feed_bytes,inject_mouse,inject_key,tool.*,popup.*,event_bus.*,extension.invoke_hook}` · `window.focus`/`view.focus` · `system.shutdown`. release 미노출 → [debug-ipc](../dev-guide/debug-ipc.md).

(`ui.screenshot` 은 focus-독립 정식 기능으로 승격 — 위 "기타 호스트" 참조. [screenshot-methods](../ai-verification/screenshot-methods.md).)

## CLI 매핑

CLI 는 위 IPC 를 감싼다(`tasty workspace list`, `tasty send text`, `tasty memory put`, `tasty agent task-create`, `tasty approval request`, `tasty plugin list`, `tasty screenshot --path …` …). debug 서브커맨드는 debug 빌드만. 환경별 접속/실행/종료 패턴은 [environments](environments.md).

## 관련

- [event-catalog](event-catalog.md) — IPC 와 별개 채널인 Event Bus
- [identity](../identity.md) — 사용자/에이전트 행동 분리(이 표면의 설계 축)
