# CLI / IPC API 규약 — 명명 + 안정성

IPC 메서드·CLI 명령의 **명명 규칙**과 **호환성/버전 정책**. 단일 진실 원천은 `crates/tasty-ipc/src/method_meta.rs::METHOD_TABLE`(release 표면) — 본 문서는 그 위의 규칙·예외·진화 절차다. 전체 메서드 카탈로그는 [reference/api](../reference/api.md).

## 형식

```
IPC 메서드: <namespace>.<verb>[_<modifier>]
CLI 명령:  tasty <namespace> <verb> [--<option>]
```

예: `surface.list` ↔ `tasty surface list`, `claude.spawn` ↔ `tasty claude spawn`.

- **namespace 단수형** (`surface`, NOT `surfaces`). list 반환 키는 복수 (`surfaces: [...]`).
- **root 예외**: `split`(pane 분할) · `tree`(surface tree)만 namespace 없이 root 에 등록(자주 쓰는 짧은 명령). 새 메서드는 이 예외에 동참 금지.
- **보조 도메인은 3단** `<namespace>.<sub>.<verb>` (예: `tool.ssh.*`, `surface.meta.*` 점 표기).

namespace 별 메서드 수는 `tests/cli_naming_count_drift.rs` 가 강제한다 — 추가는 같은 minor 내 OK(테이블 동기화 필요), **제거는 SemVer 위반**(major bump 필요). 카운트 snapshot 은 테스트가 SoT 라 본 문서에 박지 않는다.

## verb 화이트리스트

새 메서드는 적합한 카테고리의 verb 를 고르고, 밖이면 PR description 에서 정당화한다(가벼운 ADR — 별도 파일 불필요).

| 카테고리 | verb |
|----------|------|
| **Read**(부작용 없음) | `list`(컬렉션→array) · `info`(단일, id 필요) · `state`(스냅샷) · `get` · `count` · `read` |
| **Write** | `create` · `update` · `set`/`unset` · `move` · `close`(소프트) · `clear` · `remove`(closed 안 남김) · `destroy`(영구, 예약) |
| **Send/외부** | `send` · `paste` · `wait` · `wake` |
| **프로세스/세션** | `spawn` · `launch` · `kill` · `respawn` · `shutdown` |
| **권한/관리**(local-only) | `install`/`enable`/`disable`/`grant`/`revoke`/`permissions` |

**modifier 패턴** `<verb>_<modifier>` 로 변종 표현(`send_key`/`send_combo`/`read_since_mark`). 한 verb 에 modifier 5개 이상 누적되면 namespace 한 단계 분리 검토.

도메인 특수 verb(예: `claude.tell`/`broadcast`, telemetry `record`/`summary`, agent `task_*`/`barrier_*`, memory `bb_*`/`plan_*`/`cache_*`/`goal_*`)는 표준 밖이지만 도메인 의미가 명확해 채택된 것들이다. 새 영역은 표준 verb 를 우선 검토하고, 채택 시 PR 에서 사유를 남긴다.

## 인자 규칙

- 대상 식별은 항상 `--<namespace> <id>` (`--surface 42`, `--tab 7`). **활성 객체 의존 금지**(포커스 독립성 — [focus 정책](../design/policies/focus.md)).
- 옵션은 kebab-case (`--strip-ansi`, `--since-mark`).

### 잘못된 인자는 거절한다 — 자르지도, 버리지도 않는다

대상 식별자를 읽을 때 **값이 안 왔다 / 왔는데 못 읽는다** 를 가른다. 둘을 합치면 잘못된
값이 조용히 폴백으로 넘어가고, 그 폴백은 대개 **호출자 자신**이거나 **유일한 후보**다.

- `as u32` 로 **자르지 않는다.** 자르기는 값을 거절하는 게 아니라 **다른 값으로 바꾼다**
  — `4_294_967_297` 은 `1` 이 되고 `5_000_000_000` 은 `705_032_704` 가 된다. 그 결과가
  실재하는 다른 surface 를 가리키면 명령이 남의 터미널로 간다. `u32::try_from` 을 쓴다.
