# Plugin 시스템

Tasty는 외부 plugin을 별도 OS 프로세스로 실행하여 호스트를 확장할 수 있다.
호스트 ↔ plugin은 TCP + JSON 메시지로 통신한다.

## Plugin이 할 수 있는 것 — 5가지 카테고리

Plugin의 근본 역할은 다음 5가지로 분류된다.

1. **새 Window 추가** — OS-level 별도 윈도우. 현재 매니페스트 schema 없음(향후 추가 예정).
2. **새 Surface 추가** — 탭/스플릿 안에서 생성 가능한 새 surface 종류를 등록한다.
3. **새 Popup 추가** — Window 내부 가상 창. 포커스를 빼앗지 않고 터미널/surface 위에 떠 있는다 (`PopupDef` + `PopupManager` 기반, 상세는 `docs/design/popup-system.md`). 클립보드 히스토리, 알림 패널 같은 일시적 UI가 이 카테고리. plugin이 `[[contributes.popup]]`으로 contribute 가능.
4. **새 Tool 추가** — 좌측 사이드바 하단의 "도구" 메뉴에 항목을 꽂는다.
5. **이벤트별 동작 추가** — 외부 이벤트(사용자 키 입력, 다른 surface lifecycle, IPC/CLI 호출 등)에 반응한다.

매니페스트 contribute 키와의 매핑:

| 카테고리 | 매니페스트 키 | 상태 |
|---------|-------------|------|
| 새 Window | _(없음)_ | 미구현 |
| 새 Surface | `[[surface_kinds]]` | ✅ |
| 새 Popup | `[[contributes.popup]]` | ✅ (UI 렌더 통합 진행 중) |
| 새 Tool (도구 메뉴 항목) | `[[contributes.tool]]` | ✅ |
| 이벤트: 사용자 키 입력 | `[[contributes.commands]]` | ✅ (plugin 소유 surface 위에서만 매칭) |
| 이벤트: 다른 surface lifecycle | `event_subscribe = ["surface.closed"]` (Event Bus) | ✅ |
| 이벤트: 호스트/다른 plugin/CLI의 IPC 호출 | `[[contributes.ipc_namespace]]` | ✅ |
| 이벤트: CLI 서브커맨드 호출 | `[[contributes.cli]]` | ✅ |

이벤트의 "사용자 키 입력 **등**"은 단축키만이 아니라 surface lifecycle / IPC / CLI 호출 등 **모든 외부 트리거**를 포함한다. 어떤 카테고리를 몇 개 contribute할지는 plugin이 자유롭게 정한다 — 0개여도 valid다.

> **Window / Surface / Popup의 차이**는 `docs/design/ubiquitous-language.md` 참조. 요약하면: Window는 OS-level 별도 창, Surface는 탭/스플릿 안의 콘텐츠 영역, Popup은 Window 내부에 떠 있으며 포커스를 빼앗지 않는 일시적 가상 창이다. 휘발성 알림인 Toast는 plugin contribute 대상이 아니다 — Toast는 사용자 행동에서만 발사된다는 원칙(`docs/design/toast-system.md`) 때문.

### 매니페스트 선언 원칙

매니페스트는 호스트가 **라우팅·등록·UI 합성에 사용하는 정보**를 담는 곳이다.
호스트가 인지해야 동작하는 항목은 반드시 선언해야 한다:

- 새 surface 종류 (호스트가 surface kind registry에 등록)
- 도구 메뉴 항목 (호스트가 메뉴 렌더링에 합성)
- 단축키 (호스트가 키 매칭 로직에 합성)
- IPC namespace prefix (호스트가 라우팅)
- CLI 서브커맨드 (호스트가 dynamic clap 구성)

반대로 plugin 내부 구현 디테일(정렬/캐싱/렌더링 알고리즘 등 호스트가 알 필요
없는 사항)을 위한 **별도 매니페스트 항목은 존재하지 않는다**. 자유 텍스트인
`description`에 사람을 위한 메모로 기입해 두는 것은 막지 않으며, 호스트는
`description` 내용을 해석하지 않는다.

### 미구현 카테고리(Window)

Plugin이 자기 Window를 직접 띄우는 매니페스트 schema는 아직 없다. 우회로:

- **Surface로 대체**: 탭/스플릿 안의 surface로 띄운다. plugin이 UI tree를
  완전히 소유하는 대신 포커스/배치 모델이 Window와 다르다.

### Popup contribute

Plugin은 `[[contributes.popup]]`으로 자기 popup을 등록할 수 있다 (권한
`ui.popup` 필요). popup은 `<plugin_id>/<popup_id>` 전역 식별자를 갖는다.

