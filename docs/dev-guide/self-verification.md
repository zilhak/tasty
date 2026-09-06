# 수정 후 자체 검증 (Self-verification)

수정이 직접 확인 가능한 종류라면 **사용자에게 "확인해 보세요" 라고 떠넘기지 않고 본인이, 커밋 전에 확인한다.**

커밋해두고 검증을 떠넘기면 회귀가 나올 때마다 수정→커밋→재검증 사이클이 반복되고 히스토리에 "feat→fix→fix" 가 쌓인다. 사용자의 시간을 낭비하는 일이다.

## 원칙

1. **검증 가능 여부를 먼저 판단.** IPC/CLI/스크립트로 트리거 가능하면 검증 가능. 마우스 hover 같은 *사용자 입력이 있어야만 재현되는* 케이스만 사용자에게 부탁한다.
2. **검증은 커밋 전.** 빌드 통과 + 단위 테스트 통과는 *컴파일된다* 는 의미일 뿐 *기능이 동작한다* 는 보장이 아니다 — 별도 시나리오 재현이 필요.
3. **확인 안 됐으면 "확인 안 됨" 이라고 말한다.** 추측으로 "동작할 거예요" 보고 금지.
4. **검증 인스턴스는 자동 격리된다.** debug 빌드(`cargo run` / `target/debug/tasty`)는 `~/.tasty-debug/` 루트를, release 사용자 세션은 `~/.tasty/` 를 쓴다 — 따로 끄거나 환경변수를 줄 필요 없이 `cargo run` 만으로 충돌 없는 검증이 보장된다. (상세 [independent-verification.md](independent-verification.md))

## tasty 에서 직접 검증

대부분의 동작은 tasty CLI 로 시나리오를 만들 수 있다.

```bash
cargo build                                              # 먼저 빌드한다 — `cargo run &` 을 쓰면
                                                         # $! 가 cargo 의 PID 라 kill 이 앱에 닿지 않는다
target/debug/tasty --launch &                            # 백그라운드 실행 (--launch 는 아래 문단 참고)
MY_APP=$!                                                # 띄운 즉시 PID 를 잡는다
until target/debug/tasty list info 2>/dev/null; do       # IPC 대기 (sleep 루프 금지, until 조건검사)
  kill -0 "$MY_APP" 2>/dev/null || { echo "기동 실패"; break; }   # 죽은 프로세스를 무한정 기다리지 않는다
  sleep 1
done

target/debug/tasty list surfaces                         # 상태 조회
target/debug/tasty list tree
target/debug/tasty send text "echo HELLO\r" --surface 2  # 시나리오 조작
target/debug/tasty read screen --surface 2               # 결과 확인
kill "$MY_APP"                                           # 종료 — 저장한 PID 로만
```

**`cargo test` 는 `target/debug/tasty` 를 만들지 않는다 — 라이브로 재기 전에 빌드 시각을 본다.**
`cargo test --bin tasty` 가 만드는 것은 **테스트 하네스**(`target/debug/deps/tasty-<해시>`)이고
`--bin tasty` 라는 이름 때문에 실행 바이너리도 함께 갱신된 것처럼 보인다. 갱신되지 않는다.
그래서 코드를 고치고 테스트만 돌린 뒤 위 절차로 인스턴스를 띄우면 **직전 코드를 재게 되고,
그 오진은 양방향이다** — 고친 것이 안 고쳐진 것처럼도, 되돌린 것이 여전히 고쳐진 것처럼도
보인다. plugin 쪽에 이미 알려진 함정(`cargo build` 가 plugin 바이너리를 relink 하지 않는다,
`CLAUDE.md` "빌드")의 **본체 판**이고 성질이 같다. 라이브 프로브 전에 한 줄로 확인한다:

```bash
cargo build --locked --bin tasty                  # 라이브 검증용 바이너리는 이 명령이 만든다
ls -la --time-style=full-iso target/debug/tasty   # 방금 고친 시각보다 새것인지 눈으로 본다
```