- **선택 인자도 버리지 않는다.** 잘못 온 값을 `None` 으로 만들면 "안 줬다" 와
  구별되지 않아, 호출자가 지정한 대상 대신 기본값이 쓰인다.
- **`null` 은 안 왔다로 읽는다.** 직렬화가 빈 슬롯을 `null` 로 채우는 경우가 있어, 이것을
  오타로 취급하면 정상 경로가 막힌다.
- **문구를 가른다.** 값이 왔는데 "missing" 이라고 답하면 호출자가 자기가 준 값을 안
  의심한다. 잘못된 값은 그 값을 되비추며 거절한다.

호스트 쪽 공용 판정은 `src/adapters/ipc/handler/params.rs` 에 있다. 새 핸들러는 인라인으로
다시 적지 말고 그것을 쓴다 — 같은 몸통이 세 벌로 흩어져 있던 동안 셋 다 같은 결함을 갖고
있었고, 하나를 고쳐도 나머지 둘은 안 고쳐졌다.

**이것은 전수 가드로 강제된다.** `src/source_guards/params_chokepoint.rs` 가
params 를 읽는 **두 계층**(`src/adapters/ipc/handler/` 과 짝인 `handler.rs`,
그리고 `src/app/ipc/` — 관문 자신은 제외)에서 `params` 파생 값을 숫자로 읽는 자리를
찾는다. 계층이 둘인 것이 요지다: 대부분의 메서드는 앞쪽에서, 창을 소유해야 하는 것과
App 상태를 만지는 것은 뒤쪽에서 처리된다. **한쪽만 관문에 걸면 다른 쪽이 조용히 자르고
버린다** — 실제로 뒤쪽에 16 곳이 남아 있었고 그중 `remote_workspace` 는 `as u32` 로
잘랐다.

가드를 **자르기**(`as u32`)에 걸지 않고 **읽는 자리**에 건 이유: 자르기 자체는 정당한
곳이 많아(`clippy::cast_possible_truncation` 은 plugin 크레이트 둘에서만 69 건이 뜬다)
값의 **출처**가 판별식인데, 출처는 `as` 캐스트의 성질이 아니다. 스칼라 읽기를 전부 위
한 자리로 통과시키고 나서야 명제가 문법적이 된다 — "핸들러는 관문 밖에서 params 를
숫자로 읽지 않는다" 는 소스 모양만으로 판정된다.

초록의 뜻은 좁다. 잡는 것은 두 모양 — `params` 로 시작하는 식 안의 숫자 읽기와,
`let` 으로 **한 홉** 갈라 둔 뒤의 읽기다(뒤쪽이 실제로 두 자리를 숨기고 있었다).
params 를 담는 **이름**이 규약(`params` / `_params`, 또는 살아 있는 요청의
`…request.params`)을 벗어나거나, 두 홉 이상을 거치거나, 두 계층 밖이면 술어 밖이다.
한 글자 이름은 일부러 안 받는다 — `p` 는 클로저 인자로도 흔해서 이름으로 받으면
관계없는 자리를 위반으로 센다. 대신 그 바인딩들을 `params` 로 통일했다. 그래서 0 은 "이 축이 지켜진다" 가 아니라 "이 모양으로는
안 새고 있다" 로 읽는다. 자세한 범위 정의는 그 파일의 모듈 주석에 있다.

## CLI vs IPC

`crates/tasty-cli` 의 plugin CLI 빌더는 **top/sub 2단만** 지원 — plugin 이 `x.meta.set` 을 노출하려면 `tasty <plugin> meta-set` 같은 2단으로 매핑. 호스트 본체 CLI 는 3단 직접 빌드 가능.

`attach.*` IPC namespace 는 `tasty attach` 로 노출되지 않고 용도별 CLI 로 갈린다: `tasty remote attach`/`remote check`(release, 원격 SSH), `tasty debug attach`(debug 전용, 로컬 loopback). 근거·동작은 [attach-behavior](attach-behavior.md), 격리는 [debug-ipc](debug-ipc.md).

### release IPC 에 있는데 CLI 가 없는 메서드

