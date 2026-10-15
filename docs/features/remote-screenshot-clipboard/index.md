# 원격 스크린샷 → 클립보드 (Remote screenshot to clipboard)

- **Status**: Implemented
- **주체**: 로컬 사용자 (단축키 트리거)
- **ADR**: 없음 (기존 [ADR-0020](../../adr/0020-remote-connection-profiles.md) attach 채널을 그대로 재사용, 신규 프로토콜/enum 없음)
- **코드**: `crates/tasty-platform/src/screen_capture.rs`(OS 캡처 + `CaptureError`), `crates/tasty-platform/src/macos_permissions.rs`(화면 기록 권한 preflight), `src/app/screenshot_capture.rs`(폴링/스레드), `crates/tasty-settings/src/keybindings.rs`(`screenshot_to_clipboard`), `src/adapters/ui/input/shortcuts/keybinding.rs`(`match_capture_bindings`), `src/app/attach_client/bulk.rs`(원격 전송), `src/remote/capture_upload.rs` + `src/remote/server.rs`(`finalize_capture_upload`, 서버측 수신), `crates/tasty-ipc/src/stream_hub.rs`(`CaptureUploadMsg`), `crates/tasty-ipc/src/method_meta.rs`(`clipboard.set_text`), `src/app/ipc/app_methods.rs`(`ipc_handle_clipboard_set_text`)
- **화면**: 없음 — 트리거는 단축키, 피드백은 성공/실패 토스트(mirror 케이스만; [remote-attach 토스트](../remote-attach/index.md) 채널 재사용)

## 목적

포커스된 surface 가 **mirror(원격 attach) 워크스페이스**에 속할 때, 인터랙티브 화면 캡처 결과를 로컬이 아니라 **원격 인스턴스의 클립보드**에 반영한다. 로컬 surface 를 보며 찍은 스크린샷이 로컬 클립보드로 가는 기존 동작(OS 네이티브 캡처 → `ClipboardSystem::write_text(경로)`)은 그대로 유지하고, mirror 포커스일 때만 캡처 파일을 attach 채널로 원격에 올려 원격 클립보드에 쓴다 — "찍은 화면이 실제로 보고 있는 머신의 클립보드로 간다"는 사용자 기대를 만족시키기 위함.

## 내부 동작 (headless-valid)

### 로컬 vs mirror 판별

키바인딩 매치 시점에 읽기 projection에서 원 surface와 mirror workspace를 고정하고 `EngineAction::Screenshot`에 원 View를 담는다. App은 원 target이 유효한 경우에만 해당 `EngineSession.remote.pending_screenshot_captures`에 `(mirror_workspace, view)`를 넣는다. 현재 focus를 나중에 다시 조회해 목적지를 바꾸지 않는다.

### 캡처 (OS 네이티브, 인터랙티브)

`about_to_wait` 에서 큐를 drain해 요청마다 백그라운드 스레드로 OS 캡처를 실행(`crate::platform::screen_capture::capture_interactive`, 블로킹 UI 스레드 방지 — App 소유 pending 요청과 결과 채널 사용).

- **macOS**: `screencapture -i` (사용자가 영역 선택, Esc 로 취소 가능).
- **Linux**: Wayland 는 `grim`+`slurp` → `gnome-screenshot -a` 순, X11 은 `gnome-screenshot -a` → `scrot -s` → ImageMagick `import` 순으로 시도한다. 후보는 `PATH` 에서 실행 가능한 파일을 찾아 고른다. `grim`+`slurp` 는 둘 다 있을 때만 후보가 되므로 `slurp` 만 있으면 선택 UI를 열지 않고 다음 후보로 넘어간다.
- **Windows**: 인터랙티브 영역선택에 쓸 만한 OS 표준 동기 CLI 가 없어(Snipping Tool `ms-screenclip:` 은 파일이 아니라 클립보드 이미지로만 출력) PowerShell + `System.Drawing`/`System.Windows.Forms.SystemInformation.VirtualScreen` 로 **전체 가상화면**을 캡처한다(알려진 한계, 인터랙티브 영역선택 아님).
- 저장 위치: `~/.tasty/screenshots/screenshot-<epoch_ms>.png`.

### 결과 분류

