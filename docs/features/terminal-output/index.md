# 터미널 출력 구조화 (Terminal output)

- **Status**: Implemented
- **주체**: AI Agent
- **ADR**: 없음
- **코드**: `tasty-output` 크레이트, `surface.parse_since_mark`/`surface.commands`/`output.observe_*` 핸들러
- **화면**: 없음
- **메서드/파서**: [reference/api](../../reference/api.md#surface-상호작용) · [reference/output-parsers](../../reference/output-parsers.md)

## 목적

터미널 출력을 **의미 단위**로 분해해 에이전트가 다루기 쉬운 JSON 으로 제공한다. 모두 `terminal.read` 권한.

## 내부 동작 — 세 진입점

| 진입점 | 패턴 | 용도 |
|--------|------|------|
| `surface.parse_since_mark` | 일회성 batch | 마크 이후 출력을 한 번에 분해 |
| `surface.commands` (+`last_command`,`command_at`) | 일회성 batch | OSC 133 인덱싱된 **명령 단위** 메타데이터 |
| `output.observe_start` | 스트리밍 | PTY 라인마다 파서 → sink fan-out |

세 경로 모두 같은 [파서 카탈로그](../../reference/output-parsers.md)를 공유.

### parse_since_mark

`set mark` → 명령 실행 → `parse-since-mark --parsers path,url,compile_error,test_result`. `--parsers` 생략 시 기본 4종(`path,url,prompt_boundary,exit_code`). 고급 6종은 명시 opt-in. 전체 block 을 받아 멀티라인 파서(`compile_error`/`stack_trace`)도 정확히 분해.

### 명령 인덱싱 (OSC 133)

셸 통합이 OSC 133 을 보내면 각 명령의 prompt 시작/명령 시작/종료/exit code/명령 문자열을 `tasty-memory`(`surface:<id>` scope, `tasty.commands.<ms>`)에 기록. OSC 133 미지원 셸은 빈 배열.

**headless PTY 는 인덱싱 대상이 아니다.** 인덱서는 `TerminalStore` 키를 그대로 scope id 로 쓰는데 headless PTY([headless-pty](../headless-pty/index.md))의 `Terminal` 은 그 store 에 **pty id**(`>= 0x8000_0000`)로 등록돼 있다 — 그대로 기록하면 surface id 공간을 침범한 `Scope::Surface` 가 생겨 다음 부팅의 surface 카운터를 PTY 공간으로 밀어 올린다([ADR-0094](../../adr/0094-surface-id-space-bounded-below-pty-base.md)). headless PTY 의 종료코드는 `pty.wait` 가 별도로 제공한다.

**셸 통합 자동 주입(bash/zsh)**: 사용자가 `.bashrc`/`.zshrc`를 전혀 건드리지 않아도, tasty 가 셸 spawn 시점에 OSC 133 A(prompt 시작)/C(명령 실행 직전)/D(명령 종료+exit code) 훅을 자동으로 주입한다 — "사용자가 알아서 셸 통합 스크립트를 설치"해야 했던 이전 사전조건이 사라졌다.

- **bash**: `--rcfile <tasty 합성 rc>`로 띄운다. C phase 는 `PS0`(bash 4.4+ 전용)으로 낸다 — bash 4.4 미만(예: macOS 시스템 기본 bash 3.2)은 `PS0`가 없어 **미지원**이며 DEBUG trap 폴백도 두지 않는다(사용자 `trap ... DEBUG`와의 충돌 회피). A/D 는 `PROMPT_COMMAND` 체인 맨 앞에서 `$?`를 캡처해 낸다. **bash C phase 는 명령 문자열(`cmd=`)을 싣지 않는다** — 주된 이유는 bash 고유 제약이 아니라, `crates/tasty-terminal/src/vte_handler/osc.rs`의 OSC133 C phase 파서가 셸 종류와 무관하게 C phase payload 를 무조건 버리기 때문이다(`FinalTermSemanticPrompt::MarkEndOfInputAndStartOfOutput` 매칭 시 항상 빈 문자열로 치환) — 그래서 bash 든 zsh 든 C 에 `cmd=`를 실어 보내도 현재는 `command_index.rs`까지 도달하지 못한다. 부차적으로, 이 파서가 나중에 C payload 를 보존하도록 확장될 경우에 한해 의미가 생기는 얘기지만, PS0 평가 시점의 명령 문자열 조회 방식마다 신뢰도가 다르다는 것도 실제 PTY 로 수동 검증했다 — `history 1`은 "지금 실행하려는 명령"을 정확히 가리키는 반면, `$BASH_COMMAND`와 `fc -ln -1`은 한 명령 이전 값을 가리킨다(bash 가 그 시점까지 아직 현재 명령을 기록하지 않음). 어차피 파서가 버리는 payload 를 위해 `history 1`을 새로 배선하는 대신 생략을 택했다(OSC133 payload 는 스펙상 optional, `command_index.rs` 도 없는 경우를 그대로 처리한다). Windows(Git Bash)와 비-Windows 모두 같은 훅 스크립트를 공유하되(`crates/tasty-settings/src/general.rs` `BUILTIN_BASHRC_*`), 최종 CLI 인자 모양은 플랫폼마다 다르다(Windows 는 기존 `--rcfile <path>` 그대로, 비-Windows 는 `--rcfile <path> -i` — bash 로그인 셸에서는 `--rcfile`이 무시되므로 로그인 모드를 포기하고 명시적 `-i`로 대체한다). 비-Windows 는 진짜 login 셸의 프로필 탐색 순서(`~/.bash_profile` → `~/.bash_login` → `~/.profile`, 최초 존재하는 파일 하나만)를 재현해 사용자 커스터마이즈(예: Homebrew PATH)를 그대로 로드한다.
- **zsh**: `ZDOTDIR` 스왑(VSCode/iTerm2 zsh shell-integration과 동일한 업계 표준 기법)으로 주입한다. tasty 가 관리하는 `~/.tasty/zsh-integration/.zshenv`를 `ZDOTDIR`로 가리키게 해 zsh 가 셸 인스턴스당 정확히 한 번 읽는 그 파일에서 OSC133 훅(`add-zsh-hook precmd/preexec`)을 등록한 뒤, `ZDOTDIR`를 원래 값(미설정이면 unset)으로 즉시 복원하고 원본 `.zshenv`를 이어서 source 한다. zsh 의 네이티브 `preexec` 훅은 인자로 정확한 명령 텍스트를 주므로 C phase 에 `cmd=`를 함께 보내지만, 위 bash 항목에서 설명한 것과 같은 이유(osc.rs 의 C phase 파서가 payload 를 무조건 버림)로 이 `cmd=` 도 현재는 `command_index.rs`에 도달하지 않는다. **알려진 한계**: 사용자 `.zshrc`가 `precmd_functions=(...)`/`preexec_functions=(...)`처럼 배열을 통째로 재할당하거나 동명 함수를 재정의하면 tasty 훅이 제거될 수 있다(bash 의 단일 `PROMPT_COMMAND` 슬롯 충돌보다는 드묾).
- fish/nu/pwsh(비-Windows) 등 기타 셸은 이번 범위 밖 — 여전히 사용자가 직접 설치해야 한다(OSC 133 미수신 시 빈 배열인 기존 동작 그대로).

### 스트리밍 옵저버

PTY 라인마다 파서를 돌려 sink 로 fan-out(**휘발성** — 호스트 재시작 시 소멸). sink: `memory`(ring buffer, `memory.list`/`query` 로 회수) / `file`(JSONL append). 필터: `parsers`(활성 파서) + `kinds`(출력 후 kind 필터) + `surface_id`(생략 시 전체 surface wildcard). **백압**: 옵저버별 bounded channel(256), 채워지면 drop + `info.dropped` 증가(PTY 스레드 절대 block 안 함). surface 닫히면 매인 옵저버 자동 정리, wildcard 는 유지. **자동 정리는 sink 워커를 그 자리에서 join 하지 않는다** — 워크스페이스 close 가 surface 수만큼 이 경로를 렌더 스레드에서 반복하기 때문이다([ADR-0076](../../adr/0076-close-path-per-surface-blocking-removal.md)). channel 에 수락된 항목은 워커가 스스로 다 비우고 끝나므로 유실은 없고, 남은 워커는 앱 종료 시퀀스(S3b)가 회수한다. 다만 **sink 파일에 마지막 항목이 도달하는 시점은 close 응답 이후로 밀릴 수 있다.** 명시 해제(`output.observe_stop`)는 종전대로 호출 복귀 시점에 sink 가 닫혀 있음을 보장한다.

> **멀티라인 파서는 옵저버에서 발화하지 않는다**(라인별 dispatch). 컴파일 에러 수집은 `prompt_boundary` 옵저버로 종료 감지 후 `parse_since_mark` batch.

## 인터페이스

- **AI Agent / CLI**: `tasty read parse-since-mark` · `tasty read commands/last-command/command-at` · `tasty output observe {start,list,info,stop}`. [reference/api](../../reference/api.md#surface-상호작용).

## 관련

- [reference/output-parsers](../../reference/output-parsers.md) — 파서 카탈로그 · [work-area](../work-area/index.md) — surface
