# SOAK-REPORT-MACOS — macOS 메모리 누수 soak 검증 결과

> `SOAK-VERIFY-MACOS.md` 지시서에 따른 검증 사이클 결과 보고서.
> 실행 환경: macOS (Darwin 25.5.0, arm64), release 빌드, 격리 `TASTY_HOME` 인스턴스.
> 실행일: 2026-07-12. 검증 종료 후 지시서와 함께 삭제될 임시 문서.

## 요약 판정

| 계층 | 판정 | 근거 |
|------|------|------|
| **L1 진성 누수** | **PASS — 성장 0 확정** | s9 1800s 중 `leaks` diff: root 285건/14,432B가 early(T+5m)와 late(T+28m)에서 **바이트 단위 동일**. 전량 부팅 시 AppKit/XPC 고정 노이즈(NSXPCConnection ROOT CYCLE 등). plugin 9개 프로세스는 **0건/0B**. |
| **L2 heap 성장** | **성장 전액 규명** | ① IPC 상관 성분: 감속 형상 + footprint 평평 → pool/캐시 워밍업 (누수 아님). ② s6 view churn 성분(12KB/cycle): **상류 egui `WidgetRects` 버그로 스택 단위 확정** — tasty 코드 무죄. 상세는 "L2 상세". |
| **L3 GPU** | **PASS** | 전 시나리오에서 textures/buffers/views/bind_groups/mesh_targets 정수 복귀 (Δ0). |
| **L4 핸들·프로세스** | **결함 발견 → 수정 완료** | fd/proc 정수 복귀. PTY 셸 zombie 누적(s9 30분 중 16개)을 발굴, 원인 확정 후 수정·재검증 (`031e516c`, 아래 "좀비" 절). |
| **input_incidents** | **0 확정** | s4 550 cycles + s9 712 cycles 모두 0. Windows(MSYS bash) ~25% 는 MSYS 특이 현상으로 확정. 단, 관측 경로는 s4 의 `command not found` 감지뿐이므로 "s4 경로에서 0"이 정확한 서술 (하네스 격리 셸은 `/bin/sh`). |

## Windows 기준선 대비

| 시나리오 | Windows (수정 후) | macOS 실측 | 해석 |
|----------|------------------|-----------|------|
| s4 heavy output | +70KB/cycle | **+4.1KB/cycle** (R²=0.98) | 1/17. termwiz 수정(fdfcc64a) 플랫폼 공통 효과 + macOS 잔여 최소 |
| s6 view churn | +42KB/cycle | **+21.2KB/cycle** (root 12.0, R²=1.00) | 절반. 선형성 재검 결과 아래 "s6 상세" |
| s2 split churn | -1KB/cycle | +3.7KB/cycle (R²=0.95) | IPC 폴링 기여로 설명 (아래 "IPC 상관 성장") |
| s7 IPC churn | ~10KB/cycle | +19.8KB/cycle (R²=0.99) | call 량 차이 감안 시 동급. per-call ~400B (아래 상세) |
| s8 idle | ~10KB/cycle | **~평평** (부팅 정착 후 root +8KB/min) | idle 바닥 드리프트 없음 |
| input_incidents | bash(MSYS) ~25% | **0** | macOS 진짜 pty 에서 재현 안 됨 — MSYS 전용 확정 |

Windows 잔여 래칫(~450KB/cycle)의 "ToolHelp 스냅샷 폭풍" 가설 관련: macOS(libproc 단건 syscall)에서는
s1 잔여가 IPC 상관 성장으로 전부 설명되고 그 외 잔여가 사실상 0 → **가설과 부합** (macOS 에 같은
크기의 공통 원인 없음).

## L2 상세 — 관찰된 성장의 분해

### IPC 상관 성장 (~400B/call, root)

s1/s2/s4/s7 의 서로 다른 per-cycle 기울기(3.7/3.7/4.1/19.8KB)가 **IPC 호출량**으로 통일 설명된다:
- s7: 50 calls/cycle × ~400B = 20KB/cycle ← 실측 19.8KB/cycle 일치
- s4: `read_since_mark` 50ms 폴링 ≈ 1,200 calls/min × 400B ≈ 480KB/min ← 실측 ~405KB/min
- s8(idle, IPC 없음): ~평평 ← 시간 드리프트 기각, 활동(IPC) 상관 확정

s9 1800s 에서 이 성장은 **감속**(526→105KB/min)하고 phys footprint 는 평평 — SQLite page cache /
malloc pool 워밍업 형상이며 unbounded L2 가 아니라고 판정. 24h 급 장기 런으로 최종 확인 권장(미실행).

### 발견: audit 레코드의 디스크 무한 축적 (수정 아닌 보고)

