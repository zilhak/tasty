# SOAK-REPORT-LINUX — Linux 메모리 누수 soak 검증 결과

> `SOAK-VERIFY-LINUX.md` 지시서 이행 보고서. 검증 사이클 종료 시 삭제될 임시 파일.

## 환경

| 항목 | 값 |
|------|-----|
| OS | Ubuntu 24.04.4 LTS, kernel 6.17.0-1008-nvidia |
| **아키텍처** | **aarch64** (Windows 기준선 x86_64 와 다름 — 절대치 비교 시 감안) |
| CPU/RAM | 20 core / 119GB |
| GPU | NVIDIA (driver 590.48.01) — **단, 아래 "환경 제약" 참조: 실측은 전부 llvmpipe** |
| 세션 | xrdp 가상 X11 (`:10`, xorgxrdp) |
| Rust | 1.96.0, release 빌드 (`CARGO_PROFILE_RELEASE_DEBUG=1`) |
| glibc | 2.39, 페이지 4KB |

### 환경 제약 (중요)

이 머신의 tasty 는 **llvmpipe(CPU Vulkan) 로 렌더링된다** — 사용자 세션 포함.
원인 2중: ① xrdp 가상 디스플레이라 X 서버가 NVIDIA 를 구동하지 않음,
② 사용자가 `render`/`video` 그룹 미가입이라 `/dev/dri/renderD128` 접근 불가
(DRI3 offload 경로 차단). 따라서 **NVIDIA 드라이버 풀의 누수 계층은 이번
사이클에서 미검증** — GPU 활성화(그룹 가입 + xorgxrdp glamor 설정 또는 로컬
세션) 후 s2/s6 재측정이 후속 항목이다. `/dev/nvidia*` 는 0666 이라 CUDA 는
정상(ollama 등) — 그래픽 스택만 막혀 있다.

## 판정 요약 (스레드 제외 수정 + release 재빌드 후, 최종)

| 시나리오 | 결과 | Windows 기준선 | 판정 |
|----------|------|---------------|------|
| s1 tab churn | root +1.8KB/cycle (R²=1.00) | (기준 없음) | **warm-up 아티팩트** (근거 아래) |
| s2 split churn | root +1.3KB/cycle (R²=1.00) | -1KB/cycle | **warm-up 아티팩트** |
| s4 heavy output | root +1.2KB/cycle (R²=0.98) | +70KB/cycle | **Win 대비 ~60배 개선** — termwiz 수정(fdfcc64a) 플랫폼 공통 확인. 잔여는 warm-up |
| s6 view churn (혼합) | root +11.8KB/cycle | +42KB/cycle | **일부 확정 누수** — 아래 "s6 세부" 참조 |
| s7 IPC churn | root +11.3KB/cycle (R²=1.00) | ~10KB/cycle | **warm-up 아티팩트** (대조 실험으로 확정) |
| s8 idle | PASS | ~10KB/cycle | 깨끗 |
| s9 mixed 3h | 진행 중 | — | 완료 후 갱신 |
| **input_incidents** | **전 시나리오 0** | bash(MSYS) ~25% | **지시서 예측 적중** — 진짜 PTY 라 MSYS 시그널 에뮬레이션 레이스 없음 |
| **L3 (GPU 카운트)** | 전 시나리오 정수 복귀 PASS | — | shared buffer unix 해제 경로 실증 (아래) |
| **L4 (좀비/fd)** | proc_count·fd 전 시나리오 복귀 PASS | conhost 좀비 이슈 | 좀비 0 (PDEATHSIG 수정 후) |

### "warm-up 아티팩트" 판정 근거

s1/s2/s4/s7 의 FLAG(완전선형 R²≈1.00)는 **대조 실험**으로 반증했다: release
격리 인스턴스에서 `surface.list` 를 **2,000회 warmup 후 10,000회** 연타하며
RSS 측정 결과 **총 +32KB(≈3B/call)** 로 평탄. 스모크(300s)의 s7 총 호출수
(~3,000)는 전부 warm-up 구간(SQLite audit 레코드의 page cache 반영, allocator
arena 확장, lazy 초기화) 안이었다. **교훈: Linux 에서 300s 스모크의 RSS FLAG
는 신뢰 불가 — 판정은 warmup 을 수백~수천 사이클 이상 확보한 장시간 런으로만.**
analyze.py 의 10% warmup 컷은 이 규모의 warm-up 을 자르기에 부족하다.

