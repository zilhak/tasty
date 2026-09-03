# bash(MSYS): resize 후 다음 입력의 첫 바이트 유실

**⚠️ Windows 전용 이슈.** ConPTY(Windows 전용 PTY 백엔드) + MSYS bash(git-bash 등) 조합에서만 재현된다 — macOS/Linux 의 tasty PTY 백엔드는 ConPTY 를 쓰지 않으므로 이 문서의 증상과 무관하다(아래 실측의 "셸을 cmd.exe 로 교체 → 유실 0" 도 Windows 내부 비교이지, 크로스플랫폼 비교가 아님에 주의).

**증상.** ConPTY resize(레이아웃 변경 — split/unsplit, 창 크기 변경)가 일어난 surface 의 bash 프롬프트에 다음 입력을 주입하면, 첫 1바이트가 소리 없이 사라진다 — `surface.send "seq 1 5000\n"` 이 `eq 1 5000` 으로 도착해 `bash: eq: command not found`. 재현율 resize+입력 쌍당 ~25–33%.

**원인 (셸 측 — tasty 무죄).** bash 는 SIGWINCH 핸들러를 `SA_RESTART` 로 설치하고, readline 은 플래그만 세워뒀다가 **다음 입력이 read 를 깨울 때** 보류된 WINCH 를 처리한다(bash 5 동작, [fff#48](https://github.com/dylanaraps/fff/issues/48)). MSYS/Cygwin 의 시그널 에뮬레이션이 ConPTY 위에서 이 "read 를 깨운 바이트"를 소모한다.

**상태: 알려진 상류(msys2-runtime) 버그로 기록만 한다 — 상류 제보는 하지 않기로 결정(2026-07-12).** 상류 이슈 트래커에 기존 리포트 없음(당시 검색 기준). tasty 는 아래 처방으로 방어하고, 향후 마음이 바뀌면 아래 실측 표·원인 분석을 근거로 제보할 수 있다.

**임계·근거 (2026-07-12 실측, 격리 debug 인스턴스).**

| 실험 | 결과 |
|------|------|
| resize 없이 send 만 20회 | 유실 0 (resize 연루 확정) |
| settle 0.1 s / 0.6 s / 2 s 후 send | 모두 ~25–33% — **시간 무관**, "다음 입력"에 앵커 |
| 희생 개행(`\n`) 프리픽스 | 유실 0/15 — 정확히 첫 1바이트만 소모, `\n` 이 대신 먹힘 |
| 셸을 cmd.exe 로 교체 | 유실 0/27 — bash(MSYS) 전용 |
| **tasty 무관 단독 재현기** (portable-pty 단독, 단일 스레드) | **셸 기동 후 첫 resize 의 다음 입력에서 결정적 재현** (3회 연속 1/20, 항상 iter 0) — tasty 최종 무죄 확정 |

**처방 (현재 상태).**

- tasty 는 PTY 계층에서 셸 내부 상태를 알 수 없어 근본 수정 불가(상류 몫). 감시는 soak 하네스가 `input_incidents` 카운터로 계수한다 ([memory-leak-soak](../memory-leak-soak.md)).
- **에이전트 처방**: 레이아웃 변경(split/close/resize) 직후 같은 surface 의 bash 에 명령을 주입해야 하면 **텍스트 앞에 `\n` 하나를 붙인다** — 유실돼도 개행이라 무해하고, 안 유실되면 빈 프롬프트 한 줄이 생길 뿐이다. 또는 주입 후 echo 를 검증하고 오염 시 재시도한다 (`tests/soak_memory.rs` 의 `cycle_heavy_output` 가 이 패턴).

**일반 교훈.** "주입한 입력이 도착했는가"는 시간 대기로 보장되지 않는다 — 셸이 시그널을 지연 처리하는 한 유실 창은 벽시계가 아니라 *다음 read* 에 붙는다. 입력 무결성이 필요한 자동화는 echo 검증 또는 무해한 선행 바이트로 방어한다.

날짜: 2026-07-12