모든 IPC 호출(allow+deny)이 `memory.db` 에 영속되며(`src/adapters/ipc/audit.rs`), 30일 retention 의
evict 가 **audit 조회 시에만 lazy 실행**된다. 아무도 조회하지 않는 일반 사용에서 실측 **68.5KB/min**
(s9 워크로드 기준 일 ~100MB) 디스크 축적. RSS 아닌 디스크 문제라 soak 판정 범위 밖이지만,
AI 에이전트 터미널의 IPC 밀도 특성상 방치 불가 수준 — **append 경로에도 retention 집행 지점을 추가**
하거나 **레코드 수 상한**을 도입하는 설계 결정 필요 (conductor 판단 사안으로 이관).

### s6 view churn 상세 — 진짜 L2 누수 확정

300s 스모크의 FLAG(root +12KB/cycle, R²=1.00)가 pool 워밍업인지 가리기 위해 1200s 재검
(795 cycles, checkpoint 5):

- root RSS 분기별 성장률: Q2 +430 / Q3 +477 / Q4 +383 KB/min — **20분 내내 감속 없음**
- phys footprint 도 동일 구간 +450KB/min 선형 — RSS 아티팩트 아님
- shm 매핑 영역 50 고정 — mmap 경로 아님, 순수 malloc heap
- markdown plugin 프로세스 자체도 124.6→133.0MB 동반 성장 (plugin 측 별도 누수)
- GPU 카운트는 전부 복귀(텍스처 25→25) — L3 아닌 순수 L2

**Attribution (MallocStackLogging + malloc_history, 심볼 release 재빌드) — 원인 전액 규명:**

- 결정적 그룹: **328 calls × 정확히 10,240B = 3.3MB live** (cycle 수와 일치). 스택:
  `egui::widget_rect::WidgetRects::insert` → `RawVec::grow_one` (explorer 렌더 경로,
  `egui::context::Context::create_widget` 발).
- **원인 = 상류 egui 버그 (0.31.1, master 도 동일 확인)**: `WidgetRects::clear()` 가
  `by_layer: HashMap<LayerId, Vec<WidgetRect>>` 의 **Vec 내용만 비우고 map 항목을 제거하지
  않는다** (capacity 유지). tasty 는 surface 마다 고유 `egui::Area` id 를 쓰므로
  (`egui_panels.rs` `draw_panel_frame`), 닫힌 surface 의 layer 항목이 ~10KB capacity 를 물고
  영구 잔존 → churn 1회당 ~10KB. 부속 그룹 2개(768B×~cycle)까지 합치면 ~11.8KB/cycle —
  실측 12KB/cycle 과 일치, **전액 규명**.
- s1/s2 에서 이 성분이 작았던 이유: 터미널 surface 는 내용이 커스텀 셰이더 렌더라 egui 위젯
  수가 적다(rects vec 소형). explorer/markdown 같은 egui-heavy surface 에서 두드러진다.
- **처방**: 공개 API 로 by_layer purge 불가 — 상류 몫. 판정 런북에 "알려진 잔류" 로 등재
  (`memory-leak-soak.md` 주의 절). 우회안(슬롯 기반 Area id 재사용)은 egui 위젯 상태가 id 로
  keyed 라 surface 간 상태 bleed 리스크가 있어 단독 채택하지 않음 — 상류 제보/patch 여부는
  conductor 결정 사안.
- **markdown plugin 프로세스는 heap 무죄**: malloc_history 에 churn 상관 그룹 없음, footprint
  113.5M < peak 129.3M (호흡 중). s6-long 의 plugin RSS 증가(+8.4MB/20min)는 shared buffer
  페이지 회계로 추정 — 24h 런 확인 항목.

### 좀비 (L4) — 발견·수정 완료

s9 본 런 중 root 직계 자식 좀비가 0→16 으로 단조 증가하는 것을 보조 샘플러가 관찰
(하네스의 `proc_count` 는 sysinfo 가 좀비를 세지 않아 **맹점** — 11→11 로 보였음).

- **정체**: 이름 채집 재현으로 `/bin/sh`(PTY 셸)로 확정. s2 단독 재현에서 90초+ 지속 확인.
- **원인**: unix 에서 portable-pty `kill()` 은 SIGHUP 송신뿐 reap 이 없고, `process()` 의
  exit 감지 폴링(try_wait)은 close 후 더 이상 돌지 않음 → `PtyBackend::Drop` 의 kill 이후
  아무도 waitpid 하지 않는 확률적 레이스 (타이밍에 따라 일부만 좀비화 — s1 재현에선 0,
  s2/s9 에선 발생).
- **수정** (`031e516c`): Drop 에서 detached thread 로 WNOHANG 유예 poll → 미종료 시
  SIGKILL escalation reap. Windows 무변경(`#[cfg(unix)]`).
