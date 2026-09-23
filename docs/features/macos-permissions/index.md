# macOS 권한 (TCC)

- **Status**: Partial — 사용자가 요청할 때 파일 계열 + 화면 기록 일괄 발화(+ debug 빌드 한정 손쉬운 사용), Full Disk Access 추정, 미승인 권한 부팅 안내 구현
- **주체**: 로컬 사용자 (에이전트 작업 중 권한 요청으로 멈추는 상황도 줄인다)
- **ADR**: [0052](../../adr/0052-permission-prompts-are-raised-on-request-not-at-boot.md) (요청 시점) · [0012](../../adr/0012-request-admission-and-isolation.md) (손쉬운 사용 debug 격리)
- **코드**: `crates/tasty-platform/src/macos_permissions.rs` (목록 결정 + 워커 + CoreGraphics/ApplicationServices FFI + FDA 추정 + 표시용 스냅샷), 요청 호출부와 설정 탭 `src/view/settings/ui/tabs/macos_permissions.rs`, 안내 호출부 `src/app/boot_machine.rs::finish_boot`, 캡처측 소비처 `crates/tasty-platform/src/screen_capture.rs`, 키 주입측 소비처 `src/adapters/ipc/handler/input_source.rs`, 번들 usage description 은 `scripts/build-macos-dmg.sh` 의 Info.plist heredoc
- **화면**: 프롬프트는 OS 가 그린다. tasty 쪽 UI 는 부팅 안내 InfoModal + [설정 창](../settings/screens/settings.md) 일반 > 권한 탭

## 목적

macOS의 파일 접근 권한 요청은 보호된 경로에 처음 접근할 때 나타난다. 에이전트가 명령을
실행하는 중에도 사용자 응답을 기다릴 수 있으므로, 자주 사용하는 경로의 권한을 한 번에
미리 요청한다. 모든 권한 요청을 없애는 기능은 아니며 아래에 나열한 대상만 처리한다.

요청 시점은 **설정 > 일반 > 권한 의 [모든 권한 요청하기] 를 누른 때**다. 부팅 직후 자동
발화는 하지 않는다 — 앱을 처음 켠 사람에게 이유 없는 프롬프트가 줄줄이 뜨고 끄는 방법이
없었다([ADR-0052](../../adr/0052-permission-prompts-are-raised-on-request-not-at-boot.md)).
그 대신 미승인이 남아 있으면 부팅 안내로 알린다(아래 "부팅 권한 안내").

## 내부 동작

### 왜 tasty 이름으로 프롬프트가 뜨는가

PTY 로 띄운 자식 프로세스(zsh, 그 안의 에이전트)가 보호 리소스에 접근하면 macOS 는 그 접근의 *responsible process* 를 부모 GUI 앱(`Tasty.app`)으로 귀속시킨다. Terminal.app / iTerm2 와 같은 구조다. tasty 자신이 그 폴더를 읽지 않아도 프롬프트는 tasty 이름으로 뜬다.

<a id="발화-방법"></a>

### 권한 요청 방법

파일 계열 TCC 서비스에는 "미리 물어보는" API 가 없다. 유일한 방법은 앱이 그 경로에 한 번 접근하는 것 — 대상 디렉터리에 `read_dir` 을 1 회 호출하고 결과를 버린다. 성공/실패 어느 쪽이든 무시한다(**거부는 정상 결과**다 — 사용자의 정당한 선택이므로 `debug!` 로만 남기고 `warn!` 하지 않는다).

### 대상과 순서

| TCC 서비스 | 경로 | 조건 |
|---|---|---|
| `SystemPolicyDownloadsFolder` | `~/Downloads` | 존재할 때 |
| `SystemPolicyDocumentsFolder` | `~/Documents` | 존재할 때 |
| `SystemPolicyDesktopFolder` | `~/Desktop` | 존재할 때 |
| `SystemPolicyRemovableVolumes` / `SystemPolicyNetworkVolumes` | `/Volumes/<마운트>` | 마운트가 있을 때 |

