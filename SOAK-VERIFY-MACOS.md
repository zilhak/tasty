# [임시 지시서] macOS 메모리 누수 soak 검증 — 이 머신의 AI 에이전트에게

> 이 파일은 Windows 쪽 에이전트가 남긴 **크로스 플랫폼 검증 위임 지시서**다. 검증 사이클이
> 끝나면 이 파일과 보고서는 삭제될 임시 파일이다 (`docs/` 문서 아님).
> 읽는 너는 이 macOS 머신에서 tasty 레포를 받아 작업하는 AI 에이전트다.

## 배경 (2026-07-12 기준)

Windows에서 메모리 누수 검증 체계를 구축하고 누수 5건을 수정했다. **전 수치가 Windows
(ConPTY/conhost) 기준**이라 macOS 재검증이 필요하다. macOS 는 특히 **reachability 기반
라이브 누수 판정(`leaks`)이 가능한 유일한 OS** — L1(진성 누수) 확정 도장은 여기 몫이다.
반드시 먼저 읽을 것:

1. 루트 `CLAUDE.md` — 작업 규칙 전부 (커밋 정책·검증 정책 포함)
2. `docs/dev-guide/memory-leak-soak.md` — soak 체계 사용법·판정 기준·런북

Windows 기준선 (수정 완료 후):

| 시나리오 | Windows 결과 | macOS 기대 |
|----------|-------------|-----------|
| s4 heavy output | +70KB/cycle (수정 전 1,850) | 비슷하거나 낮아야 함 — termwiz change log 수정(fdfcc64a)은 플랫폼 공통 |
| s6 view churn | +42KB/cycle (수정 전 1,600) | shared buffer 수정(802c0b15, e9ceaed5)의 **unix(fd/mmap) 경로** 작동이 관건 |
| s2 split churn | -1KB/cycle | 동등 |
| s7 IPC churn / s8 idle | ~10KB/cycle | 동등 |
| input_incidents | bash(MSYS) 전용 ~25% | **0이어야 함** (zsh/bash + 진짜 pty). 0이 아니면 그 자체가 발견 — 즉시 보고 |

## 실행 절차

```bash
cargo build --workspace   # 필수 — plugin 바이너리 drift 방지 (e2e-tests.md §0)
cargo test --workspace --lib --bins --locked
cargo test --test e2e_tests
for s in s1 s2 s4 s6 s7 s8; do SOAK_SCENARIO=$s SOAK_DURATION_SECS=300 cargo test --release --test soak_memory -- --ignored --nocapture; done
SOAK_SCENARIO=s9 SOAK_DURATION_SECS=1800 cargo test --release --test soak_memory -- --ignored --nocapture
python3 scripts/soak/analyze.py <jsonl>
# 여력이 되면 본 실행: s9 수 시간~24h (release 빌드)
```

- **사용자의 활성 tasty/셸은 절대 건드리지 말 것** — soak 하네스는 격리 `TASTY_HOME` 자체
  인스턴스를 쓴다.

## 네가 조사·검증할 것 (정답을 주지 않는다 — 알아내서 보고하라)

1. **L1 확정 도장 (macOS 전용 임무)**: s9 soak 도중과 종료 직전에 `leaks <pid>` 를 실행해
   unreachable 누수 0 을 확인하라 (필요시 `MallocStackLogging=1` 재실행으로 스택 확보,
   `xcrun xctrace` Allocations/Leaks 는 런북 참조). 0이 아니면 스택과 함께 보고.
2. **shared buffer 해제의 unix 경로**: `release_plugin_buffer` 가 macOS(fd+mmap)에서 실제
   매핑을 해제하는지 — 코드(SharedMemory Drop unix 구현) + s6 실측 양쪽으로.
3. **L4 의 macOS 형태**: conhost 없음. 좀비/자식 프로세스(`tasty-reaper` 는 비-Windows
   no-op — SIGHUP 의존) 와 fd 수(하네스가 lsof 카운트)가 churn 후 기준선 복귀하는지 중점 감시.
4. **macOS 고유 계층을 스스로 의심 목록화**: NSMenu/objc 브릿지, Metal 드라이버,
   CoreText/폰트, NSPasteboard 등 — churn 상관 성장이 있으면 시나리오 분리(2단계 프로토콜)로
   국소화한 뒤 Instruments Allocations 구간 diff 로 스택을 떠라.
5. **Windows 에서 나온 잔여 래칫과의 대조**: Windows 잔여(~450KB/cycle)는 ToolHelp 스냅샷
   폭풍 + Win 힙 보유의 합작이었다. macOS foreground 폴링은 libproc 단건 syscall 이라
   폭풍이 없다 — s6/s1 잔여가 0 에 가까운지 확인하고, 있으면 별도 원인이니 국소화하라.

## 산출물·규칙

- 발견 버그는 **원인 확정 → 수정 → 검증 → 개별 커밋** (CLAUDE.md 커밋 정책). 플랫폼 분기는
  `#[cfg(...)]` — 다른 OS 컴파일을 깨뜨리지 않는 것(불가침 원칙 4)을 `cargo check` 로 확인.
- 결과 보고서를 루트 `SOAK-REPORT-MACOS.md` 로 작성해 커밋 (형식: 시나리오별 수치 표 +
  Windows 기준선 대비 + `leaks` 결과 + 발견/수정 목록 + 미해결). 완료 후 **push**.
- 판정 함정: analyze.py 는 짧은 런에서 lazy 초기화(+1 텍스처)로 false-FAIL 이 날 수 있다
  (memory-leak-soak.md 주의 절 참조). debug 빌드 24h 는 로그 팽창 문제로 금지 — release.