```toml
permissions = ["ui.popup"]

[[contributes.popup]]
id = "search"
trigger = { kind = "event", event_key = "com.example.search.opened" }
size_hint = { width = 480, height = 320 }
anchor = "screen-center"            # 또는 "active-surface-center" / "cursor"
dismiss_on_outside_click = true

[[contributes.popup]]
id = "result"
trigger = { kind = "ipc" }          # plugin이 host IPC `popup.open`으로 명시 open
```

- `trigger.kind = "event"`: 매니페스트의 `event_key`가 host/plugin 어디서든
  발화되면 호스트가 자동으로 popup 인스턴스를 만든다. envelope payload가
  popup.open IPC의 `context`로 전달된다.
- `trigger.kind = "ipc"`: plugin이 직접 host IPC를 호출해 popup을 연다 (현재는
  debug IPC 경로만 production-노출. 도구 메뉴 액션이나 자기 event 발화로 우회
  가능 — 아래 참고).
- `[[contributes.tool]] action = { kind = "open_popup", popup_id = "<plugin_id>/<id>" }`
  로 도구 메뉴 항목에서 popup을 띄울 수도 있다.

호스트는 popup마다 `instance_id`(u64)를 발급해 동일 popup_id의 여러 인스턴스를
구분한다. 인스턴스를 닫는 방법:

- plugin이 `popup.event` 응답에 `close=true`를 실으면 호스트가 자동으로 close.
- plugin이 host IPC `popup.close`(`{"instance_id": <id>}`, `ui.popup` 권한)를 호출하면
  `PluginRequest` 사유로 close. 자기 plugin이 소유한 인스턴스만 닫을 수 있다.
- 사용자가 popup 바깥을 클릭(`dismiss_on_outside_click=true`) 또는 Escape.

### contribute가 0개인 plugin

매니페스트에 surface kind / menu item / command / observer / IPC / CLI 어느 것도
선언하지 않은 plugin도 valid다. 호스트는 이런 plugin을 spawn해 살려두기만 하고
별도 dispatch 대상으로 삼지 않는다. plugin은 자기 프로세스 안에서 스레드/타이머
/외부 입력 등으로 동작하면 되며, 필요할 때 `host.call`로 호스트 IPC를
호출하거나 `ipc.invoke:<prefix>` 권한으로 다른 plugin의 메서드를 호출할 수 있다.

### 다른 plugin을 확장하는 plugin

Plugin A가 plugin B의 IPC namespace를 호출하려면 매니페스트 권한에
`"ipc.invoke:<B의 prefix>"`를 선언하고 사용자가 `tasty plugin grant`로 동의해야
한다. B가 설치되지 않았거나 비활성이면 호출은 `-32601 method not found`로
회신되므로 A 쪽에서 분기 처리하면 된다. 설치 여부를 사전에 확인하고 싶다면
`plugin.list` IPC를 활용한다. 자기 plugin은 별도 contribute 없이 다른 plugin만
활용하는 형태도 valid하다.

## 설치 위치

| OS | Plugin 루트 |
|----|-------------|
| 모든 OS | `~/.tasty/plugins/` |

각 plugin 디렉터리:

```
~/.tasty/plugins/com.example.explorer/
  tasty-plugin.toml      # 매니페스트 (필수)
  tasty-plugin-explorer  # entry binary (또는 PATH 의존 가능)
```

비활성화/활성화 상태와 권한 grant는 `~/.tasty/plugins.toml`에 영속화된다.

```toml
[disabled]
ids = ["com.example.broken"]

[grants."com.example.explorer"]
granted = ["fs.read", "surface.write"]
```

추가 격리 디렉터리:

| 디렉터리 | 용도 |
|----------|------|
| `~/.tasty/plugins/<id>/` | plugin 본체 (실행파일·매니페스트·정적 자산) |
| `~/.tasty/plugin-data/<id>/` | plugin 런타임 데이터 (DB, 캐시 등) — 업그레이드 시 보존 |
| `~/.tasty/plugin-config/<id>.toml` | 사용자 편집 설정 |
| `~/.tasty/plugins-logs/<id>.log` | stdout/stderr (자동 redirect) |

호스트가 spawn 시 자식 프로세스에 다음 환경변수를 주입한다.

| 환경변수 | 값 |
|----------|------|
| `TASTY_PLUGIN_ID` | plugin id (예: `com.example.explorer`) |
| `TASTY_PLUGIN_DIR` | 본체 디렉터리 절대 경로 |
| `TASTY_PLUGIN_DATA_DIR` | 데이터 디렉터리 절대 경로 |
| `TASTY_PLUGIN_CONFIG_PATH` | 설정 파일 절대 경로 |
| `TASTY_PLUGIN_LOG_PATH` | 로그 파일 절대 경로 |
| `TASTY_HOST_IPC_PORT` | 호스트 listener port |
| `TASTY_PLUGIN_TOKEN` | 핸드셰이크 토큰 (1회용) |
| `TASTY_HOST_API_VERSION` | 호스트 protocol 메이저 버전 |