**종료는 자기가 띄운 PID 로만 한다.** 이름이나 명령줄 패턴으로 찾아서 죽이면
**자기 것이 아닌 인스턴스까지 죽인다** — 사용자 release · 다른 검증 세션 · 병렬 lane 의
debug 인스턴스가 동시에 떠 있는 것이 이 레포의 일상이고, 실제로 그 형태가 다른 세션의
프로세스를 죽인 사고가 두 번 났다. 레포의 PreToolUse 훅도 같은 이유로 패턴 기반 프로세스
종료를 차단한다. PID 를 놓쳤을 때 소유자를 확인하는 방법은
[ai-verification/screenshot-methods](../ai-verification/screenshot-methods.md) "사용자 세션을
건드리지 않고 격리 실행".

**이미 다른 tasty 인스턴스(사용자의 release 등)가 떠 있어도 충돌하지 않는다.** `cargo run` 은 debug 빌드라 데이터 루트가 `~/.tasty-debug/`(포트파일 `~/.tasty-debug/tasty.port`)로 release 의 `~/.tasty/` 와 **완전히 분리**된다 — 포트·layout·scrollback 모두 별도. 그러니 인스턴스가 떠 있는지 따지지 말고 그냥 `cargo run` 으로 자기 debug 인스턴스를 띄워 검증한다. (격리 표·`TASTY_HOME` override: [independent-verification.md](independent-verification.md))

**`--headless` 는 기본 빌드에서 headless 로 동작하지 않는다.** 기본 빌드는 `gui` feature 가 켜져 있고 그 빌드에는 headless 모드가 들어 있지 않아, `--headless` 를 줘도 **GUI 로 폴백해 실제 창을 띄운다.** 로그에 이렇게 남는다:

```
--headless requested in gui build; gui build does not embed headless mode.
Build with --no-default-features to enable headless. Falling back to run_gui.
```

즉 `--headless` 만 믿고 "창은 안 뜬다" 고 가정하면 **세마포어 없이 공용 디스플레이에 창을 띄우게 된다**(실제로 그렇게 밟은 적이 있다). headless 검증에는 `cargo build --no-default-features` 로 만든 바이너리를 쓴다. GUI 가 떠도 되는 검증이면 폴백을 그대로 써도 되지만, 그때는 GUI 검증 규약(디스플레이 직렬화, 종료 확인)을 따른다.

**tasty 터미널 내부(`TASTY_SURFACE_ID` 환경변수가 설정된 셸)에서 검증 인스턴스를 띄울 때는 `--launch` 플래그가 필수다.** `cargo run --bin tasty -- <플래그>` 를 `--launch` 없이 실행하면 `boot.rs:111` 의 GUI 부팅 skip 조건(`TASTY_SURFACE_ID` 설정 + `--launch` 미지정)에 걸려 GUI 가 뜨지 않고 CLI 도움말만 출력한 채 조용히 종료된다 — 이 상태로 `until target/debug/tasty list info ...` 같은 readiness poll 을 돌리면 죽은 프로세스를 무한정 기다리게 된다. 즉 `cargo run &` 을 `--launch` 없이 tasty 터미널 안에서 실행했다면, poll 이 멈추지 않을 때 프로세스가 애초에 GUI 로 뜬 게 맞는지부터 의심한다.

### 자주 쓰는 시나리오

- **PTY 입출력**: `send text` → `read screen` 으로 echo/명령 결과 확인. `read screen`(`surface.screen_text`/`pty.read`)은 dim(ghost-suggestion, 예: Claude Code CLI 가 그리는 미제출 자동완성 제안) 셀을 기본 제외한다 — 제안 텍스트가 실제 입력된 것처럼 오독되는 걸 막기 위함. 제안까지 보고 싶으면 `--show-dim`.
  - `--lines N` 은 **내용 기준 마지막 N 줄**이다 — grid 하단 N 행이 아니다. 내용 아래의 공백 행은 건너뛰고, 화면 내용이 N 에 모자라면 스크롤백에서 채운다. 따라서 N 의 크기와 무관하게 의미가 같고, **살아 있는 터미널이 `--lines` 때문에 빈 결과를 내는 일은 없다.** 빈 결과가 나왔다면 실제로 출력이 없는 것이므로 "surface 가 죽었다" 로 넘어가기 전에 `--lines` 없는 전체 화면과 대조한다.