- **홈 폴더 3 곳이 먼저, 볼륨이 마지막.** 네트워크 볼륨의 `read_dir` 은 응답 없는 마운트에서 수 초~수십 초 걸리거나 끝나지 않을 수 있다. 앞에 두면 사용자가 실제로 겪는 홈 폴더 프롬프트가 그만큼 늦어진다.
- **볼륨은 depth-1 로만** 나열해 마운트 항목당 한 번씩만 접근한다. 하위로 내려가지 않는다.
- **존재하지 않는 경로는 건너뛴다** — 없는 폴더를 읽어봐야 프롬프트가 뜨지 않는다.
- 볼륨 목록은 **경로로 정렬**한다. `read_dir` 순서는 파일시스템에 따라 달라서 정렬하지 않으면 프롬프트 순서가 실행마다 달라진다.

### 발화 시점 — 사용자의 요청 1 회

`request_all_permissions` 은 설정 > 일반 > 권한 의 [모든 권한 요청하기] 에서만 불린다. 부팅 경로에는 호출이 없다.

요청 중 재진입은 원자 플래그(`REQUEST_RUNNING`)로 막는다. 이미 도는 중이면 아무것도 하지 않고 `false` 를 돌려주고, 화면도 그동안 버튼을 비활성으로 그린다. 시퀀스가 겹치면 아래 "순차 발화" 전제가 깨져 프롬프트가 포개져 뜬다.

끝나면 표시용 스냅샷을 갱신하고 `on_finished` 로 호출자를 깨운다. 요청이 끝나는 시점에는 사용자 입력이 없어 repaint 가 저절로 안 난다. 순서는 갱신 → 플래그 해제 → 통지다 — 반대로 하면 호출자가 낡은 스냅샷을 읽는다.

<a id="워커-스레드--순차-발화"></a>

### 워커 스레드에서 순서대로 요청

프롬프트가 떠 있는 동안 `read_dir` 은 **사용자가 응답할 때까지 리턴하지 않는다.** 따라서 두 제약이 따른다:

- **단일 워커 스레드**로 분리한다. 메인 스레드(winit 이벤트 루프)에서 부르면 프롬프트가 떠 있는 내내 UI가 멈추고, `boot_total` 계측도 사용자 응답 시간만큼 부풀려진다.
- 그 **한 스레드에서 경로를 하나씩 순차로** 처리한다. 동시에 건드리면 프롬프트가 겹쳐 뜬다 — 순차면 앞의 것을 닫아야 다음이 뜬다.

워커 스레드는 egui draw 안에서 시작된다. draw 에서 시퀀스를 **직접** 돌리면 프롬프트가 떠 있는 내내 설정 창이 통째로 언다 — 그래서 버튼은 스레드를 띄우고 즉시 반환한다.

### 몇 번을 눌러도 무해하다 (요청 이력 플래그 없음)

이미 허용/거부가 결정된 항목에는 프롬프트가 뜨지 않으므로 반복 비용은 `read_dir` 몇 번뿐이다. 오히려 반복이 정확하다:

- 새 마운트(이동식/네트워크 볼륨)는 실행할 때마다 달라져 1 회로는 못 덮는다
- "이미 요청했다" 플래그를 설정에 두면, 플래그만 남고 TCC 는 초기화된 상태(재설치·`tccutil reset`·ad-hoc 재빌드로 앱 식별 변경 이후)에서 요청이 영영 안 도는 어긋남이 생긴다

따라서 요청 이력 플래그를 두지 않는다. 버튼은 언제 눌러도 같은 일을 한다.

### 화면 기록 (Screen Recording)

파일 계열과 달리 **미리 요청하는 공개 API 가 있다** — CoreGraphics 의 `CGPreflightScreenCaptureAccess()`(상태 조회, 프롬프트 없음)와 `CGRequestScreenCaptureAccess()`(프롬프트 발생). 리소스에 직접 접근하지 않고 이 API로 요청한다. 두 함수는 새 크레이트 없이 `#[link(name = "CoreGraphics", kind = "framework")]` 로 직접 선언한다 — `surface.raw_key` 의 CoreGraphics 선언과 같은 방식이다.