## 매니페스트

`tasty-plugin.toml` 형식:

```toml
manifest_version = 1
id = "com.example.explorer"           # 역도메인, 전역 유일
name = "Explorer"
version = "1.2.0"
authors = ["alice@example.com"]
description = "File explorer surface for tasty"
homepage = "https://example.com/explorer"
api_version = "1"                     # 호스트 protocol 메이저 버전과 일치 필요
permissions = ["fs.read", "surface.write", "notification"]  # 권한 토큰 (아래 표 참조)
event_subscribe = ["surface.created", "clipboard.*"]          # Event Bus 구독 패턴
event_publish = ["com.example.explorer.refreshed"]            # Event Bus 발화 패턴 (자기 namespace만)

[entry]
type = "process"                      # 향후 "wasm" 추가 가능
command = "tasty-plugin-explorer"     # 매니페스트 디렉터리 기준 상대 또는 PATH
args = []

[[surface_kinds]]
kind = "explorer"                     # 소문자 + '_' + 숫자만
display_name_i18n_key = "surface.kind.explorer"
icon = "📁"

[[contributes.commands]]
id = "explorer.refresh"
title_i18n_key = "explorer.command.refresh"
default_keybinding = "F5"
# binding_mode 옵션:
#   "independent"  (기본) — plugin이 자기 키를 갖는다. 사용자는 설정에서 따로 변경
#   "inherit:<host action>" — 호스트 액션(예: "clipboard.copy")의 사용자 설정을 그대로 따라간다
#                             plugin이 작성자 의도로 inherit를 선택한 command는
#                             설정 UI에서 사용자가 "독립 설정"으로 떼어낼 수 있다
binding_mode = "independent"
# scope 옵션:
#   "global"  (기본) — 어디서나 동작. 단축키는 조합키 권장
#   "surface" — owner plugin이 만든 surface에 포커스가 있을 때만 동작. 단일 키 허용
scope = "global"

[[events_emitted]]                    # plugin이 publish하는 이벤트 카탈로그 (optional)
key = "com.example.explorer.refreshed"
description = "Explorer surface re-scanned the file tree"
stability = "stable"                  # "stable" (기본) | "experimental"
# payload_schema = "{ \"path\": \"string\", \"count\": \"u32\" }"  # 자유 텍스트
```

필수 필드 (없으면 plugin 로드 거부):

- `manifest_version` — 정확히 `1`
- `id` — 역도메인 형식 (소문자 + 숫자 + `.-_`, `.` 포함 필수)
- `name` — 표시용 이름
- `version` — plugin 자체 버전 문자열 (semver 권장 — 호스트는 표시·로그에 사용)
- `api_version` — 호스트 protocol 메이저 버전과 일치 (현재 `"1"`)
- `entry` — 실행 진입점 (`type = "process"`, `command = ...`)

옵셔널 필드 (없어도 valid, 기본값으로 처리):

- `authors`, `homepage`, `description`, `lang_dir`(기본 `"lang"`)
- `permissions` (기본 빈 배열)
- `event_subscribe`, `event_publish` (기본 빈 배열 — Event Bus 사용 안 함)
- `surface_kinds`, `contributes.*` (모두 기본 빈 배열 — **0개여도 됨**)

추가 검증 규칙 (위반 시 plugin 로드 거부):

- `surface_kinds[].kind`는 소문자 + `_` + 숫자만
- `contributes.ipc_namespace[].prefix`는 호스트 예약어 사용 금지 (아래 IPC namespace 절 참조)
- `event_subscribe[]` / `event_publish[]` 패턴은 정확 키(`surface.created`) 또는 끝의 namespace 와일드카드(`surface.*`)만 허용. 단독 `"*"`·중간/시작 와일드카드는 거부
- `event_publish[]`는 예약 namespace(`surface`, `system`, `tab`, ...) 거부 — 호스트만 발화 가능. 자기 plugin 도메인의 namespace로만 발화할 것
- `[[events_emitted]]` 키는 매니페스트의 카탈로그 선언. plugin이 publish할 키마다 `key` / `description` / 옵션 `stability`("stable"|"experimental", 기본 stable) / 옵션 `payload_schema`를 적는다. `event_publish` 권한 패턴으로 커버되는 키만 선언 가능 (검증 실패 시 로드 거부)

> **TOML 주의**: top-level 키(`permissions = [...]` 등)는 모든 `[table]` 헤더보다 *먼저* 와야 한다. 그렇지 않으면 가장 가까운 테이블 안의 키로 해석된다.

## 호스트 CLI / IPC 확장 (1st-class plugin commands)

