# Event Bus 1.0 카탈로그

호스트와 plugin 이 공유하는 사건의 wire 계약. **plugin 이 의존하는 공개 API** 이자 호환성 정책의 단일 출처(SoT). plugin 이 구독·발화 API 를 *어떻게* 쓰는지는 [concepts/plugins](../concepts/plugins.md) · dev-guide; 여기는 사건의 *구조* 만 다룬다. 페이로드 Rust 타입은 `tasty_plugin_protocol::events::payloads` 가 정답이며, 본 표는 사람이 읽기 위한 요약이다.

## Envelope

```jsonc
{
  "key": "surface.focused",
  "payload": { "surface_id": 42, "prev_surface_id": 7 },
  "meta": { "trace_id": "trace-abc", "hop": 0, "origin": { "kind": "host" }, "scope": "surface" }
}
```

| 필드 | 의미 |
|------|------|
| `key` | `<namespace>.<event_name>`. 예약 네임스페이스는 호스트만 publish |
| `payload` | 이벤트별(아래 카탈로그) |
| `meta.trace_id` | chain 전체 공유 opaque id. 호스트 발화 시 생성, plugin 재발화해도 전파 |
| `meta.hop` | 호스트 발화 시 `0`, plugin 재발화 시 `+1`. **`hop > 16`(MAX_HOP) 이면 dispatcher 차단** |
| `meta.origin` | `{kind:host}` 또는 `{kind:plugin, plugin_id}` |
| `meta.scope` | `system`(전역) 또는 `surface`(대상 id 는 payload 필드로) |

- **Lifecycle `reason`** (종료 계열 `*.closed` / `plugin.unloaded`): `user`(사용자 직접) / `ipc`(CLI·plugin 자동화) / `crash`(비정상). cascade 별도 분류 없음 — 부모를 닫은 주체가 자식 reason 에 전파.
- **쓰로틀**: `surface.resized`(키별), `split.ratio_changed`(group 별)는 150ms leading+trailing. drag 시작·종료엔 무조건 1회.

## 예약 네임스페이스 (호스트만 발화)

```
system, surface, tab, pane, workspace, window, command, ime, split,
notification, hook, tool, plugin, extension, process, clipboard, theme, language, debug
```

이 외는 plugin 자유 — 관례상 자기 `id`(`com.tasty.claude.*`)를 네임스페이스로.

## 안정성 등급

- **Stable** — major 전까지 키·필수 필드 불변, 옵션 필드 추가만.
- **Experimental** — minor 마다 변경 가능, 매니페스트 `experimental_events = true` 필요.
- **Internal** — debug 빌드 전용.

## 카탈로그

### Surface (scope=surface)
| 키 | 시점 | payload | 등급 |
|----|------|---------|------|
| `surface.created` | 생성 직후 | `surface_id, kind, tab_id, pane_id, workspace_id, created_by` | Stable |
| `surface.closed` | 종료 직전 | `surface_id, kind, reason` | Stable |
| `surface.focused` | 포커스 | `surface_id, prev_surface_id?` | Stable |
| `surface.resized` | 크기 변경(쓰로틀) | `surface_id, width_px, height_px` | Stable |
| `surface.title_changed` | 표시 이름 변경 | `surface_id, title` | Stable |

`created_by`: `{kind:user}` 또는 `{kind:agent, source_plugin}`.

### Tab / Pane / Split (scope=system)
| 키 | payload |
|----|---------|
| `tab.created` | `tab_id, pane_id, workspace_id, kind` |
| `tab.closed` | `tab_id, pane_id, reason` |
| `tab.focused` | `tab_id, pane_id, prev_tab_id?` |
| `tab.moved` | `tab_id, from_pane, to_pane` |
| `tab.renamed` | `tab_id, title` |
| `pane.created` | `pane_id, parent_pane_group?, workspace_id` |
| `pane.closed` | `pane_id, reason` |
| `pane.split` | `original_pane, new_pane, direction` |
| `split.ratio_changed` (쓰로틀) | `group_id, level(pane/surface), ratio` |

