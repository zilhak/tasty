# Event Bus 1.0 카탈로그

Tasty 호스트와 plugin이 공유하는 사건의 1.0 wire 계약을 정의한다. 이 문서는 **plugin이 의존하는 공개 API**이며, 호환성 정책의 단일 출처(SoT)다.

> Plugin이 구독·발화 API를 어떻게 쓰는지는 [plugins.md](plugins.md) 참조. 본 문서는 사건의 *구조*만 다룬다.

## Envelope

모든 이벤트는 동일한 wrapper 구조로 흐른다.

```jsonc
{
  "key": "surface.focused",
  "payload": { "surface_id": 42, "prev_surface_id": 7 },
  "meta": {
    "trace_id": "trace-abc",
    "hop": 0,
    "origin": { "kind": "host" },
    "scope": "surface"
  }
}
```

| 필드 | 타입 | 의미 |
|---|---|---|
| `key` | string | `<namespace>.<event_name>` 포맷. 예약 네임스페이스는 호스트만 publish 가능. |
| `payload` | object | 이벤트별 페이로드. 스키마는 본 문서의 [카탈로그](#카탈로그) 섹션 참조. |
| `meta.trace_id` | string | 한 사건이 만든 chain 전체에 공유되는 opaque 식별자. 호스트 발화 시점에 생성, plugin이 재발화해도 그대로 전파. |
| `meta.hop` | u8 | 호스트 발화 시 `0`. plugin이 publish API로 다시 발화하면 `+1`. `hop > 16`이면 dispatcher가 차단. |
| `meta.origin` | object | `{"kind":"host"}` 또는 `{"kind":"plugin","plugin_id":"..."}`. |
| `meta.scope` | string | `"system"` 또는 `"surface"`. |

### `meta.scope`

| Scope | 의미 |
|---|---|
| `system` | 특정 surface에 매이지 않는 전역 사건 (`theme.changed`, `plugin.loaded`, `workspace.activated` 등). |
| `surface` | 특정 surface 기준 사건 (`surface.focused`, `process.exited` 등). 대상 surface ID는 payload 필드(`surface_id` 등)로 전달 — scope 자체는 ID를 들고 다니지 않는다. |

scope는 빠른 필터링·로그 분류용이다. 더 세분화된 scope(tab/pane/workspace)는 1.0에서 도입하지 않는다.

### Hop count

이벤트 chain의 무한 루프를 방지한다. plugin A가 받은 이벤트를 plugin B가 받을 키로 다시 publish하는 경우 hop이 `+1` 된다. 의도된 사용 패턴에서 hop은 2~3을 넘기 어렵다 — `MAX_HOP=16`은 안전 장치다.

### Lifecycle `reason`

종료 계열 이벤트 (`surface.closed`, `tab.closed`, `pane.closed`, `workspace.closed`, `window.closed`, `plugin.unloaded`)의 `reason` 필드는 3종으로 한정된다.

| 값 | 의미 |
|---|---|
| `user` | 사용자가 단축키/마우스/UI 버튼으로 직접 닫음. |
| `ipc` | CLI 또는 plugin의 IPC 호출로 닫음 (에이전트 자동화 포함). |
| `crash` | 비정상 종료 (PTY 프로세스 크래시, plugin 강제 종료 등). |

cascade(parent_closed) 별도 분류 없음. 부모를 닫은 주체가 자식 reason에도 그대로 전파된다.

### 쓰로틀

다음 이벤트는 호스트가 **150ms leading + trailing 쓰로틀**을 적용한 뒤 발화한다. plugin은 별도 디바운싱 없이 처리해도 부담 없다.

| 이벤트 | 쓰로틀 단위 |
|---|---|
| `surface.resized` | `(key, surface_id)` |
| `split.ratio_changed` | `(key, group_id)` |

drag 시작·종료에는 무조건 1회씩 발화된다 (leading + flush). 윈도우 중간에 들어온 호출은 마지막 payload만 보관 후 윈도우 종료 시 trailing으로 발화.

## 예약 네임스페이스

호스트만 `publish_host`로 발화할 수 있는 네임스페이스. plugin이 매니페스트에 `event_publish`로 적어도 거부된다.

```
system, surface, tab, pane, workspace, window,
command, ime, split, notification, hook, tool,
plugin, extension, process, clipboard, theme, language, debug
```

이 외 네임스페이스는 plugin이 자유롭게 쓴다. 관례적으로 자기 `id`(예: `com.tasty.claude.*`) 또는 짧은 별칭을 네임스페이스로 두면 충돌 위험이 적다.

## 안정성 등급

| 등급 | 의미 | 변경 정책 |
|---|---|---|
| Stable | 1.0 공개 계약 | major 버전 전까지 키·필수 필드 불변. 옵션 필드 추가만 허용. |
| Experimental | 시험 중 | minor 버전마다 변경 가능. plugin 매니페스트에 `experimental_events = true` 필요. |
| Internal | debug 빌드 전용 | release에 노출되지 않음. |

## 카탈로그

페이로드 Rust 타입은 `tasty_plugin_protocol::events::payloads` 모듈에 정의되어 있다. 필드명·옵션 여부는 그 정의가 정답이며, 본 표는 사람이 읽기 위한 요약.

### Surface

| 키 | 시점 | Payload (요약) | scope | 등급 |
|---|---|---|---|---|
| `surface.created` | Surface 인스턴스 생성 직후 | `surface_id, kind, tab_id, pane_id, workspace_id, created_by` | surface | Stable |
| `surface.closed` | Surface 종료 직전 | `surface_id, kind, reason` | surface | Stable |
| `surface.focused` | 포커스 받음 | `surface_id, prev_surface_id?` | surface | Stable |
| `surface.resized` | 픽셀 크기 변경 (150ms 쓰로틀) | `surface_id, width_px, height_px` | surface | Stable |
| `surface.title_changed` | 표시 이름 변경 | `surface_id, title` | surface | Stable |

`surface.created.created_by`:
- `{"kind":"user"}` — 사용자가 UI로 split / 탭 추가한 결과.
- `{"kind":"agent","source_plugin":"com.tasty.claude"}` — plugin이 IPC로 spawn한 결과.

### Tab

| 키 | 시점 | Payload (요약) | scope | 등급 |
|---|---|---|---|---|
| `tab.created` | Tab 생성 직후 | `tab_id, pane_id, workspace_id, kind` | system | Stable |
| `tab.closed` | Tab 종료 직전 | `tab_id, pane_id, reason` | system | Stable |
| `tab.focused` | tab bar 내 active 변경 | `tab_id, pane_id, prev_tab_id?` | system | Stable |
| `tab.moved` | 다른 pane으로 이동 | `tab_id, from_pane, to_pane` | system | Stable |
| `tab.renamed` | Tab 이름 변경 | `tab_id, title` | system | Stable |

### Pane

| 키 | 시점 | Payload (요약) | scope | 등급 |
|---|---|---|---|---|
| `pane.created` | Pane 생성 | `pane_id, parent_pane_group?, workspace_id` | system | Stable |
| `pane.closed` | Pane 종료 | `pane_id, reason` | system | Stable |
| `pane.split` | Pane 분할 발생 | `original_pane, new_pane, direction` (horizontal/vertical) | system | Stable |

### Split (PaneGroup / SurfaceGroup)

| 키 | 시점 | Payload (요약) | scope | 등급 |
|---|---|---|---|---|
| `split.ratio_changed` | Splitter 비율 변경 (150ms 쓰로틀) | `group_id, level (pane/surface), ratio` | system | Stable |

### Workspace

| 키 | 시점 | Payload (요약) | scope | 등급 |
|---|---|---|---|---|
| `workspace.created` | Workspace 생성 | `workspace_id, window_id, name` | system | Stable |
| `workspace.closed` | Workspace 종료 | `workspace_id, reason` | system | Stable |
| `workspace.activated` | 사용자가 다른 workspace로 전환 | `workspace_id, prev_workspace_id?` | system | Stable |
| `workspace.renamed` | 이름/부제/설명 변경 | `workspace_id, name?, subtitle?, description?` | system | Stable |

### Window (OS)

| 키 | 시점 | Payload (요약) | scope | 등급 |
|---|---|---|---|---|
| `window.created` | OS window 생성 | `window_id, kind, modality (modeless/modal)` | system | Stable |
| `window.closed` | OS window 종료 | `window_id, reason` | system | Stable |
| `window.focused` | OS focus 받음 | `window_id` | system | Stable |

`kind`/`modality`는 [유비쿼터스 언어](../concepts/ubiquitous-language.md)와 일치.

### Clipboard (OS)

| 키 | 시점 | Payload (요약) | scope | 등급 |
|---|---|---|---|---|
| `clipboard.copied` | OS 클립보드에 사용자 복사 감지 | `kind (text/image), text?, image_b64?, timestamp_ms` | system | Stable |

### Plugin lifecycle

| 키 | 시점 | Payload (요약) | scope | 등급 |
|---|---|---|---|---|
| `plugin.loaded` | Plugin 로드 완료 | `plugin_id, version` | system | Stable |
| `plugin.unloaded` | Plugin 종료 | `plugin_id, reason` | system | Stable |
| `plugin.error` | Plugin 에러 발생 | `plugin_id, error_kind, message` | system | Stable |
| `plugin.enabled` / `plugin.disabled` | 사용자가 토글 | `plugin_id` | system | Stable |

### Extension lifecycle

| 키 | 시점 | Payload (요약) | scope | 등급 |
|---|---|---|---|---|
| `extension.activated` | Extension의 hooks가 dispatcher에 등록됨 | `extension_id, target_id` | system | Stable |
| `extension.pending` | Target 부재/호환성 깨짐으로 pending 전환 | `extension_id, target_id, reason` | system | Stable |
| `extension.conflict` | 같은 target에 충돌 | `extension_id, target_id, conflicting_id` | system | Stable |

### Tool

| 키 | 시점 | Payload (요약) | scope | 등급 |
|---|---|---|---|---|
| `tool.invoked` | 사용자가 도구 메뉴 항목 클릭 | `tool_id, source (builtin/plugin)` | system | Stable |

### Command (Option D)

plugin은 단축키 자체를 보지 않는다. 매니페스트에 command id만 선언하고, 호스트가 매핑을 관리한다.

| 키 | 시점 | Payload (요약) | scope | 등급 |
|---|---|---|---|---|
| `command.invoked` | 단축키/메뉴/IPC로 command 트리거 (owner unicast) | `plugin_id, command_id, scope (global/surface), source_surface_id?, trigger (shortcut/menu/ipc)` | system or surface | Stable |
| `command.shortcut_changed` | 사용자가 설정창에서 단축키 매핑 변경 | `plugin_id, command_id, shortcut?, prev_shortcut?` | system | Stable |

`command.invoked`는 **owner plugin에게만 unicast**로 전달된다 (broadcast 아님). 다른 plugin은 같은 키를 구독해도 받지 않는다. `command.shortcut_changed`는 broadcast — 사용자 설정 변화는 모든 plugin이 알 수 있다.

scope=global인 command의 단축키는 **조합키만** 허용 (`Ctrl`/`Alt`/`Cmd`/`Shift` + key). scope=surface는 owner surface에 포커스가 있을 때만 동작하므로 **단일 키도** 허용된다.

### IME

| 키 | 시점 | Payload (요약) | scope | 등급 |
|---|---|---|---|---|
| `ime.composition_start` | IME 입력 시작 | `surface_id` | surface | Experimental |
| `ime.composition_end` | IME 입력 종료/확정 | `surface_id, committed_text` | surface | Experimental |

`composition_update`는 1.0 제외. 필요한 plugin은 별도 IPC 채널 검토.

### Theme / Language

| 키 | 시점 | Payload (요약) | scope | 등급 |
|---|---|---|---|---|
| `theme.changed` | 라이트/다크 또는 사용자 테마 변경 | `theme_id` | system | Stable |
| `language.changed` | UI 언어 변경 | `language_code` (BCP 47 또는 토큰) | system | Stable |

호스트 설정의 일반 변경 알림(`settings.changed`)은 1.0에 포함되지 않는다 — schema-less broadcast는 권한 게이트가 어렵다. 의미 있는 변화는 각각 전용 이벤트로 노출하고, plugin 자기 설정 변경은 별도 Plugin Settings 기능에서 owner unicast로 처리한다.

### Notification / Hook

| 키 | 시점 | Payload (요약) | scope | 등급 |
|---|---|---|---|---|
| `notification.created` | 호스트가 알림 생성 | `id, title, body, source` | system | Stable |
| `notification.dismissed` | 알림 닫힘 (dismiss) | `id` | system | Planned |
| `hook.fired` | `tasty-hooks`의 surface/global hook 발화 | `hook_id, event_kind, surface_id?, payload` | surface (surface_id 있음) / system (global hook) | Experimental |

- `notification.dismissed` 의 등급은 **Planned** — payload 와 키는 1.0 에 예약됐지만 *현재 호스트 코드에서 발화하지 않는다*. 알림 dismiss 동작이 도입될 때 발화한다.
- 알림 *read* 처리 (`DomainIntent::MarkNotificationRead` / `MarkAllNotificationsRead`) 는 호스트 event 를 발화하지 않는다. read flag 는 단순 표시 상태 (notification panel 의 시각 강조) 이고 dismiss 와 의미가 다르므로, plugin 관심사가 아닌 것으로 본다. 만약 plugin 이 read 추이를 알아야 한다면 별 이벤트 (`notification.read`) 신규 추가가 필요하다.

### Process (PTY)

| 키 | 시점 | Payload (요약) | scope | 등급 |
|---|---|---|---|---|
| `process.started` | PTY 프로세스 시작 | `surface_id, pid, command` | surface | Stable |
| `process.exited` | PTY 프로세스 종료 | `surface_id, exit_code?` | surface | Stable |

호스트가 정규식 매칭을 해주는 `process.output_match`는 1.0에 포함되지 않는다. plugin이 자기 state transition 시점에 비파괴 read IPC로 화면을 peek하고 클라이언트 측에서 매칭하면 된다. 사용자가 등록한 `tasty-hooks` 패턴 결과는 `hook.fired`로 받을 수 있다.

### Memory

| 키 | 시점 | Payload (요약) | scope | 등급 |
|---|---|---|---|---|
| `memory.changed` | `tasty-memory` regular entry 의 put/delete/expire/scope cleanup 직후 | `scope, key, kind ∈ {created,updated,deleted,expired}, version?` | system | Stable |

- **Secret 영역 변경은 발화하지 않는다.** owner/key 노출을 막기 위함. 자기 plugin 의 secret 상태는 직접 호출한 IPC 응답으로만 관찰한다.
- `scope` 값은 `surface:42`, `workspace:1`, `window:3`, `global`, `account:default` 같은 token 문자열.
- 호스트가 1 변경 = 1 envelope 으로 발화한다. surface/workspace 가 닫혀 한 번에 다수 key 가 사라질 때는 각각 별도 `deleted` 이벤트가 온다.
- Subscribe 예: `event_subscribe = ["memory.changed"]`. 권한: `MemoryRead`.

### System / Debug

| 키 | 시점 | Payload (요약) | scope | 등급 |
|---|---|---|---|---|
| `system.startup_complete` | 엔진 부트 완료 | `{}` | system | Stable |
| `system.shutdown_initiated` | 종료 시작 | `reason` | system | Stable |
| `debug.*` | 디버그 시나리오 | (가변) | system | Internal (debug 빌드만) |

## 권한 범주

매니페스트 `permissions.event_subscribe`는 다음 패턴 형식을 받는다.

| 형식 | 예 | 매칭 |
|---|---|---|
| 정확한 키 | `"surface.closed"` | 그 키만 |
| 와일드카드 (네임스페이스 끝에서만) | `"surface.*"` | `surface.`로 시작하는 모든 키 |
| Plugin id 네임스페이스 | `"com.tasty.claude.*"` | 그 plugin이 publish하는 모든 키 (target plugin 매니페스트의 `[[events_emitted]]` 선언 필요) |

다음 형식은 거부된다.

- `"*"` (전체) — 너무 광범위.
- `"*.bar"` 또는 `"foo*"` — 와일드카드는 네임스페이스 끝에서만.

`permissions.event_publish`는 동일한 패턴 형식이며, 예약 네임스페이스의 키는 거부된다. plugin이 자기 네임스페이스(예: `com.example.x.*`)와 명시 키만 발화할 수 있다.

## 후속 변경 정책

1.0 출시 후:

- Stable 이벤트의 키/필수 필드 제거 → major bump 필요.
- 옵션 필드 추가 → 가능 (plugin은 모르는 필드 무시).
- Experimental → Stable 승격 → minor bump, plugin 호환성 그대로.
- 새 이벤트 추가 → 가능 (plugin이 안 쓰면 영향 없음).
- 새 reserved namespace 추가 → 기존 plugin이 우연히 쓰던 키와 충돌 가능 → major bump 또는 마이그레이션 안내.