파일 폴더를 모두 처리한 **뒤 같은 워커 스레드에서** 이어서 부른다(프롬프트가 겹치지 않게). 절차는 preflight → 미승인일 때만 request **1 회**다:

- 이미 승인돼 있으면 아무것도 하지 않는다(프롬프트 없음).
- 이미 거부된 상태면 `CGRequestScreenCaptureAccess()` 가 프롬프트 없이 즉시 false 를 반환한다. 그래서 **재시도 루프를 두지 않는다** — 되돌리는 것은 시스템 설정에서 사용자가 할 일이다.
- 권한을 켜면 앱 재시작이 필요한 경우가 있고, 그 안내는 macOS 가 자체적으로 띄운다.

캡처 시점의 소비는 [원격 스크린샷 → 클립보드](../remote-screenshot-clipboard/index.md) 가 담당한다 — `screen_recording_authorized()` 를 캡처 **직전**에 불러 권한 미승인을 사용자 취소와 구분한다. 비-macOS 에는 같은 이름의 함수가 항상 `true` 를 반환해 그쪽 캡처 경로가 기존과 똑같이 동작한다.

### 손쉬운 사용 (Accessibility)

`surface.raw_key` 는 `CGEventPost` 로 시스템에 키 이벤트를 주입한다. 이 API 는 손쉬운 사용(`kTCCServiceAccessibility`) 권한을 요구하며, 권한이 없으면 **이벤트가 조용히 버려진다** — 호출자는 성공 응답을 받고도 아무 일도 일어나지 않는 것을 본다.

**이 기능은 debug 빌드 전용이다** — OS 전역 키 주입은 사용자 입력 재현이라 release IPC/CLI 표면에 없다([ADR-0012](../../adr/0012-request-admission-and-isolation.md), [debug-ipc](../../dev-guide/debug-ipc.md)). 따라서 **손쉬운 사용 권한을 소비하는 코드가 release 빌드에는 하나도 없다.**

손쉬운 사용 권한은 debug 빌드에서만 요청한다. release에는 이 권한을 사용하는 코드가
없으므로 요청 프롬프트나 활성화 안내를 표시하지 않는다.

화면 기록과 마찬가지로 **정식 요청 API 가 있다** — ApplicationServices 의 `AXIsProcessTrusted()`(프롬프트 없는 상태 조회)와 `AXIsProcessTrustedWithOptions()`(`kAXTrustedCheckOptionPrompt: kCFBooleanTrue` 를 넘기면 프롬프트). 새 크레이트 없이 `#[link(name = "ApplicationServices", kind = "framework")]` 로 선언한다. 옵션 딕셔너리는 `CFDictionaryCreate` 에 `kCFTypeDictionaryKeyCallBacks`/`ValueCallBacks` 를 함께 넘겨 만든다 — 콜백을 생략하면 키가 `CFEqual` 이 아니라 포인터 동일성으로 비교돼 옵션이 무시된다.

**시퀀스에서 맨 마지막**(debug 빌드). 파일 폴더 → 화면 기록 → 손쉬운 사용 순으로 같은 워커 스레드에서 이어 부른다. 앞의 둘은 그 자리에서 허용/거부가 끝나지만, 손쉬운 사용 프롬프트는 "시스템 설정을 열겠느냐"는 안내여서 **그 자리에서 권한이 켜지지 않고** 사용자를 시스템 설정으로 내보낸다. 그 이탈을 시퀀스 맨 끝에 둬야 앞의 프롬프트들이 묻히지 않는다. release 빌드의 시퀀스는 화면 기록에서 끝난다.

- 이미 승인돼 있으면(`AXIsProcessTrusted()`) 요청하지 않는다.
- 요청은 **부팅당 1 회**다. 미설정·거부 상태에서 반복 호출하면 프롬프트가 계속 뜬다.
- 권한을 켜면 실행 중인 프로세스에 즉시 반영되지 않아 **앱 재시작이 필요한 경우가 많다.**

