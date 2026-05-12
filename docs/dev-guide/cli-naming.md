# CLI / IPC 명명 규칙

이 문서는 Tasty의 IPC 메서드와 CLI 명령에 대한 명명 규칙을 정의한다. 단일 진실 원천은 `src/ipc/method_meta.rs::METHOD_TABLE`이며, 본 문서는 그 위에서의 규칙·예외·deprecation을 설명한다.

## 기본 형식

```
IPC 메서드: <namespace>.<verb>[_<modifier>]
CLI 명령:  tasty <namespace> <verb> [--<option>]
```

예: `surface.list` ↔ `tasty surface list`, `claude.spawn` ↔ `tasty claude spawn`.

## namespace 화이트리스트

현재 등록된 namespace (단일 진실: `METHOD_TABLE`):

| namespace | 도메인 | 메서드 수 |
|-----------|--------|----------|
| `surface` | 레이아웃 surface — 구조 조작 / 터미널 I/O / IME prefix | 21 + `surface.ime_*` prefix |
| `claude` | Claude Code agent 통합 (spawn/wait/broadcast 등) | 11 |
| `plugin` | plugin 관리 (install/enable/grant 등) — local-only | 8 |
| `debug` | 디버그 (사용자 입력 재현) — local-only | 7 |
| `tool.clipboard` | 클립보드 (`tool` namespace 2단 안의 서브) | 5 |
| `workspace`, `tab`, `message`, `window` | 레이아웃·메시지 큐·window 관리 | 각 4 |
| `notification`, `hook`, `global_hook` | 알림·hook | 각 2-3 |
| `system`, `ui`, `pane` | 호스트 정보·UI 상태·pane 조작 | 각 1-2 |
| `<plugin_prefix>` | plugin이 매니페스트로 점유한 namespace | 가변 |

**root namespace 예외**: 다음 두 메서드는 `<namespace>.` 없이 root에 등록되어 있다. 사용자가 자주 호출하는 짧은 명령이라 namespace 부여를 미룬 결정.

- `split` — pane 분할
- `tree` — surface tree 조회

새 메서드는 이 형식 예외에 동참하지 말 것 (`split.<x>`, `tree.<x>` 같은 자식도 만들지 말 것).

## verb 화이트리스트

명명 규칙의 정합성을 강화하기 위해 verb를 카테고리별로 분류한다. 새 메서드를 추가할 때 적합한 카테고리를 선택하고, 화이트리스트 밖이라면 PR description에서 정당화한다 (가벼운 ADR — 별도 파일 불필요).

### Read (idempotent, 부작용 없음)
- `list`: 컬렉션 조회, 항상 array 반환 (현재 등록: `surface.list`, `workspace.list`, `tab.list`, `pane.list`, `plugin.list`, `window.list`, `tool.clipboard.list`, `notification.list`, `hook.list`, `global_hook.list` — 10개)
- `info`: 단일 객체 상세 조회 (id 필요) — `system.info`
- `state`: 비-구조화 상태 스냅샷 (idle/active 등)
- `get`: 단일 값 조회 — `surface.meta_get`, `tool.clipboard.get`
- `count`: 컬렉션 수 — `message.count`
- `read`: 외부 시스템에서 데이터 읽기 — `surface.read_since_mark`, `message.read`

### Write (생성/변경)
- `create`: 새 객체 생성, id 자동 부여 또는 명시 인자
- `update`: 기존 객체 부분 변경 (id 필요)
- `set`: 단일 필드 값 지정 (idempotent)
- `unset`: set의 역, 필드 제거
- `move`: 객체 위치/소속 변경
- `close`: 객체 닫기 (소프트 — 사용자 복원 가능)
- `clear`: 컬렉션 비우기
- `remove`: 단일 항목 제거 (closed_items에 안 남음)
- `destroy`: 영구 삭제. 현재 사용처 없음, 예약

### Send/외부 영향
- `send`: 외부 시스템(터미널/소켓)으로 데이터 송신
- `paste`: 클립보드에서 외부로 (tool.clipboard.paste)
- `wait`: 조건 충족까지 차단/대기 (`claude.wait`)
- `wake`: 다른 컴포넌트에 신호 (`surface.wake`)

### 프로세스/세션
- `spawn`: 새 프로세스/세션 시작 (`claude.spawn`)
- `launch`: 매니페스트 기반 실행 (`claude.launch`)
- `kill`: 강제 종료 (`claude.kill`)
- `respawn`: 종료 후 재시작 (`claude.respawn`)
- `shutdown`: 호스트 종료

