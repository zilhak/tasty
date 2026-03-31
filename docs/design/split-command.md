# Split 명령어 설계

## 현황과 문제

현재 split 관련 명령:

| IPC | CLI | 동작 |
|-----|-----|------|
| `pane.split` | `tasty split` | focused pane group을 상위 레이아웃에서 분할 |

문제점:

1. **하위 레이아웃(surface) 분할이 없다** — 같은 탭 안에서 터미널을 나눌 수 없다
2. **대상을 지정할 수 없다** — 항상 focused 대상만 분할한다
3. **cross-workspace 동작이 안 된다** — active workspace만 대상이다
4. **split 시 포커스가 이동한다** — 새로 생긴 쪽으로 포커스가 옮겨간다

## 설계

### 단일 `split` 명령 + 명시적 파라미터

IPC와 CLI 모두 하나의 `split` 명령으로 통합한다. 어떤 레벨에서 무엇을 분할할지 파라미터로 지정한다.

#### IPC

```json
{
  "method": "split",
  "params": {
    "level": "surface",
    "target_id": 3,
    "direction": "vertical"
  }
}
```

#### CLI

```bash
tasty split --level surface --target 3 --direction vertical
tasty split --level pane-group --target 2 --direction horizontal
```

### 파라미터

| 파라미터 | 필수 | 타입 | 설명 |
|----------|------|------|------|
| `level` | yes | `"pane-group"` \| `"surface"` | 분할 레벨 |
| `target_id` | no | u32 | 분할 대상 ID. 생략 시 focused 대상 |
| `direction` | no | `"vertical"` \| `"horizontal"` | 분할 방향. 기본값 `"vertical"` |

- `level: "pane-group"` — `target_id`는 PaneGroup(Pane) ID. 상위 레이아웃에 새 PaneGroup을 추가한다.
- `level: "surface"` — `target_id`는 Surface ID. 해당 surface가 속한 탭의 하위 레이아웃에 새 surface를 추가한다.

### ID 해석 규칙

ID는 전역 고유하므로, `target_id`가 주어지면 **모든 workspace를 검색**하여 대상을 찾는다.

1. `target_id`가 주어진 경우: 전체 workspace에서 해당 ID를 검색
2. `target_id`가 생략된 경우: focused pane group 또는 focused surface를 대상으로 사용

### 포커스 정책

**split은 포커스를 이동하지 않는다.**

- 새로 생긴 pane group이나 surface에 포커스를 옮기지 않는다
- 분할 전의 focused pane group, focused surface가 분할 후에도 유지된다
- 새 영역에 포커스를 옮기려면 `pane.focus` 또는 `surface.focus`를 별도로 호출한다

이유: AI 에이전트가 분할 후 즉시 새 영역에서 작업하고 싶을 수도 있고, 기존 영역에서 계속 작업하고 싶을 수도 있다. 분할과 포커스를 분리하면 에이전트가 의도를 명시적으로 표현할 수 있다.

### 응답

분할 성공 시 새로 생성된 리소스의 ID를 반환한다:

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

AI 에이전트는 이 ID를 사용해 즉시 `surface.send_to`, `surface.focus` 등을 호출할 수 있다.

### 기존 명령과의 관계

| 기존 | 신규 대응 | 비고 |
|------|-----------|------|
| `pane.split --direction vertical` | `split --level pane-group --direction vertical` | 기존 명령은 deprecated → 제거 |
| (없음) | `split --level surface --direction vertical` | 신규 |

`pane.split` IPC 메서드와 `tasty split` CLI 명령을 새 `split` 명령으로 교체한다.

## 사용 시나리오

### AI가 모니터링 영역을 만들 때

```bash
# 현재 surface 오른쪽에 새 터미널 생성
tasty split --level surface --direction vertical
# 응답에서 new_surface_id를 받아서 명령 전송
tasty send-to --surface 8 "tail -f /var/log/app.log"
```

### AI가 독립 탭 바 영역을 만들 때

```bash
# 현재 pane group 아래에 새 pane group 생성
tasty split --level pane-group --direction horizontal
# 새 pane group으로 포커스 이동 후 탭 조작
tasty pane-focus --id 5
tasty new-tab
```

### 다른 workspace의 surface를 분할할 때

```bash
# workspace 2에 있는 surface 12를 분할
tasty split --level surface --target 12 --direction vertical
# 포커스는 현재 위치에 유지됨
```
