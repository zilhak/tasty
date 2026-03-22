# ipc/handler.rs 분할 계획

`src/ipc/handler.rs` (591줄)를 `src/ipc/handler/` 디렉토리로 분할한다.

## 현재 구조 분석

handler.rs는 하나의 `handle()` 라우터 함수와 20개의 개별 핸들러 함수로 구성된다.

| 줄 범위 | 내용 | 카테고리 |
|---------|------|---------|
| 1-6 | use 선언 | — |
| 8-39 | `handle()` 라우터 함수 (20개 메서드 매칭) | 라우터 |
| 41-50 | `handle_system_info` | system |
| 52-67 | `handle_workspace_list` | workspace |
| 69-97 | `handle_workspace_create` | workspace |
| 99-119 | `handle_workspace_select` | workspace |
| 121-166 | `handle_pane_list`, `handle_pane_split` | pane |
| 168-220 | `handle_tab_list`, `handle_tab_create`, `handle_tab_close` | tab |
| 214-228 | `handle_pane_close`, `handle_surface_close` | pane/surface |
| 230-286 | `handle_surface_list`, `collect_surface_info`, `collect_surface_layout_info` | surface |
| 288-341 | `handle_surface_send`, `handle_surface_send_key` | surface |
| 343-420 | `handle_notification_list`, `handle_notification_create`, `handle_tree` | system |
| 422-504 | `handle_hook_set`, `handle_hook_list`, `handle_hook_unset` | hook |
| 506-539 | `handle_set_mark`, `handle_read_since_mark` | surface(mark) |
| 541-591 | `handle_claude_launch` | claude |

## 분할 후 구조

```
src/ipc/handler/
├── mod.rs              — handle() 라우터 + workspace/pane/tab/system 핸들러
├── surface.rs          — surface 관련 핸들러
└── hooks.rs            — hook/claude 핸들러
```

## 각 파일 상세

### mod.rs (~280줄)

라우터와 기본 핸들러. 가장 자주 사용되는 workspace/pane/tab 관련 함수를 담는다.

**포함 내용:**
- use 선언 (줄 1-6)
- `handle()` 라우터 함수 (줄 8-39)
- `handle_system_info` (줄 41-50)
- `handle_workspace_list` (줄 52-67)
- `handle_workspace_create` (줄 69-97)
- `handle_workspace_select` (줄 99-119)
- `handle_pane_list` (줄 121-142)
- `handle_pane_split` (줄 144-166)
- `handle_tab_list` (줄 168-185)
- `handle_tab_create` (줄 187-204)
- `handle_tab_close` (줄 206-212)
- `handle_pane_close` (줄 214-220)
- `handle_notification_list` (줄 343-361)
- `handle_notification_create` (줄 363-381)
- `handle_tree` (줄 383-420)

라우터에서 surface/hook 모듈의 핸들러를 호출:

```rust
mod surface;
mod hooks;

pub fn handle(state: &mut AppState, request: &JsonRpcRequest) -> JsonRpcResponse {
    match request.method.as_str() {
        // ... workspace/pane/tab은 여기서 직접 처리
        "surface.close" => surface::handle_surface_close(state, id),
        "surface.list" => surface::handle_surface_list(state, id),
        "surface.send" => surface::handle_surface_send(state, id, &request.params),
        "surface.send_key" => surface::handle_surface_send_key(state, id, &request.params),
        "surface.set_mark" => surface::handle_set_mark(state, id, &request.params),
        "surface.read_since_mark" => surface::handle_read_since_mark(state, id, &request.params),
        "hook.set" => hooks::handle_hook_set(state, id, &request.params),
        "hook.list" => hooks::handle_hook_list(state, id, &request.params),
        "hook.unset" => hooks::handle_hook_unset(state, id, &request.params),
        "claude.launch" => hooks::handle_claude_launch(state, id, &request.params),
        // ...
    }
}
```

**의존:**
- `crate::ipc::protocol::{JsonRpcRequest, JsonRpcResponse}`
- `crate::model::SplitDirection`
- `crate::state::AppState`
- serde_json

### surface.rs (~190줄)

Surface 관련 핸들러. 터미널 직접 제어, 리스트, 마크 API.

**포함 내용:**
- `handle_surface_close` (줄 222-228)
- `handle_surface_list` (줄 230-241)
- `collect_surface_info` (줄 243-263) — 헬퍼
- `collect_surface_layout_info` (줄 265-286) — 헬퍼
- `handle_surface_send` (줄 288-302)
- `handle_surface_send_key` (줄 304-341)
- `handle_set_mark` (줄 508-520)
- `handle_read_since_mark` (줄 522-539)

**의존:**
- `crate::ipc::protocol::JsonRpcResponse`
- `crate::model::{Panel, SurfaceGroupLayout}`
- `crate::state::AppState`
- serde_json

### hooks.rs (~120줄)

Hook 시스템과 Claude 런처 핸들러.

**포함 내용:**
- `handle_hook_set` (줄 424-462)
- `handle_hook_list` (줄 464-490)
- `handle_hook_unset` (줄 492-504)
- `handle_claude_launch` (줄 543-591)

Claude 런처를 hook 모듈에 두는 이유:
- `claude.launch`는 자동화 기능으로, Hook과 같은 "에이전트 자동화" 카테고리에 속한다.
- 향후 hook 기반 claude 자동 실행 확장 시 같은 파일에서 작업하게 된다.

**의존:**
- `crate::hooks::HookEvent`
- `crate::ipc::protocol::JsonRpcResponse`
- `crate::state::AppState`
- serde_json, shell_escape