Plugin은 매니페스트에 자기 CLI 서브커맨드와 IPC namespace prefix를 선언하여,
`tasty <name> <subcommand>` 형태로 호출되고 `<prefix>.<method>` IPC 메서드를
받을 수 있다. 호스트 코드는 plugin의 메서드 이름이나 인자를 알 필요가 없다.

### 매니페스트 — IPC namespace

```toml
[[contributes.ipc_namespace]]
prefix = "codex"
```

선언된 prefix로 시작하는 모든 IPC 메서드는 해당 plugin으로 forward된다. 예약
prefix(`system`, `surface`, `tab`, `pane`, `workspace`, `plugin`,
`hook`, `global_hook`, `message`, `tool`, `notification`, `window`, `debug`,
`ui`, `ime`, `ipc`, `split`, `tree`)는 사용 불가. 같은 prefix를 두 plugin이 동시에
선언하면 나중에 로드된 쪽은 거부된다. `claude`, `codex` 같은 번들 plugin이 이미
점유한 prefix를 외부 plugin이 중복 선언해도 동일하게 거부된다.

### 매니페스트 — CLI

```toml
[[contributes.cli]]
name = "codex"
description_i18n_key = "codex.cli.desc"
subcommands = [
  { name = "spawn", ipc_method = "codex.spawn", args = "spawn_args" },
]

[contributes.cli.arg_groups.spawn_args]
flags = [
  { name = "surface", type = "u32",    flag = "--surface", required = false },
  { name = "prompt",  type = "string", flag = "--prompt",  required = false },
]
```

플러그인 등록 시 호스트 CLI는 `tasty codex spawn --prompt "hi"` 같이 호출
가능해진다. 인자는 자동으로 JSON-RPC params 객체로 직렬화되어 plugin에
`{ "surface": ..., "prompt": "hi" }` 형태로 전달된다. 정적 호스트 명령이 항상
우선 매칭되며, plugin은 그 위에 동적으로 합쳐진다 (plugin이 호스트 명령을
가릴 수 없다).

서브커맨드를 빠뜨리고 `tasty <name>`만 입력하면 호스트 정적 명령(`tasty claude`
등)과 동일하게 풀 도움말이 출력된다. 매니페스트가 `surface` 이름의 `u32` 인자를
정의한 경우, 사용자가 `--surface`를 명시하지 않으면 호출자 surface의
`TASTY_SURFACE_ID` 환경변수에서 자동으로 채워진다 — `tasty claude tell` 등
호스트 명령의 `resolve_surface_id` 동작과 동일.

### 다른 plugin namespace 호출

자기 plugin이 다른 plugin의 IPC namespace를 호출하려면 매니페스트 권한에
`"ipc.invoke:<prefix>"`를 추가하고 사용자의 grant를 받아야 한다.

plugin이 자기 namespace 메서드를 `host.call("<자기 prefix>.method", ...)`로
호출하면 호스트는 forward를 건너뛰고 호스트 dispatcher로 통과시킨다. 무한
forward 루프 없이 plugin이 자기 namespace의 구현을 호스트 본문으로 위임할 수
있다 (예: `com.tasty.image` plugin은 모든 `image.*` 호출을 호스트의 동명 IPC로
trampoline한다). 호스트에 동명 메서드가 없으면 일반 `-32601 method not found`가
반환된다.

## 도구 메뉴 항목 (Tool Contribute)

좌측 사이드바 하단의 "도구" 팝업에 항목을 꽂는다. 호스트 빌트인(클립보드
히스토리 등) 위에 plugin 항목이 합쳐져 한 메뉴로 표시된다.

### 매니페스트

```toml
permissions = ["ui.tool_item"]

[[contributes.tool]]
id = "todo"                                          # plugin 내 유일, 소문자+숫자+'-'
label_i18n_key = "com.example.todo.tool.label"       # 메뉴에 표시할 라벨 (i18n 키)
icon = "✅"                                          # 옵션 (1자 이모지/짧은 문자열)
order_hint = 100                                     # 정렬 가중치 (작을수록 위, 기본 100)

[contributes.tool.action]
kind = "event"                                       # "event" | "open_surface" | "open_popup"
event_key = "com.example.todo.menu_clicked"          # plugin namespace의 이벤트 키
```

`action.kind`별 필드:

- **`event`** — 클릭 시 호스트가 `event_key`로 Event Bus 이벤트를 발화한다.
  payload는 `{"tool_id": "<plugin_id>/<tool_id>"}`. plugin은 `event_subscribe`에
  같은 키를 선언해 받는다 (자기 namespace 이벤트도 명시 구독 필요).
- **`open_surface`** — 클릭 시 포커스된 pane에 `surface_kind`로 새 탭을 연다.
  `surface_kind`는 같은 매니페스트의 `[[surface_kinds]]`에 선언된 kind여야 한다.
