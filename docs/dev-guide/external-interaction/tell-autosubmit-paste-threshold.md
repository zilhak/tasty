# child tell 단일라인 자동제출 — 63자 paste 임계

`tasty claude tell` / `tasty codex tell` 로 child(외부 TUI: Claude Code / codex CLI)에 입력을
주입할 때, 단일라인 본문과 제출 Enter 를 **같은 PTY write burst** 에 섞으면 수신측의
bracketed-paste 휴리스틱이 끝의 `\r` 을 제출이 아닌 paste 본문으로 흡수하는 함정.

## 증상

- `tell` 단일라인 메시지가 **63자(code point) 이상**이면 자동제출(Enter)되지 않고 입력창(❯)에
  텍스트만 남는다. **62자 이하는 정상 제출.**
- **멀티라인(개행 포함) 은 길이 무관 제출**된다.
- 임계는 byte 도 표시폭도 아니라 **code point 수** 다 — 영문 63B 와 한글 189B 가 **같은 63자**
  에서 끊긴다(한글은 2배폭인데도 char 기준). 콘텐츠(영문/한글/공백/슬래시/경로/특수문자)·
  타이밍(즉시/지연 read) 모두 무관, 결정적.

## 원인

수신측 Claude Code / codex TUI 는 한 번의 read 로 들어온 입력 burst 가 일정 크기(~64 codepoint,
= 63자 본문 + `\r`) 이상이면 bracketed-paste 휴리스틱으로 **paste 로 판정**하고, burst 끝의
`\r` 을 제출 Enter 가 아니라 paste 본문 리터럴로 흡수한다.

tasty 가 단일라인 tell 을 본문과 제출 `\r` 을 **한 문자열(`"{msg}\r"`)로 만들어 한 번의
`surface.send` = 한 PTY write** 로 보내면, 본문이 길 때 그 write 전체가 paste 로 처리되어
미제출된다. 멀티라인은 원래부터 본문과 `\r` 을 **별도 PTY write** 로 분리해 보내므로 단독
`\r` 이 제출로 처리되어 길이와 무관하게 안정적이었다.

**근본 원인 소속**: 외부 TUI 의 paste 감지 자체는 합리적 동작이다. 함정은 tasty 가 "제출"
신호를 본문과 같은 write burst 에 섞어 보낸 데 있다 — tasty 측에서 고친다.

## 임계·근거

내부 재현 실험으로 확정(2026-06-25, idle child 에 `tell --no-wait` 후 화면 판정):

| 입력 | 문자수 | byte | 개행 | 결과 |
|------|-------|------|------|------|
| 영문 | 62 | 62 | 0 | 제출 |
| 영문 | 63 | 63 | 0 | **미제출** |
| 한글 | 62 | 186 | 0 | 제출 |
| 한글 | 63 | 189 | 0 | **미제출** |
| 슬래시/경로·한글+경로 혼합 | 62 / 63 | — | 0 | 62 제출 / 63 미제출 |
| 멀티라인 | 17·81 | — | 1 | 제출 (길이 무관) |

→ 경계는 콘텐츠 무관 **본문 63자**(본문+`\r` = 64 codepoint burst). 미제출 케이스는 텍스트가
입력창에 온전히 들어가 있고 수동 Enter 1회로 정상 제출됨.

## 처방 (현재 상태)

단일라인 tell 도 멀티라인과 동일하게 **본문과 제출 `\r` 을 별도 PTY write 로 분리 전송**한다.

- **호스트 `terminal.tell` 에 중앙화됐다** — `src/adapters/ipc/handler/terminal.rs` 의
  `build_tell_payload`(개행 유무와 무관하게 `\r` 미포함 본문만 만든다. 멀티라인은 bracketed
  paste 로 감싼다) + `handle_tell`(본문 `surface.send` → 별도 `\r` `surface.send`, 두 번의
  write 로 분리)이 유일한 구현이다.
- `claude`/`codex` 플러그인의 `tell` 은 자체 페이로드 조립 로직을 갖지 않고 `terminal.tell`
  IPC 를 그대로 호출해 위임한다(`crates/tasty-plugin-claude/src/handlers.rs`,
  `crates/tasty-plugin-codex/src/handlers.rs` 의 `handle_tell` — 둘 다 `host_call(host,
  "terminal.tell", …)` 한 줄). `terminal.spawn` 의 초기 command 주입도 같은
  `build_tell_payload` 를 거친다.
- 호스트 `terminal.broadcast`(`handle_broadcast`)도 동일 함정을 피한다 — `build_broadcast_payload`
  가 호출자가 넣은 trailing `\r`(제출 의도)을 본문에서 분리해, 본문은 `build_tell_payload` 로
  감싸 한 write, 제출 `\r` 은 별도 write 로 보낸다. tell 과 달리 broadcast 는 `\r` 이 없으면
  제출 write 를 생략해 "sent as-is" 주입 계약을 보존한다.
- **적용 범위 밖(주의)**: `claude`/`codex` 플러그인이 자기 프로세스를 처음 띄우는
  `start_claude_in_surface`류 launch 경로는 별도 `surface.send`(쉘 커맨드라인 + `\r` 한 덩어리)
  라 같은 사각지대다 — 다만 이쪽은 대상이 TUI 가 아니라 아직 아무것도 안 뜬 shell 프롬프트라
  bracketed-paste 휴리스틱이 걸릴 대상 자체가 없어 실질 위험은 없다.

결과: `tell`/`terminal.spawn`/`terminal.broadcast` 경로 모두 메시지 길이·콘텐츠와 무관하게
결정적으로 동작한다. 단위 테스트가 본문 payload 에 제출 `\r` 이 섞이지 않음(63자+ 회귀 가드
`broadcast_payload_splits_trailing_cr_for_submit`)과 멀티라인 본문 분리(`broadcast_payload_multiline_wraps_bracketed_and_submits`)를 검증한다.

## 일반 교훈

외부 TUI 에 입력을 주입할 때, **"제출(Enter)" 같은 제어 신호는 본문과 다른 write 로 분리**해야
외부의 paste/모드 감지에 흡수되지 않는다. 한 write burst 에 본문+제어를 섞으면 burst 크기에
따라 제어 신호가 비결정적으로 삼켜질 수 있다.

날짜: 2026-06-25 (근거는 내부 조사로 실측 확정 — 재현 매트릭스 핵심 수치를 위에 옮김).