[identity §2.2](../identity.md) 원칙 2 는 "**에이전트가 자기 작업에 필요한 기능**은 IPC + CLI 양면으로 동작해야 한다" 이다. 걸리는 대상은 **에이전트 기능**이지 release IPC 표면 전체가 아니다 — plugin 이 host 에게 자기 자원을 요청하는 서비스 메서드는 애초에 CLI 호출자가 존재하지 않는다.

그래서 "release 표에 있는데 CLI 가 없다" 는 그 자체로 결함이 아니다. 아래가 현재 그런 메서드 전부이고, 각 행이 왜 원칙 2 밖인지 또는 어떻게 이미 충족되는지를 적는다. **새로 그런 메서드를 만들면 여기에 행을 추가한다** — `tests/cli_method_table_parity.rs` 가 이 표와 실제 집합을 양방향으로 대조하므로, 빠뜨리면 테스트가 떨어진다. 아래 개수와, 사유 열이 "대신 이걸 쓰라" 고 든 명령이 실재하는지도 같은 가드가 본다. 개수는 표에서 파생되지 않는 값이라(마크다운 표는 스스로 세지 않는다) 행을 고칠 때 함께 고쳐야 하고, 안 고치면 그 가드가 실제 값을 알려준다.

총 29개.

| 이유 | 메서드 | 왜 CLI 가 없나 |
|---|---|---|
| plugin → host 서비스 †plugin-only | `banner.open` · `banner.close` · `popup.close` | plugin 이 **자기** contribute UI 인스턴스를 여닫는다. 대상 식별이 caller plugin 자신이라 CLI 호출자가 존재하지 않는다 |
| plugin → host 서비스 | `file_picker.trigger` | plugin 프로세스가 못 여는 host 소유 popup 을 대신 연다. 결과는 응답이 아니라 `event.dispatch` 로 그 plugin 에 push 된다 |
| plugin → host 서비스 | `git_viewer.query` · `markdown.navigate` | 특정 plugin(git-viewer · markdown 주소창)이 자기 surface 를 위해 부른다. `git_viewer.query` 는 `request_id` 만 회신하고 결과를 그 plugin 에 unicast push 하므로 셸이 결과를 받을 수 없고, `markdown.navigate` 는 그 namespace 를 번들 plugin 이 점유해 외부 호출이 plugin 으로 forward 된다([ADR-0153](../adr/0153-a-bundled-namespace-hands-host-methods-back.md)) |
| plugin → host 서비스 | `settings.get_plugin_setting` | `caller_plugin_id` 를 요청 파라미터가 아니라 `CallerContext` 에서 강제 도출한다 — CLI 호출자는 plugin 신원이 없어 **원리적으로** 부를 수 없다 |
| plugin → host 서비스 †plugin-only | `host.shared_buffer.create` | 응답이 main 채널 하나로 끝나지 않는다 — 공유 메모리 핸들(Unix fd / Windows HANDLE)이 그 plugin 프로세스의 **보조 채널**로 함께 전달되고, 받는 쪽은 그것을 자기 주소공간에 매핑한다. CLI 프로세스에는 그 채널도 매핑 대상도 없어 결과를 받을 수 없다 |
| CLI 는 있고 IPC 를 안 탄다 | `remote.attach` · `remote.workspaces` | `tasty remote attach` / `tasty remote workspaces` 가 SSH 터널을 직접 열고 클라이언트 주도로 실행한다. 이 IPC 는 같은 일을 **원격/에이전트가 시킬 때**의 판이다 |
| CLI 는 있고 IPC 를 안 탄다 | `remote.profile.add` · `remote.profile.get` · `remote.profile.list` · `remote.profile.list_local` · `remote.profile.detect` · `remote.profile.import` · `remote.profile.remove` | `tasty tool remote-profile …` 이 로컬 프로필 파일을 직접 다룬다(IPC 없음). 인스턴스가 떠 있지 않아도 되어야 하는 명령이라 그쪽이 옳다 |
| CLI 는 있고 IPC 를 안 탄다 | `remote.passkey.add` · `remote.passkey.get` · `remote.passkey.list` · `remote.passkey.remove` | `tasty tool passkey …` 가 같은 이유로 로컬 처리한다 |
| 다른 이름으로 이미 있다 | `view.create` · `view.close` · `view.list` | `window.*` 의 어휘 통일 alias 로 동작이 동등하다. CLI 는 `tasty new window` · `tasty close window` · `tasty list windows` 쪽 한 벌만 노출한다 |
| 같은 능력을 다른 명령이 준다 | `surface.send_combo` | `surface.send_key` 가 `"ctrl+c"` 형태를 파싱하므로 `tasty send key ctrl+c` 로 덮인다. 이쪽은 modifier 를 배열로 받는 JSON 친화 변종이다 |
| 같은 능력을 다른 명령이 준다 | `surface.send_to` | `surface.send` 와 동형이라 `tasty send text --surface <id>` 로 덮인다 |
| 연결 경계가 대신한다 | `attach.acquire` · `attach.release` · `attach.list` | 위 "CLI vs IPC" 의 `attach.*` 항목 참조. `client_id` 가 `stream.open` 핸드셰이크 발급물이라 one-shot CLI 가 들 수 없고, 사람이 쓰는 표면은 `tasty remote attach` / `tasty tool attach` 가 세션 전체를 안에서 처리한다 |