**주입 시점의 소비** (debug 빌드) — `handle_raw_key` 는 주입 직전에 `AXIsProcessTrusted()` 를 부르고, 미승인이면 `-32001 permission_denied: …` 에러를 돌려준다(권한 계열 거부의 기존 코드·접두사와 같다). 이 조회는 **호출 시점마다** 한다 — 부팅 값을 캐시하면 사용자가 그 사이 시스템 설정에서 켠 것을 반영하지 못한다. 판정 자체(`raw_key_decision`)는 `#[cfg(any(debug_assertions, test))]` 순수 함수라 전 플랫폼에서, release 빌드에서도 유닛테스트된다. 주입 경로에는 `--enable-input-simulation` 런타임 게이트가 손쉬운 사용 권한 확인보다 **먼저** 걸린다 — 플래그 없이 띄운 debug 인스턴스에서는 권한 여부와 무관하게 `-32001` 로 거부된다.

### Full Disk Access — 추정과 안내

FDA(`kTCCServiceSystemPolicyAllFiles`)를 부여하면 "다른 앱의 데이터" 를 포함한 **파일 접근 계열 전부**가 프롬프트 없이 통과한다. 대상 앱 디렉터리 단위로 갈라져 사전 발화가 불가능한 AppData 계열을 없앨 수 있는 유일한 수단이다.

**앱이 요청할 수 없다.** 요청 API 가 없고 `tccutil`/TCC.db 조작은 SIP 가 막는다. tasty 가 할 수 있는 것은 (a) 보유 추정과 (b) 해당 패널로 보내는 안내뿐이다.

**추정 방법과 그 한계** — FDA 로만 읽히는 것으로 알려진 경로(`/Library/Application Support/com.apple.TCC/TCC.db`, 보조로 사용자 홈의 같은 경로)를 열어본다. 그 경로는 거부될 때 **프롬프트 없이 조용히** `EPERM` 을 내므로 안전하게 시도할 수 있다. 다만 이는 공개 API 가 아니라 우회 판정이며, macOS 가 그 경로의 보호 정책을 바꾸면 **오탐**이 난다. 그래서 이 값은 **안내를 띄울지 여부에만** 쓰고 어떤 기능도 이 값으로 막지 않는다.

판정은 `FullDiskAccess::{Granted, Denied, Unknown}`의 세 상태다. 파일을 열면 허용,
`PermissionDenied`면 미허용, 경로가 없는 경우 등은 확인 불가로 처리한다. 경로 부재를
미허용으로 처리하면 macOS가 파일을 옮기거나 없앤 환경에서 매 부팅마다 잘못 안내할 수 있다.

**안내 방식** — 부팅 안내는 FDA 전용이 아니라 **권한 전반**의 안내다. 아래 "부팅 권한 안내" 절 참조.

안내를 표시했다는 기록이나 끄는 토글은 두지 않고 매 부팅마다 다시 판정한다. 사용자가
권한을 취소하거나 `tccutil reset`, ad-hoc 재빌드로 앱 식별이 바뀐 경우도 반영하기 위해서다.
권한이 확인되면 다음 부팅부터 안내하지 않는다. 파일 권한 요청이 "이미 요청했다" 플래그를
두지 않는 것과 같은 규율이다.

**안내 문구가 지켜야 할 것** — FDA 는 파일 접근 프롬프트만 없앤다. **Automation(다른 앱 제어) · 화면 기록 · 손쉬운 사용은 FDA 와 별개 TCC 서비스라 그대로 남는다.** 문구가 "모든 프롬프트가 사라진다" 로 읽히면 안 된다. 또 ad-hoc 서명 빌드는 재빌드마다 다른 앱으로 인식돼 FDA 가 초기화되므로, 직접 빌드하는 사용자에게 `Tasty Dev` 인증서 서명이 선행 조건임을 함께 알린다([build.md](../../dev-guide/build.md) 참조).

### 부팅 권한 안내