### 권한/관리
- `install` / `enable` / `disable` / `permissions` / `grant` / `revoke` — plugin 관리 전용 (local-only)

### 도메인 예외 verb (1회 등장, 도메인 특수)

다음 verb는 표준이 아닌 도메인 특수 표현이다. 새 영역에서 채택할 때는 표준 verb로 우선 검토:

| 도메인 예외 | 의미 | 채택 시 검토할 표준 |
|-------------|------|---------------------|
| `tell` (claude.tell) | 한 줄 메시지 전달 (side-effect 있는 호출) | `send` |
| `broadcast` (claude.broadcast) | 다수에 동시 전송 | `send` (대상 = 다수) |
| `wake` (surface.wake) | 잠든 surface 깨우기 | `send` 또는 신규 verb |
| `parent` / `children` (claude.*) | 트리 탐색 | `info` (relation 필드) |
| `screen_text`, `screen_attrs`, `cell_info`, `cursor_position`, `is_typing`, `glyph_color` | 터미널 내부 상태 조회 | `read` 또는 `state` |
| `feed_bytes`, `inject_mouse`, `inject_key`, `raw_key`, `send_key`, `send_combo` | 사용자 입력 재현 | (debug 전용, 명명 자유로움) |
| `fire_hook` | hook 트리거 강제 | (sample size 1, 일관 규칙 정착 보류) |
| `clipboard_viewer_open` | UI 창 열기 | `window.create` 등 분리 검토 |

## modifier 패턴

`<verb>_<modifier>` 형태로 verb의 변종을 표현. 예:

- `send` / `send_key` / `send_combo` / `send_to` / `send_wait_idle` — send 계열 5개
- `set_idle_state`, `set_needs_input` — set 계열 modifier
- `read_since_mark` — read 계열 modifier

**휴리스틱**: modifier가 한 verb에 5개 이상 누적되면 namespace 한 단계 분리를 검토한다. 예: `surface.send_*`(5개)는 차후 `surface.input.{send,key,combo,wait_idle}` 같은 재구조화 후보.

단, 휴리스틱은 시점에 따라 적용한다. 현재 `surface.send_*` 5개는 1.0 freeze 전까지 유지, 1.0 직전 재검토.

## 명사 규칙

- namespace는 **단수형** (`surface`, `tab`, NOT `surfaces`)
- list 반환값의 키는 단수형 + s (`surfaces: [...]`)
- 보조 도메인은 `<namespace>.<sub>.<verb>` 3단으로. 예: 클립보드는 `tool.clipboard.*` (현재 패턴)
- `surface.meta_*` (현재 underscore 합성) → `surface.meta.*`로 alias 전환 (3-3 참조)

## 인자 규칙

- 대상 식별: 항상 `--<namespace> <id>` (예: `--surface 42`, `--tab 7`)
- **활성 객체 의존 금지** (포커스 독립성 원칙, `CLAUDE.md` 참조)
- 옵션은 kebab-case (`--strip-ansi`, `--since-mark`)

## CLI vs IPC의 차이

`src/cli/dynamic.rs`의 plugin CLI 빌더는 **top/sub 2단 구조만** 지원한다. 따라서 plugin이 `surface.meta.set`을 노출하려면 `tasty <plugin> meta.set` 또는 `tasty <plugin> meta-set` 같은 2단으로 매핑한다. 호스트 본체 CLI(`src/cli/request.rs`)는 3단 구조 직접 빌드 가능.

## Plugin이 점유하는 namespace의 규칙

plugin이 매니페스트에서 contribute하는 IPC namespace는 다음 호스트 예약어와 충돌 금지:

```
system, surface, tab, pane, workspace, claude, plugin, hook, global_hook,
message, tool, notification, window, debug, ui, ime, split, tree
```

(상세는 `docs/dev-guide/plugin-development.md` "매니페스트 검증 / 예약 prefix")

## 변경 절차

- **새 verb/namespace 추가**: PR description에 "이 verb/namespace가 위 화이트리스트 어디에 속하는지, 또는 왜 예외인지" 한 문단. 별도 ADR 파일 불필요.
- **이름 변경 (rename)**: alias map(`src/ipc/...`)에서 old → new 매핑 + `CHANGELOG.md` Deprecated 절. 옛 이름은 1.0 tag 직전 제거.
- **메서드 제거**: minor 버전에서는 deprecated 표시만, 실제 제거는 major.

자세한 break 분류·deprecation 절차는 (예정) `docs/dev-guide/ipc-stability.md` 참조.
