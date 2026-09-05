# CLI ↔ IPC 표면 — 무엇이 CLI 로 닿고, 무엇이 왜 안 닿는가

[`docs/identity.md`](../identity.md) 원칙 2 는 에이전트 기능이 **IPC 와 CLI 양면**으로
동작해야 한다고 못 박는다. 이 문서는 그 대조의 **현재 상태**와, CLI 로 못 부르는 것들의
**사유**를 적는다. 결정의 근거·대안·재검토 조건은
[ADR-0160](../adr/0160-every-ipc-method-is-cli-reachable-or-carries-a-reason.md).

정합은 `src/source_guards/cli_entry_census.rs` 가 강제한다 — IPC 표의 모든 메서드는 CLI
로 닿거나 아래 표에 근거와 함께 한 줄을 가져야 하고, **그 근거는 매번 다시 확인된다.**

## 판별식

> **호출자가 누구인지가 응답의 일부인가.**

응답이 호출자의 신원(자기 배너·자기 팝업·자기 plugin 설정)이나 호출자에게 push 되는
이벤트 수신처에 매여 있으면 셸은 호출자가 될 수 없다 — 셸에는 plugin 신원도 이벤트
수신처도 없다. 매여 있지 않으면(전역 스냅샷 조회든 id 로 대상을 지정하는 쓰기든) 진입점이
있어야 한다.

## 어떻게 세는가

**실행으로 센다.** 소스에서 이름이 비슷한 잎을 찾는 방식은 두 방향으로 틀린다.

- 플래그 뒤에 숨은 진입점을 못 본다. `message.clear` 는 `tasty read queue --clear` 가,
  `surface.send_wait_idle` 은 `tasty send text --wait-idle` 이 보낸다 — 서브커맨드
  이름에는 그 메서드가 없다.
- **와이어 침묵을 진입점 부재로 오해한다.** `tasty tool remote-profile add-ssh` 는 rc=0
  인데 IPC 를 한 번도 안 탄다 — `crates/tasty-cli/src/local/` 이 그 자리에서 실행한다.
  그런 명령은 진입점이 **있는** 것이다.

세는 절차는 살아 있는 인스턴스 앞에 프록시를 세워 각 CLI 잎이 실제로 실은 메서드를
관측하는 것이다. 인자를 못 맞춰 실행이 안 된 잎은 **미측정**이지 부재가 아니다 — 그
방향의 편향은 한쪽으로만 작용하므로(부재 집합은 줄어들 수만 있다) 상한으로 쓴다.

## CLI 로 못 부르는 것과 그 사유

2026-09-05 실측: IPC 표 326 중 CLI 로 못 닿는 것 **40**.

### 로컬 실행 — CLI 잎이 IPC 없이 그 자리에서 처리한다 (16)

`attach.acquire` · `attach.list` · `attach.release` · `remote.attach` ·
`remote.workspaces` · `remote.passkey.{add,get,list,remove}` ·
`remote.profile.{add,detect,get,import,list,list_local,remove}`

같은 이름의 IPC 메서드는 **원격 쪽 또는 plugin 호출자**를 위한 것이다. 로컬 CLI 는
`crates/tasty-cli/src/local/` 에서 SSH·파일 I/O 를 직접 하므로 IPC 왕복이 없다.
근거 데이터: 그 `local/` 파일의 실재.

### plugin 이 기여하는 동적 CLI (8)

`image.{export_png,list,next,open,paste,prev,save}` · `markdown.navigate`

진입점이 host CLI 가 아니라 plugin 에 있다(`tasty image …`). host 쪽에 잎을 만들면
plugin 설치 여부에 따라 흔들린다 — 번들 plugin 이 namespace 를 점유하면 외부 호출이
plugin 으로 forward 되기 때문이다([ADR-0153](../adr/0153-a-bundled-namespace-hands-host-methods-back.md)).
근거 데이터: 그 plugin 크레이트 소스에 그 이름이 있다.

### 호출자가 plugin 이어야 성립하는 것 (8)

| 메서드 | 왜 셸이 호출자가 될 수 없나 |
|--------|------------------------------|
| `banner.open` · `banner.close` | 자기 contribute banner 를 자기 surface 에 띄우고 닫는다. 소유권 검증이 caller 를 본다 |
| `popup.close` | 자기 `instance_id` 만 닫을 수 있고 다른 plugin 의 인스턴스는 응답에서 거부된다 |
| `settings.get_plugin_setting` | `caller_plugin_id` 를 요청 파라미터가 아니라 `CallerContext` 에서 강제 도출한다 |
| `file_picker.trigger` | `request_id` 만 회신하고 결과는 `event.dispatch` unicast 로 **호출자에게** push 된다 |
| `git_viewer.query` | 같은 형태 — 비동기 accept 후 unicast push |
| `host.shared_buffer.create` | 응답이 공유 버퍼 핸들이다. 프로세스를 넘지 못한다 |
| `fs.pick_file` | native 다이얼로그를 host UI 스레드에서 연다. 셸이 부르면 사용자가 고를 때까지 이벤트 루프가 멎는다 |

근거 데이터: IPC 표의 `plugin_callable`.

### 같은 일을 하는 진입점이 이미 다른 이름으로 있다 (5)

| 메서드 | 이미 있는 진입점 |
|--------|------------------|
| `view.close` · `view.create` · `view.list` | `window.*` 의 별칭 |
| `surface.send_to` | `surface.send` — 둘 다 `surface_id` + `text` 로 같은 `dispatch_send` 를 탄다 (`tasty send text --surface`) |
| `surface.send_combo` | `surface.send_key` — `"ctrl+c"` 형태를 이미 파싱해 같은 바이트를 만든다 (`tasty send key ctrl+c`) |

근거 데이터: 별칭 대상이 표에 있고 **CLI 로 닿는다**.

### 사용자 행동이라 에이전트 표면에 두지 않는 것 (3)

`system.shutdown` · `window.focus` · `view.focus`

종료와 포커스 전환은 사용자 영역이다(원칙 1·3). 셋 다 release 표에 없다 — release IPC
로도 안 열려 있으므로 "IPC 는 되는데 CLI 만 막혔다" 가 아니다. 근거 데이터: release
표(`METHOD_TABLE`)에 없다.

## 선행 작업이 필요해 미룬 것

- **`markdown.navigate` 를 host CLI 로 노출** — 위 "plugin 이 기여하는 동적 CLI" 로
  분류해 두었으나, `tasty markdown navigate` 자체는 아직 없다. plugin 의 CLI 기여를
  추가하고 매니페스트 버전을 올리는 작업이라 plugin 크레이트를 건드린다.
- **`fs.pick_file` 의 비동기화** — 지금은 사용자 선택까지 gui 이벤트 루프를 막는다.
  `file_picker.trigger` 처럼 `request_id` 만 즉시 회신하고 결과를 이벤트로 push 하는
  형태로 바꾸면 셸에서도 부를 수 있다. 그 전에는 진입점을 만들면 셸이 무한 대기한다.

## 관련 문서

- [ADR-0160](../adr/0160-every-ipc-method-is-cli-reachable-or-carries-a-reason.md) — 이 규칙의 결정
- [headless-ipc-surface](headless-ipc-surface.md) — 같은 표를 조합(gui/headless) 축으로 가른 대조
- [debug-ipc](debug-ipc.md) — debug 격리 정책과 CLI 의 debug 트리
