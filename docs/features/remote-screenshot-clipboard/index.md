# 원격 스크린샷 → 클립보드 (Remote screenshot to clipboard)

- **Status**: Implemented
- **주체**: 로컬 사용자 (단축키 트리거)
- **ADR**: 없음 (기존 [ADR-0020](../../adr/0020-remote-connection-profiles.md) attach 채널을 그대로 재사용, 신규 프로토콜/enum 없음)
- **코드**: `crates/tasty-platform/src/screen_capture.rs`(OS 캡처 + `CaptureError`), `crates/tasty-platform/src/macos_permissions.rs`(화면 기록 권한 preflight), `src/app/screenshot_capture.rs`(폴링/스레드), `crates/tasty-settings/src/keybindings.rs`(`screenshot_to_clipboard`), `src/adapters/ui/input/shortcuts/keybinding.rs`(`match_capture_bindings`), `src/app/attach_client.rs`(원격 전송), `src/core/capture_upload.rs` + `src/core/attach_runtime.rs`(`finalize_capture_upload`, 서버측 수신), `crates/tasty-ipc/src/stream_hub.rs`(`CaptureUploadMsg`), `crates/tasty-ipc/src/method_meta.rs`(`clipboard.set_text`), `src/app/ipc/app_methods.rs`(`ipc_handle_clipboard_set_text`)
- **화면**: 없음 — 트리거는 단축키, 피드백은 성공/실패 토스트(mirror 케이스만; [remote-attach 토스트](../remote-attach/index.md) 채널 재사용)

## 목적

포커스된 surface 가 **mirror(원격 attach) 워크스페이스**에 속할 때, 인터랙티브 화면 캡처 결과를 로컬이 아니라 **원격 인스턴스의 클립보드**에 반영한다. 로컬 surface 를 보며 찍은 스크린샷이 로컬 클립보드로 가는 기존 동작(OS 네이티브 캡처 → `ClipboardSystem::write_text(경로)`)은 그대로 유지하고, mirror 포커스일 때만 캡처 파일을 attach 채널로 원격에 올려 원격 클립보드에 쓴다 — "찍은 화면이 실제로 보고 있는 머신의 클립보드로 간다"는 사용자 기대를 만족시키기 위함.

## 내부 동작 (headless-valid)

### 로컬 vs mirror 판별

키바인딩 매치 시점에 `state.focused_surface_id` → `CoreState::find_workspace_index_for_surface` → `Workspace.mirror` 로 대상 워크스페이스가 mirror 인지 즉시 판별한다(동기, OS 캡처 이전). mirror 면 그 워크스페이스 id 를, 아니면 `None` 을 `CoreState.pending_screenshot_captures` 큐에 push 한다 — 판별 자체는 focus 조회일 뿐 사용자 상태를 바꾸지 않는다.

### 캡처 (OS 네이티브, 인터랙티브)

`about_to_wait` 에서 큐를 drain해 요청마다 백그라운드 스레드로 OS 캡처를 실행(`crate::platform::screen_capture::capture_interactive`, 블로킹 UI 스레드 방지 — `auto_attach`/`pending_gui_attach` 와 동일한 pending-queue + 스레드 위임 패턴).

- **macOS**: `screencapture -i` (사용자가 영역 선택, Esc 로 취소 가능).
- **Linux**: Wayland `grim`+`slurp`, X11 은 `gnome-screenshot -a` → `scrot -s` → ImageMagick `import` 순 폴백(첫 성공 사용, 바이너리 부재는 다음 도구로).
- **Windows**: 인터랙티브 영역선택에 쓸 만한 OS 표준 동기 CLI 가 없어(Snipping Tool `ms-screenclip:` 은 파일이 아니라 클립보드 이미지로만 출력) PowerShell + `System.Drawing`/`System.Windows.Forms.SystemInformation.VirtualScreen` 로 **전체 가상화면**을 캡처한다(알려진 한계, 인터랙티브 영역선택 아님).
- 취소/성공 판정은 **종료 코드가 아니라 출력 파일 존재 여부**로 한다 — 도구별로 취소 시 exit code 관례가 다르기 때문.
- 저장 위치: `~/.tasty/screenshots/screenshot-<epoch_ms>.png`.

### 화면 기록 권한 (macOS)

