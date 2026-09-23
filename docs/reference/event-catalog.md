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
| `meta.trace_id` | chain 전체 공유 opaque id. 호스트 발화 시 생성. 재발화에서 전파되는 것은 plugin 이 받은 값을 실어 보낼 때다 — 호스트는 이 값을 고치지 않는다 |
| `meta.hop` | host는 0. plugin publish는 미응답 dispatch의 최대 hop+1과 보낸 값 중 큰 값을 사용하며 16 초과는 거절한다. 아래 재발행 규칙 참조 |
| `meta.origin` | `{kind:host}` 또는 `{kind:plugin, plugin_id}` |
| `meta.scope` | `system`(전역) 또는 `surface`(대상 id 는 payload 필드로) |

- **Lifecycle `reason`** (종료 계열 `*.closed` / `plugin.unloaded`): `user`(사용자 직접) / `ipc`(CLI·plugin 자동화) / `crash`(비정상). cascade 별도 분류 없음 — 부모를 닫은 주체가 자식 reason 에 전파.
- **쓰로틀**: `surface.resized`(키별), `split.ratio_changed`(group 별)는 150ms leading+trailing. drag 시작·종료엔 무조건 1 회.

### 재발행과 응답

host는 성공적으로 보낸 event.dispatch의 request ID·hop을 응답까지 보관한다.
publish 도착 때 하한을 적용하고 hook 처리 뒤까지 미루지 않는다. trace_id는 plugin이 전달한 값을 유지한다.
응답은 기록을 한 번 소비하며 plugin 정지·재시작은 전부 정리한다.
plugin당 최대 1024 건을 유지하고 넘으면 오래된 기록부터 버린다.

SDK는 on_event 뒤에 응답한다. callback 안 재발행은 이 하한을 적용받지만 먼저 응답하고 나중에 발행하는 루프는 막지 못한다.
처리 중 무관한 publish도 hop이 올라갈 수 있고, 응답하지 않으면 높은 하한이 오래 남아 정상 publish도 거절될 수 있다.
plugin은 event.dispatch에 응답해야 한다. SDK callback·응답 순서와 host pump의 사건 우선 처리를 함께 유지한다.

## 지나간 사건 — 위치로 읽는다

버스는 debug·release에서 같은 메모리 ring을 사용한다.
보존은 EVENT_RING_CAPACITY(1024 건)와 EVENT_RING_BYTES_LIMIT(16MiB 직렬화 길이) 중 먼저 닿는 쪽으로 제한한다.
단 가장 새 사건 하나는 크기와 무관하게 남기므로 16MiB를 절대 메모리 상한으로 보면 안 된다.
JSON Value 메모리와 직렬화 길이도 다르다. 길이는 버스 lock 밖에서 buffer 없이 센다.
이 보존과 plugin subscription fan-out은 별개이며 fan-out은 승인된 사건을 모두 전달한다.

| 응답 | 의미 |
|------|------|
| events·next_offset | 각 사건의 offset과 다음 조회 위치 |
| epoch | 버스 세대. 재시작하면 기록과 offset이 초기화되므로 함께 보존한다 |
| truncated·skipped | 요청 위치보다 앞선 사건이 이미 제거돼 건너뛴 개수 |
| ahead_of_stream·stream_end | 요청 offset이 현재 끝보다 큼. 끝과 같으면 정상 대기 위치 |

서버는 consumer별 cursor를 보관하지 않는다. 같은 인자로 반복해도 cursor 부작용은 없지만
새 사건과 eviction 때문에 답은 달라질 수 있다. 사건은 디스크에 저장하지 않는다.
epoch는 정상 시계에서는 버스 생성의 wall-clock nanos, UNIX_EPOCH 이전이면 OS random-seeded hash와 PID·시간 차로 만든다.
영구 고유성을 보장하는 ID는 아니다.

`events.fetch {offset,max,filter,wait_ms}`는 Local 전용이다. token Agent·Plugin은 사용할 수 없고 Plugin은 subscription을 쓴다.
필터는 정확 key 또는 namespace.*이며 조건변수로 최대 60 초 대기한다. 관심 없는 사건으로 깨어나면 남은 시간만 기다린다.
미래 위치도 요청한 대기를 유지하고 next_offset을 임의 변경하지 않는다. 즉시 점검하려면 `wait_ms=0`으로 조회한다.
개별 대기 상한은 동시 요청 thread 수 상한이 아니다.

