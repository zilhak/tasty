# 수정 후 자체 검증 (Self-Verification)

수정이 직접 확인 가능한 종류라면, **사용자에게 "확인해 보세요" 라고 떠넘기지 않고 본인이 즉시 확인한다.** 그리고 **커밋 전에** 확인한다.

커밋해두고 사용자한테 검증을 떠넘기면, 회귀가 발견될 때마다 또 수정 → 또 커밋 → 또 검증 요청 사이클이 반복된다. 커밋 히스토리에 "feat → fix → fix" 가 쌓이고, 사용자는 매번 "동작 안 함" 을 보고해야 한다. 이건 사용자의 시간을 낭비하는 거다.

## 원칙

1. **검증 가능 여부를 먼저 판단.** 수정한 동작이 IPC/CLI/스크립트로 트리거 가능하면 검증 가능. GUI 마우스 hover 같은 정말로 사용자 입력이 있어야만 재현되는 케이스만 사용자에게 부탁할 수 있다.
2. **검증은 커밋 전에 수행.** 빌드 통과 + 단위 테스트 통과는 코드가 컴파일된다는 의미일 뿐, **기능이 실제 동작한다는 보장이 아니다.** 별도의 시나리오 재현이 필요하다.
3. **확인 안 됐으면 "확인 안 됨" 이라고 말한다.** 추측으로 "동작할 거예요" 라고 보고하지 않는다.

## Tasty 에서 직접 검증하는 방법

대부분의 동작은 tasty CLI 로 시나리오를 만들 수 있다.

### 백그라운드로 띄우고 CLI 로 조작

```bash
# 빌드 + 백그라운드 실행
cargo run &              # 또는 run_in_background

# IPC 가 뜰 때까지 대기 (sleep 루프 금지 — until 로 조건 검사)
until target/debug/tasty list info 2>/dev/null; do sleep 1; done

# 상태 조회
target/debug/tasty list surfaces
target/debug/tasty list tree

# 시나리오 조작
target/debug/tasty send text "echo HELLO\r" --surface 2
target/debug/tasty surface-meta set --surface 2 --key foo --value bar

# 결과 확인
target/debug/tasty read screen --surface 2

# 종료
pkill -f "target/debug/tasty\$"
```

### 자주 쓰는 시나리오 패턴

- **PTY 입력/출력 검증**: `send text` → `read screen` 으로 echo / 명령 결과 확인.
- **레이아웃 저장/복원**: 메타 설정 후 `set workspace --subtitle X` 같은 dirty 트리거 발생 → `~/.tasty/layout.json` 직접 확인 → kill → 재시작 → `read screen` 으로 복원 결과 확인.
- **Surface meta 동작**: `surface-meta set/get/list` 로 키-값을 직접 확인. `~/.tasty/` (또는 `$TMPDIR/tasty-surfaces/`) 의 meta.json 파일도 직접 읽을 수 있다.
- **Hook/플러그인 동작**: `tasty list hooks`, `tasty plugin list` 로 등록 상태 확인. 호출 결과는 plugin 로그 (`~/.tasty/plugins-logs/`) 에서 본다.
- **레이아웃 트리 변형**: split/close/new → `list tree` 로 트리 구조 변화 확인.

### GUI 시각 검증이 필요한 경우

스크린샷이 필요한 변경 (색상, 레이아웃 정렬, 폰트 등) 은 `docs/ai-verification/visual-verification.md` 와 `docs/ai-verification/screenshot-methods.md` 를 따른다. CLI 만으로는 색상 대비 같은 시각적 회귀를 잡지 못한다.

## 안티패턴

- ❌ "빌드 통과했어요. 확인해 주세요." → 빌드는 검증이 아니다.
- ❌ "단위 테스트 545개 통과. 커밋했어요." → 테스트가 cover 못 하는 통합 동작이 있을 수 있다.
- ❌ "동작할 것으로 보입니다. push 할까요?" → "보입니다" 가 아니라 직접 돌려본 결과를 보고한다.
- ❌ 한 번에 여러 수정을 커밋한 뒤 한꺼번에 검증 → 회귀가 어느 커밋에서 들어왔는지 추적 비용 증가.

## 패턴

- ✅ 수정 → 빌드 → 단위 테스트 → **시나리오 재현** → 결과 보고 → 커밋.
- ✅ "재현 시나리오를 만들 수 없어서 확인 못 했습니다" 를 인정. 그러면 사용자가 검증을 도와줄 수 있다.
- ✅ 회귀가 의심되는 부분은 수정 전후 동일 시나리오를 두 번 돌려 비교한다.

## 관련 문서

- `docs/dev-guide/tui-testing.md` — 터미널 동작 회귀 테스트
- `docs/dev-guide/debug-ipc.md` — debug 빌드 전용 IPC (사용자 입력 재현)
- `docs/ai-verification/` — 시각 검증 가이드
