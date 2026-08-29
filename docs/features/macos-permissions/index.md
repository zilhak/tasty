# macOS 권한 (TCC)

- **Status**: Partial — 파일 계열 + 화면 기록 pre-warm 구현. 손쉬운 사용·Full Disk Access 안내는 미구현
- **주체**: 로컬 사용자 (AI Agent 가 간접 수혜 — 작업 도중 프롬프트로 멈추지 않는다)
- **ADR**: 없음
- **코드**: `src/platform/macos_permissions.rs` (목록 결정 + 워커 + CoreGraphics FFI), 호출부 `src/app/boot_machine.rs::finish_boot`, 캡처측 소비처 `src/platform/screen_capture.rs`, 번들 usage description 은 `scripts/build-macos-dmg.sh` 의 Info.plist heredoc
- **화면**: 없음 — 프롬프트는 OS 가 그린다

## 목적

macOS 는 보호 리소스에 **실제로 접근하는 그 순간**에만 권한 프롬프트를 띄운다. 터미널은 사용자가 친 명령을 대신 실행하는 것이 본업이라, 새 폴더를 처음 건드릴 때마다 프롬프트가 새로 뜬다. AI 에이전트가 터미널 안에서 자율 진행하는 도중에 프롬프트가 뜨면 **그 시점에서 작업이 멈춘다** — 자율 진행이 깨지는 실질적 원인이다.

발화 시점을 **부팅 직후로 몰아** 작업 중 중단을 없애는 것이 이 기능의 목적이다. 프롬프트의 총량을 줄이는 것이 아니라 **시점을 앞당기는 것**이다.

## 내부 동작

### 왜 tasty 이름으로 프롬프트가 뜨는가

PTY 로 띄운 자식 프로세스(zsh, 그 안의 에이전트)가 보호 리소스에 접근하면 macOS 는 그 접근의 *responsible process* 를 부모 GUI 앱(`Tasty.app`)으로 귀속시킨다. Terminal.app / iTerm2 와 같은 구조다. tasty 자신이 그 폴더를 읽지 않아도 프롬프트는 tasty 이름으로 뜬다.

### 발화 방법

파일 계열 TCC 서비스에는 "미리 물어보는" API 가 없다. 유일한 방법은 앱이 그 경로를 스스로 한 번 건드리는 것 — 대상 디렉터리에 `read_dir` 을 1 회 호출하고 결과를 버린다. 성공/실패 어느 쪽이든 무시한다(**거부는 정상 결과**다 — 사용자의 정당한 선택이므로 `debug!` 로만 남기고 `warn!` 하지 않는다).

### 대상과 순서

| TCC 서비스 | 경로 | 조건 |
|---|---|---|
| `SystemPolicyDownloadsFolder` | `~/Downloads` | 존재할 때 |
| `SystemPolicyDocumentsFolder` | `~/Documents` | 존재할 때 |
| `SystemPolicyDesktopFolder` | `~/Desktop` | 존재할 때 |
| `SystemPolicyRemovableVolumes` / `SystemPolicyNetworkVolumes` | `/Volumes/<마운트>` | 마운트가 있을 때 |

- **홈 폴더 3 곳이 먼저, 볼륨이 마지막.** 네트워크 볼륨의 `read_dir` 은 응답 없는 마운트에서 수 초~수십 초 걸리거나 영영 안 끝날 수 있다. 앞에 두면 사용자가 실제로 겪는 홈 폴더 프롬프트가 그만큼 늦어진다.
- **볼륨은 depth-1 로만** 나열해 마운트 항목당 한 번씩만 건드린다. 하위로 내려가지 않는다.
- **존재하지 않는 경로는 건너뛴다** — 없는 폴더를 읽어봐야 프롬프트가 뜨지 않는다.
- 볼륨 목록은 **경로로 정렬**한다. `read_dir` 순서는 파일시스템 마음이라 정렬하지 않으면 프롬프트 순서가 실행마다 달라진다.

### 워커 스레드 · 순차 발화

