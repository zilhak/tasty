# 클립보드 히스토리 도구

Tasty는 시스템 클립보드 변경을 자동 기록하는 메모리 전용 히스토리를 제공한다. AI
에이전트는 CLI와 IPC 양쪽에서 조회/조작할 수 있다.

기록 동작:

- 별도 스레드가 `settings.clipboard.poll_interval_ms`(기본 500ms) 주기로 시스템
  클립보드를 읽어 변경이 있으면 기록 (source=`system`).
- Tasty 내부 복사(선택 영역 복사, OSC 52 등)는 즉시 기록 (source=`internal`).
- 연속 중복은 자동 제거.
- 재시작 시 휘발.
- 민감 정보(비밀번호 등) 필터링 없음. 민감한 작업에는 `settings.clipboard.history_enabled=false`.

## CLI

```
tasty tool clipboard list [--limit N]
tasty tool clipboard get --index 0
tasty tool clipboard paste --index 2
tasty tool clipboard remove --index 5
tasty tool clipboard clear
```

`list` 출력 예시 (JSON):

```json
{
  "total": 3,
  "entries": [
    { "index": 0, "text": "Hello", "source": "system",   "age_ms": 1234 },
    { "index": 1, "text": "def foo", "source": "internal", "age_ms": 45000 },
    { "index": 2, "text": "https://...", "source": "system", "age_ms": 3600000 }
  ]
}
```

- `index 0 = 최신`. 내림차순.
- `paste`: 해당 항목을 시스템 클립보드에 다시 써넣음(+ 내부 기록으로 재등록).

## IPC (JSON-RPC)

| method | params | 설명 |
|---|---|---|
| `tool.clipboard.list` | `{ "limit"?: number }` | 최신 순 N개 반환 |
| `tool.clipboard.get` | `{ "index": number }` | 단건 조회 |
| `tool.clipboard.paste` | `{ "index": number }` | 시스템 클립보드에 재기입 |
| `tool.clipboard.remove` | `{ "index": number }` | 단건 삭제 |
| `tool.clipboard.clear` | `{}` | 전체 삭제 |

인덱스 범위 초과 시 `invalid_params` 에러.

> **참고**: 사용자가 보는 클립보드 viewer popup 은 빌트인
> `com.tasty.clipboard-history` plugin 이 contribute 한 popup 이다. 단축키
> `toggle_clipboard_viewer` (기본 `Ctrl+Shift+H`) 로 토글하며, IPC 로 viewer 를
> 여는 release API 는 제공되지 않는다. 에이전트는 위 `tool.clipboard.*` API 로
> 직접 history 를 다루면 된다. Viewer 내부에서 사용자는 항목 클릭 → paste,
> × 버튼 → remove, 헤더 "Clear all" → clear 를 트리거할 수 있다 — 셋 다
> `tool.clipboard.*` IPC 의 thin wrapper.

## 포커스 독립성

- 모든 `tool.clipboard.*` 명령은 활성 워크스페이스/탭/서피스 포커스에 **의존하지
  않는다**. Window가 여러 개여도 history는 각 Window의 EngineState에 독립 저장되며,
  IPC가 라우팅되는 Window의 history를 대상으로 동작한다.

## 수동 회귀 테스트 (Viewer)

GUI 자동화 테스트는 없으므로 plugin viewer 변경 시 다음 시나리오를 수동으로 확인한다.

1. **복사 → viewer → paste 골든 경로**
   - 외부 앱에서 텍스트 A 복사 → 잠시 대기 (poll_interval_ms 만큼)
   - `Ctrl+Shift+H` 로 viewer 열기 → 항목 A 가 최상단에 보이는지 확인
   - 항목 A 클릭 → popup 자동 종료 + 시스템 클립보드에 A 가 다시 들어갔는지 (다른 앱에서 paste) 확인
2. **단일 항목 제거 (×)**
   - 2 개 이상 항목이 있는 상태에서 viewer 열기
   - 한 항목 옆 × 클릭 → popup 닫히지 않고 해당 항목만 사라지는지
   - `tasty tool clipboard list --json` 으로 동일하게 1 개 줄어들었는지
3. **전체 비우기 (Clear all)**
   - viewer 열기 → 헤더 "Clear all" 클릭
   - popup 이 "No clipboard entries yet" 으로 바뀌는지 + `list --json` total=0
4. **중복 인스턴스 가드**
   - viewer 열린 상태에서 `Ctrl+Shift+H` 다시 누르기 → 두 번째 popup 은
     "Clipboard viewer is already open" placeholder 만 표시되는지
5. **빈 상태**
   - history 가 비었을 때 viewer 열기 → "No clipboard entries yet" 표시
