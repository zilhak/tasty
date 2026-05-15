# Debug 전용 IPC

CLAUDE.md "사용자 행동과 에이전트 행동의 분리" 원칙에 따라, **사용자 입력(키보드/마우스 단축키)을 그대로 재현하는 IPC**는 release 빌드에서 노출되지 않는다. `#[cfg(debug_assertions)]`로 감싸 debug 빌드에서만 동작한다.

판단 기준:

- **에이전트 기능** (release IPC에 노출): surface/tab/workspace 생성·조회·닫기, 클립보드 히스토리, 알림 등 — 에이전트가 자기 작업을 수행하기 위해 필요한 동작.
- **디버그 기능** (debug 전용 IPC): 사용자 단축키/마우스로 트리거되는 동작을 자동화로 재현 — 키 송신, 마우스 클릭 주입, 단축키로만 여는 popup의 IPC 트리거 등.

## 라우팅

```rust
pub fn handle(state: &mut AppState, request: &JsonRpcRequest) -> JsonRpcResponse {
    if let Some(resp) = route_engine_handler(state, request, id.clone()) { return resp; }
    if let Some(resp) = route_gui_handler(state, request, id.clone()) { return resp; }
    #[cfg(debug_assertions)]
    if let Some(resp) = route_debug_handler(state, request, id.clone()) { return resp; }
    JsonRpcResponse::method_not_found(id, &request.method)
}
```

`route_debug_handler` 자체가 `#[cfg(debug_assertions)]`로 감싸여 있어 release 바이너리에는 코드 자체가 들어가지 않는다.

## 메서드 목록

| method | params | 설명 |
|---|---|---|
| `ui.state` | `{}` | 현재 UI 상태 (settings_open, popup 상태, focus 등) 조회 |
| `debug.cell_info` | `surface_id, row, col` | 셀 단위 렌더 속성 조회 (텍스트, fg/bg, bold 등) |
| `debug.screen_attrs` | `surface_id, row` | 한 행 전체 셀 속성 조회 |
| `debug.glyph_color` | `surface_id, row, col` | GPU 렌더러가 실제로 그리는 글리프 색상 (renderer 검증용) |
| `debug.feed_bytes` | `surface_id, bytes` 또는 `text` | VTE 바이트를 PTY 우회하여 터미널에 직접 주입 |
| `debug.inject_mouse` | `surface_id, button, x, y, ...` | SGR mouse(1006) 시퀀스로 마우스 이벤트 주입 |
| `debug.inject_key` | `surface_id, key, modifiers` | 키 이벤트 주입 |
| `debug.tool.list` | `{}` | 도구 메뉴 항목 전체를 표시 순서대로 반환 (`source`, `action`, `order_hint` 포함) |
| `debug.tool.invoke` | `key` | 도구 항목 key(`<plugin_id>/<tool_id>`)로 사용자 클릭과 동일한 dispatch 실행 |
| `debug.popup.list` | `{}` | 매니페스트로 contribute된 popup 정의 + 현재 열린 instance 반환 |
| `debug.popup.open` | `plugin_id, popup_id, context?` | popup 인스턴스를 강제로 open. 응답에 `instance_id`. |
| `debug.popup.close` | `instance_id` | popup 인스턴스를 강제로 close (PluginRequest 사유) |
| `debug.event_bus.list_subscribers` | `key` | 해당 키에 매칭되는 plugin 구독 목록 (`plugin_id`, `sub_id`, 매니페스트 패턴) |
| `debug.event_bus.publish` | `key, payload(json string), scope("system"\|"surface")` | 임의 키로 host envelope 발화. 응답에 `trace_id` 포함 |
| `debug.event_bus.trace` | `trace_id` | 최근 256개 envelope 링버퍼에서 같은 trace_id를 가진 envelope들을 발화 순서로 반환 |
| `debug.extension.invoke_hook` | `extension_id, kind("event"\|"ipc"), phase("pre"\|"post"), mode("transform"\|"filter"\|"observe"), target, payload` | 매니페스트 hook 매칭을 우회하여 extension의 `handle_extension_hook`을 직접 호출. 응답에 `modified_payload`, `pass`가 그대로 전달된다. fail-open/backoff 우회 — 테스트/디버그 전용. |

`debug.event_bus.*`는 `App`-level dispatch에서 `PluginManager::event_bus`를 직접 호출한다 (다른 `debug.*`처럼 `route_debug_handler`를 거치지 않는다 — `AppState`가 PluginManager를 들고 있지 않기 때문). CLI는 `tasty debug event-bus {list-subscribers|publish|trace}` 서브커맨드로 노출된다.

## CLI 노출

CLI 서브커맨드도 동일하게 debug 빌드에서만 등록된다. 예를 들어 `tasty tool clipboard viewer`는 release 바이너리에 존재하지 않는다 (`src/cli/mod.rs::ClipboardCommands::Viewer`가 `#[cfg(debug_assertions)]`).

## 새 디버그 IPC 추가 시

1. 핸들러를 `#[cfg(debug_assertions)]`로 감싼다.
2. `route_debug_handler`의 match 분기에 추가한다.
3. **`src/ipc/method_meta.rs`의 `DEBUG_METHODS` 표에 등록한다** (METHOD_TABLE이 아님).
4. CLI에서 호출할 일이 있으면 CLI 서브커맨드 variant도 `#[cfg(debug_assertions)]`로 감싼다.
5. 본 문서에 메서드를 추가한다.
6. **`docs/agent-guide/`에는 작성하지 않는다.** 릴리스 에셋에 포함되지 않아야 한다.

## release 빌드에서의 동작

release 빌드(`cargo build --release` 또는 `--profile dist`)에서는:

- `DEBUG_METHODS` 상수 자체가 빈 슬라이스로 컴파일된다 (`#[cfg(not(debug_assertions))]`).
- `method_meta("debug.inject_key")` 같은 lookup이 `None`을 반환한다.
- plugin이 debug 메서드 호출을 시도하면 `CallerError::UnknownMethod`로 거부된다 (debug 빌드의 `NotPluginCallable`과 메시지는 다르지만 거부라는 결과는 동일).
- local caller(CLI/네트워크 IPC)는 라우터가 해당 분기 자체를 컴파일하지 않으므로 `method_not_found`로 떨어진다.

회귀 방지: `src/ipc/method_meta.rs::tests::debug_methods_absent_in_release` 및 `src/ipc/caller.rs::tests::plugin_call_to_debug_method_is_unknown_in_release`가 release 표면에서 debug 메서드 노출 0건을 검증한다.

## 참고

- 사용자 입력 vs 에이전트 행동 구분: `CLAUDE.md` "핵심 원칙: 사용자 행동과 에이전트 행동의 분리"
- 포커스 독립성: `CLAUDE.md` "핵심 원칙: 포커스 독립성"