macOS 에서 `screencapture` 는 **화면 기록(Screen Recording) 권한**을 요구한다. 권한이 없으면 파일을 전혀 남기지 않으므로, "파일 존재 여부" 판정만으로는 **사용자 취소와 구분되지 않는다** — 사용자에겐 둘 다 "아무 일도 안 일어남"으로 보인다.

그래서 캡처 실패를 세 상태로 나눈다(`CaptureError`):

| 상태 | 판정 | 처리 |
|---|---|---|
| 권한 미승인 | 캡처 **직전** `CGPreflightScreenCaptureAccess()` 가 false | `toast.screen_recording_permission_required` 안내 토스트 + `warn!` |
| 사용자 취소 | 권한은 있는데 결과 파일이 없음 | `debug!` 만 (정상 흐름, 토스트 없음) |
| 도구 실패 | 캡처 도구 실행/파일 읽기 자체가 실패 | `warn!` |

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

client 측(`src/app/attach_client.rs::forward_capture_to_remote_clipboard`)은 로컬 워크스페이스 id 로 해당 attach 세션을 찾아 청크+커밋을 순서대로 보낸다. 서버(원격 인스턴스) 측은 `stream_hub.rs::pump_inbound` 가 `StreamControl` 파싱 실패 시 `CaptureUploadMsg` 로 재시도해 분류하고, `CaptureUploadRegistry`(`(client_id, upload_id)` 키)에 청크를 누적하다 커밋에서 `attach_runtime::finalize_capture_upload` 를 호출한다.

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

## 구현

- 캡처: `crates/tasty-platform/src/screen_capture.rs`(`capture_interactive`, 플랫폼별 `capture_to_path`).
- App 폴링/스레딩: `src/app/screenshot_capture.rs`(`poll_screenshot_captures`/`trigger_pending_screenshot_captures`/`drain_screenshot_capture_results`), `src/app.rs`(`screenshot_capture_tx/rx`), `src/app/event.rs`(`AppEvent::ScreenshotCaptureReady`).
- 큐: `src/core/state.rs`(`CoreState.pending_screenshot_captures: Vec<Option<u32>>`).
- 키바인딩: `crates/tasty-settings/src/keybindings.rs`(`screenshot_to_clipboard` 필드 + `default_screenshot_to_clipboard`), `presets.rs`(4 프리셋 공통값), `crud.rs`(`GENERAL_BINDING_FIELDS`/`get_bindings(_mut)`), 매치는 `src/adapters/ui/input/shortcuts/keybinding.rs`(`match_capture_bindings`). Settings UI: `src/view/settings/ui/keybindings_tab.rs`(Clipboard 서브탭).
- 원격 전송(client): `src/app/attach_client.rs` 하단 독립 블록(`forward_capture_to_remote_clipboard`, `send_capture_control_frame`, `parse_capture_result`, `MirrorEvent::CaptureResult`).
- 원격 수신(server): `crates/tasty-ipc/src/stream_hub.rs`(`CaptureUploadMsg`, `pump_inbound` 분류), `src/core/capture_upload.rs`(`CaptureUploadRegistry`), `src/core/attach_runtime.rs`(`finalize_capture_upload`, `save_capture_and_set_clipboard`). GUI(`src/app/event_handler.rs::apply_capture_upload_msg`)와 headless(`src/boot/headless_stream.rs::apply_capture_uploads`) 양쪽 진입점에서 동일 서버 로직을 호출.
- IPC/CLI: `crates/tasty-ipc/src/method_meta.rs`(`clipboard.set_text` → `Permission::ClipboardWrite`), `src/app/ipc/app_methods.rs`(`ipc_handle_clipboard_set_text`), `crates/tasty-cli/src/commands/clipboard.rs`(`ClipboardCommands::SetText`), `crates/tasty-cli/src/request/clipboard.rs`.
- 테스트: `crates/tasty-ipc/src/method_meta_tests.rs`(`clipboard_set_text_is_release`), `crates/tasty-ipc/src/stream_hub.rs`(`pump_inbound_classifies_capture_chunk_and_commit`), `src/core/capture_upload.rs`(누적/격리 단위 테스트), `crates/tasty-platform/src/screen_capture.rs`(경로/폴백 단위 테스트).