- **`open_popup`** — 클릭 시 `popup_id`(`<plugin_id>/<id>` 형식) popup을 띄운다.
  popup이 contribute되어 있어야 하며 plugin이 실행 중이어야 한다. 매니페스트
  검증 단계에서 같은 plugin의 popup id를 가리키면 존재 여부를 cross-check 한다.

### 정렬과 키

전체 메뉴 항목은 `order_hint` 오름차순으로 정렬되며 동률이면 키 순. 호스트 빌트인은
`0..99`, plugin 기본값은 `100` — 같은 가중치에서는 안정 정렬이다. 항목 키는
호스트가 자동으로 합성한다:

- 호스트 빌트인: `builtin:<name>` (현재 등록된 빌트인 항목 없음 — 클립보드 히스토리도 plugin 으로 이전됨)
- plugin: `<plugin_id>/<tool_id>` (예: `com.tasty.clipboard-history/open-viewer`)

### 표시 조건

매니페스트에 `permissions = ["ui.tool_item"]`이 선언되고 사용자가 grant한 경우에만
메뉴에 노출된다. plugin을 disable하면 항목도 자동으로 사라진다. 권한이 회수되면
같은 효과로 즉시 사라진다.

### i18n fallback

`label_i18n_key`에 해당하는 키가 catalog에 등록돼 있지 않으면 키 자체를 표시한다.
plugin 측 `lang/<locale>.toml`이나 호스트 `lang/`에 키를 추가해 둘 것.

## 권한 모델

Plugin이 호스트 IPC를 호출하려면 매니페스트의 `permissions`에 권한 토큰을 선언하고
사용자가 `tasty plugin grant`로 동의해야 한다. CLI install은 사용자 의도적 명령이므로
매니페스트의 모든 권한이 자동 grant된다.

| 권한 토큰 | 허용되는 IPC 카테고리 |
|-----------|-----------------------|
| `surface.read` | `surface.list`, `tab.list`, `pane.list`, `workspace.list`, `tree`, meta 조회 |
| `surface.write` | `tab.create`, `surface.close`, `pane.close`, `split`, `tab.move`, `workspace.create/update/move`, hooks |
| `notification` | `notification.create`, `notification.list` |
| `clipboard.read` | `tool.clipboard.list/get` |
| `clipboard.write` | `tool.clipboard.paste/remove/clear` |
| `fs.read` | (예약 — `tool.read_file` 등) |
| `fs.write` | (예약 — `tool.write_file` 등) |
| `process.spawn` | (예약 — `tool.run_shell` 등) |
| `terminal.spawn` | 신규 PTY 생성을 동반하는 메서드 (예: `surface.respawn_terminal`, plugin의 child 생성) |
| `terminal.write` | `surface.send/send_key/send_combo/send_to/send_wait_idle` |
| `terminal.read` | `surface.set_mark/read_since_mark/screen_text/cursor_position/is_typing` |
| `ui.tool_item` | `[[contributes.tool]]`로 도구 메뉴 항목을 노출할 수 있음 (호스트 IPC 호출은 아님 — UI 카테고리 권한) |
| `ipc.invoke:<prefix>` | 다른 plugin의 namespace 메서드 호출 (`<prefix>`는 호출 대상 plugin이 contribute한 IPC namespace) |
| `ext:<target_plugin_id>` | `[extends]` 블록으로 다른 plugin의 IPC/이벤트 흐름을 가로채는 권한. target plugin 단위 단일 토큰 — 세부 mode(transform/filter/observe)와 hook 대상은 매니페스트의 `[[extends.*]]`로 표현된다 |
| `network` | (예약) |

Local-only 메서드 (CLI/사용자만, plugin은 항상 거부):

- `plugin.*`, `window.*` — plugin/window 관리
- `surface.ime_*` — IME 입력
- `system.shutdown`, `debug.*`, `ui.state`, `ui.screenshot` — debug 빌드 전용

권한 변경은 plugin process 재시작 없이 즉시 반영된다. Plugin이 권한 없는 메서드를
호출하면 JSON-RPC 에러 코드 `-32001`로 거부되고 호스트 로그에 `permission_denied`가 남는다.

## CLI

```
tasty plugin list                          # 설치된 plugin 일람
tasty plugin show <id>                     # 단일 plugin 전체 정보 (매니페스트 + 권한 + 명령어 + runtime 상태)
tasty plugin install <path>                # 디렉터리를 plugins/로 복사 (매니페스트 권한 자동 grant)
tasty plugin remove <id>                   # graceful shutdown + 디렉터리 삭제
tasty plugin enable <id>                   # 활성화 + spawn
tasty plugin disable <id>                  # graceful shutdown + plugins.toml 갱신
tasty plugin logs <id> [--follow]          # ~/.tasty/plugins-logs/<id>.log 출력
tasty plugin permissions <id>              # 매니페스트 + granted 표시
tasty plugin grant <id> <permission>       # 권한 추가 (매니페스트에 선언된 경우만)
tasty plugin revoke <id> <permission>      # 권한 제거
tasty plugin extension list                # 모든 extension의 상태 일람 (active/pending/disabled/conflict)
```