캡처 결과는 파일 존재, 종료 상태, 도구가 내는 취소 메시지를 함께 보고 `CaptureError` 로 분류한다. 취소의 종료 코드는 도구와 버전마다 달라, 도구가 취소를 구별 가능한 신호로 알리지 않는 결과는 사용자 취소로 확정하지 않는다.

| 결과 | 판정 | 다음 후보 |
|---|---|---|
| 성공 | 정상 종료하고 파일을 남겼으며 PNG 서명과 마지막 IEND 청크가 맞다 | — |
| 손상 파일 | 정상 종료했지만 PNG 서명이나 IEND 청크가 없다(잘린 파일·다른 형식) | 파일을 지우고 시도 |
| 도구 없음 | 실행 파일이 없거나 실행 권한이 없다. 선택 UI를 열지 않는다 | 시도 |
| 도구 실패 | 비정상 종료했고 취소 메시지가 없다(백엔드 미지원, 저장 실패 등). 남은 부분 파일은 지운다 | 시도 |
| 사용자 취소 (`Cancelled`) | 비정상 종료와 도구의 취소 메시지가 함께 있다 | 멈춤 |
| 출력 없음 (`NoOutput`) | 파일이 없고 취소 신호도 없다. 취소인지 출력 없는 실패인지 구별할 수 없다 | 멈춤 |

- 모든 후보가 없으면 `Unavailable`, 하나 이상 실행했지만 모두 실패하면 각 후보의 실패 사유를 모은 `Tool` 오류다.
- 저장 디렉터리를 만들지 못하거나 mirror 전송을 위해 저장된 파일을 다시 읽지 못하면 `Storage` 다. 도구가 지정 경로에 쓰지 못한 경우는 도구 실패로 나타난다.
- 사용자 취소와 출력 없음에서 멈추는 것은 사용자가 이미 닫은 선택 UI 뒤에 다른 도구의 선택 UI를 연달아 열지 않기 위해서다. 두 경우 모두 클립보드를 바꾸지 않는다.

도구별 판정은 다음과 같다.

| 도구 | 사용자 취소 | 출력 없음 | 근거 |
|---|---|---|---|
| `scrot -s` | 종료 코드 1과 `aborting shot` 메시지 | 정상 종료인데 파일 없음 | scrot 1.10 실측: Esc 에 `Key was pressed, aborting shot` 와 종료 코드 1, 저장 경로 오류에 `Saving to file … failed` 와 종료 코드 1, X 디스플레이 없음에 종료 코드 1 |
| `slurp` | 종료 코드 1과 `selection cancelled` 메시지 | 정상 종료인데 선택 영역이 비었음 | 소스 기준. Wayland 도구가 없는 환경이라 실제 실행은 하지 않았다 |
| `grim` | 없음 | 정상 종료인데 파일 없음 | 비정상 종료는 영역 선택 이후의 도구 실패로 보고 다음 후보를 시도한다 |
| `gnome-screenshot -a` | 없음 | 정상 종료인데 파일 없음 | gnome-screenshot 41.0 실측: Esc 에 종료 코드 0과 파일 없음. 종료 상태로 취소와 실패를 구별할 수 없다 |
| `import` | 없음 | 정상 종료인데 파일 없음 | 설치되지 않은 환경이라 실제 실행은 하지 않았다 |
| `screencapture -i` (macOS) | 없음 | 파일 없음(종료 상태와 무관) | 취소의 종료 코드가 macOS 버전마다 달라 파일이 없으면 출력 없음으로 둔다. 실 기기 측정은 하지 않았다 |
| PowerShell (Windows) | 없음(선택 UI 없음) | 없음 | 비정상 종료, 그리고 정상 종료인데 파일 없음은 모두 도구 실패다. 실 기기 측정은 하지 않았다 |

### 화면 기록 권한 (macOS)

macOS 에서 `screencapture` 는 **화면 기록(Screen Recording) 권한**을 요구한다. 권한이 없으면 파일을 전혀 남기지 않으므로, "파일 존재 여부" 판정만으로는 **사용자 취소와 구분되지 않는다** — 사용자에겐 둘 다 "아무 일도 안 일어남"으로 보인다.

