# 파일 핸들러 시스템

Tasty 는 경로/URI 입력을 두 단계로 라우팅한다:

1. **형식 식별** — `FileFormatRegistry` 가 detector rule (확장자/glob/디렉토리 등)을 평가해 `DetectorId` 를 결정.
2. **핸들러 디스패치** — `FileHandlerRegistry` 에서 그 detector 에 attach 된 핸들러 중 하나를 실행 (자동 또는 사용자 선택).

두 registry 는 **host default + plugin contribute + user TOML** 세 출처를 통합 보관한다. plugin 을 disable/uninstall 해도 host/user 항목은 그대로 남는다.

> **현재 상태:** cheap path (확장자/glob/is-directory) + deep path (magic bytes / MIME / Lua) 평가 가능. `structure_check` 는 Phase D MD2 에서 구현 예정. mouse.rs 콜사이트 (Ctrl+click 시 picker 자동 표시) 변경은 별도 작업 — 현재는 기존 `terminal_link::open_uri` 가 그대로 동작한다.

---

## 1. 형식 식별 (Detector)

### DetectorId 네임스페이스

| 패턴 | 예시 | 비고 |
|------|------|------|
| `[a-z0-9-]{1,64}` | `markdown`, `pdf`, `dockerfile` | 일반 — host/plugin/user 모두 사용 |
| `$<word>` | `$directory` | 호스트 예약. plugin 추가 시 reject |

`$unknown` 은 detector 가 아니라 *식별 실패 sentinel* 이다 — `identify()` 가 매칭에 실패하면 `None` 을 반환한다.

### Rule 종류

```toml
[[detector]]
id = "markdown"
disabled = false
display_name_i18n_key = "file_format.markdown"
[[detector.rule]]
kind = "extension"
values = ["md", "markdown"]
[[detector.rule]]
kind = "path_glob"
pattern = "README.*"
[[detector.rule]]
kind = "magic"
offset = 0
bytes_hex = "255044462D"  # %PDF- — Phase B 이상에서 평가
```

| kind | 필드 | Phase A 평가 |
|------|------|-------------|
| `extension` | `values: string[]` (대소문자 무시) | ✅ |
| `path_glob` | `pattern: string` (`*` wildcard 만) | ✅ |
| `is_directory` | (없음) | ✅ |
| `magic` | `offset: int`, `bytes_hex: hex string` (대소문자 무관, 짝수 길이) | ✅ Phase B |
| `mime` | `types: string[]` (예: `["image/png"]`, infer 추정) | ✅ Phase B |
| `lua` | `script: string` (host/user TOML 만) | ✅ Phase D MD1 |
| `structure_check` | `spec: string` | ⏳ Phase D MD2 |

### Pre-filter

- **디렉토리** 대상은 `is_directory` rule 가진 detector 만 평가.
- **파일** 대상은 그 외 detector 만 평가.

cross-match (디렉토리에 markdown 매칭 등) 방지.

### Lua rule (host/user 전용)

```toml
[[detector.rule]]
kind = "lua"
script = """
if target.has_prefix("%PDF") then return true end
return false
"""
```

`script` 는 인라인 Lua 5.4 코드. 평가 시 sandbox VM 에 다음 글로벌이 주입된다:

| 필드 | 타입 | 비고 |
|------|------|------|
| `target.path` | string | 파일 시스템 표시 경로 |
| `target.is_directory` | bool | 디렉토리 여부 |
| `target.bytes_head` | string | 최대 8KB head bytes (regular file 만, 그 외 nil) |
| `target.mime` | string | `infer` 가 추정한 MIME (없으면 nil) |
| `target.has_prefix(prefix)` | function | `bytes_head` 가 `prefix` 로 시작하는지 |

스크립트는 boolean 을 리턴해야 한다. 그 외 타입은 false 로 처리되고 warn 로그.

**Sandbox 제약**:
- 메모리 cap **8 MB** — 큰 string/table 폭발 차단.
- 명령어 cap **1,000,000** — 무한 루프 abort.
- 위험 글로벌 제거: `io`, `os`, `debug`, `package`, `require`, `dofile`, `loadfile`, `load`, `loadstring`. `string`, `math`, `table` 은 사용 가능.
- bytecode 청크 금지 (텍스트 전용).

