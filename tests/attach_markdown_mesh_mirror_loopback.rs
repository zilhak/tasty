//! markdown plugin + attach mesh-mirror 조합을 실제로 구동해 검증하는
//! loopback 통합 테스트. 기존 attach loopback 테스트들(`attach_local_creation_tap.rs`
//! 등)과 동일 접근 — attach client 는 실제 GUI 앱이 아니라 raw `TcpStream` 으로
//! `stream.open{target_workspace}` 핸드셰이크만 흉내낸다. mesh-mirror 는 순수 터미널
//! mux 와 달리 실제 markdown plugin 프로세스가 렌더링을 수행해야 하므로,
//! `MeshContext`/`MeshData`/`MeshInput` 왕복 전체를 구동한다
//! (`docs/dev-guide/attach-behavior.md` "mesh mirror 채널" 절).
//!
//! 검증 대상:
//! 1. `MeshContext` 구독이 성립하고(별도 ack 없음 — 구독 요청 자체가 신호) 최소
//!    1개 이상의 `MeshData` 프레임이 도착한다.
//! 2. 그 프레임을 재조립+디코드(`tasty_plugin_protocol::mesh_wire::decode_paint`)했을
//!    때 markdown 렌더 결과(tessellate 된 primitive)가 비어있지 않다.
//! 3. `MeshInput`(스크롤 이벤트)이 서버의 holder/구독 검증을 실제로 통과·실패시키는지 —
//!    구독 전에는 `MeshError` 로 거부되고, 구독 후에는 거부되지 않는다.
//!
//! **알려진 스코프 제약(3)**: 여기서 검증하는 건 프로토콜 수락(holder 검증 통과,
//! `MeshError` 없음)까지다. 이 테스트가 구동하는 서버는 실제 GUI 창을 띄운
//! "gui-as-server, 살아있는 window" 케이스인데, 그 relay 훅
//! (`src/view/main/egui_mesh.rs::forward_mesh_to_attach_subscribers`)은
//! `mesh_mirror.take_dirty` 만 drain 하고 `take_pending_events` 를 소비하지 않는다 —
//! attach client 의 `MeshInput` 이 로컬 plugin 의 `raw_input` 에 실제로 병합되는 배선은
//! headless/parked 서버 전용(`plugin_bridge::mesh_forward::forward_mesh_frames_for_engine`)
//! 이라, 이 테스트 환경(Linux, macOS 전용 window-park 메커니즘 없음)에서는 스크롤이
//! markdown 렌더 결과를 실제로 바꾸는지까지는 관측할 수 없다(`docs/dev-guide/
//! attach-behavior.md` "gui-as-server 의 살아있는 window 는 attach client 의 입력
//! 역방향 forward 를 아직 로컬 plugin 에 되먹이지 않는다" 절 — 알려진 gap).

mod common;

use std::io::Write;
use std::net::TcpStream;
use std::path::Path;
use std::time::{Duration, Instant};

use common::TastyInstance;
use serde_json::{Value, json};
use tasty_ipc::mesh_stream::{MeshChunkMeta, MeshFrameAssembler};
use tasty_ipc::stream::{StreamControl, StreamTag, read_frame, write_frame};
use tasty_plugin_protocol::mesh_wire;
use tasty_plugin_protocol::{ModifiersWire, RawInputEventWire, RawInputWire};