#### † plugin-only — 외부 호출자는 무엇을 받는가

위 표에서 †plugin-only 로 표시한 넷(`banner.open` · `banner.close` · `popup.close` ·
`host.shared_buffer.create`)은 **CLI 잎이 없는 것에 그치지 않고 외부 dispatch arm 자체가
없다.** plugin host-call 진입부가 직접 인터셉트하기 때문이다. 나머지 행들은 사정이 다르다 —
`git_viewer.query` · `markdown.navigate` · `settings.get_plugin_setting` 같은 것은 외부에서
쏘면 실제로 라우팅되어 인자 오류나 plugin 의 답이 돌아온다. 두 부류가 같은 표에 있는 것은 이
표의 축이 **CLI 진입점**이지 라우팅이 아니기 때문이다.

이 넷은 `METHOD_TABLE` 에 `plugin_only(&[…])` 로 등재되고, 외부 호출자는 `-32601`("그런
메서드 없다")이 아니라 다음을 받는다:

    -32016  method '<name>' is plugin-only: only the plugin host-call path dispatches it,
            so CLI and network IPC callers have no entry point

`-32601` 이면 호출자는 **이름을 의심한다** — 오타를 고치거나 표를 다시 읽는다. 사실은 이름이
맞고 표에도 있으며 부를 수 있는 주체가 다를 뿐이라, 같은 코드로 답하면 호출자를 틀린 방향으로
보낸다. 플랫폼 축에서 같은 거짓을 고친 [ADR-0154](../adr/0154-a-platform-gated-dispatch-arm-answers-why-not-what.md)
와 같은 형태이고, 이쪽은 caller 축이다([ADR-0163](../adr/0163-a-registered-name-answers-who-not-whether.md)). 표식과 인터셉트가 갈라지지 않는지는
`src/source_guards/plugin_only_dispatch_parity.rs` 가 양방향으로 본다.

실측(2026-09-05, gui debug 인스턴스 · plugin 설치된 세계 · 외부 프로브): `plugin_callable` 인
**231** 개 중 외부 호출이 `-32601` 로 끝난 것이 이 **4** 개였다(나머지는 `-32602` 188 ·
실행 성공 37 · `-32000` 2). 같은 집합을 "외부 라우터 소스에 이름이 안 보이는 것" 으로 세면
**14** 개가 나온다 — `window.*` · `view.*` 처럼 match 팔이 아닌 명부로 라우팅되는 것이 섞여
들어오기 때문이다. 이 부류는 소스 텍스트가 아니라 **실행**으로만 정해진다.

### 등재된 이름인데 이 바이너리에 arm 이 없을 때

같은 거짓의 세 번째 얼굴이다. 이름이 표에 있고 구현도 있는데 **이 빌드 조합에서 그 `match`
팔이 통째로 사라진** 경우 — `#[cfg(feature = "gui")]` 뒤에 있는 메서드를 헤드리스
(`--no-default-features`) 데몬에서 부르는 것이 그것이다. 팔이 없으면 호출은 `_` 로 떨어져
종단에 오고, 종단은 예전에 `-32601` 로 답했다.

그 답은 오타와 **바이트 단위로 같았다.** 실측(2026-09-05, 헤드리스 데몬):

    window.creat    -32601 Method not found: window.creat      ← 오타
    window.create   -32601 Method not found: window.create     ← 표에 있고 이 빌드엔 없다

지금은 갈린다:

    -32017  method '<name>' is registered but this binary has no dispatch arm for it:
            it is gated out of this build combination (headless / release)

호출자가 다음에 할 일이 다르기 때문이다 — `-32601` 은 이름을 고치게 하고, `-32017` 은
**조합을 보게** 한다(gui 빌드로 부르거나, 그 표면이 헤드리스에 열려야 하는지를 묻는다).
근거·대안·재검토 조건은 [ADR-0167](../adr/0167-a-registered-name-answers-whether-it-is-in-this-binary.md).

**이 갈래의 술어는 표를 그 이름 그대로 조회하는 것**(`method_meta::is_registered_name`)이지
`method_meta()` 가 아니다. 저 함수는 마지막 단계에서 **런타임 등록 plugin prefix** 까지
해소하므로, 그것으로 갈래를 타면 설치된 plugin 의 이름과 그 아래 오타까지 host 가 삼킨다 —
실측으로 `claude.children` · `agent_stream.list` · `markdown.no_such_thing` 이 전부 이 코드를
받았고, plugin 으로 갈 호출이 안 갔다.

세 코드의 관계:

| 사실 | 코드 | 호출자가 다음에 할 일 |
|------|------|----------------------|
| 부를 수 있는 주체가 다르다 | `-32016` | 호출 주체를 본다 |
| 이 플랫폼에서 안 된다 | `-32015` | 플랫폼을 본다 |
| 이 바이너리에 안 들어 있다 | `-32017` | 빌드 조합을 본다 |
| 소유 plugin 이 지금 안 떠 있다 | `-32002` | plugin 을 켠다 |
| 이름이 틀렸다 | `-32601` | 이름을 고친다 |

`-32002` 가 여기 있는 이유는 [ADR-0173](../adr/0173-namespace-resolution-reads-the-manifest-not-the-process-table.md)
이다. namespace 소유는 **설치된 매니페스트**가 정하고 생존은 따로 물으므로, disable 된
plugin 의 메서드는 "그런 메서드 없다"(거짓)가 아니라 "있는데 꺼져 있다"(참)로 답한다.

### plugin 을 거쳐 온 실패도 호스트가 준 코드를 그대로 낸다

plugin namespace 의 메서드는 owner plugin 으로 forward 되고, plugin 은 자기 일을 하려고
호스트 메서드를 되부른다(`claude.parent` → `terminal.parent`). 그 되부름이 거절되면 사유가
plugin 을 거쳐 원래 호출자에게 돌아오는데, **코드는 그 왕복을 넘어 살아남는다.**

    claude.parent {"surface_id": 999}
    → -32602  host call 'call#4' failed: no live surface 999 (named by 'terminal.parent'); …

문구는 한 겹 감싸진다(`host call '<call#N>' failed:` 접두). 그건 plugin 을 거쳤다는 사실
그대로이고 [ADR-0153](../adr/0153-a-bundled-namespace-hands-host-methods-back.md) 이 정한
바다. **코드는 안 감싼다** — `-32602`("인자를 고쳐라")가 `-32000`("서버 사정")이 되면
호출자가 재시도 정책을 반대로 고른다. 근거는
[ADR-0171](../adr/0171-a-host-error-code-survives-the-plugin-boundary.md).

호스트가 코드를 안 준 실패(plugin 내부 오류, SDK 의 연결·인코딩 오류)는 종전대로
`-32000` 이다.

### debug 표에 있는데 CLI 가 없는 메서드

원칙 2 는 debug 빌드의 에이전트 표면에도 걸린다 — `debug.*` 는 release 에 없을 뿐,
있는 빌드에서는 에이전트가 쓰는 기능이다. 아래는 debug 표(`DEBUG_METHODS`)에 있으면서
`tasty debug …` 로도 부를 수 없는 것 전부다. release 쪽 표와 나눠 두는 이유는 두 집합의
문장이 다르기 때문이다("release IPC 에 있는데 CLI 가 없다" vs "debug 빌드에만 있는데
그 빌드의 CLI 에도 없다").

debug 표 기준 총 3개.

| 이유 | debug 메서드 | 왜 CLI 가 없나 |
|---|---|---|
| 사용자 행동 | `system.shutdown` | 호스트 종료는 사용자가 직접 하는 동작이다. debug 빌드에서도 에이전트 표면에 두지 않는다 |
| 사용자 행동 | `window.focus` · `view.focus` | 포커스 전환은 사용자의 단축키/마우스 영역이다(원칙 3). debug IPC 에 재현 수단이 있는 것과, 그것을 CLI 한 줄로 상시 노출하는 것은 다르다 |

## 응답 계약 — mirror 워크스페이스로 간 구조 op

대상이 **mirror(원격 attach client) 워크스페이스**인 구조 op(`tab.create`/`split`/`tab.close`/`tab.move`/`pane.close`/`surface.close`/convert 등)는 로컬에서 실행되지 않고 원격으로 forward 된다([remote-attach](../features/remote-attach/index.md#mirror-워크스페이스-내-구조-변경)). 그 응답은 **fire-and-forget success** 다:

```json
{ "forwarded": true, "workspace_index": 2 }
```

즉 **생성된 id(surface/tab/pane)를 담지 않는다.** 원격 실행은 비동기라 응답 시점에 아직 아무것도 만들어지지 않았기 때문이다. 결과는 나중에 `StructuralDelta` 역반영으로 mirror 트리에 반영된다.

따라서 **구조 op 의 응답에서 생성된 id 를 동기로 꺼내 쓰는 호출자를 새로 만들지 않는다.** 그런 호출자는 mirror 워크스페이스에서 조용히 깨지고(응답에 필드가 없다), 게다가 forward 큐는 IPC 응답과 무관하게 드레인되므로 **로컬은 실패인데 원격에는 리소스가 남는** 고아를 만든다. 그 id 가 반드시 필요한 method 는 mirror 워크스페이스를 대상으로 **거부**해야 한다 — 실제 선례가 `terminal.spawn` 이며, 그 결정과 배경은 [ADR-0086](../adr/0086-reject-terminal-spawn-into-mirror-workspace.md).

## 권한 표 등재 (라우터 ↔ METHOD_TABLE)

**IPC 라우터에 dispatch 분기가 있는 메서드는 예외 없이 권한 표에 등재한다** — plugin 에 열 것이면 `plugin(&[..])`, local caller 전용으로 둘 것이면 `local_only()`. 표는 `METHOD_TABLE`(+ debug 빌드 전용 `DEBUG_METHODS`, prefix fallback `PREFIX_RULES`, `crates/tasty-ipc/src/method_meta.rs`).

미등재는 "닫혀 있음"으로 대충 넘어가지 않는다. `method_meta()` 가 `None` 이면 plugin/agent 호출자는 `UnknownMethod` 로 거부되긴 하지만, 그 거부가 **정책인지 등재 누락인지 표만 봐서는 구분되지 않는다** — 나중에 권한을 재검토하는 쪽이 "닫으려던 것"과 "잊은 것"을 판별할 수 없다. `local_only()` 등재는 그 판단을 코드에 남기는 선언이다(거부 자체는 `NotPluginCallable` 로 바뀔 뿐 동작은 같다).

`tests/ipc_router_table_parity.rs` 가 라우터 소스를 훑어 강제한다. `"<method>" =>` 팔과 `… .method == "…"` 비교(`||` 로 이어진 다중 비교 포함, `src/app/ipc/app_methods.rs`·`window_required.rs` 가 그 형태다)를 **둘 다** 잡는다. 소스 목록은 고정 목록(`ROUTER_SOURCES`)에 더해 `src/app/ipc/` 를 **디렉토리째** 걷는다(`ROUTER_DIRS`) — dispatch 스텝이 몰려 있는 이 디렉토리에 새 파일을 만들어도 목록에 손으로 추가하는 걸 잊어 사각지대가 생기지 않게 한다(실제로 `window_required.rs` 가 그렇게 빠져 6 메서드가 통과했다). 등재 누락은 조용히 오래 남는 종류의 결함이라(형제 메서드가 전부 등재된 상태에서 한둘만 빠져도 아무 신호가 없다) 리뷰가 아니라 게이트로 잡는다. debug 빌드에서만 도는데, release 에서는 `DEBUG_METHODS` 가 설계상 비어 IPC 표면에서 사라지기 때문이다([debug-ipc](debug-ipc.md)). `src/app/ipc/` **밖**의 새 라우터 파일(예: `src/adapters/ipc/`)은 여전히 `ROUTER_SOURCES` 에 직접 추가한다.

## plugin 점유 namespace

plugin 이 매니페스트로 contribute 하는 IPC namespace 는 호스트 예약어와 충돌 금지(`system surface tab pane workspace claude plugin hook global_hook webhook message tool notification window debug ui ime split tree memory output approval telemetry timer` 등). 상세는 [plugin-development](plugin-development.md) "예약 prefix".

### 대상 surface 는 `surface` / `surface_id` 어느 이름으로 와도 같은 필드다

CLI 인자는 `--surface`(매니페스트의 `surface`)이고 호스트 IPC 의 표준 키는 `surface_id` 다. 그래서 CLI dynamic runner 는 **두 키를 모두 채워** 보낸다. agent plugin(`claude`/`codex`)의 핸들러는 그 두 이름을 **한 필드로** 읽고, 둘이 다른 값이면 고르지 않고 `-32602` 로 거절한다 — 어느 쪽을 골라도 절반의 호출자에게는 지목하지 않은 대상이 된다. 판정은 `tasty-plugin-agent-common` 에 한 벌만 있다.

아무 이름도 안 오면 아무것도 호스트로 넘기지 않는다. 그때 호스트는 **부모가 하나뿐이면 그것**으로 푸는데(`--surface` 생략의 정의), 그 폴백은 *이름을 안 준 호출* 을 위한 것이지 *이름을 줬는데 못 읽은 호출* 을 위한 것이 아니다. 대상을 읽고도 안 실어 보내면 실재하지 않는 id 를 지목한 호출이 남의 자식에 성공한다 — 호스트의 "named target is never resolved by focus" 가드가 그 자리를 지키는데, 이름이 어긋나면 그 가드에 애초에 닿지 않는다.

### auto_wait chain

일부 plugin 명령은 1차 IPC 응답 직후 wait IPC 를 자동 chain 해 대상이 terminal state(`idle`/`needs_input`/`exited`)에 도달할 때까지 block 할 수 있다. child terminal 의 파생 상태 `stale`([ADR-0072](../adr/0072-child-state-hook-observation-fusion.md))은 **기본 terminal state 집합에 넣지 않는다** — 무출력 임계값 기반 판정은 휴리스틱이라 오탐 시 아직 일하는 자식을 종결 처리하게 된다. 다만 hook 유실로 영구 대기하는 것보다 조기 탈출이 나은 소비자는 `terminal_states` 에 직접 `"stale"` 을 추가해 선택할 수 있다. 매니페스트 `[[contributes.cli.subcommand]].auto_wait` 한 필드로 선언적으로 켠다(plugin 핸들러 미수정, CLI dynamic runner 가 chain). `map_from_response`(1차 응답→wait params, 우선) + `map_from_request`(요청→fallback) + `polling`(state_field/terminal_states/interval). `polling` 과 `auto_wait` 동시 선언은 validator 가 reject(직교 — 전자는 *이 명령 자체가 wait*, 후자는 *응답 직후 다른 method chain*). `surface`↔`surface_id` 키는 자동 alias.

**`claude spawn`/`tell`, `codex spawn`/`tell` 은 더 이상 이 메커니즘을 쓰지 않는다** — 동기 블로킹 대신 완료 시 caller surface 에 알림 훅을 주입하는 이벤트 기반 모델로 대체됐다. claude 는 `claude-idle`/`needs-input`/`process-exit` hook → `claude.notify_done`(`crates/tasty-plugin-claude/src/handlers.rs`의 `register_notify_hooks` 참조), codex 는 `codex-idle`/`process-exit` hook → `codex notify-caller`([`docs/plugins/codex/index.md`](../plugins/codex/index.md) 참고)로 각각 구현. 두 핸들러 모두 hook 이 한 번 fire 되면 알림 후 `surface.locate` 로 target 생존을 확인해, 아직 살아있으면(process-exit 가 아니었으면) 형제 hook 을 재등록한다(자기재무장) — "spawn/tell 당 알림 1회"가 아니라 "child 가 exit 할 때까지 상태 전환마다 알림"이다. auto_wait/polling 스키마 자체는 삭제되지 않았다 — 번들 plugin 중 이를 실사용하는 소비자는 없으며(전수 grep 확인), 향후 외부/서드파티 plugin 소비자를 위해 스키마만 유지한다.

---

## 안정성 정책

### 버전 단계

| 단계 | break 정책 |
|------|-----------|
| 0.x (현재) | 적극 변경. break 는 `CHANGELOG.md` 에 `(BREAK)` 표기 + **한 minor 이상 deprecation 우선**(보안 예외 즉시 제거 가능). major bump 는 사용자 결정으로만 |
| 안정선 | SemVer 엄격. `api_version = "1"` schema 는 추가만. 진입 시점은 사용자가 결정 |
| 1.x | minor 추가, major break |
| 2.0 | `api_version = "2"` 시작. plugin 이 매니페스트로 명시 선택 |

### Break 분류

| 변경 | 분류 |
|------|------|
| 새 메서드/명령 추가 · 응답에 Option/Default 필드 추가 · optional+default 파라미터 추가 | minor |
| 메서드 rename (alias 있음) · `#[serde(other)]` fallback 있는 enum variant 추가 | minor (deprecation) |
| 메서드 rename (alias 없이) · required 파라미터 추가 · optional→required 승격 | **major** |
| 응답 필드 의미/타입/nullability 변경 · 제거 · 단위·포맷 변경(ms↔s) | **major** |
| default 값 의미 변경 · 새 권한 필요(기존 plugin 중단) · 에러 코드 의미 변경 | **major** |
| fallback 없는 enum variant 추가 · 컬렉션 정렬/페이지네이션 의미 변화 | **major** |
| 비동기 이벤트(`command.invoke`/`ipc.result`/`event.dispatch`) 의미 변화 · handshake/env(`TASTY_HOST_API_VERSION`/auth token) 계약 변경 · 예약 namespace·권한 토큰 정책 변경 | **major** |

이 표는 출발선이다. 새 분류가 필요하면 PR 에 명시하고 표에 추가한다.

### Deprecation 절차

1. 옛 표면 유지 + 새 표면 추가.
2. 옛 표면 호출 시 `tracing::warn!("deprecated: <old>, use <new>")`(`crates/tasty-ipc/src/alias.rs`).
3. `CHANGELOG.md` `Deprecated` 절에 제거 기한 기록.
4. 기한 직전 일괄 제거 PR.

deprecation 기간은 "한 minor 이상"이 원칙(보안·심각 버그는 즉시 제거 가능).

### plugin-protocol schema

`api_version` 메이저를 올리는 변경: 메시지 필드 의미 변경 · 메서드 제거(alias 없이) · 응답 형식 의미 변경 · handshake/auth 계약 변경. 추가만(새 메시지, optional+default 필드)은 같은 `api_version` 내 `crates/tasty-plugin-protocol/Cargo.toml` minor bump. 이력은 `crates/tasty-plugin-protocol/CHANGELOG.md`.

### 자동화 보조

`tests/changelog_unreleased.rs`(CHANGELOG `[Unreleased]` 절 존재 검증) + `cli_naming_count_drift.rs`(메서드 카운트 drift) + `ipc_router_table_parity.rs`(라우터 팔 ↔ 권한 표 등재 대조, 위 "권한 표 등재"). PR 템플릿·`git-cliff` 초안·경로 기반 규칙은 점진 도입 대상.

## 관련

- [reference/api](../reference/api.md) — 전체 IPC/CLI 메서드 카탈로그
- [plugin-development](plugin-development.md) · [plugin-ecosystem](plugin-ecosystem.md) · [release](release.md)
