# 헤드리스 IPC 표면 — 무엇이 답하고, 무엇이 왜 없는가

헤드리스는 **CLI 전용 실행 형태**다. [`docs/identity.md`](../identity.md) 원칙 2 는 에이전트
기능이 IPC + CLI 양면으로 동작할 것을 요구하므로, 헤드리스에서 메서드가 없다는 것은
성능이나 편의 문제가 아니라 **원칙의 문제**다. 다만 창이 없으면 답 자체가 정의되지 않는
메서드도 있다. 이 문서는 그 둘을 메서드별로 가른다 — "이 표면은 GUI 가 필요하다" 같은
뭉뚱그림을 두지 않는 것이 이 문서의 목적이다.

## 라우팅 구조

gui 는 5-step 라우터(`src/app/ipc.rs`)를 쓴다. 헤드리스 pump(`src/boot/headless_dispatch.rs`)
는 caller 해석 → engine handler 직결로 간소화하되, **`App` 층 상태를 읽어야만 답할 수 있는
것**만 그 앞에서 가로챈다. 현재 가로채는 것은 둘이다.

- `timer.list` — `App` 의 TimerHub 를 읽는다.
- 읽기 전용 `plugin.*` 조회 — `App.plugin_manager` 를 읽는다.

두 가로채기 모두 **gui 와 같은 함수**를 부른다. 읽기 전용 plugin 조회의 라우팅 표는
`crate::adapters::ipc::handler::plugin::READONLY_METHODS` 하나뿐이고, gui 라우터도 헤드리스
pump 도 같은 `dispatch_readonly` 를 통과한다. 표를 두 벌로 두면 한쪽만 고쳐지는 순간
갈라지며, 이 저장소는 같은 실패형(같은 로직이 두 곳에 복제돼 서로 다르게 자란 것)을 이미
겪었다.

## `plugin.*` — 19 개 메서드의 판정

### 답한다 (7)

`App.plugin_manager` 또는 `Core` 만 읽으면 답이 정해지는 것들이다. 창과 무관하다.

| 메서드 | 읽는 것 |
|--------|---------|
| `plugin.list` | `plugin_manager.packages` |
| `plugin.show` | `plugin_manager.packages` + config |
| `plugin.permissions` | `plugin_manager` config |
| `plugin.extension.list` | `plugin_manager.extensions` |
| `plugin.audit_query` | `Core` 의 audit store |
| `plugin.audit_summary` | `Core` 의 audit store |
| `plugin.list_agent_permissions` | `Core` 의 세션 권한 |

### 아직 없다 — 쓰기이지만 창은 필요 없다 (3)

`Core` 만 있으면 되므로 기술적 장벽은 없다. 읽기 표면과 **함께 열지 않은** 이유는
쓰기이기 때문이다. 감사 로그를 지우고 에이전트 권한을 바꾸는 것은 조회와 같은
판단으로 열 대상이 아니며, 권한 표면은 그 자체로 별도 결정을 요구한다.

`plugin.audit_clear` · `plugin.grant_agent_permission` · `plugin.revoke_agent_permission`

### 아직 없다 — `App` 이분이 선행이다 (8)

`plugin_enable` 계열 헬퍼는 `src/app/plugin_glue/` 에 있고 그 모듈은 `gui` feature 로
게이트돼 있다. 이어지는 `cascade_plugin_events` 는 `src/app/dispatch_domain.rs` 의 `App`
메서드이며 헤드리스 스텁(`dispatch_domain_stubs.rs`)에 대응물이 없다. 이 경계를 여는 것은
[ADR-0127](../adr/0127-e2e-harness-binary-selection.md) 이 "`App` 이분이 선행" 이라고
적어 둔 그 자리다.

`plugin.enable` · `plugin.disable` · `plugin.install` · `plugin.remove` · `plugin.grant` ·
`plugin.revoke` · `plugin.upgrade_builtins` · `plugin.audit_follow`

`plugin.audit_follow` 는 `Core` 만 읽지만 구독을 여는 스트리밍 표면이라, 헤드리스에서
구독 수명을 무엇에 묶을지가 위 결정과 함께 정해져야 한다.

### 없는 것이 정답 (1)

`plugin.request_permission` 은 첫 main window 의 state 를 빌려 elevation popup 을 띄운다.
popup 을 보여 줄 창이 없으면 이 메서드가 하는 일 자체가 없다. 헤드리스에서 이것이
답하지 않는 것은 결함이 아니라 정의다.

## 조회는 plugin 을 기동하지 않는다

헤드리스 데몬은 attach 세션이 없으면 plugin 을 하나도 띄우지 않는 것이 기본값이다. 그래서
`plugin.list` 에 답하려면 매니저를 세워야 하는데, 그 과정을 통째로 부르면 **조회가 자기
관측 대상을 바꾼다.** `src/boot/headless_plugins.rs` 는 그래서 둘로 갈라져 있다.

| 함수 | 하는 일 | 조회가 부르는가 |
|------|---------|-----------------|
| `ensure_plugin_manager_metadata` | 매니저 생성 + `refresh_packages`(디스크 스캔) | 예 |
| `ensure_plugin_manager` | 위 + `install_builtins_if_needed` + `discover_and_start` | 아니오 |

경계가 `install_builtins_if_needed` **위**인 것이 중요하다. 그 함수는 번들에서 파일을
복사하고 매니페스트 권한을 `plugins.toml` 에 자동 grant 한다 — 관측 대상을 정확하게 만드는
것이 아니라 **없던 설치를 만들어낸다.** 프로세스를 띄우는 것보다 앞서 배제된다.

그 결과 아무것도 설치되지 않은 홈에서는 목록이 빌 수 있다. 그것은 거짓이 아니라 그 시점의
사실이며, 매니저가 아예 없을 때의 응답과 구분된다.

| 응답 | 뜻 |
|------|-----|
| `-32000 plugin manager not initialized` | 매니저를 세우지 못했다(예: waker factory 부재) |
| `{"plugins": []}` | 매니저는 있고, 디스크에 설치된 plugin 이 없다 |

이 구분이 성립하려면 `Option<&PluginManager>` 를 받는 네 핸들러가 `None` 을 **같은 방식으로**
표현해야 한다. `handle_list` 만 빈 목록을 성공으로 돌려주던 이탈이 있었고, 지금은 넷이
같다. `src/adapters/ipc/handler/plugin.rs` 의 단위 테스트가 넷을 한 자리에서 비교한다.

## 남은 표면

`plugin.*` 밖에서도 헤드리스가 답하지 않는 app 층 메서드가 있다 —
`view.*` / `window.*` / `ui.screenshot` / `clipboard.set_text` / `remote.*` /
`system.gpu_stats` / `agent.task_await` / `approval.await`, 그리고 debug 빌드의
`debug.*`. 이 중 창을 요구하는 것들은 위 `plugin.request_permission` 과 같은 판정이고,
나머지는 `App` 이분과 함께 판단할 대상이다. 개별 판정은 아직 적히지 않았다.
