# Split 명령어 설계

## 단일 `split` 명령

IPC와 CLI 모두 하나의 `split` 명령으로 상위/하위 레이아웃 분할을 통합한다.

### IPC

```json
{
  "method": "split",
  "params": {
    "level": "surface",
    "target_surface": "build-server",
    "direction": "vertical",
    "type": "terminal",
    "meta": {"nickname": "logs"}
  }
}
```

### CLI

```bash
tasty split --level surface --target-surface this --direction vertical --meta '{"nickname":"logs"}'
tasty split --level pane --target-pane 2 --direction horizontal
tasty split --level pane --target-surface this --direction vertical --type markdown --file /path/to/doc.md
```

## 파라미터

| 파라미터 | 필수 | 타입 | 설명 |
|----------|------|------|------|
| `level` | yes | `"pane"` \| `"surface"` | 분할 레벨 |
| `target_surface` | * | string/number | 대상 surface ID, "this", 또는 nickname |
| `target_pane` | * | number | 대상 pane ID (pane level만) |
| `direction` | no | `"vertical"` \| `"horizontal"` | 분할 방향. 기본값 `"vertical"` |
| `type` | no | `"terminal"` \| `"markdown"` \| `"explorer"` \| `"html"` | surface 타입. 기본값 `"terminal"` |
| `file` | markdown시 필수 | string | 마크다운 파일 경로 |
| `path` | no | string | explorer 루트 경로 |
| `url` | html시 필수 | string | HTML URL |
| `cwd` | no | string | 터미널 작업 디렉토리 |
| `meta` | no | JSON object | 새 surface에 설정할 메타데이터 |

\* `target_surface`와 `target_pane` 중 하나는 반드시 지정해야 한다. 둘 다 지정하면 에러.

### target 해석 규칙

#### target_surface

| 형태 | 해석 |
|------|------|
| 숫자 (`9`) | surface ID로 직접 사용 |
| `"this"` | CLI에서 `TASTY_SURFACE_ID` 환경변수로 해석 (자기 자신의 surface) |
| 문자열 (`"build-server"`) | surface_meta의 `nickname` 키로 검색 |

#### target_pane

| 형태 | 해석 |
|------|------|
| 숫자 (`3`) | pane ID로 직접 사용 |

### level별 target 조합

| level | target_surface | target_pane | 동작 |
|-------|---------------|-------------|------|
| pane | ✅ | - | surface가 속한 pane을 찾아 해당 pane 옆에 분할 |
| pane | - | ✅ | 지정한 pane 옆에 분할 |
| surface | ✅ | - | 지정한 surface 내부를 분할 |
| surface | - | ✅ | **에러** (surface 분할에 pane 지정 불가) |

ID는 전역 고유하므로, target이 주어지면 **모든 workspace를 검색**하여 대상을 찾는다.

### 하위 호환: `target` (deprecated)

기존 `target` 파라미터는 `target_surface`와 동일하게 동작한다. CLI에서는 `--target` (숨겨진 옵션). 새 코드에서는 `--target-surface` 또는 `--target-pane`을 사용할 것.

### TASTY_SURFACE_ID 환경변수

각 surface의 셸 프로세스는 PTY 생성 시 `TASTY_SURFACE_ID` 환경변수를 자동으로 받는다. CLI에서 `--target-surface this`를 사용하면 이 값을 읽어 자신의 surface ID로 해석한다.

### meta 파라미터

새로 생성된 surface에 key-value 메타데이터를 설정한다. JSON 객체로 전달하며, 각 key-value가 `surface.meta_set`으로 저장된다.

주요 용도:
- `nickname`: surface를 이름으로 참조할 수 있게 함
- 커스텀 태그: AI 에이전트가 surface를 분류/추적하는 데 사용

## 포커스 정책

**split은 포커스를 이동하지 않는다.** workspace.create, tab.create도 IPC/CLI 호출 시 포커스를 이동하지 않는다.

| 동작 | UI (키보드/클릭) | IPC/CLI |
|------|-----------------|---------|
| split | 새 영역으로 포커스 | 포커스 유지 |
| workspace 생성 | 새 workspace로 전환 | 포커스 유지 |
| tab 생성 | 새 탭으로 전환 | 포커스 유지 |

포커스 이동은 CLI/IPC로 불가능하며, 키보드 단축키 또는 마우스 클릭으로만 가능하다.

## 응답

```json
// level: "pane"
{
  "new_pane_id": 5,
  "new_surface_id": 8
}

// level: "surface"
{
  "new_surface_id": 8
}
```

## 사용 시나리오

### 닉네임으로 모니터링 영역 생성

```bash
tasty split --level surface --target-surface this --direction vertical --meta '{"nickname":"logs"}'
tasty send text "tail -f /var/log/app.log\r" --surface logs
```

### 다른 workspace의 surface를 닉네임으로 분할

```bash
tasty split --level surface --target-surface build-server --direction horizontal
```

### 독립 탭 바 영역 생성 (pane ID 지정)

```bash
tasty split --level pane --target-pane 2 --direction horizontal
```

### surface ID로 pane 분할 (surface가 속한 pane을 자동 탐색)

```bash
tasty split --level pane --target-surface this --direction vertical --type markdown --file /path/to/board.md
```
