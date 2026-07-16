# 원격 스크린샷 → 클립보드 (Remote screenshot to clipboard)

- **Status**: Implemented
- **주체**: 로컬 사용자 (단축키 트리거)
- **ADR**: 없음 (기존 [ADR-0032](../../adr/0032-remote-attach-two-layer-split.md) attach 채널을 그대로 재사용, 신규 프로토콜/enum 없음)
- **코드**: `src/platform/screen_capture.rs`(OS 캡처), `src/app/screenshot_capture.rs`(폴링/스레드), `crates/tasty-settings/src/keybindings.rs`(`screenshot_to_clipboard`), `src/adapters/ui/input/shortcuts/keybinding.rs`(`match_capture_bindings`), `src/app/attach_client.rs`(원격 전송), `src/core/capture_upload.rs` + `src/core/attach_runtime.rs`(`finalize_capture_upload`, 서버측 수신), `src/adapters/production/stream_hub.rs`(`CaptureUploadMsg`), `crates/tasty-ipc/src/method_meta.rs`(`clipboard.set_text`), `src/app/ipc/app_methods.rs`(`ipc_handle_clipboard_set_text`)
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

### 로컬 케이스

캡처 성공 시 파일 경로 문자열을 `ClipboardSystem::write_text(path)` 로 로컬 클립보드에 쓴다(기존 `ui.screenshot`/이미지 붙여넣기와 동일한 "경로를 클립보드에 넣는다" 관례).

### 원격(mirror) 케이스 — 기존 attach 채널 확장 (신규 프로토콜 없음)

**채널**: 별도 scp/ssh-exec 를 새로 만들지 않고, attach 세션이 이미 열어둔 JSON-RPC-over-TCP 연결(`AttachClientSession` 의 writer/reader, `StreamTag::Control` 프레임)을 그대로 재사용한다. `StreamControl`(`crates/tasty-ipc/src/stream.rs`) 은 이 작업에서 **손대지 않았다** — 대신 그 enum 의 `#[serde(tag="event")]` 파싱이 인식 못 하는 이벤트 값은 조용히 무시되는 기존 전방호환 동작을 이용해, `StreamControl` 파싱이 실패하는 fallback 경로에 완전히 별도인 미니 프로토콜을 얹었다:

- `capture_chunk` { `upload_id`, `seq`, `total`, `data_b64` } — 캡처 파일을 raw 700KiB 청크 단위로 base64 인코딩해 전송(base64 인플레이션 후에도 `MAX_FRAME_LEN`(1MiB) 아래 유지).
- `capture_commit` { `upload_id`, `file_name` } — 마지막 청크 뒤 1회, 업로드 종료를 알림.
- `capture_result` { `ok`, `path?`, `reason?` } — 서버(원격)→client 회신.

client 측(`src/app/attach_client.rs::forward_capture_to_remote_clipboard`)은 로컬 워크스페이스 id 로 해당 attach 세션을 찾아 청크+커밋을 순서대로 보낸다. 서버(원격 인스턴스) 측은 `stream_hub.rs::pump_inbound` 가 `StreamControl` 파싱 실패 시 `CaptureUploadMsg` 로 재시도해 분류하고, `CaptureUploadRegistry`(`(client_id, upload_id)` 키)에 청크를 누적하다 커밋에서 `attach_runtime::finalize_capture_upload` 를 호출한다.

**서버측 처리(`finalize_capture_upload`)**:
1. 그 client 가 실제로 이 워크스페이스를 hard 점유 중인지(`OccupancyRegistry::client_holds_workspace`) 확인 — attach 연결 자체가 이미 권한 경계이므로 별도 인증 계층을 새로 만들지 않는다(구조 변경 forward 와 동일한 신뢰 모델).
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
- **`StreamControl` enum 확장** — 이 작업은 그 enum 을 건드리지 않고, 완전히 별도인 이벤트 태그로 같은 `StreamTag::Control` 채널 위에 병행 프로토콜을 얹는다.
- **비-mirror(일반 attach 없는) 원격 클립보드 반영** — mirror 판별은 오직 `Workspace.mirror` 로만 하며, surface 단위 attach(워크스페이스 아님)는 이 판별 대상이 아니다.

## Acceptance Criteria