`logs`는 호스트 IPC를 거치지 않고 파일을 직접 읽는다 — 호스트가 죽었을 때도 동작.

## IPC

| 메서드 | 파라미터 | 설명 |
|--------|---------|------|
| `plugin.list` | 없음 | `{plugins: [{id,name,version,description,enabled,running,surface_kinds,log_path}]}` |
| `plugin.show` | `id: string` | 단일 plugin의 매니페스트 전체 + runtime 상태. `{id,name,version,description,authors,homepage,api_version,manifest_version,dir,enabled,running,log_path,permissions:{manifest,granted},event_subscribe,event_publish,events_emitted,surface_kinds,commands,menu_items,ipc_namespace,cli,extends?,extension_state?}`. `extends`는 `[extends]` 블록을 선언한 extension plugin일 때만 채워진다. `extension_state`는 `{status:"active"\|"pending"\|"disabled"\|"conflict", ...}` 형식. |
| `plugin.install` | `path: string` | 매니페스트 검증 후 `plugins/<id>/`로 재귀 복사 + 매니페스트 권한 자동 grant + 자동 활성화 시 spawn |
| `plugin.remove` | `id: string` | graceful shutdown + 디렉터리 삭제 |
| `plugin.enable` | `id: string` | 활성화 + spawn |
| `plugin.disable` | `id: string` | graceful shutdown |
| `plugin.permissions` | `id: string` | `{id, manifest:[...], granted:[...]}` |
| `plugin.grant` | `id: string, permission: string` | granted에 추가 (매니페스트에 선언된 권한만) |
| `plugin.revoke` | `id: string, permission: string` | granted에서 제거 |
| `plugin.extension.list` | 없음 | 모든 extension의 `{extensions: [{extension_id, target_id?, state:{status,...}}]}`. `status`는 `active` / `pending` / `disabled` / `conflict`. extension(=`[extends]` 블록 선언) plugin만 포함. |

## Event Bus

Plugin은 Event Bus 1.0을 통해 호스트가 자동 발화한 사건(surface 생성/종료, 클립보드 변경 등)을 구독하거나, 자기 namespace의 사건을 다른 plugin에 broadcast 할 수 있다. wire 계약 전체는 [event-catalog.md](event-catalog.md) 참조.

### 구독·발화 권한

매니페스트의 `event_subscribe` / `event_publish` 패턴이 권한 게이트다. 매니페스트에 선언되지 않은 패턴은 SDK가 호스트에 알려도 호스트가 거부하고 경고 로그만 남긴다.

```toml
event_subscribe = ["surface.*", "clipboard.entry_added"]
event_publish = ["com.example.explorer.refreshed", "com.example.explorer.bookmark.*"]
```

- `event_subscribe`는 어떤 namespace든 허용 (호스트 발화 이벤트도 구독 가능).
- `event_publish`는 예약 namespace 금지 — `surface`, `system`, `tab`, `pane`, `split`, `workspace`, `window`, `clipboard`, `plugin`, `extension`, `tool`, `command`, `ime`, `theme`, `language`, `notification`, `hook`, `process` 등은 호스트만 발화한다.
- 와일드카드는 namespace 단위 마지막 세그먼트에만 허용 (`foo.*`, `foo.bar.*`). 단독 `"*"`나 중간 와일드카드는 거부.

### SDK 사용

```rust
use tasty_plugin_sdk::{BusHandle, EventDispatchCtx, EventScope, HostHandle, Plugin};

impl Plugin for MyPlugin {
    fn on_start(&mut self, _host: HostHandle, bus: BusHandle) {
        // 패턴 구독 — sub_id를 보관해 두면 나중에 unsubscribe 가능
        self.surface_sub = bus.subscribe("surface.*").ok();
        // 자기 namespace 이벤트 발화
        let _ = bus.publish_fresh(
            "com.example.explorer.refreshed",
            serde_json::json!({ "files": 42 }),
            EventScope::System,
        );
    }

    fn on_event(&mut self, ctx: EventDispatchCtx) {
        // ctx.envelope.key / payload / meta (origin, hop, trace_id, scope)
        if ctx.envelope.key == "surface.closed" { /* ... */ }
    }
}
```