**Plugin 출처 금지**: plugin TOML 의 `kind = "lua"` rule 은 install 단계에서 drop 되고 warn. 신뢰 영역은 host/user 만이다 (사용자가 자기 머신에 직접 적은 스크립트 = 사용자 권한). Lua 와 다른 rule 이 섞인 detector 는 Lua 만 떨어져 나가고 나머지는 정상 install.

---

## 2. 핸들러 디스패치 (Handler)

### HandlerId 형식

| 출처 | 형식 | 예시 |
|------|------|------|
| host | `host/<short>` | `host/markdown-viewer` |
| plugin | `<plugin_id>/<short>` | `com.tasty.image/viewer` |
| user | `user/<short>` | `user/my-pdf-opener` |

`<short>` 는 `[a-z0-9-]{1,32}`. 슬래시 추가 금지.

### HandlerAction 종류

```toml
[[handler]]
id = "host/markdown-viewer"
detector = "markdown"
priority = 50
display_name_i18n_key = "file_handler.host.markdown-viewer"
disabled = false
[handler.action]
kind = "open_surface"
surface_kind = "markdown"
param_key = "path"
```

| kind | 필드 | 의미 | actor 제약 |
|------|------|------|-----------|
| `open_surface` | `surface_kind`, `param_key` | 포커스 pane 에 surface 추가 | host / plugin / user |
| `ipc` | `method` (plugin 의 경우 `<plugin_id>.*` 강제) | 플러그인 IPC 호출 | host / plugin / user |
| `system` | (없음) | OS 기본 열기 (Finder/Explorer/xdg-open) | host / user 만, **plugin 불가** |

> Plugin 이 `system` 을 쓰면 sandbox 일관성이 깨지므로 serde 단계에서 reject.

### 정렬

`handlers_for(detector)` 가 반환하는 enabled handler 의 순서:

1. **priority asc** — 낮을수록 먼저 (높을수록 substitute candidate).
2. tie → owner: **user > plugin > host**.
3. 더 tie → handler id 사전순.

자동 디스패치 시 첫 항목을 선택. 사용자 picker 가 열리면 모든 후보를 보여준다.

---

## 3. User 설정

`~/.tasty/file-handlers.toml` 한 파일에 두 섹션 혼재 가능.

```toml
# ── 새 detector ──────────────────────────────────────
[[detector]]
id = "pdf"
[[detector.rule]]
kind = "extension"
values = ["pdf"]

# ── 기존 detector 의 priority 만 override ─────────────
# (id 가 host/plugin 과 같으면 patch — last-writer-wins)
[[handler]]
id = "host/markdown-viewer"
priority = 10

# ── 직접 system opener 등록 ──────────────────────────
[[handler]]
id = "user/pdf-preview"
detector = "pdf"
priority = 30
[handler.action]
kind = "system"
```

규칙:
- 파일 없음 → 정상 (사용자 설정 없는 상태).
- parse 실패 → warn 로그 + 사용자 설정 전체 무시 (host + plugin 만으로 동작).
- entry 단위 schema 오류 → 그 entry 만 reject, 나머지 적용.
- `disabled = true` 만 override 한다. `false` 명시는 무시 (다른 출처의 disabled 를 되살리려면 명시값 무관).

---

## 4. Plugin contribution

### 매니페스트

```toml
permissions = [
  "file_handler.define",            # 새 detector + handler 추가
  "file_handler.extend:markdown",   # 기존 detector 에 rule 추가
  "file_handler.handle:pdf",        # 기존 detector 에 handler 만 추가
]

[[contributes.detector]]
id = "csv"
[[contributes.detector.rule]]
kind = "extension"
values = ["csv", "tsv"]

[[contributes.handler]]
id = "com.example.csv/viewer"
detector = "csv"
priority = 40
[contributes.handler.action]
kind = "open_surface"
surface_kind = "csv-viewer"
param_key = "path"
```

검증 단계:
- detector id 가 `$` 로 시작하면 plugin 은 reject (예약 sentinel).
- `file_handler.handle:<id>` 권한이 있어야 그 detector 에 handler attach 가능.
- handler id 의 prefix segment 는 plugin id 와 일치해야 함 (`com.example.csv/...`).
- `surface_kind` 는 plugin 이 등록한 것만 (또는 host 가 미리 알려진 것), `ipc.method` 는 plugin 의 namespace prefix.