부팅 시 **확인 가능한 권한 중 하나라도 미승인으로 확인되면** InfoModal 을 띄운다(`should_show_permission_notice`). 판정 입력은 FDA 추정 3 상태와 화면 기록 승인 여부 둘이다. 모달의 [권한 설정 열기] 는 **Tasty 의 권한 화면**(설정 > 일반 > 권한)을 연다 — OS 패널이 아니다. 권한 확인과 요청을 한자리에 모으는 것이 목적이고, 안내는 그 자리로 가는 입구다. FDA 는 결국 시스템 설정으로 가야 하지만 그 링크는 권한 화면 안에 이미 있다(아래 "설정 탭").

**2 단계가 되는 것은 의도다.** 버튼은 popup draw 에서 곧장 창을 열지 않는다 — draw 는 `AppState` 만 들고 있어 winit `EventLoopProxy` 에 닿지 않기 때문이다. 대신 `dialogs.permission_settings_requested` 슬롯을 세우고, App 레이어의 `dispatch_pending_info_modal_requests` 가 프레임 시작에 드레인해 `AppEvent::OpenSettings` 를 보낸다(file handler picker 의 result 슬롯과 같은 형태). 진입 탭은 **L1 `General` + L2 `MacosPermissions` 를 함께** 세운다 — L1 만 맞추면 일반 탭 기본 화면이 열리고, 그 오답은 "설정 창이 열렸다" 로는 안 보인다.

**설정 창이 이미 열려 있으면** 새 창을 요청하지 않고 그 창의 탭을 바꾸고 포커스를 준다. `open_settings_modal` 이 모달이 이미 있으면 즉시 return 하므로, 그 갈래에서 1 회성 플래그를 세우면 소비되지 않은 채 남아 **다음번에 설정 창을 열 때** 엉뚱하게 권한 탭으로 튄다.

**버튼을 눌러도 안내는 닫히지 않는다.** 설정 창은 별도 winit 창이라 안내를 덮을 수 있고, 그 창을 닫고 나면 무엇을 왜 켜야 했는지 다시 읽을 자리가 필요하다. [확인] 을 눌러야 큐에서 빠진다.

**FDA 만 보면 안 되는 이유** — FDA 를 이미 가진 사용자는 화면 기록이 미승인이어도 아무 안내를 못 받는다. 화면 기록은 한 번 거부하면 앱이 다시 물을 수 없고 시스템 설정에서만 되돌릴 수 있어, FDA 와 똑같은 발견성 문제를 갖는다.

**판정에 넣을 수 있는 것과 없는 것이 갈린다.**

- 넣는다: FDA(경로 프로브 추정), 화면 기록(`CGPreflightScreenCaptureAccess` — 프롬프트 없는 정식 조회)
- **넣을 수 없다: 파일 폴더**(다운로드·문서·데스크탑·볼륨). 파일 계열 TCC 에는 preflight API 가 없고, 상태를 알아보는 유일한 수단인 `read_dir` 이 미결정 상태에서는 곧 프롬프트다(위 "발화 방법"). 판정하려고 재는 것 자체가 사용자가 누르지 않은 프롬프트를 띄우는 일이 된다.
- 넣지 않는다: 손쉬운 사용. release 에 그 권한을 소비하는 코드가 없어(위 "손쉬운 사용") 켤 이유가 없는 항목으로 안내를 띄우게 된다.

**따라서 구조적 사각이 남는다** — FDA 와 화면 기록을 모두 허용했지만 파일 폴더를 한 번도 요청하지 않은 사용자에게는 안내가 뜨지 않는다. 감추지 않고 여기에 적어 둔다. 이 사각을 없애려면 파일 폴더 상태를 재야 하고, 재는 순간 프롬프트가 뜬다.

**`Unknown` 을 미승인으로 접지 않는다** — FDA 판정은 경로 프로브 우회라 macOS 가 그 경로를 옮기면 근거 자체가 사라진다. 그것을 미승인으로 세면 승인을 가진 사용자 전원에게 안내가 매 부팅 뜨고, 안내에는 "다시 보지 않기" 가 없으므로 탈출구가 없다. 다만 화면 기록 쪽 근거는 확실하므로, FDA 가 `Unknown` 이어도 화면 기록이 미승인이면 안내는 뜬다.

