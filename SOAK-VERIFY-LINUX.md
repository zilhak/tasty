# [임시 지시서] Linux 메모리 누수 soak 검증 — 이 머신의 AI 에이전트에게

> 이 파일은 Windows 쪽 에이전트가 남긴 **크로스 플랫폼 검증 위임 지시서**다. 검증 사이클이
> 끝나면 이 파일과 보고서는 삭제될 임시 파일이다 (`docs/` 문서 아님).
> 읽는 너는 이 Linux 머신에서 tasty 레포를 받아 작업하는 AI 에이전트다.

## 배경 (2026-07-12 기준)

Windows에서 메모리 누수 검증 체계를 구축하고 누수 5건을 수정했다. **전 수치가 Windows
(ConPTY/conhost) 기준**이라 Linux 재검증이 필요하다. 반드시 먼저 읽을 것:

1. 루트 `CLAUDE.md` — 작업 규칙 전부 (커밋 정책·검증 정책 포함)
2. `docs/dev-guide/memory-leak-soak.md` — soak 체계 사용법·판정 기준·런북
3. `docs/dev-guide/linux.md` — 이 OS에서의 실행 방법

Windows 기준선 (수정 완료 후):

| 시나리오 | Windows 결과 | Linux 기대 |
|----------|-------------|-----------|
| s4 heavy output | +70KB/cycle (수정 전 1,850) | 비슷하거나 낮아야 함 — termwiz change log 수정(fdfcc64a)은 플랫폼 공통 |
| s6 view churn | +42KB/cycle (수정 전 1,600) | shared buffer 수정(802c0b15, e9ceaed5)의 **unix(fd/mmap) 경로**가 실제 작동하는지가 관건 |
| s2 split churn | -1KB/cycle | 동등 |
| s7 IPC churn / s8 idle | ~10KB/cycle | 동등 |
| input_incidents | bash(MSYS) 전용 ~25% | **0이어야 함** (진짜 pty라 MSYS 시그널 에뮬레이션 없음). 0이 아니면 그 자체가 발견 — 즉시 보고 |

## 실행 절차

```bash
cargo build --workspace   # 필수 — plugin 바이너리 drift 방지 (e2e-tests.md §0)
cargo test --workspace --lib --bins --locked          # 단위테스트 (Linux born-broken 테스트 유무)
cargo test --test e2e_tests                           # e2e
# soak 스모크 (시나리오별 짧게 → analyze.py 판정)
for s in s1 s2 s4 s6 s7 s8; do SOAK_SCENARIO=$s SOAK_DURATION_SECS=300 cargo test --release --test soak_memory -- --ignored --nocapture; done
SOAK_SCENARIO=s9 SOAK_DURATION_SECS=1800 cargo test --release --test soak_memory -- --ignored --nocapture
python3 scripts/soak/analyze.py <jsonl>               # 각 결과 판정
# 여력이 되면 본 실행: s9 를 수 시간~24h (release 빌드)
```

- GUI 필요 — `DISPLAY`/Wayland 유무를 확인하고 없으면 headless 구동 수단(xvfb 등)을 **네가 알아봐서** 마련하라. soak 하네스는 격리 `TASTY_HOME` 으로 자체 인스턴스를 띄우므로 사용자 세션과 충돌하지 않지만, **사용자의 활성 tasty/셸은 절대 건드리지 말 것**.

## 네가 조사·검증할 것 (정답을 주지 않는다 — 알아내서 보고하라)

1. **shared buffer 해제의 unix 경로**: Windows 는 `DuplicateHandle`, unix 는 fd+mmap 이다.
   `crates/tasty-host-plugin/src/manager/buffer.rs` 의 `release_plugin_buffer` 가 unix 에서
   실제로 매핑을 해제하는지(SharedMemory Drop 의 unix 구현 확인) — 코드 + s6 실측 양쪽으로.
2. **L4 의 Linux 형태**: conhost 는 없다. 대신 ① 좀비 프로세스 — `crates/tasty-reaper` 는
   비-Windows 에서 no-op(SIGHUP 의존)이므로 s1 churn 후 zombie/자식 수가 기준선으로
   복귀하는지, ② fd 수(하네스가 `/proc/<pid>/fd` 카운트) 복귀를 중점 감시.
3. **glibc malloc 특성**: Windows 잔여 래칫(~450KB/cycle)은 Win 힙 + ToolHelp 스냅샷 폭풍의
   합작이었다. Linux 의 foreground 폴링은 `/proc` 읽기라 폭풍이 없다 — s6/s1 잔여가 실제로
   0 에 가까운지 확인하고, 있다면 glibc arena 보유인지 진짜 누수인지 heaptrack 으로 갈라라.
4. **attribution**: FLAG/FAIL 이 나오면 `heaptrack -p <pid>` (미설치면 설치)로 스택을 떠라.
   여력이 되면 ASAN+LSAN 1회(런북 참조, nightly 필요)를 돌려 GPU 드라이버 suppression
   목록을 채집하고 파일로 커밋을 제안하라.
5. **Linux 고유 의심 지점을 스스로 목록화**: 위 항목 외에 이 OS 에서만 존재하는 계층
   (Wayland/X11 연결, Mesa/드라이버, fontconfig 등)에서 churn 과 상관하는 성장이 있는지
   — 시나리오 분리 기법(memory-leak-soak.md 의 2단계 프로토콜)으로 국소화하라.

## 산출물·규칙

- 발견 버그는 **원인 확정 → 수정 → 검증 → 개별 커밋** (CLAUDE.md 커밋 정책). 플랫폼 분기는
  `#[cfg(...)]` — 다른 OS 컴파일을 깨뜨리지 않는 것(불가침 원칙 4)을 `cargo check` 로 확인.
- 결과 보고서를 루트 `SOAK-REPORT-LINUX.md` 로 작성해 커밋 (형식: 시나리오별 수치 표 +
  Windows 기준선 대비 + 발견/수정 목록 + 미해결). 완료 후 **push**.
- 판정 함정: analyze.py 는 짧은 런에서 lazy 초기화(+1 텍스처)로 false-FAIL 이 날 수 있다
  (memory-leak-soak.md 주의 절 참조). debug 빌드 24h 는 로그 팽창 문제로 금지 — release.