- **레이아웃 저장/복원**: dirty 트리거 발생 → 슬롯 파일 확인(debug 검증이면 `~/.tasty-debug/layouts/NN.json`) → kill → 재시작 → `read screen` 으로 복원 확인.
- **Surface meta**: `surface-meta set/get/list` 로 키-값 확인.
- **Hook/플러그인**: `tasty list hooks` · `tasty plugin list` 로 등록 상태, 호출 결과는 plugin 로그(`~/.tasty/plugins-logs/`).
- **레이아웃 트리 변형**: split/close/new → `list tree` 로 구조 변화 확인.

### debug 전용 IPC 로만 가능한 검증

사용자 입력 재현(키/마우스 주입, popup 강제 open/close, 도구 메뉴 클릭)이나 렌더 셀 덤프는 release 표면에 없다 — debug 빌드의 `debug.*` 로 구동한다. [debug-ipc.md](debug-ipc.md) 참조.

### GUI 시각 검증

색상·정렬·폰트처럼 스크린샷이 필요한 변경은 CLI 만으로 잡지 못한다 — [`ai-verification/visual-verification`](../ai-verification/visual-verification.md) 체크리스트를 따른다.

**스크린샷은 OS 화면 캡처(`screencapture` / PowerShell `CopyFromScreen` 등)를 쓰지 말고 tasty 자체 `ui.screenshot` IPC 를 쓴다.** OS 화면 캡처는 화면 녹화 권한이 필요해 *빌드할 때마다 사용자가 권한을 다시 풀어주지 않는 한 막힌다* — 자기검증 흐름이 권한 프롬프트에서 멈춘다. `ui.screenshot` 은 tasty 가 실제 렌더한 프레임을 권한 없이 PNG 로 떨구므로 자동 검증에 적합하다(다른 창 가림·포커스 상태에도 영향 없음). 호출법·격리 실행은 [`ai-verification/screenshot-methods`](../ai-verification/screenshot-methods.md). OS 캡처는 IPC 를 못 쓰는 셸 설정 모드 등에서만 폴백.

## 가드를 검증할 때 — 두 가지 침묵은 **다른 물음**이다

가드(소스 스캔·명부 재검사)를 검증하다 보면 침묵이 두 가지 모양으로 온다. 둘 다
"표적에 결함이 없다" 로 읽히는데, 실제로는 둘 다 **내 모형이 틀렸다** 인 경우가 많다.
그리고 **틀렸을 때 남는 오류의 부호가 서로 반대**라, 한 규칙으로 합치면 안 된다.

### ① 양성 대조가 **안 죽는다** → 표적이 아니라 모형부터

변이를 심었는데 가드가 초록이면, "가드가 못 잡는다" 로 적기 전에 둘을 먼저 본다.

- **그 자극이 실제로 실행됐나.** 변이가 컴파일 오류로 죽었거나, 심은 자리가 그 가드의
  모수 밖이거나, 파일이 다시 안 빌드된 경우다.
- **모형이 표적의 규칙과 맞나.** 가드가 좁게 판정하는 것이 **옳아서** 통과한 경우다.

실측 사례 셋(2026-09-07, 하루):

| 심은 곳 | 왜 안 죽었나 |
|---|---|
| `test.yml` 의 `test-linux-x64` 에 `--ignored` | 그 잡이 `workflow_dispatch` 전용이라 **자동 잡이 아니다.** 가드의 물음이 "자동 채널" 이므로 옳게 통과 |
| 본체에 `const FRAME_MAX_W: LogicalPx = LogicalPx::new(100.0)` | `LogicalPx::new` 가 없어 **컴파일이 죽었다.** 변이가 실행된 적이 없다 |
| `ci-gates.md` 의 한 절에 통과 수만 적기 | 그 절의 **하위 절**에 표지가 있었다. 가드 규칙이 "그 절이나 하위 절" 이라 옳게 통과 |

셋 다 1 차에서 멈췄으면 **거짓 결함 보고**였다. 모형을 고치니 셋 다 죽었다.

### ② 술어가 **0 을 낸다** → 잔여가 아니라 술어부터

스캔이 "위반 0" 을 내면, 그 0 이 "정말 없어서 0" 인지 "안 보여서 0" 인지 먼저 가른다.
가르는 방법은 하나다 — **이미 아는 양성으로 술어를 쏜다.** 안 잡으면 술어가 고장이다.