이 warm-up 성장의 상당 부분은 IPC audit 로그의 SQLite page cache 반영이었는데,
**이후 macOS 사이클에서 같은 계열의 실제 버그**(audit retention 이 query 시에만
lazy 실행돼 무조회 상황에서 `memory.db` 가 무한 축적 — 커밋 `aedc5ed2`, macOS
실측 ~68.5KB/min)가 발견·수정됐다. 즉 이 Linux 사이클의 "warm-up" 분석이
가리킨 성장 지점과 macOS 가 확정한 실제 누수가 같은 서브시스템(audit retention)
이었다 — 크로스 플랫폼 병렬 검증이 상호 보완된 사례.

### s6 세부 — explorer 는 확정 누수, markdown 은 깨끗

s6(혼합 +11.8KB/cycle)를 explorer/markdown 채널로 분리 재측정(각 400 cycle,
warmup 100~200cycle 제외):

| 채널 | 기울기 | 판정 |
|------|--------|------|
| explorer (host view) | **+36~39KB/cycle** (여러 조건에서 일관) | **확정 누수** — warm-up 아님 |
| markdown (plugin egui-mesh) | +1.3~1.5KB/cycle | 사실상 깨끗 |

**반증 실험으로 배제한 가설** (전부 기울기 불변으로 기각):
- 디렉토리 크기(원본 레포 vs 빈 격리 홈) — 무관
- egui `Context::data` 영속 맵(프레임마다 강제 `clear()`) — 무관
- `malloc_trim(0)` 주기 호출(fragmentation 가설) — 무관
- IPC 응답 대기(`sleep`) 유무, 스레드 수, fd 수 — 전부 무관/불변

heaptrack 은 이 aarch64 박스에서 스택 unwind 가 전면 실패해(attach/preload/
frame-pointer 재빌드 전부 `0x0 ??`) 쓸 수 없었다. dhat(trim=32)·코어 덤프
텍스트 스캔·힙 내 함수포인터 히스토그램(gdb `info symbol` 배치 조회)으로 우회
공격했으나, **explorer 생성/소멸 자체(스레드·fd 불변, 렌더 대기와도 무관)에서
나는 36KB급 성장의 단일 지점을 이번 사이클에서 확정하지 못했다** — host
`ExplorerViewStore`/`command_index`/`output_observer` 의 `drop_surface` 계열은
모두 코드 리뷰상 정상(HashMap remove, 정확히 대칭)이었고, `RecentFiles::add`
(상한 10, DB 기록)도 사용자 Navigate 액션에서만 발화해 churn 사이클과 무관함을
확인했다. **미해결 — 후속 조사 필요** (아래 참조).

## 지시서 조사항목 회답

1. **shared buffer unix 해제 경로** — 코드+실측 양면 확인.
   코드: `release_plugin_buffer` → 맵 remove → `SharedMemory` Drop →
   `PlatformMapping::drop`(linux.rs) 의 `munmap` + `OwnedFd` close. 호출
   3지점(surface 닫힘/popup·banner 닫힘/`SharedBufferReleased`) 모두 플랫폼
   중립. dup 송신 fd 는 `PlatformPayload` Drop 이 닫음. 실측: s6 에서
   gpu.textures/buffers/mesh_targets 정수 복귀 + fd 복귀 → **작동 확인**.