/// 이 워크트리는 `target/` 이 별도 NFS mount(`/mnt/tasty-build/...`)로 symlink
/// 되어 있다 — 이 환경에서 스폰되는 `tasty` 프로세스의 `std::env::current_exe()`
/// (Linux 는 `/proc/self/exe` 를 커널이 이미 canonicalize 해서 돌려준다)는 그
/// mount 아래 경로를 반환하는데, 그 조상 디렉터리에는 `target/` 만 있고 `crates/`
/// 가 없다 — 그 결과 `tasty_host_plugin::builtin::ensure_dev_bundle` 의
/// workspace-root 추정이 실패해 builtin plugin 이 *전혀* 설치되지 않는다(기존
/// `tests/e2e_tests.rs` 의 `markdown.recent` 호출도 이 환경에서 동일 원인으로 이미
/// 실패한다 — 이 신규 테스트만의 문제가 아니라 이 워크트리 전역의 환경 특이사항이며,
/// 프로덕션 코드의 결함이 아니다). `crates/tasty-host-plugin/src/builtin.rs` 는
/// 건드리지 않고, 그 코드가 이미 최우선으로 지원하는 override 인
/// `TASTY_BUILTIN_PLUGINS_DIR` 로 이 테스트가 스폰할 `tasty` 자식 프로세스에게
/// markdown bundle 위치를 직접 알려준다.
fn ensure_builtin_markdown_bundle_env() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let src_dir = manifest_dir.join("crates/tasty-plugin-markdown");
    let src_manifest = src_dir.join("tasty-plugin.toml");
    let bin_name = if cfg!(windows) {
        "tasty-plugin-markdown.exe"
    } else {
        "tasty-plugin-markdown"
    };
    let src_bin = Path::new(env!("CARGO_BIN_EXE_tasty"))
        .parent()
        .expect("CARGO_BIN_EXE_tasty has a parent dir")
        .join(bin_name);
    assert!(
        src_manifest.exists(),
        "markdown plugin manifest missing: {}",
        src_manifest.display()
    );
    assert!(
        src_bin.exists(),
        "markdown plugin binary missing (did `cargo build --workspace` run first?): {}",
        src_bin.display()
    );

    let bundle_dir = std::env::temp_dir().join(format!(
        "tasty-test-builtin-bundle-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let dest_dir = bundle_dir.join("com.tasty.markdown");
    std::fs::create_dir_all(&dest_dir).expect("create bundle dest dir");
    std::fs::copy(&src_manifest, dest_dir.join("tasty-plugin.toml")).expect("copy manifest");
    std::fs::copy(&src_bin, dest_dir.join(bin_name)).expect("copy binary");
    let src_lang = src_dir.join("lang");
    if src_lang.is_dir() {
        copy_dir_recursive(&src_lang, &dest_dir.join("lang"));
    }

    // SAFETY: 이 test binary 는 `#[test]` fn 하나뿐이라 동시 접근이 없다.
    unsafe {
        std::env::set_var("TASTY_BUILTIN_PLUGINS_DIR", &bundle_dir);
    }
}

fn copy_dir_recursive(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).expect("mkdir");
    for entry in std::fs::read_dir(src).expect("read_dir") {
        let entry = entry.expect("dir entry");
        let ty = entry.file_type().expect("file_type");
        let dest_path = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&entry.path(), &dest_path);
        } else {
            std::fs::copy(entry.path(), &dest_path).expect("copy file");
        }
    }
}

/// `stream.open{target_workspace}` 핸드셰이크 — `attach_local_creation_tap.rs::
/// open_workspace_attach` 와 동형(중복 허용 — test 유틸 공유 모듈은 이 변경 범위 밖).
fn open_workspace_attach(port: u16, workspace_id: u64) -> TcpStream {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect to server");
    stream
        .set_read_timeout(Some(Duration::from_secs(15)))
        .expect("set read timeout");

    let req = json!({
        "jsonrpc": "2.0",
        "method": "stream.open",
        "params": {"proto": 1, "target_workspace": workspace_id},
        "id": 1,
    });
    let mut msg = serde_json::to_string(&req).unwrap();
    msg.push('\n');
    stream.write_all(msg.as_bytes()).expect("send handshake");

    loop {
        let frame = read_frame(&mut stream).expect("read handshake frame");
        if frame.tag != StreamTag::Control {
            continue;
        }
        let v: Value = serde_json::from_slice(&frame.payload).unwrap();
        if let Some(ok) = v.get("ok") {
            assert_eq!(ok, true, "handshake rejected: {v:?}");
            continue;
        }
        match v.get("event").and_then(|e| e.as_str()) {
            Some("attached_workspace") => return stream,
            Some("attach_error") => panic!("workspace attach rejected: {v:?}"),
            _ => continue, // 기존 터미널의 초기 스냅샷 등 무관한 프레임 — 계속 대기.
        }
    }
}