실측 사례: mtime 해상도에 기댄 단정을 찾는 첫 술어(한 줄 안에서
`assert…!(… modified() …)`)가 **0** 을 냈는데, 그 0 은 **이미 알려진 양성조차 못 잡는**
0 이었다. 그 자리는 mtime 을 변수에 담아 다섯 줄 밖에서 비교한다. 술어를 함수 블록
단위로 고치니 그 하나를 잡았다(그리고 그것뿐이었다).

**0 만 의심하지 마라 — 0 이 아닌 값도 틀어져 있고, 어느 쪽으로 틀어졌는지는 미리 안다.**
철자로 찾는 술어가 자기가 찾는 것을 **덜 찾았다면**(형태 하나를 놓치면 그만큼 안
세어진다), 그 편향이 보고 값에 붙는 부호는 **바늘이 무엇을 찾느냐**로 뒤집힌다.

| 바늘이 찾는 것 | 덜 찾으면 | 보고 값은 |
|---|---|---|
| 그 자리의 **존재**("이런 자리가 있나") | 적게 센다 | **하한** — 실제는 이보다 많다 |
| 근거의 **부재**("사유가 안 붙었나") | 근거를 덜 찾으니 위반이 늘어난다 | **상한** — 실제는 이보다 적다 |

그래서 수를 적을 때 "약 N" 이 아니라 **"N 이상" 인지 "N 이하" 인지**를 적을 수 있다.
그 부호는 술어를 보면 도출되지, 값을 다시 재야 아는 것이 아니다.

★ **이건 부호 규칙이지 빈도 규칙이 아니다.** "철자 술어는 대체로 덜 잡는다" 는 **여기서
말하지 않는다** — 그 물음의 모수는 *정정된 술어* 가 아니라 **쓰인 술어 전수**여야 하고,
그것은 아직 아무도 안 쟀다. 정정된 것만 모으면 맞은 술어는 정정이 없어 표에 안 오므로,
그 표본으로 빈도를 말하는 것은 선택 편향을 값으로 쓰는 것이다. 빈도가 필요하면 그건
**새 측정**이고 모수를 다시 뽑아야 한다.

**정답을 알고 있어도 첫 술어가 놓친다.** 보통은 정답을 모르는 채로 스캔하므로, 이
절차가 없으면 그 0 을 "없다" 로 적게 된다. 그것이 이 부류의 실제 비용이다.

### 왜 둘을 합치지 않는가

같은 처방("모형을 먼저 의심하라")으로 보이지만 **입력과 오류의 부호가 반대**다.

| | 입력 | 안 하면 남는 것 |
|---|---|---|
| ① | 대조가 **안 죽는다** | 없는 결함을 보고한다 (거짓 양성) |
| ② | 표적이 **안 잡힌다** | 있는 결함을 없다고 적는다 (거짓 음성) |

물어야 할 것도 다르다 — ①은 *"내 자극이 저 규칙 안에 들어갔나"*, ②는 *"내 바늘이 저
모수의 실제 모양과 맞나"* 다. 합치면 둘 다 "모형을 봐라" 로 뭉개지고, 그러면 어느 쪽을
어떻게 봐야 하는지가 사라진다.

## 안티패턴 / 패턴

- ❌ "빌드 통과했어요, 확인해 주세요" — 빌드는 검증이 아니다.
- ❌ "테스트 545개 통과, 커밋했어요" — 테스트가 cover 못 하는 통합 동작이 있다.
- ❌ "동작할 것으로 보입니다" — 직접 돌려본 결과를 보고한다.
- ✅ 수정 → 빌드 → 단위 테스트 → **시나리오 재현** → 결과 보고 → 커밋.
- ✅ "재현 시나리오를 못 만들어 확인 못 했습니다" 를 인정하면 사용자가 검증을 도울 수 있다.

## 관련

- [debug-ipc.md](debug-ipc.md) — debug 전용 IPC (사용자 입력 재현)
- [independent-verification.md](independent-verification.md) — debug 격리 + 자기검증 배경
- [`ai-verification/visual-verification`](../ai-verification/visual-verification.md) — 시각 검증 · [`ai-verification/screenshot-methods`](../ai-verification/screenshot-methods.md) — `ui.screenshot`(OS 캡처 금지)