2. **L4 의 Linux 형태** — s1 churn 후 proc_count(좀비 포함)·fd 모두 기준선
   복귀. 단 **부팅 시 plugin 전멸 버그**(아래 발견 #2)가 있어 수정 전에는
   매 부팅 좀비 9개가 잔존했다. 수정 후 0.
3. **glibc malloc 잔여** — Windows 잔여 래칫(~450KB/cycle)에 대응하는 Linux
   잔여는 s1/s2/s4/s7 기준 **~3B/call 수준으로 사실상 0** (위 대조 실험).
   ToolHelp 스냅샷 폭풍이 없는 /proc 폴링 + glibc arena 특성의 조합으로
   예측대로 깨끗 — 단 s6 explorer 채널은 예외(위 참조).
4. **attribution** — heaptrack 이 이 aarch64 박스에서 전면 실패 → dhat +
   코어 덤프 판독으로 대체. s6 explorer 는 이 방법으로도 단일 원인 미확정
   (후속). ASAN/LSAN 은 런북의 x86_64 타깃 문자열이 이 박스에 부적용
   (`aarch64-unknown-linux-gnu` 필요) — 시간 관계상 미실행, 후속.
5. **Linux 고유 의심 지점** — X11/xrdp 세션에서 s1/s2/s4/s7/s8 은 churn 상관
   성장 미검출. fontconfig/Mesa(llvmpipe) 계층도 무혐의. explorer 만 예외
   (churn 상관 확정, 원인 미상). NVIDIA 드라이버 계층은 환경 제약으로 미검증.

## 발견·수정 (개별 커밋)

| # | 커밋 | 내용 | 심각도 |
|---|------|------|--------|
| 1 | `d6038367` | soak S4 입력유실 감지가 bash 전용 문자열 — dash(`/bin/sh`)에서 계수 불가·60s panic | 하네스 (Linux born-broken) |
| 2 | `cb6b99c8` | **PDEATHSIG 가 fork 한 스레드 수명에 결박** — 부트 워커 종료 순간 **매 부팅 plugin 전원 SIGKILL**, 60s healthcheck 재스폰까지 plugin 기능 전멸 + 좀비 9. 영속 spawner 스레드(`spawn_bound`)로 수정, 회귀 테스트 + 음성 대조(구 경로 = signal 9) 포함 | **release 사용자 직격** (Linux 전 부팅) |
| 3 | `db5befc8` | soak 하네스가 Linux 에서 스레드를 프로세스로 합산 — tree RSS 57배 증폭(21GB), false-FLAG | 하네스 (Linux born-broken) |
| 4 | `bca468d8` | macOS 커밋(7f5e36f9)의 `libc` 의존성이 macos 전용 선언 — **Linux 빌드 붕괴**(E0433) 수정 | 빌드 (불가침 원칙 4) |
| 5 | `1d7b43d7` | lint 커밋(cc3b1702, dead_code deny 승격) 이후 **release 빌드 전면 붕괴** — debug-only IPC/`#[cfg(test)]` 전용 API 9개소가 release+non-test 빌드에서 dead 판정. `cfg_attr(not(debug_assertions), allow(dead_code))` 로 원복 | **release 빌드 전면 붕괴** |

부수 발견(내 커밋 아님, 병렬 검증 상호 확인): macOS 사이클이 audit retention
무한 축적 버그(`aedc5ed2`)를 발견·수정 — 이 Linux 사이클의 warm-up 성장 분석이
가리켰던 것과 같은 서브시스템.

절차상 함정: 매니페스트 버전 bump 후 `.sig` stale 로 release 빌드가 builtin
전원 거부(`plugin.list` 빈 배열) → s6/s9 크래시 유발. `scripts/sign-bundle.sh
--all-builtins` 재서명으로 해소 (커밋 대상 아님). **교훈: 매니페스트를 건드린
뒤 release 검증 전 재서명 필수.**

## 미해결·후속

- **s6 explorer 채널 +36KB/cycle 확정 누수, 원인 미상** — 최우선 후속.
  heaptrack 이 되는 x86_64 머신에서 재현하거나, 이 박스에서 gdb 소스레벨
  브레이크포인트(`ExplorerPanel::new_with_mode`/`drop` 페어링) 로 접근 권장.
- **NVIDIA GPU 계층 미검증** — 환경 제약(위). GPU 활성화 후 s2/s6 재측정.
- **ASAN+LSAN 1회** — aarch64 타깃으로 미실행. GPU suppression 채집도 이월.
- **s9 3시간 장시간 런** — 진행 중, 완료 후 최종 판정 갱신 예정 (본 커밋
  시점 기준 진행 중이라 이 보고서에는 스모크 30분 결과까지만 반영).
