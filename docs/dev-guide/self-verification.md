# 수정 후 자체 검증 (Self-verification)

수정이 직접 확인 가능한 종류라면 **사용자에게 "확인해 보세요" 라고 떠넘기지 않고 본인이, 커밋 전에 확인한다.**

커밋해두고 검증을 떠넘기면 회귀가 나올 때마다 수정→커밋→재검증 사이클이 반복되고 히스토리에 "feat→fix→fix" 가 쌓인다. 사용자의 시간을 낭비하는 일이다.

## 원칙

1. **검증 가능 여부를 먼저 판단.** IPC/CLI/스크립트로 트리거 가능하면 검증 가능. 마우스 hover 같은 *사용자 입력이 있어야만 재현되는* 케이스만 사용자에게 부탁한다.
2. **검증은 커밋 전.** 빌드 통과 + 단위 테스트 통과는 *컴파일된다* 는 의미일 뿐 *기능이 동작한다* 는 보장이 아니다 — 별도 시나리오 재현이 필요.
3. **확인 안 됐으면 "확인 안 됨" 이라고 말한다.** 추측으로 "동작할 거예요" 보고 금지.

## tasty 에서 직접 검증

대부분의 동작은 tasty CLI 로 시나리오를 만들 수 있다.

```bash
cargo run &                                              # 백그라운드 실행
until target/debug/tasty list info 2>/dev/null; do sleep 1; done  # IPC 대기 (sleep 루프 금지, until 조건검사)

target/debug/tasty list surfaces                         # 상태 조회
target/debug/tasty list tree
target/debug/tasty send text "echo HELLO\r" --surface 2  # 시나리오 조작
target/debug/tasty read screen --surface 2               # 결과 확인
pkill -f "target/debug/tasty\$"                          # 종료
```

### 자주 쓰는 시나리오

- **PTY 입출력**: `send text` → `read screen` 으로 echo/명령 결과 확인.
- **레이아웃 저장/복원**: dirty 트리거 발생 → `~/.tasty/layout.json` 확인 → kill → 재시작 → `read screen` 으로 복원 확인.
- **Surface meta**: `surface-meta set/get/list` 로 키-값 확인.
- **Hook/플러그인**: `tasty list hooks` · `tasty plugin list` 로 등록 상태, 호출 결과는 plugin 로그(`~/.tasty/plugins-logs/`).
- **레이아웃 트리 변형**: split/close/new → `list tree` 로 구조 변화 확인.

### debug 전용 IPC 로만 가능한 검증

사용자 입력 재현(키/마우스 주입, popup 강제 open/close, 도구 메뉴 클릭)이나 렌더 셀 덤프는 release 표면에 없다 — debug 빌드의 `debug.*` 로 구동한다. [debug-ipc.md](debug-ipc.md) 참조.

### GUI 시각 검증

색상·정렬·폰트처럼 스크린샷이 필요한 변경은 CLI 만으로 잡지 못한다 — `ai-verification/` *(재작성 예정)* 의 visual-verification 체크리스트를 따른다.

## 안티패턴 / 패턴

- ❌ "빌드 통과했어요, 확인해 주세요" — 빌드는 검증이 아니다.
- ❌ "테스트 545개 통과, 커밋했어요" — 테스트가 cover 못 하는 통합 동작이 있다.
- ❌ "동작할 것으로 보입니다" — 직접 돌려본 결과를 보고한다.
- ✅ 수정 → 빌드 → 단위 테스트 → **시나리오 재현** → 결과 보고 → 커밋.
- ✅ "재현 시나리오를 못 만들어 확인 못 했습니다" 를 인정하면 사용자가 검증을 도울 수 있다.

## 관련

- [debug-ipc.md](debug-ipc.md) — debug 전용 IPC (사용자 입력 재현)
- [independent-verification.md](independent-verification.md) — debug 격리 + 자기검증 배경
- `ai-verification/` *(재작성 예정)* — 시각 검증