### CLI follow와 재연결

`tasty events follow`는 offset과 epoch를 함께 관리한다. --epoch로 이전 세대를 지정할 수 있다.
연결마다 첫 요청은 `wait_ms=0`이며 epoch 변경·ahead_of_stream이면 그 답의 사건을 출력하지 않고 0부터 다시 읽는다.
전송 실패 때는 기본적으로 재부착할 --offset·--epoch 인자를 stderr에 쓰고 종료한다.
--reconnect일 때만 1 초마다 discovery 파일을 다시 읽고 연결한다. host가 반환한 오류는 전송 단절과 구별한다.
stdout은 사건 JSON 줄만, 유실·세대·연결 통지는 stderr로 쓴다.

epoch 없이 새 세대의 끝이 이미 옛 offset을 넘었다면 재시작을 구별하지 못한다.
같은 세대에서 손으로 미래 offset을 지정해도 초기화 후 과거 사건이 다시 나올 수 있다.
consumer가 cursor와 중복 처리를 책임진다. feed로 사용자 key·mouse 원문이나 화면 복원을 구현하지 않는다.

## 예약 네임스페이스 (호스트만 발화)

```
system, surface, tab, pane, workspace, window, command, ime, split,
notification, hook, tool, plugin, extension, process, clipboard, theme, language, memory,
agent
```

이 외는 plugin 자유 — 관례상 자기 `id`(`com.tasty.claude.*`)를 네임스페이스로.

## 안정성 등급

- **Stable** — major 전까지 키·필수 필드 불변, 옵션 필드 추가만.
- **Experimental** — minor 마다 변경 가능. **경고일 뿐 구독 조건이 아니다** — 구독 조건은 등급과 무관하게 매니페스트 `event_subscribe` 패턴이 요청 패턴을 덮는가 하나다. 근거는 [ADR-0633](../adr/0633-event-feed-delivery.md).
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
| `plugin.surface_kind_registered` | `plugin_id, kind, rendering` |
| `plugin.window_declared` | `plugin_id, window_id` |
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

### Agent (scope=system, Experimental)

협업 primitive 의 **종결 사실**만 싣는다. 대상 workspace 는 `meta.scope` 가 아니라 payload 의 `workspace_id` 로 온다 — envelope 의 scope 축은 `system`/`surface` 둘뿐이고 `workspace.*` 계열이 이미 같은 방식이다.

| 키 | 시점 | payload | 등급 |
|----|------|---------|------|
| `agent.task_finished` | task 가 종결 상태에 들어간 직후 | `workspace_id, task_id, state` | Experimental |
| `agent.barrier_closed` | barrier 가 요구 수를 채워 닫힌 직후 | `workspace_id, name, count_required` | Experimental |

- `state` 는 `succeeded` · `failed` · `cancelled` · `skipped` 넷 중 하나다. **비종결 전이(`waiting`/`ready`/`running`)는 발화하지 않는다** — 종결에는 모든 진입 경로가 지나는 공통 처리 지점이 있고(`agent.task_await` 가 그것으로 깨어난다) 비종결에는 없다.
- **실패 사유·task 결과·명령 출력을 안 싣는다.** 그 문자열은 task 가 돌린 명령의 출력을 담을 수 있고 피드는 구독 권한만 있으면 받는다. 필요하면 `task_id` 로 `agent.task_get` 을 부른다.
- **`agent.barrier_closed` 에 시간 초과는 안 온다.** barrier 의 `timed_out` 은 전이가 일어나는 순간이 없고 조회할 때 현재 시각으로 판단한다.
- **lease 만료는 사건이 아니다.** 같은 이유다 — 만료는 읽을 때 확인하는 조건이고, 그것을 사건으로 내면 발화 시점이 "누가 언제 조회했나" 에 달린다.
- 등급이 Experimental 이라 minor 에서 키·payload 가 바뀔 수 있다. 구독 조건은 다른 키와 같다 — 매니페스트 `event_subscribe` 가 그 키를 덮으면 받는다.

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