fn first_workspace_id(instance: &TastyInstance) -> u64 {
    let workspaces = instance.call("workspace.list", json!({}));
    workspaces.as_array().unwrap()[0]["id"].as_u64().unwrap()
}

fn first_pane_id_for_workspace(instance: &TastyInstance, workspace_id: u64) -> u64 {
    let panes = instance.call("pane.list", json!({}));
    panes
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["workspace_id"].as_u64() == Some(workspace_id))
        .expect("pane for workspace")["id"]
        .as_u64()
        .unwrap()
}

/// markdown surface 를 만든다. 신규 격리 HOME 인 첫 실행에서는 builtin markdown
/// plugin 이 번들 복사 + spawn + hello(kind 등록)를 마치기 전에 `tab.create` 가 먼저
/// 도착하면 `"unknown surface kind: markdown"` 으로 거부될 수 있다 — plugin 준비를
/// 기다리는 별도 IPC 없이, `TastyInstance::spawn()` 이 셸 준비를 폴링하는 것과 동일한
/// 패턴으로 `tab.create` 자체를 재시도한다.
fn create_markdown_tab(server: &TastyInstance, pane_id: u64, file: &str) -> u32 {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let resp = server.call_raw(
            "tab.create",
            json!({ "pane_id": pane_id, "type": "markdown", "file": file }),
        );
        if let Some(result) = resp.get("result") {
            return result["surface_id"].as_u64().expect("surface_id") as u32;
        }
        if Instant::now() >= deadline {
            let plugins = server.call("plugin.list", json!({}));
            panic!(
                "markdown plugin never registered its surface kind: {resp:?}\nplugin.list: {plugins:#?}"
            );
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn send_control(stream: &mut TcpStream, ctrl: &StreamControl) {
    let payload = serde_json::to_vec(ctrl).expect("serialize StreamControl");
    write_frame(stream, StreamTag::Control, &payload).expect("write control frame");
}

/// 한 원시 프레임을 읽어 "관심있는" 이벤트(완성된 mesh frame 또는 `MeshError`)로
/// 분류한다. `Data`/`Ping`/미완 mesh chunk 등은 `Ok(None)` — 호출자는 계속 폴링한다.
/// mesh chunk 재조립은 항상 같은 `assembler` 로 흘려, 이 함수를 여러 단계에서
/// 나눠 호출해도 진행 중이던 frame 조립 상태가 끊기지 않는다.
enum AttachEvent {
    MeshFrame(MeshChunkMeta, Vec<u8>),
    MeshError { surface_id: u32, reason: String },
}

fn poll_attach_event(
    stream: &mut TcpStream,
    assembler: &mut MeshFrameAssembler,
) -> std::io::Result<Option<AttachEvent>> {
    let frame = read_frame(stream)?;
    Ok(match frame.tag {
        StreamTag::MeshData => match assembler.push_chunk(&frame.payload) {
            Ok(Some((meta, bytes))) => Some(AttachEvent::MeshFrame(meta, bytes)),
            Ok(None) => None,
            Err(e) => panic!("mesh chunk assembly failed: {e:?}"),
        },
        StreamTag::Control => match serde_json::from_slice::<StreamControl>(&frame.payload) {
            Ok(StreamControl::MeshError { surface_id, reason }) => {
                Some(AttachEvent::MeshError { surface_id, reason })
            }
            _ => None,
        },
        _ => None,
    })
}

/// `surface_id` 의 다음 완성된 mesh frame이 도착할 때까지 기다린다. 그 사이 같은
/// surface 대상 `MeshError` 가 오면 즉시 패닉 — 조용한 실패보다 명시 실패가 낫다는
/// 프로토콜 설계(`docs/dev-guide/attach-behavior.md` "MeshError" 절)와 같은 태도.
fn wait_for_mesh_frame(
    stream: &mut TcpStream,
    assembler: &mut MeshFrameAssembler,
    surface_id: u32,
    timeout: Duration,
) -> (MeshChunkMeta, Vec<u8>) {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("set read timeout");
    let deadline = Instant::now() + timeout;
    let mut last_err = None;
    loop {
        match poll_attach_event(stream, assembler) {
            Ok(Some(AttachEvent::MeshFrame(meta, bytes))) if meta.surface_id == surface_id => {
                return (meta, bytes);
            }
            Ok(Some(AttachEvent::MeshError {
                surface_id: sid,
                reason,
            })) if sid == surface_id => {
                panic!("server replied MeshError for surface {surface_id}: {reason}");
            }
            Ok(_) => {}
            Err(e) => last_err = Some(e),
        }
        if Instant::now() >= deadline {
            panic!(
                "timed out waiting for MeshData frame for surface {surface_id} \
                 (last read error: {last_err:?})"
            );
        }
    }
}

/// `surface_id` 대상 `MeshError` 가 도착할 때까지 기다린다 — holder/구독 검증이
/// 실제로 거부 경로를 타는지 확인하는 음성(negative) 케이스 전용.
fn expect_mesh_error(
    stream: &mut TcpStream,
    assembler: &mut MeshFrameAssembler,
    surface_id: u32,
    timeout: Duration,
) -> String {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("set read timeout");
    let deadline = Instant::now() + timeout;
    let mut last_err = None;
    loop {
        match poll_attach_event(stream, assembler) {
            Ok(Some(AttachEvent::MeshError {
                surface_id: sid,
                reason,
            })) if sid == surface_id => {
                return reason;
            }
            Ok(_) => {}
            Err(e) => last_err = Some(e),
        }
        if Instant::now() >= deadline {
            panic!(
                "timed out waiting for MeshError for surface {surface_id} \
                 (last read error: {last_err:?})"
            );
        }
    }
}

/// `window` 동안 `surface_id` 대상 `MeshError` 가 오지 않는지 확인한다 — 구독 후
/// `MeshInput` 이 정상 수락됐는지의 양성(positive) 케이스 전용.
fn assert_no_mesh_error_within(
    stream: &mut TcpStream,
    assembler: &mut MeshFrameAssembler,
    surface_id: u32,
    window: Duration,
) {
    stream
        .set_read_timeout(Some(Duration::from_millis(300)))
        .expect("set read timeout");
    let deadline = Instant::now() + window;
    while Instant::now() < deadline {
        if let Ok(Some(AttachEvent::MeshError {
            surface_id: sid,
            reason,
        })) = poll_attach_event(stream, assembler)
            && sid == surface_id
        {
            panic!("구독 중인 surface 의 MeshInput 이 MeshError 로 거부됨: {reason}");
        }
    }
}

#[test]
fn markdown_mesh_mirror_subscribe_and_receive_frame() {
    ensure_builtin_markdown_bundle_env();
    let server = TastyInstance::spawn();
    let ws_id = first_workspace_id(&server);
    let pane_id = first_pane_id_for_workspace(&server, ws_id);

    let tmp_dir = tempfile::tempdir().expect("tempdir");
    let md_path = tmp_dir.path().join("mesh-mirror-test.md");
    std::fs::write(
        &md_path,
        "# Mesh Mirror Test\n\n\
         This paragraph exists so the markdown plugin has real content to render — \
         a heading plus prose gives tessellation actual vertices/textures to \
         produce, unlike a blank document.\n\n\
         - item one\n- item two\n",
    )
    .expect("write markdown fixture");

    let md_surface_id = create_markdown_tab(&server, pane_id, md_path.to_str().unwrap());

    let mut attach_stream = open_workspace_attach(server.port(), ws_id);
    let mut assembler = MeshFrameAssembler::new();

    // 0. 구독 전 MeshInput 은 거부돼야 한다(holder 는 맞지만 아직 mesh_mirror 구독
    // 엔트리가 없음 — `CoreState::apply_attached_mesh_input`/`MeshMirrorRegistry::
    // push_input` 계약). 뒤에 나오는 "구독 후엔 거부되지 않음" 판정이 유효하려면,
    // 먼저 우리 MeshError 감지 경로 자체가 실제로 동작함을 이 음성 케이스로 확인해야
    // 한다.
    send_control(
        &mut attach_stream,
        &StreamControl::MeshInput {
            surface_id: md_surface_id,
            input: RawInputWire {
                time: None,
                focused: true,
                modifiers: ModifiersWire::default(),
                events: vec![RawInputEventWire::Scroll { x: 0.0, y: 50.0 }],
            },
        },
    );
    expect_mesh_error(
        &mut attach_stream,
        &mut assembler,
        md_surface_id,
        Duration::from_secs(10),
    );

    // 1. 구독 성립 — MeshContext 자체가 구독 신호(별도 ack 없음, "구독 = MeshContext" 절).
    send_control(
        &mut attach_stream,
        &StreamControl::MeshContext {
            surface_id: md_surface_id,
            width_px: 800,
            height_px: 600,
            pixels_per_point: 1.0,
            theme: None,
            focused: true,
        },
    );

    // 2. 최소 1개 이상의 MeshData 프레임 도달 검증. 실제 plugin 프로세스 기동(이미
    // 떠 있을 가능성이 높지만) + tessellate + IPC 왕복을 거치므로 넉넉한 타임아웃.
    let (_meta, bytes) = wait_for_mesh_frame(
        &mut attach_stream,
        &mut assembler,
        md_surface_id,
        Duration::from_secs(30),
    );
    assert!(!bytes.is_empty(), "mesh frame bytes 가 비어있음");
    // `meta.full_textures` 는 여기서 검증하지 않는다 — 텍스처(폰트 아틀라스 등)가
    // 로컬 authoritative loop 의 기존 렌더로 이미 client 텍스처 캐시에 있다고 서버가
    // 판단하면(같은 plugin 프로세스가 로컬 창도 같이 그리고 있어 텍스처 상태가 이미
    // 확립돼 있음) 새 구독자의 첫 프레임도 `textures_delta` 없이(정점/geometry만) 올 수
    // 있다 — 이 테스트가 검증해야 하는 건 "렌더 결과(텍스처/버텍스 데이터)가 비어있지
    // 않음"이지 "첫 프레임이 반드시 풀 텍스처를 포함"이 아니다. 아래 primitives
    // 비어있지 않음 검증이 그 기준을 그대로 반영한다.

    // 3. markdown 렌더 결과(텍스처/버텍스 데이터)가 비어있지 않음을 디코드로 확인.
    let decoded = mesh_wire::decode_paint(&bytes).expect("decode mesh frame");
    assert!(
        !decoded.primitives.is_empty(),
        "markdown 렌더 결과에 tessellate 된 primitive 가 없음 — 빈 화면 회귀"
    );

    // 4. 구독 후엔 MeshInput 이 거부되지 않아야 한다 — holder 검증 통과 + 구독
    // 엔트리 존재를 증명(스코프 제약은 파일 상단 doc 참고).
    send_control(
        &mut attach_stream,
        &StreamControl::MeshInput {
            surface_id: md_surface_id,
            input: RawInputWire {
                time: None,
                focused: true,
                modifiers: ModifiersWire::default(),
                events: vec![RawInputEventWire::Scroll { x: 0.0, y: 50.0 }],
            },
        },
    );
    assert_no_mesh_error_within(
        &mut attach_stream,
        &mut assembler,
        md_surface_id,
        Duration::from_secs(3),
    );
}
