# Plugin 시스템

Tasty는 외부 plugin을 별도 OS 프로세스로 실행하여 surface 종류를 추가할 수
있다. 호스트 ↔ plugin은 TCP + JSON 메시지로 통신한다.

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
```

검증 규칙 (위반 시 plugin 로드 거부):

- `manifest_version`은 정확히 `1`
- `api_version`은 호스트 버전과 일치 (현재 `"1"`)
- `id`는 역도메인 형식 (소문자 + 숫자 + `.-_`, `.` 포함 필수)
- `surface_kinds[].kind`는 소문자 + `_` + 숫자만

> **TOML 주의**: top-level 키(`permissions = [...]` 등)는 모든 `[table]` 헤더보다 *먼저* 와야 한다. 그렇지 않으면 가장 가까운 테이블 안의 키로 해석된다.

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
| `terminal.spawn` | `claude.launch`, `claude.spawn` |
| `terminal.write` | `surface.send/send_key/send_combo/send_to/send_wait_idle` |
| `terminal.read` | `surface.set_mark/read_since_mark/screen_text/cursor_position/is_typing` |
| `claude.read` | `claude.children/parent/wait` |
| `claude.invoke` | `claude.kill/respawn/set_idle_state/set_needs_input/broadcast/tell/launch/spawn` |
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
tasty plugin install <path>                # 디렉터리를 plugins/로 복사 (매니페스트 권한 자동 grant)
tasty plugin remove <id>                   # graceful shutdown + 디렉터리 삭제
tasty plugin enable <id>                   # 활성화 + spawn
tasty plugin disable <id>                  # graceful shutdown + plugins.toml 갱신
tasty plugin logs <id> [--follow]          # ~/.tasty/plugins-logs/<id>.log 출력
tasty plugin permissions <id>              # 매니페스트 + granted 표시
tasty plugin grant <id> <permission>       # 권한 추가 (매니페스트에 선언된 경우만)
tasty plugin revoke <id> <permission>      # 권한 제거
```

`logs`는 호스트 IPC를 거치지 않고 파일을 직접 읽는다 — 호스트가 죽었을 때도 동작.

## IPC

| 메서드 | 파라미터 | 설명 |
|--------|---------|------|
| `plugin.list` | 없음 | `{plugins: [{id,name,version,description,enabled,running,surface_kinds,log_path}]}` |
| `plugin.install` | `path: string` | 매니페스트 검증 후 `plugins/<id>/`로 재귀 복사 + 매니페스트 권한 자동 grant + 자동 활성화 시 spawn |
| `plugin.remove` | `id: string` | graceful shutdown + 디렉터리 삭제 |
| `plugin.enable` | `id: string` | 활성화 + spawn |
| `plugin.disable` | `id: string` | graceful shutdown |
| `plugin.permissions` | `id: string` | `{id, manifest:[...], granted:[...]}` |
| `plugin.grant` | `id: string, permission: string` | granted에 추가 (매니페스트에 선언된 권한만) |
| `plugin.revoke` | `id: string, permission: string` | granted에서 제거 |

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

설정 → 단축키 → **Plugins** 탭에서 다음을 확인할 수 있다 (현재 read-only 표시).

1. 좌측 사이드 카테고리에서 `Plugins` 선택
2. 상단 드롭다운으로 plugin 선택 (단축키를 contribute한 plugin만 노출)
3. 해당 plugin이 선언한 command 목록과 현재 적용 중인 effective binding 표시:
   - **inherit 모드**: `Follows <host_action> (현재 키)` 형태로 호스트 액션에 동행함을 표시
   - **independent 모드**: 매니페스트의 `default_keybinding` 또는 사용자 오버라이드 값을 표시
4. focused surface가 plugin 소유면 plugin 단축키가 **무조건 우선**한다.
   매칭되면 이벤트가 소모되어 호스트 액션은 트리거되지 않는다 (inherit 모드도
   동일 — plugin이 받는 것으로 끝). 그 외 영역(터미널, 다른 surface)에서는
   호스트 키가 정상 동작.

> 변경 UI(모드 토글 / 키 캡처)는 현재 단계에서 제공되지 않는다. 사용자가
> override를 적용하려면 직접 `~/.tasty/plugins.toml`을 편집해야 한다.

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