### 설정 탭 (일반 > 권한)

macOS 에서만 노출된다. **권한 요청이 시작되는 유일한 자리다.** 담는 것은 상태 행(FDA 추정 · 화면 기록 · 파일 폴더 · debug 의 손쉬운 사용), FDA 가 추정임을 밝히는 주석, 버튼 둘([모든 권한 요청하기] · [전체 디스크 접근 권한 설정 열기]), 그리고 그 아래 설명 줄이다. 부팅 안내를 지나쳤어도 여기서 현재 상태를 볼 수 있다.

FDA 행은 3 상태를 그대로 보여준다 — 허용됨 / 허용 안 됨 / **확인 불가**. 판정 근거가 없는 상태를 "허용 안 됨" 으로 적으면 이미 허용된 사용자에게도 잘못 안내할 수 있다.

파일 폴더 행은 상태를 못 적는다 — 조회 수단 자체가 없어 "조회 수단 없음" 을 적는다. 빈칸으로 두면 승인된 것으로 읽힌다.

요청 중에는 [모든 권한 요청하기] 가 비활성이고 아래 설명 줄이 진행 문구로 바뀐다. 비활성은 플랫폼 쪽 원자 교환 거부를 눈에 보이게 하는 것이지 유일한 방어가 아니다.

**상태는 이 화면이 재지 않는다 — 스냅샷을 읽는다.** 측정(FDA 프로브의 `File::open`, `CGPreflightScreenCaptureAccess`, debug 의 `AXIsProcessTrusted`)은 `refresh_permission_snapshot()` 안에서만 일어나고, draw 는 `permission_snapshot()` 으로 보관된 값을 읽기만 한다. 측정을 draw 에 두면 TCC 데몬 IPC 가 프레임마다 반복되고(설정 창은 dirty 구동이라 가만히 두면 안 돌지만, 마우스 이동·호버처럼 repaint 를 요구하는 입력이 있는 동안에는 그 횟수만큼 돈다), tccd 응답이 늦는 만큼 창이 멈춘다.

**갱신 트리거는 셋이다** — 부팅 1 회(`wants_full_disk_access_notice` 가 재는 그 측정을 그대로 보관한다. 부팅 안내 판정과 화면 표시가 같은 측정 1 회를 공유한다), 권한 화면 L2 진입(`apply_l2_select`), **설정 창 포커스 복귀**(`SettingsView::handle_event` 의 `WindowEvent::Focused(true)`). 마지막 것이 빠지면 캐시가 곧 오판이 된다 — FDA 는 앱이 요청할 수 없어 사용자가 시스템 설정에 다녀오는 왕복이 반드시 생기고, 화면 기록도 거부 이후에는 시스템 설정에서만 되돌릴 수 있는데, 그 복귀가 값이 달라지는 유일한 순간이기 때문이다.

**캡처·주입 경로는 이 스냅샷을 쓰지 않는다.** `screen_capture.rs` 의 `screen_recording_authorized()` 와 `input_source.rs` 의 `accessibility_trusted()` 는 그 동작 직전 실측이 의도된 정책이다(위 "화면 기록" · "주입 시점의 소비"). 표시용 캐시로 묶으면 캡처·주입이 낡은 값으로 판정하게 된다.

**손쉬운 사용 상태 행은 debug 빌드에만 있다.** release 에는 이 권한을 소비하는 코드가 없어 프롬프트도 띄우지 않으므로, 행을 남기면 켤 이유도 끌 이유도 없는 항목이 영구히 "미승인" 으로 보인다. debug 빌드에서는 주입 경로를 자기검증할 때 승인 상태를 확인할 자리가 필요해 그대로 둔다.

### 프롬프트 본문 설명 문구