- [ ] Given 로컬(비-mirror) surface 에 포커스 When `screenshot_to_clipboard` 트리거 Then OS 인터랙티브 캡처 후 캡처 파일 경로가 로컬 클립보드에 쓰인다.
- [ ] Given mirror 워크스페이스의 surface 에 포커스 When `screenshot_to_clipboard` 트리거 Then 캡처 파일이 attach 채널로 원격에 전송되고, 원격 인스턴스의 클립보드에 그 원격 경로가 쓰인다(로컬 클립보드는 바뀌지 않는다).
- [ ] Given 캡처가 사용자에 의해 취소됨(Esc 등) Then 로컬/원격 어느 클립보드도 바뀌지 않는다.
- [ ] Given `clipboard.set_text` IPC 호출(text 파라미터 포함) Then 로컬 클립보드가 그 텍스트로 바뀐다.
- [ ] Given plugin 이 `ClipboardWrite` 권한 없이 `clipboard.set_text` 호출 Then permission_denied.

> **검증 한계(문서화)**: 위 항목 중 mirror 원격 반영 e2e 는 이 작업 환경에서 물리적으로 분리된 두 머신을 준비할 수 없어, `--ssh 127.0.0.1:<port>` loopback 2-인스턴스 구성(같은 머신, 별도 `TASTY_HOME`)으로 attach 파이프라인·미니 프로토콜 분류·`finalize_capture_upload` 저장/clipboard 반영까지는 검증했으나, **실제 원격 OS 클립보드에 물리적으로 다른 사용자가 붙여넣기를 시도하는 최종 확인**은 코드/로직 리뷰로 대체했다(`docs/features/remote-attach/index.md` 의 기존 loopback e2e 관례와 동일한 한계).

## 구현

- 캡처: `src/platform/screen_capture.rs`(`capture_interactive`, 플랫폼별 `capture_to_path`).
- App 폴링/스레딩: `src/app/screenshot_capture.rs`(`poll_screenshot_captures`/`trigger_pending_screenshot_captures`/`drain_screenshot_capture_results`), `src/app.rs`(`screenshot_capture_tx/rx`), `src/app/event.rs`(`AppEvent::ScreenshotCaptureReady`).
- 큐: `src/core/state.rs`(`CoreState.pending_screenshot_captures: Vec<Option<u32>>`).
- 키바인딩: `crates/tasty-settings/src/keybindings.rs`(`screenshot_to_clipboard` 필드 + `default_screenshot_to_clipboard`), `presets.rs`(4 프리셋 공통값), `crud.rs`(`GENERAL_BINDING_FIELDS`/`get_bindings(_mut)`), 매치는 `src/adapters/ui/input/shortcuts/keybinding.rs`(`match_capture_bindings`). Settings UI: `src/view/settings/ui/keybindings_tab.rs`(Clipboard 서브탭).
- 원격 전송(client): `src/app/attach_client.rs` 하단 독립 블록(`forward_capture_to_remote_clipboard`, `send_capture_control_frame`, `parse_capture_result`, `MirrorEvent::CaptureResult`) — `apply_mirror_structural_delta`(별도 병행 작업)와 분리된 별도 함수/블록으로 작성.
- 원격 수신(server): `src/adapters/production/stream_hub.rs`(`CaptureUploadMsg`, `pump_inbound` 분류), `src/core/capture_upload.rs`(`CaptureUploadRegistry`), `src/core/attach_runtime.rs`(`finalize_capture_upload`, `save_capture_and_set_clipboard`). GUI(`src/app/event_handler.rs::apply_capture_upload_msg`)와 headless(`src/boot.rs`) 양쪽 진입점에서 동일 서버 로직을 호출.
- IPC/CLI: `crates/tasty-ipc/src/method_meta.rs`(`clipboard.set_text` → `Permission::ClipboardWrite`), `src/app/ipc/app_methods.rs`(`ipc_handle_clipboard_set_text`), `crates/tasty-cli/src/commands/clipboard.rs`(`ClipboardCommands::SetText`), `crates/tasty-cli/src/request/clipboard.rs`.
- 테스트: `crates/tasty-ipc/src/method_meta_tests.rs`(`clipboard_set_text_is_release`), `src/adapters/production/stream_hub.rs`(`pump_inbound_classifies_capture_chunk_and_commit`), `src/core/capture_upload.rs`(누적/격리 단위 테스트), `src/platform/screen_capture.rs`(경로/폴백 단위 테스트).