그래서 권한 미승인은 캡처 **직전** `CGPreflightScreenCaptureAccess()` 가 false 일 때 `PermissionDenied` 로 따로 분류하고 도구를 실행하지 않는다. 처리는 `toast.screen_recording_permission_required` 안내 토스트와 `warn!` 이다. 권한이 있는데 파일이 없으면 위 "결과 분류" 의 출력 없음이다.

preflight 는 **캡처 직전**에 부른다 — 부팅 시점 값을 캐시해두면 그 사이 사용자가 시스템 설정에서 권한을 바꾼 경우를 잘못 판정한다. 안내 토스트는 요청이 올라온 윈도우에 띄운다(다중 윈도우 세션에서 엉뚱한 창에 뜨지 않게).

권한 프롬프트는 자동으로 뜨지 않는다. 사용자가 설정 > 일반 > 권한에서 [모든 권한 요청하기]를 눌렀을 때만 나타난다([macOS 권한](../macos-permissions/index.md)). 그 화면을 한 번도 열지 않았다면 캡처 시점에 권한이 없을 수 있고, 그때는 캡처가 "화면 기록 권한이 필요합니다" 안내로 끝난다. macOS가 아닌 환경에서는 이 검사를 하지 않으며 preflight가 항상 승인됨을 반환한다.

### 로컬 케이스

캡처 성공 시 파일 경로 문자열을 `ClipboardSystem::write_text(path)` 로 로컬 클립보드에 쓴다(기존 `ui.screenshot`/이미지 붙여넣기와 동일한 "경로를 클립보드에 넣는다" 관례).

### 원격(mirror) 케이스 — 기존 attach 채널 확장 (신규 프로토콜 없음)

**채널**: attach 세션의 기존 writer/reader와 `StreamTag::Control`을 재사용한다.
`StreamControl` enum에 없는 이벤트를 별도로 파싱해 아래 세 메시지를 처리한다.

- `capture_chunk` { `upload_id`, `seq`, `total`, `data_b64` } — 캡처 파일을 raw 700KiB 청크 단위로 base64 인코딩해 전송(base64 인플레이션 후에도 `MAX_FRAME_LEN`(1MiB) 아래 유지).
- `capture_commit` { `upload_id`, `file_name` } — 마지막 청크 뒤 1회, 업로드 종료를 알림.
- `capture_result` { `ok`, `path?`, `reason?` } — 서버(원격)→client 회신.

client 측(`src/app/attach_client/bulk.rs::forward_capture_to_remote_clipboard`)은 로컬 워크스페이스 id 로 해당 attach 세션을 찾아 청크+커밋을 순서대로 보낸다. 서버(원격 인스턴스) 측은 `stream_hub.rs::pump_inbound` 가 `StreamControl` 파싱 실패 시 `CaptureUploadMsg` 로 재시도해 분류하고, `CaptureUploadRegistry`(`(client_id, upload_id)` 키)에 청크를 누적하다 커밋에서 `attach_runtime::finalize_capture_upload` 를 호출한다.

**서버측 처리(`finalize_capture_upload`)**:
1. 그 client 가 해당 engine의 workspace를 hard 점유 중인지(`OccupancyRegistry::client_holds_workspace`) 확인 — attach 연결 자체가 이미 권한 경계이므로 별도 인증 계층을 새로 만들지 않는다(구조 변경 forward 와 동일한 신뢰 모델).
2. `file_name` 을 **basename 만** 취해(path traversal 방지) `~/.tasty/screenshots/<name>` 에 저장.
3. `Core::clipboard_arc().write_text(경로)` 로 **원격 인스턴스의** 클립보드에 쓴다(`clipboard.set_text` IPC 와 동일 코드 경로, 다만 attach 미니 프로토콜은 이 IPC 를 거치지 않고 직접 `ClipboardSystem` 을 호출한다 — 같은 프로세스 내부 호출이라 JSON-RPC 왕복이 불필요).
4. 결과를 `capture_result` 프레임으로 client 에 회신.

client 는 `capture_result` 를 받아 성공/실패 토스트(`attach.toast.mirror_capture_saved`/`attach.toast.mirror_capture_failed`)를 띄운다.

## 인터페이스