번들 `Info.plist` 의 `NS*UsageDescription` 키가 프롬프트 본문에 그대로 표시된다. 키가 없으면 이유 없는 프롬프트가 뜨고, 일괄 요청처럼 여러 개를 연달아 띄우면 그 문제가 커진다. 키 목록과 문구 정책은 [build.md](../../dev-guide/build.md) 의 배포 패키징 절 참조.

### 플랫폼 격리

목록 결정 로직(`prewarm_targets`)은 파일시스템 조회를 `FsProbe` 로 추상화한 **순수 함수**라 전 플랫폼에서 컴파일·유닛테스트된다. 실제 파일 접근부(`RealFs`·`request_all_permissions`)만 `#[cfg(all(target_os = "macos", feature = "gui"))]` 로 좁힌다. cfg 로 잘린 코드는 rustc 가 타입체크 전에 걷어내므로, 로직을 순수부에 몰아둘수록 다른 플랫폼에서도 더 많은 로직을 검증할 수 있다. headless 는 프롬프트를 띄울 GUI 주체가 없으므로 macOS 여도 돌지 않는다. 비-macOS/headless 에는 같은 이름의 no-op 이 노출돼 호출부에 `#[cfg]` 가 흩어지지 않는다.

## 비-목표 (Out of scope)

**사전 발화가 원천적으로 불가능한 것** — 대상별로 프롬프트가 갈라져 사전 열거가 안 된다:

- **다른 앱의 데이터** (`kTCCServiceSystemPolicyAppData`, macOS 14+) — `~/Library/Application Support/<앱>`, `~/Library/Containers/<번들ID>` 처럼 **대상 앱 디렉터리 단위로 개별 프롬프트**가 뜬다. 존재하는 디렉터리를 전부 순회하면 프롬프트가 수십 개 뜨므로 현실적 선택지가 아니다. Full Disk Access 로 덮이며, 그 안내는 위 "Full Disk Access" 절이 담당한다.
- **다른 앱 제어** (Automation / Apple Events) — 대상 앱 단위. 셸에서 `osascript` 를 쓸 때 발생하며 사전 열거 불가. **FDA 로도 덮이지 않는다** — FDA 는 파일 접근 서비스이고 Automation 은 완전히 별개 서비스라, FDA 를 줘도 대상 앱별 승인은 계속 요구된다. tasty는 이를 미리 일괄 요청하지 않는다. tasty 자신은 Apple Events 를 보내지 않는다.

**해당 없음** (레포 전체 확인): 카메라·마이크·위치(`AVCaptureDevice`/`AVAudioSession`/`CLLocationManager` 미사용), OS 알림 센터(tasty 는 자체 토스트만 쓴다), Local Network(IPC 서버는 loopback bind — 대상 제외), 드래그앤드롭·네이티브 파일 피커(사용자가 그 자리에서 명시적으로 고른 파일이라 macOS 가 별도 동의로 취급, 프롬프트 없음).

## Acceptance Criteria

