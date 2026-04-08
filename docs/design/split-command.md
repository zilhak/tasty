# Split 명령어 설계

## 단일 `split` 명령

IPC와 CLI 모두 하나의 `split` 명령으로 상위/하위 레이아웃 분할을 통합한다.

### IPC

```json
{
  "method": "split",
  "params": {
    "level": "surface",
    "target": "build-server",
    "direction": "vertical",
    "meta": {"nickname": "logs"}
  }
}
```

### CLI

```bash
tasty new split --level surface --target this --direction vertical --meta '{"nickname":"logs"}'
tasty new split --level pane-group --target 2 --direction horizontal
```

## 파라미터

| 파라미터 | 필수 | 타입 | 설명 |
|----------|------|------|------|
| `level` | yes | `"pane-group"` \| `"surface"` | 분할 레벨 |
| `target` | no | string | 분할 대상. 생략 시 focused 대상 |
| `direction` | no | `"vertical"` \| `"horizontal"` | 분할 방향. 기본값 `"vertical"` |
| `meta` | no | JSON object | 새 surface에 설정할 메타데이터 |

### target 해석 규칙

| 형태 | 해석 |
|------|------|
| 숫자 문자열 (`"3"`) | surface/pane ID로 직접 사용 |
| `"this"` | CLI 측에서 `TASTY_SURFACE_ID` 환경변수로 해석 (자기 자신의 surface) |
| 임의 문자열 (`"build-server"`) | surface_meta의 `nickname` 키로 검색 (surface level만) |
| 생략 | focused 대상 |

ID는 전역 고유하므로, target이 주어지면 **모든 workspace를 검색**하여 대상을 찾는다.

### TASTY_SURFACE_ID 환경변수

각 surface의 셸 프로세스는 PTY 생성 시 `TASTY_SURFACE_ID` 환경변수를 자동으로 받는다. CLI에서 `--target this`를 사용하면 이 값을 읽어 자신의 surface ID로 해석한다.

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

포커스를 옮기려면 `pane.focus` 또는 `surface.focus`를 별도로 호출한다.

## 응답

```json
// level: "pane-group"
{
  "new_pane_group_id": 5,
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
tasty new split --level surface --target this --direction vertical --meta '{"nickname":"logs"}'
tasty send-to --surface logs "tail -f /var/log/app.log"
```

### 다른 workspace의 surface를 닉네임으로 분할

```bash
tasty new split --level surface --target build-server --direction horizontal
```

### 독립 탭 바 영역 생성

```bash
tasty new split --level pane-group --direction horizontal
```