- `BusHandle::publish_fresh`는 `trace_id` 생성과 `EventOrigin::Plugin`을 자동 채워 broadcast 한다. 다른 plugin의 이벤트를 받아 재발화하려면 받은 envelope의 hop을 `+1` 한 채로 [`BusHandle::publish`]에 직접 넘긴다 (`MAX_HOP=16` 초과 시 호스트가 폐기).
- fan-out은 fire-and-forget. plugin은 응답을 돌려주지 않으며 다른 구독자에게 영향을 주지 않는다.

### Command 이벤트 (Option D)

Plugin이 `[[contributes.commands]]`로 command_id를 선언하면 단축키 매핑은 호스트가 관리한다. 사용자가 키를 눌러 매칭되면 호스트가 owner plugin에 `command.invoked`를 **owner-unicast**로 보낸다 (`sub_id=0` 센티넬, 구독 없이 직접 전달, 다른 plugin은 같은 envelope을 보지 못함).

- 받는 페이로드: `CommandInvoked { plugin_id, command_id, scope, source_surface_id?, trigger }`
- 사용자가 설정 UI에서 매핑을 바꾸면 `command.shortcut_changed` System scope broadcast가 발화된다 (`plugin_id`, `command_id`, `shortcut?`, `prev_shortcut?`). 표시용 단축키 문자열이며, 정확한 상태가 필요하면 IPC로 재조회한다.

### 옛 surface_observer 마이그레이션

`[[contributes.surface_observer]]` 매니페스트 필드는 제거됐다. 같은 효과는 Event Bus 구독으로 얻는다:

```toml
# 이전 (제거됨):
# [[contributes.surface_observer]]
# event = "closed"

# 현재:
event_subscribe = ["surface.closed"]
```

SDK 측에서는 `BusHandle::subscribe("surface.closed")` 호출 후 `on_event` 콜백에서 `ctx.envelope.payload`의 `surface_id`를 꺼내 처리한다. 옛 `on_surface_lifecycle` 트레이트 메서드도 함께 제거됐다.

## 단축키 (Plugin Shortcuts)

Plugin이 자체 surface를 추가하는 경우, 그 surface 위에서 동작하는 단축키 역시
plugin이 매니페스트의 `[[contributes.commands]]`로 선언한다. 호스트는 이를 모아
설정 UI와 단축키 매칭 로직에 통합한다.

### 작성자가 결정하는 두 가지 정책

각 command는 매니페스트에서 `binding_mode`를 선언한다. 이건 **plugin 작성자가
이 command가 호스트 설정과 의미상 동일한지를 판단해 주는** 필드다.

| 값 | 의미 |
|----|------|
| `"independent"` (기본) | 호스트 단축키와 무관한 자기 키를 갖는다. 사용자가 설정에서 변경해도 호스트 설정에 영향 없음 |
| `"inherit:<host_action>"` | 호스트의 의미론적 액션에 위임. 호스트 키가 바뀌면 plugin도 자동 동행 |

inherit 가능한 host action 목록 (현재 4종, 추후 확장):

- `clipboard.copy`
- `clipboard.paste`
- `clipboard.cut`
- `select_all`

예: 파일 탐색기 plugin의 "선택한 파일 복사"를 호스트의 클립보드 복사 단축키와
동일하게 두고 싶다면 `binding_mode = "inherit:clipboard.copy"`. 반대로 plugin
고유의 동작(예: "트리 새로 고침")이라면 `"independent"`.

### 사용자가 결정할 수 있는 것

설정 → 단축키 → **Plugins** 탭에서 다음을 조정할 수 있다.

1. 좌측 사이드 카테고리에서 `Plugins` 선택
2. 상단 드롭다운으로 plugin 선택 (단축키를 contribute한 plugin만 노출)
3. 해당 plugin이 선언한 command 목록 표시. 각 항목별로:
   - **Mode 콤보**: `Inherit` / `Custom` / `(disabled)` 중 선택
     - `Inherit`: 호스트의 의미론적 액션 키를 따라감. source 콤보로 화이트리스트
       (clipboard.copy/paste/cut, select_all) 중 선택. 호스트 키가 바뀌면 자동 동행
     - `Custom`: 단일 라인 텍스트 입력에 직접 키 조합을 적는다 (예: `ctrl+f5`,
       콤마로 다중 키도 가능)
     - `(disabled)`: 명시적으로 키를 비활성
   - **Reset 버튼**: override를 제거하고 매니페스트 `default_keybinding`으로 복귀
4. 변경한 내용은 설정 모달을 **저장하지 않고 닫을 때도 반영**된다 (settings.toml과
   달리 plugin shortcut override는 모달 close 시점에 디스크에 즉시 기록됨).
5. focused surface가 plugin 소유면 plugin 단축키가 **무조건 우선**한다.
   매칭되면 이벤트가 소모되어 호스트 액션은 트리거되지 않는다 (inherit 모드도
   동일 — plugin이 받는 것으로 끝). 그 외 영역(터미널, 다른 surface)에서는
   호스트 키가 정상 동작.