---

## 5. Picker popup

자동 디스패치가 적합하지 않을 때 (여러 후보가 동순위, 또는 사용자가 명시적으로 선택을 원할 때) `file_handler_picker` PopupDef 가 열린다.

레이아웃:

```
┌─ 파일 핸들러 선택: .../foo.md ──────────────────┐
│ 대상: /path/to/foo.md                            │
│ 형식: markdown                                   │
├─── 후보 ──────────────┬──── 최근 선택 ──────────┤
│ ▸ host/markdown-viewer│ ▸ user/my-md-handler    │
│   user/my-md-handler  │   host/image-viewer     │
│   ...                 │   ...                   │
├───────────────────────┴─────────────────────────┤
│                            [ 취소 ] [ 열기 ]    │
└─────────────────────────────────────────────────┘
```

- **좌측 후보**: handler id 사전순 (deterministic).
- **우측 최근**: `~/.tasty/file-handler-recent.json` LRU(cap 10) 순서, 현재 등록 안 된 id 는 표시 제외 (저장 파일은 유지).
- **더블클릭 또는 [열기]** → Selected dispatch + recent 기록.
- **[취소] / ESC / X** → Cancelled, recent 갱신 없음.
- 빈 상태 (handler 0개) → empty-state 메시지 + [취소] 만.

i18n 키 prefix: `file_handler.picker.*`.

---

## 6. 사용자 설정 reload

`~/.tasty/file-handlers.toml` 을 편집한 뒤 reload 하려면:

```
tasty file-handler reload
```

또는 IPC 로 `file_handler.reload` 호출 (local-only — plugin 노출 안 됨).

응답:

```json
{ "path": "/home/.../file-handlers.toml", "exists": true }
```

특징:
- **Transactional**: 새 파일 parse 가 실패하면 기존 user 설정 그대로 보존 (warn 로그만 남김).
- 파일이 없으면 user origin 항목 전부 제거 (host + plugin 만 남음).
- **host / plugin contribution 은 영향 없음** — user 출처 항목만 swap.

## 7. 트리거

같은 dispatch 흐름이 들어오는 진입점:

| trigger | 진입 함수 | 비고 |
|---------|----------|------|
| 터미널의 hyperlink ctrl+click | `mouse.rs` → `parse_link` → `dispatch_file_target(Deep)` | `file://` 만 디스패치, http/mailto 등은 webbrowser 위임 |
| 외부 → Tasty drag&drop | winit `WindowEvent::DroppedFile` → `dispatch_file_target(Deep)` | hover 중 시각 overlay 표시, 다중 파일은 각각 dispatch |
| explorer plugin 더블클릭 | tree node `double_clicked()` → `UiEvent::TreeActivate` → plugin `host.call("file_handler.dispatch")` → host `handle_dispatch` → `dispatch_file_target(Deep)` | 디렉토리는 plugin 내부 root 변경, 파일만 host 로 디스패치 |

drag&drop 좌표는 마지막 cursor 위치 기준. 터미널 영역 밖으로 드롭한 파일은
toast 안내 후 무시된다. 다중 파일을 한 번에 드롭하면 각 파일이 별 surface tab
으로 열린다.

> **에이전트 주의**: drag&drop 은 사용자 입력 재현 동작이므로 IPC/CLI 로
> 노출되지 않는다. 같은 효과가 필요하면 `tasty surface new ...` 류 명령으로
> 직접 surface 를 만들 것.

## 8. 디버깅

| 확인 사항 | 방법 |
|-----------|------|
| host default 등록 결과 | `cargo test --bin tasty file_format::registry::tests` |
| user TOML 파싱 | `~/.tasty/file-handlers.toml` 작성 후 부팅, 로그에서 `file_handler:` / `file_format:` warn 확인 |
| plugin contribute | plugin enable/disable 후 `tasty plugin list` + registry 호출 |
| recent picks 파일 | `~/.tasty/file-handler-recent.json` (cap 10, JSON pretty) |

저장 경로:
- Linux/macOS: `~/.tasty/file-handlers.toml`, `~/.tasty/file-handler-recent.json`
- Windows: `%USERPROFILE%\.tasty\file-handlers.toml`, `...\file-handler-recent.json`