프롬프트가 떠 있는 동안 `read_dir` 은 **사용자가 응답할 때까지 리턴하지 않는다.** 따라서 두 제약이 따른다:

- **단일 워커 스레드**로 분리한다. 메인 스레드(winit 이벤트 루프)에서 부르면 프롬프트가 떠 있는 내내 UI 가 얼고, `boot_total` 계측도 사용자 응답 시간만큼 부풀려진다.
- 그 **한 스레드에서 경로를 하나씩 순차로** 처리한다. 동시에 건드리면 프롬프트가 겹쳐 뜬다 — 순차면 앞의 것을 닫아야 다음이 뜬다.

호출 지점은 `finish_boot` 의 `emit_startup_complete_event()` 직후다. 첫 윈도우가 등록돼 앱이 foreground 로 활성화된 뒤라야 프롬프트가 사용자에게 보인다 — 윈도우 생성 전에 부르면 백그라운드로 밀릴 수 있다.

### 매 부팅 반복 (첫 실행 플래그 없음)

이미 허용/거부가 결정된 항목에는 프롬프트가 뜨지 않으므로 반복 비용은 `read_dir` 몇 번뿐이다. 오히려 반복이 정확하다:

- 새 마운트(이동식/네트워크 볼륨)는 실행할 때마다 달라져 첫 실행 1 회로는 못 덮는다
- "첫 실행 여부" 플래그를 설정에 두면, 플래그만 남고 TCC 는 초기화된 상태(재설치·`tccutil reset` 이후)에서 pre-warm 이 영영 안 도는 어긋남이 생긴다

따라서 상태 플래그도, 끄는 토글도 두지 않는다. 이 기능은 프롬프트를 없애는 게 아니라 시점을 앞당기는 것이라 끄면 원래의 산발적 프롬프트로 돌아갈 뿐이다.

### 화면 기록 (Screen Recording)

파일 계열과 달리 **미리 요청하는 공개 API 가 있다** — CoreGraphics 의 `CGPreflightScreenCaptureAccess()`(상태 조회, 프롬프트 없음)와 `CGRequestScreenCaptureAccess()`(프롬프트 발화). 리소스를 몰래 건드려 유도할 필요 없이 정식으로 물어본다. 두 함수는 새 크레이트 없이 `#[link(name = "CoreGraphics", kind = "framework")]` 로 직접 선언한다 — `surface.raw_key` 의 CoreGraphics 선언과 같은 방식이다.

파일 폴더를 모두 처리한 **뒤 같은 워커 스레드에서** 이어서 부른다(프롬프트가 겹치지 않게). 절차는 preflight → 미승인일 때만 request **1 회**다:

- 이미 승인돼 있으면 아무것도 하지 않는다(프롬프트 없음).
- 이미 거부된 상태면 `CGRequestScreenCaptureAccess()` 가 프롬프트 없이 즉시 false 를 반환한다. 그래서 **재시도 루프를 두지 않는다** — 되돌리는 것은 시스템 설정에서 사용자가 할 일이다.
- 권한을 켜면 앱 재시작이 필요한 경우가 있고, 그 안내는 macOS 가 자체적으로 띄운다.

캡처 시점의 소비는 [원격 스크린샷 → 클립보드](../remote-screenshot-clipboard/index.md) 가 담당한다 — `screen_recording_authorized()` 를 캡처 **직전**에 불러 권한 미승인을 사용자 취소와 구분한다. 비-macOS 에는 같은 이름의 함수가 항상 `true` 를 반환해 그쪽 캡처 경로가 기존과 똑같이 동작한다.

### 프롬프트 본문 설명 문구

번들 `Info.plist` 의 `NS*UsageDescription` 키가 프롬프트 본문에 그대로 표시된다. 키가 없으면 이유 없는 프롬프트가 뜨고, pre-warm 처럼 여러 개를 연달아 띄우면 그 문제가 커진다. 키 목록과 문구 정책은 [build.md](../../dev-guide/build.md) 의 배포 패키징 절 참조.

### 플랫폼 격리