### Workspace / Window (scope=system)
| 키 | payload |
|----|---------|
| `workspace.created` | `workspace_id, window_id, name` |
| `workspace.closed` | `workspace_id, reason` |
| `workspace.activated` | `workspace_id, prev_workspace_id?` |
| `workspace.renamed` | `workspace_id, name?, subtitle?, description?` |
| `window.created` | `window_id, kind, modality` |
| `window.closed` | `window_id, reason` |
| `window.focused` | `window_id` |

`window.kind`/`modality` 는 [hierarchy](../concepts/hierarchy.md) 와 일치.

### Process (scope 표시)
| 키 | scope | payload |
|----|-------|---------|
| `process.started` | surface | `surface_id, pid, command` |
| `process.exited` | surface | `surface_id, exit_code?` |

### Plugin / Extension / Tool (scope=system)
| 키 | payload |
|----|---------|
| `plugin.loaded` | `plugin_id, version` |
| `plugin.unloaded` | `plugin_id, reason` |
| `plugin.error` | `plugin_id, error_kind, message` |
| `plugin.enabled` / `plugin.disabled` | `plugin_id` |
| `extension.activated` | `extension_id, target_id` |
| `extension.pending` | `extension_id, target_id, reason` |
| `extension.conflict` | `extension_id, target_id, conflicting_id` |
| `tool.invoked` | `tool_id, source(builtin/plugin)` |

### Command (Option D — plugin 은 단축키를 보지 않음)
| 키 | 전달 | payload |
|----|------|---------|
| `command.invoked` | **owner unicast**(broadcast 아님) | `plugin_id, command_id, scope, source_surface_id?, trigger(shortcut/menu/ipc)` |
| `command.shortcut_changed` | broadcast | `plugin_id, command_id, shortcut?, prev_shortcut?` |

scope=global command 단축키는 조합키만, scope=surface 는 단일 키도 허용.

### Memory (scope=system, Stable)
`memory.changed`: regular entry 의 put/delete/expire/cleanup 직후 — `scope, key, kind∈{created,updated,deleted,expired}, version?`. **secret 영역은 발화 안 함**(owner/key 노출 방지). 1 변경 = 1 envelope. 구독 권한 `memory.read`.

### IME / Theme / Language / Notification / Hook / System
| 키 | scope | 등급 | payload |
|----|-------|------|---------|
| `ime.composition_start` / `_end` | surface | Experimental | `surface_id` / `surface_id, committed_text` |
| `theme.changed` | system | Stable | `theme_id` |
| `language.changed` | system | Stable | `language_code` |
| `notification.created` | system | Stable | `id, title, body, source` |
| `notification.dismissed` | system | **Planned**(예약, 미발화) | `id` |
| `hook.fired` | surface/system | Experimental | `hook_id, event_kind, surface_id?, payload` |
| `system.startup_complete` | system | Stable | `{}` |
| `system.shutdown_initiated` | system | Stable | `reason` |
| `debug.*` | system | Internal | (가변, debug 빌드만) |

> `composition_update`·`process.output_match`·`settings.changed` 는 1.0 제외. 알림 *read* 처리는 host event 미발화(표시 상태일 뿐).

## 구독·발화 권한 패턴

매니페스트 `event_subscribe` / `event_publish` 가 권한 게이트다.

| 형식 | 예 | 매칭 |
|------|-----|------|
| 정확 키 | `surface.closed` | 그 키만 |
| 네임스페이스 와일드카드(끝만) | `surface.*` | `surface.` prefix 전부 |
| plugin id 네임스페이스 | `com.tasty.claude.*` | 그 plugin publish 전부(대상 매니페스트 `[[events_emitted]]` 선언 필요) |

거부: `"*"`(전체), `"*.bar"`/`"foo*"`(중간/시작 와일드카드). `event_publish` 는 예약 네임스페이스 키 거부.

## 후속 변경 정책

Stable 키/필수 필드 제거 → major bump. 옵션 필드 추가·새 이벤트 추가·Experimental→Stable 승격 → minor 이하(plugin 호환 유지). 새 예약 네임스페이스 추가는 충돌 가능 → major/마이그레이션 안내.

## 관련

- [concepts/plugins](../concepts/plugins.md) — plugin 통합 축(events 포함)
- [reference/api](api.md) — IPC/CLI 표면