- **검증**: 수정 후 s2 동일 런에서 지속 좀비 0 (25샘플 중 24개 0, 1개는 ≤200ms reap 창
  transient — 기대값 부합), s9-mini 420s 도 지속 0.

## shared buffer unix 경로 검증 (지시서 임무 2)

- **코드**: `tasty-shm/platform/macos.rs` 의 `PlatformMapping::Drop` 이 munmap 수행, fd 는 OwnedFd
  RAII — 해제 경로 건재.
- **실측**: s9 1800s 동안 매핑 영역 수(vmmap 카운트) **60 으로 완전 평평**, s6 포함 전 시나리오에서
  GPU 카운트 정수 복귀. → 802c0b15(닫힘 시 해제) + e9ceaed5(성장 교체 시 해제)의 unix(fd/mmap)
  경로 **작동 확인**.

## macOS 고유 계층 의심 목록 점검 (지시서 임무 4)

| 계층 | 결과 |
|------|------|
| NSMenu/objc 브릿지 | churn 상관 성장 없음 (s9 leaks diff 에 objc 증가분 0) |
| Metal 드라이버 | footprint 평평 + gpu_stats 정수 복귀 — 축적 없음 |
| CoreText/폰트 | s4(숫자 위주)로는 커버리지 제한 — 판정 유보. 다국어 heavy-output 변형은 미실행 (한계 절 참조) |
| NSPasteboard | 클립보드 churn 시나리오 부재로 미검증 (한계 절 참조) |

## 검증 사이클 중 발견·수정된 결함 (개별 커밋)

1. **`fix(cli)` — TASTY_SURFACE_ID env 테스트 flaky** (`4c56be7e`): 병렬 테스트 간 process-global
   env race. 통합 테스트로 합류해 해소, 10회 반복 green.
2. **`style` — 워크스페이스 fmt 드리프트 127건** (`d19b74b2`): 훅 미설치 머신發 누적. pre-commit
   A.2 가 전 커밋을 차단하던 상태 해소.
3. **`fix(plugin)` — PLUGIN_VERSION 하드코딩 drift** (`9021cc42`): 9개 plugin 전부 하드코딩,
   8개가 Cargo.toml 과 불일치 (soak 로그의 version drift WARN 으로 발굴). `env!("CARGO_PKG_VERSION")`
   로 교체해 클래스 자체 제거. 각 plugin 패치 +1 + manifest lockstep.

4. **`fix(terminal)` — PTY close 시 자식 셸 zombie 미회수** (`031e516c`): 위 "좀비" 절 참조.
   soak 이 발굴한 macOS/unix 실결함 — L4 계층의 이번 사이클 핵심 수확.

## 방법론 노트·한계 (차기 사이클 참고)

- **RSS 단독 판정 금지**: macOS 메모리 압축이 idle 페이지를 RSS 에서 숨길 수 있어 phys footprint
  (vmmap) 병행 측정을 도입했다 — 이번 판정의 핵심 근거.
- **`leaks` 는 diff 로만**: 절대값은 AppKit/XPC 노이즈 포함 (Apple 자체 바이너리도 ROOT LEAK 보고).
  early/late 2점 비교로 성장분만 판정.
- **s8 은 300s 로 측정 불능**: cycle=30s 라 체크포인트가 2개(2초 간격)뿐. `SOAK_CHECKPOINT_EVERY=1`
  + 600s 이상 필요 — 지시서 매트릭스의 맹점.
- **좀비는 하네스 맹점**: sysinfo 가 Z 상태를 안 세므로 `ps stat=Z` 외부 감시 필요.
- **MallocStackLogging 은 본 런과 격리**: MSL 자체가 RSS 를 왜곡 — 스택 필요 시 별도 짧은 런.
- **App Nap/잠듦**: 전 런을 `caffeinate -i` 로 감쌌다. s8 idle 드리프트는 App Nap 영향 가능성이
  이론상 남음 (실측은 평평이라 문제 안 됨).
- **release 하네스의 teardown panic**: `TastyInstance` Drop 경로가 debug 전용 `system.shutdown` 을
  호출해 release 에서 매 런 panic (측정엔 무영향, kill 폴백 동작). 하네스 개선 여지.
- **미실행/미해결 항목**:
  - 24h 본 런 (지시서 "여력 시") — s9 1800s 까지만 실행. IPC 상관 성장의 완전 plateau 와
    markdown plugin RSS 회계는 24h 런이 최종 확인.
  - CoreText 다국어 heavy-output 변형, NSPasteboard churn 시나리오 — 미커버 (시나리오 부재).
  - egui `WidgetRects` 상류 잔류 — 상류 제보/patch 여부 conductor 결정 대기.
  - audit 레코드 디스크 축적(68.5KB/min) — retention 집행 지점 설계 결정 대기.
  - release 하네스 teardown 의 `system.shutdown` panic — 하네스 개선 여지 (측정 무영향).