- **사용자 트리거**: `screenshot_to_clipboard` 키바인딩(`KeybindingSettings`, 기본 `ctrl+alt+s`, 전 프리셋 동일값). Settings → Keybindings → Clipboard 탭에서 편집.
- **AI Agent (IPC/CLI)**: `clipboard.set_text` — 로컬 클립보드에 텍스트를 쓴다. IPC `{"method":"clipboard.set_text","params":{"text":"..."}}`(plugin 은 `Permission::ClipboardWrite` 필요) / CLI `tasty clipboard set-text <text>`. 원격 mirror 캡처 전송 경로가 원격 인스턴스에서 최종적으로 클립보드에 반영하는 것과 동일한 하부 동작(`ClipboardSystem::write_text`)을 노출한 것 — attach 미니 프로토콜 자체는 이 IPC 를 경유하지 않는다(위 참고).
- **원격 / 점유**: mirror 워크스페이스에 대해서만 원격 전송이 발생한다. 대상 attach 세션이 이미 그 워크스페이스를 점유하고 있어야 하며(mirror 존재의 전제조건), 점유가 아니면(이론상 불가능한 상태) 서버가 거부한다.

## 비-목표 (Out of scope)

- **원격 스크린샷 파일 자체를 로컬로 가져오는 것** — 반대 방향(원격→로컬)이며 이 기능의 범위가 아니다.
- **scp/ssh-exec 등 attach 와 무관한 별도 전송 채널** — 명시적으로 채택하지 않음(기존 attach 채널 재사용으로 확정).
- **`StreamControl` enum 확장** — 별도의 이벤트 태그를 같은 `StreamTag::Control` 채널에서 처리한다.
- **비-mirror(일반 attach 없는) 원격 클립보드 반영** — mirror 판별은 오직 `Workspace.mirror` 로만 하며, surface 단위 attach(워크스페이스 아님)는 이 판별 대상이 아니다.

## Acceptance Criteria

- Given 로컬(비-mirror) surface 에 포커스 When `screenshot_to_clipboard` 트리거 Then OS 인터랙티브 캡처 후 캡처 파일 경로가 로컬 클립보드에 쓰인다.
- Given mirror 워크스페이스의 surface 에 포커스 When `screenshot_to_clipboard` 트리거 Then 캡처 파일이 attach 채널로 원격에 전송되고, 원격 인스턴스의 클립보드에 그 원격 경로가 쓰인다(로컬 클립보드는 바뀌지 않는다).
- Given 캡처가 사용자에 의해 취소됨(Esc 등) Then 로컬/원격 어느 클립보드도 바뀌지 않는다.
- Given Linux 에서 첫 캡처 도구가 비정상 종료하고 다음 후보가 설치됨 When 캡처 트리거 Then 다음 후보를 실행하고 그 결과를 사용한다. 모든 후보가 실패하면 결과는 취소가 아니라 도구 실패다.
- Given 캡처 도구가 정상 종료했지만 파일을 남기지 않음 When 캡처 트리거 Then 다음 후보를 실행하지 않고 어느 클립보드도 바꾸지 않으며, 결과는 사용자 취소가 아니라 출력 없음으로 분류된다.
- Given 캡처 도구가 잘린 PNG 를 남김 When 캡처 트리거 Then 그 파일을 지우고 다음 후보를 시도한다.
- Given macOS 에서 화면 기록 권한이 미승인 When 캡처 트리거 Then 취소와 구분되는 권한 안내 토스트가 뜬다(클립보드는 바뀌지 않는다).
- Given `clipboard.set_text` IPC 호출(text 파라미터 포함) Then 로컬 클립보드가 그 텍스트로 바뀐다.
- Given plugin 이 `ClipboardWrite` 권한 없이 `clipboard.set_text` 호출 Then permission_denied.

### 검증 범위

같은 머신의 독립 인스턴스 두 개로 attach 전송·메시지 분류·서버 저장·클립보드 반영을
확인한 이력이 있다. 물리적으로 다른 원격 머신에서 사용자가 붙여넣는 최종 동작은
그 검증에 포함되지 않았다.