- Given `~/Documents` 가 없는 홈 When 요청 목록을 결정 Then 그 경로가 목록에서 빠진다
- Given `/Volumes` 에 마운트 2 개 When 목록을 결정 Then 홈 폴더 뒤에, 경로 정렬 순으로 붙는다
- Given 마운트가 하나도 없음 When 목록을 결정 Then 볼륨 항목이 하나도 없다
- Given macOS 에서 TCC 승인을 초기화 When Tasty 실행 Then 권한 프롬프트가 하나도 뜨지 않는다
- Given 같은 상태 When 설정 > 일반 > 권한 의 [모든 권한 요청하기] 클릭 Then 프롬프트가 순차로 뜨고, 떠 있는 동안에도 설정 창이 그려지고 클릭·스크롤이 반응한다
- Given 요청이 도는 중 When 버튼을 다시 누름 Then 두 번째 시퀀스가 시작되지 않는다
- Given 요청이 끝남 When 화면을 봄 Then 상태 표시가 갱신돼 있다
- Given 프롬프트를 모두 허용 When 터미널에서 `ls ~/Downloads; ls ~/Documents; ls ~/Desktop` Then 추가 프롬프트가 뜨지 않는다
- Given 화면 기록 권한 미결정 When 요청 Then 파일 프롬프트들 **뒤에** 화면 기록 프롬프트가 뜬다
- Given 화면 기록 권한을 거부한 뒤 다시 요청 Then 프롬프트가 다시 뜨지 않는다(무한 재요청 없음)
- Given release 빌드 When 요청 Then 손쉬운 사용 프롬프트는 뜨지 않는다
- Given FDA 프로브 경로가 열리고 화면 기록도 승인됨 When 부팅 Then 안내를 띄우지 않는다
- Given FDA 프로브가 `PermissionDenied` 로 거부됨 When 부팅 Then 안내를 띄운다 — 몇 번째 부팅인지는 보지 않는다
- Given FDA 는 허용됐지만 화면 기록이 미승인 When 부팅 Then 안내를 띄운다
- Given FDA 프로브 경로가 하나도 존재하지 않음(`NotFound`) + 화면 기록 승인 When 부팅 Then 판정 불가로 보고 안내를 띄우지 않는다
- Given FDA 판정 불가 + 화면 기록 미승인 When 부팅 Then 안내를 띄운다 — 화면 기록 쪽 근거는 확실하다
- Given FDA 와 화면 기록을 모두 허용했지만 파일 폴더는 한 번도 요청하지 않음 When 부팅 Then 안내가 뜨지 않는다 (알려진 사각 — 재는 행위 자체가 프롬프트다)
- Given 안내를 본 뒤 권한을 부여 When 재부팅 Then 안내가 뜨지 않는다
- Given 안내를 본 뒤 권한을 주지 않음 When 재부팅 Then 안내가 다시 뜬다
- Given ad-hoc 서명 빌드에 FDA 를 준 뒤 재빌드 When 부팅 Then 승인이 초기화돼 안내가 다시 뜬다
- Given 권한 탭이 열려 있음 When 그 위에서 마우스를 움직여 repaint 가 반복됨 Then TCC 측정은 한 번도 더 일어나지 않는다
- Given 권한 탭이 열린 설정 창 When 시스템 설정에 다녀와 그 창에 포커스가 돌아옴 Then 상태를 다시 재서 표시가 갱신된다
- Given FDA 안내가 떠 있음 When 설정 열기 버튼 클릭 Then 전체 디스크 접근 권한 패널이 열린다
- Given 손쉬운 사용 권한 미결정 When debug 빌드 실행 Then 화면 기록 프롬프트 **뒤에** 손쉬운 사용 프롬프트가 뜬다
- Given 손쉬운 사용 권한 미결정 When release 빌드 실행 Then 손쉬운 사용 프롬프트가 뜨지 않는다(요청 자체가 없다)
- Given 손쉬운 사용 권한이 이미 승인됨 When debug 빌드 실행 Then 프롬프트가 뜨지 않는다
- Given 손쉬운 사용 권한 미승인 + `--enable-input-simulation` 으로 띄운 debug 빌드 When `surface.raw_key` 호출 Then 성공이 아니라 `permission_denied` 에러가 돌아온다
- Given 손쉬운 사용 권한 승인 후 재시작 + `--enable-input-simulation` 으로 띄운 debug 빌드 When `surface.raw_key` 호출 Then OS 포커스를 가진 대상에 키가 실제로 입력된다
- Given `--enable-input-simulation` 없이 띄운 debug 빌드 When `surface.raw_key` 호출 Then 손쉬운 사용 권한 여부와 무관하게 `-32001` 로 거부된다
- Given release 빌드 When `surface.raw_key` 호출 Then `method_not_found` 로 떨어진다(메서드가 표에도 라우터에도 없다)
- Given release 빌드 When 설정 > 일반 > 권한 탭 열기 Then 손쉬운 사용 상태 행이 없다(FDA·화면 기록 두 행만)
- Given debug 빌드 When 설정 > 일반 > 권한 탭 열기 Then 손쉬운 사용 상태 행이 있다