### 영속화

사용자가 plugin 단축키를 수정/오버라이드한 결과는 `~/.tasty/plugins.toml`에
plugin별 섹션으로 저장된다.

```toml
[keybindings."com.example.explorer"]
"explorer.refresh" = { mode = "key", value = ["F6"] }       # 사용자 변경
"explorer.copy"    = { mode = "inherit", source = "clipboard.copy" }  # 호스트 따라가기
```

매니페스트 기본값과 사용자 오버라이드가 모두 없으면 해당 command는 키 없이
메뉴/팔레트에서만 호출 가능한 상태가 된다.

### Plugin 측 구현

호스트가 매칭에 성공하면 `surface.event` 형식과 별개의 `command.invoke` IPC
메시지를 plugin에 송신한다.

```jsonc
// 호스트 → plugin
{ "method": "command.invoke",
  "params": { "command_id": "explorer.refresh",
              "surface_id": 42 } }
```

Plugin은 자기 SDK 콜백에서 받아 처리하면 된다. inherit 모드인 command도 plugin
입장에서는 동일한 메시지로 도착한다 — 호스트 키가 매핑되어 있을 뿐 dispatch
경로는 같다.

## 국제화 (i18n)

Plugin은 자체 lang 파일을 동봉할 수 있고, 호스트는 이를 자동으로 호스트
i18n registry에 합쳐 둔다. 그 결과:

- plugin 매니페스트의 `title_i18n_key` 같은 키를 호스트 설정 UI에서도 번역해서
  표시할 수 있다
- plugin 자체 코드에서도 동일 키를 `tasty_plugin_sdk::t(key)`로 호출 가능

### 디렉터리 구조

```
~/.tasty/plugins/com.example.explorer/
  tasty-plugin.toml
  tasty-plugin-explorer
  lang/
    en.toml
    ko.toml
    ja.toml
```

매니페스트에서 `lang_dir`을 명시하지 않으면 기본 `"lang"`. 다른 경로를 쓰려면:

```toml
lang_dir = "i18n"        # 매니페스트 디렉터리 기준 상대 경로
```

### 키 명명 규칙 / 충돌

- 호스트 i18n registry는 base(호스트 lang 파일)와 plugin 별 namespace overlay로
  구성된다.
- lookup 순서: **base가 먼저** → 못 찾으면 plugin namespace를 순회.
  base에 동일 키가 있으면 plugin은 호스트 키를 덮어쓸 수 없다.
- plugin 간 키 충돌 시 선택은 보장되지 않으므로, plugin이 자기 키를
  `<plugin_id_short>.<...>` 같이 prefix하는 것을 강하게 권장 (예:
  `explorer.cmd.refresh`).

### locale 협상

호스트가 부팅 시 활성 locale을 결정하고, plugin이 발견될 때 plugin의
`lang_dir/{locale}.toml`을 읽어 namespace에 머지한다. en은 fallback으로 항상
같이 머지된다. plugin 자체 프로세스에서 동일 키를 번역해 쓰려면 plugin이 자기
SDK에서 lang 파일을 별도로 로드하거나, IPC로 호스트에 위임해야 한다 (현재
SDK에는 i18n 헬퍼가 없음).

## 생명주기 동작

- **부팅 시**: 호스트가 `~/.tasty/plugins/`를 스캔. enabled plugin 모두 spawn 시도.
- **헬스체크**: 15초마다 `ping` 송신. 60초 무응답 시 process를 강제 재시작.
- **자동 비활성화**: 10초 내 spawn 실패 3회면 사용자가 `tasty plugin enable`로 수동 재개할 때까지 정지.
- **종료 시**: 모든 plugin에 `shutdown` 메서드 송신 후 2초 timeout, timeout 시 kill.

## 보안

- 호스트는 부팅 시 `127.0.0.1:0` (랜덤 포트)로 listen.
- plugin spawn 시 환경변수로 `TASTY_HOST_IPC_PORT` + `TASTY_PLUGIN_TOKEN` 전달.
- plugin은 그 포트로 connect 후 첫 줄에 `{plugin_id, token}`을 보내야 인증 통과.
- 토큰 mismatch 시 connection을 즉시 끊는다.

## 한계

- 권한 게이트는 IPC 호출만 막는다. Plugin이 직접 `std::fs::write`로 임의 경로에 쓰면 호스트는
  알 수 없다 — 매니페스트 위반이지만 OS 샌드박스가 없는 한 강제 불가. (향후 WASM 또는
  OS-level 샌드박스로 보강 가능.)
- plugin 작성용 SDK 크레이트 (`tasty-plugin-sdk`)는 단계 08에서 추가.