로컬 캡처와 취소는 Xvfb 환경에서 입력을 주입하고 X11 클립보드를 읽어 확인했다.
당시 GNOME 캡처 포털이 없어 테스트용 shim과 `scrot`으로 성공·파일 미생성을 재현했으므로,
이 결과가 네이티브 영역 선택 UI의 검증을 뜻하지는 않는다. `clipboard.set_text`는 실제 CLI로,
권한 거절은 `plugin_missing_clipboard_write_denied_for_clipboard_set_text` 단위 시험으로 확인했다.

결과 분류의 후보 순서·폴백·멈춤은 임시 `PATH` 의 도구 shim 으로 단위 시험한다
(`crates/tasty-platform/src/screen_capture/linux_tests.rs`). shim 결과는 OS 네이티브 취소의 검증이 아니다.
실제 도구로는 Linux X11(Xvfb)에서 scrot 1.10 의 Esc 취소·영역 선택 성공, 그리고 실행 중인
gnome-screenshot 41.0 이 비정상 종료한 뒤 scrot 으로 넘어가 Esc 로 취소되는 흐름을 확인했다.
Wayland 도구(grim·slurp)와 macOS·Windows 는 실제 실행하지 않았고 소스 기준이다.

## 구현

- 캡처: `crates/tasty-platform/src/screen_capture.rs`(`capture_interactive`, 플랫폼별 `capture_to_path`, `validate_png`, Linux 후보 실행 `linux::run_candidates`).
- App 폴링/스레딩: `src/app/screenshot_capture.rs`(`poll_screenshot_captures`/`trigger_pending_screenshot_captures`/`drain_screenshot_capture_results`), `src/app.rs`(`screenshot_capture_tx/rx`), `src/app/event.rs`(`AppEvent::ScreenshotCaptureReady`).
- 큐: `src/remote/state.rs`의 `RemoteState.pending_screenshot_captures` — 원 mirror workspace와 View를 보관하며 `src/app/engine_action.rs`에서 채운다.
- 키바인딩: `crates/tasty-settings/src/keybindings.rs`(`screenshot_to_clipboard` 필드 + `default_screenshot_to_clipboard`), `presets.rs`(4 프리셋 공통값), `crud.rs`(`GENERAL_BINDING_FIELDS`/`get_bindings(_mut)`), 매치는 `src/adapters/ui/input/shortcuts/keybinding.rs`(`match_capture_bindings`). Settings UI: `src/view/settings/ui/keybindings_tab.rs`(Clipboard 서브탭).
- 원격 전송(client): `src/app/attach_client/bulk.rs`(`forward_capture_to_remote_clipboard`), `src/app/attach_client/wire.rs`(`send_control_frame`, `parse_capture_result`), `src/app/attach_client/output.rs`(`MirrorEvent::CaptureResult`).
- 원격 수신(server): `crates/tasty-ipc/src/stream_hub.rs`(`CaptureUploadMsg`, `pump_inbound` 분류), `src/remote/capture_upload.rs`(`CaptureUploadRegistry`), `src/remote/server.rs`(`finalize_capture_upload`, `save_capture_and_set_clipboard`). GUI(`src/app/event_handler.rs::apply_capture_upload_msg`)와 headless(`src/boot/headless_stream.rs::apply_capture_uploads`) 양쪽 진입점에서 동일 서버 로직을 호출.
- IPC/CLI: `crates/tasty-ipc/src/method_meta.rs`(`clipboard.set_text` → `Permission::ClipboardWrite`), `src/app/ipc/app_methods.rs`(`ipc_handle_clipboard_set_text`), `crates/tasty-cli/src/commands/clipboard.rs`(`ClipboardCommands::SetText`), `crates/tasty-cli/src/request/clipboard.rs`.
- 테스트: `crates/tasty-ipc/src/method_meta_tests.rs`(`clipboard_set_text_is_release`), `crates/tasty-ipc/src/stream_hub.rs`(`pump_inbound_classifies_capture_chunk_and_commit`), `src/remote/capture_upload.rs`(누적/격리 단위 테스트), `crates/tasty-platform/src/screen_capture.rs`(경로·PNG 검사 단위 테스트), `crates/tasty-platform/src/screen_capture/linux_tests.rs`(후보 폴백·취소·출력 없음 분류 shim 시험).