목록 결정 로직(`prewarm_targets`)은 파일시스템 조회를 `FsProbe` 로 추상화한 **순수 함수**라 전 플랫폼에서 컴파일·유닛테스트된다. 실제 파일 접근부(`RealFs`·`spawn_prewarm`)만 `#[cfg(all(target_os = "macos", feature = "gui"))]` 로 좁힌다. cfg 로 잘린 코드는 rustc 가 타입체크 전에 걷어내므로, 로직을 순수부에 몰아둘수록 다른 플랫폼에서 검증되는 면적이 넓어진다. headless 는 프롬프트를 띄울 GUI 주체가 없으므로 macOS 여도 돌지 않는다. 비-macOS/headless 에는 같은 이름의 no-op 이 노출돼 호출부에 `#[cfg]` 가 흩어지지 않는다.

## 비-목표 (Out of scope)

**pre-warm 이 원천적으로 불가능한 것** — 대상별로 프롬프트가 갈라져 사전 열거가 안 된다:

- **다른 앱의 데이터** (`kTCCServiceSystemPolicyAppData`, macOS 14+) — `~/Library/Application Support/<앱>`, `~/Library/Containers/<번들ID>` 처럼 **대상 앱 디렉터리 단위로 개별 프롬프트**가 뜬다. 존재하는 디렉터리를 전부 순회하면 프롬프트가 수십 개 뜨므로 현실적 선택지가 아니다. Full Disk Access 로 덮인다 — FDA 안내는 이 기능 밖.
- **다른 앱 제어** (Automation / Apple Events) — 대상 앱 단위. 셸에서 `osascript` 를 쓸 때 발생하며 사전 열거 불가. **FDA 로도 덮이지 않는다** — FDA 는 파일 접근 서비스이고 Automation 은 완전히 별개 서비스라, FDA 를 줘도 대상 앱별 승인은 계속 요구된다. **어떤 방법으로도 사전 일괄 발화가 불가능**하며 tasty 가 할 수 있는 일이 없다. tasty 자신은 Apple Events 를 보내지 않는다.

**이 기능이 다루지 않는 다른 TCC 서비스**: 손쉬운 사용(`surface.raw_key` 의 `CGEventPost` 키 주입). 전용 요청 API 가 있어 파일 pre-warm 과 발화 방식이 다르다.

**해당 없음** (레포 전체 확인): 카메라·마이크·위치(`AVCaptureDevice`/`AVAudioSession`/`CLLocationManager` 미사용), OS 알림 센터(tasty 는 자체 토스트만 쓴다), Local Network(IPC 서버는 loopback bind — 대상 제외), 드래그앤드롭·네이티브 파일 피커(사용자가 그 자리에서 명시적으로 고른 파일이라 macOS 가 별도 동의로 취급, 프롬프트 없음).

## Acceptance Criteria

- [ ] Given `~/Documents` 가 없는 홈 When pre-warm 목록을 결정 Then 그 경로가 목록에서 빠진다
- [ ] Given `/Volumes` 에 마운트 2 개 When 목록을 결정 Then 홈 폴더 뒤에, 경로 정렬 순으로 붙는다
- [ ] Given 마운트가 하나도 없음 When 목록을 결정 Then 볼륨 항목이 하나도 없다
- [ ] Given macOS 에서 TCC 승인을 초기화 When Tasty 실행 Then 부팅 직후 프롬프트가 순차로 뜨고, 떠 있는 동안에도 창이 그려지고 클릭·스크롤이 반응한다
- [ ] Given 프롬프트를 모두 허용 When 터미널에서 `ls ~/Downloads; ls ~/Documents; ls ~/Desktop` Then 추가 프롬프트가 뜨지 않는다
- [ ] Given 화면 기록 권한 미결정 When Tasty 실행 Then 파일 프롬프트들 **뒤에** 화면 기록 프롬프트가 뜬다
- [ ] Given 화면 기록 권한을 거부한 뒤 재실행 Then 프롬프트가 다시 뜨지 않는다(무한 재요청 없음)
