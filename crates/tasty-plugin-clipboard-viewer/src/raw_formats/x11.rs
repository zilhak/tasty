//! Linux(X11/XWayland) raw 클립보드 타겟 열거 — "기타" 버킷(TODO50).
//!
//! arboard 는 포맷 열거를 노출하지 않아 ICCCM 표준 절차(`TARGETS` atom 을
//! `ConvertSelection` 으로 요청 → `SelectionNotify` 응답 → `GetProperty` 로 atom 목록
//! 회수, 개별 타겟도 동일 절차로 재조회)를 직접 구현한다. text/files/image/html 로
//! 이미 소비된 atom 은 arboard 자신의 x11 백엔드가 실제로 읽는 atom
//! (`arboard-3.6.1/src/platform/linux/x11.rs`)과 동일한 이름으로 제외한다.
//!
//! `wayland-data-control` feature 를 켜지 않아(text/image/html 도 이미 이 경로만
//! 쓴다) XWayland 경유로만 동작한다 — 순수 Wayland(XWayland 미실행) 세션에서는 연결
//! 자체가 실패하며, 이건 새 회귀가 아니라 기존 한계의 연장이지만 조용히 빈 목록만
//! 내보내면 "기타 포맷이 없다"와 "애초에 조회를 못 했다"가 구분이 안 돼 사용자를
//! 오도한다 — 그래서 실패 시 `tracing::debug!` 로 원인을 남긴다(TODO50 Codex
//! 크로스체크).
//!
//! arboard 의 x11 백엔드는 자기 연결/윈도우를 노출하지 않으므로, 이 모듈은 완전히
//! 별개의 독립 연결과 임시 윈도우를 새로 만든다 — read-only 1회성 조회라 공유가
//! 필요하지도, 가능하지도 않다.

use std::time::{Duration, Instant};

use x11rb::{
    COPY_DEPTH_FROM_PARENT, COPY_FROM_PARENT, NONE,
    connection::Connection,
    protocol::{
        Event,
        xproto::{
            AtomEnum, ConnectionExt as _, CreateWindowAux, EventMask, Property, Time, WindowClass,
        },
    },
    rust_connection::RustConnection,
    wrapper::ConnectionExt as _,
};

use super::MAX_RAW_BYTES;
use crate::clipboard::OtherFormatEntry;

x11rb::atom_manager! {
    Atoms: AtomCookies {
        CLIPBOARD,
        TARGETS,
        INCR,
        MULTIPLE,
        SAVE_TARGETS,
        TIMESTAMP,

        // text 로 이미 소비 — arboard `x11.rs`의 Atoms(UTF8_STRING/STRING/TEXT/
        // UTF8_MIME_0/UTF8_MIME_1/TEXT_MIME_UNKNOWN)와 1:1 대응.
        UTF8_STRING,
        STRING,
        TEXT,
        UTF8_MIME_0: b"text/plain;charset=utf-8",
        UTF8_MIME_1: b"text/plain;charset=UTF-8",
        TEXT_MIME_UNKNOWN: b"text/plain",

        // files/html/image 로 이미 소비.
        URI_LIST: b"text/uri-list",
        HTML: b"text/html",
        PNG_MIME: b"image/png",

        // 우리 요청 응답을 받을 임시 프로퍼티 이름(임의 이름 — 다른 앱과 안 겹치게
        // 네임스페이싱).
        TASTY_OTHER_REPLY,
    }
}

/// 최초 `SelectionNotify` 를 기다리는 총 타임아웃 — 이미지처럼 오너가 준비하는 데
/// 오래 걸리는 포맷도 있어 넉넉히 잡는다(arboard 의 `LONG_TIMEOUT_DUR` 동형).
const CONVERT_TIMEOUT: Duration = Duration::from_millis(3000);
/// INCR 세그먼트 사이 `PropertyNotify` 대기 타임아웃 — 세그먼트 하나하나는 훨씬
/// 빨리 온다(arboard 의 `SHORT_TIMEOUT_DUR` 동형).
const INCR_SEGMENT_TIMEOUT: Duration = Duration::from_millis(500);

pub(super) fn read_other() -> Vec<OtherFormatEntry> {
    match try_read_other() {
        Ok(entries) => entries,
        Err(e) => {
            tracing::debug!("clipboard other-format X11 enumeration skipped: {e}");
            Vec::new()
        }
    }
}

fn try_read_other() -> Result<Vec<OtherFormatEntry>, String> {
    let (conn, screen_num) =
        RustConnection::connect(None).map_err(|e| format!("X11 connect failed: {e}"))?;
    let screen = conn
        .setup()
        .roots
        .get(screen_num)
        .ok_or_else(|| "X11: no screen found".to_string())?;
    let win = conn.generate_id().map_err(|e| e.to_string())?;
    conn.create_window(
        COPY_DEPTH_FROM_PARENT,
        win,
        screen.root,
        0,
        0,
        1,
        1,
        0,
        WindowClass::COPY_FROM_PARENT,
        COPY_FROM_PARENT,
        &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
    )
    .map_err(|e| e.to_string())?;
    conn.flush().map_err(|e| e.to_string())?;

    let atoms = Atoms::new(&conn)
        .map_err(|e| e.to_string())?
        .reply()
        .map_err(|e| e.to_string())?;

    let Some(targets_bytes) = convert_and_read(&conn, win, &atoms, atoms.TARGETS)? else {
        // 클립보드에 오너가 아예 없거나 TARGETS 를 지원하지 않음 — "기타 없음"과
        // 동일하게 처리(빈 클립보드와 구분이 필요하면 상위(clipboard::read_other)가
        // 이 함수를 arboard 의 4종 리더와 별개로 호출하므로 다른 타입 표시에는
        // 영향 없다).
        return Ok(Vec::new());
    };
    let target_atoms: Vec<u32> = targets_bytes
        .chunks_exact(4)
        .map(|c| u32::from_ne_bytes([c[0], c[1], c[2], c[3]]))
        .collect();

    let consumed = [
        atoms.TARGETS,
        atoms.MULTIPLE,
        atoms.SAVE_TARGETS,
        atoms.TIMESTAMP,
        atoms.UTF8_STRING,
        atoms.STRING,
        atoms.TEXT,
        atoms.UTF8_MIME_0,
        atoms.UTF8_MIME_1,
        atoms.TEXT_MIME_UNKNOWN,
        atoms.URI_LIST,
        atoms.HTML,
        atoms.PNG_MIME,
    ];

    let mut out = Vec::new();
    for atom in target_atoms {
        if consumed.contains(&atom) {
            continue;
        }
        let name = match conn.get_atom_name(atom).map_err(|e| e.to_string())?.reply() {
            Ok(reply) => String::from_utf8_lossy(&reply.name).into_owned(),
            // 이름을 못 얻으면 이 타겟만 건너뛴다(개별 격리 — 전체 열거를 실패시키지
            // 않는다).
            Err(_) => continue,
        };
        match convert_and_read(&conn, win, &atoms, atom) {
            Ok(Some(bytes)) => out.push(OtherFormatEntry::from_bytes(name, &bytes, MAX_RAW_BYTES)),
            // TARGETS 조회와 개별 재조회 사이 클립보드 소유자가 바뀌는 race 등으로
            // 오너가 이 타겟을 거절한 경우 — 정상적인 개별 격리 대상(TODO50).
            Ok(None) => tracing::debug!("clipboard other-format {name}: owner declined"),
            Err(e) => tracing::debug!("clipboard other-format {name} read failed: {e}"),
        }
    }
    Ok(out)
}

/// `target` 을 `CLIPBOARD` selection 에 요청하고 `SelectionNotify`/`PropertyNotify`
/// (INCR)를 폴링해 raw 프로퍼티 바이트를 회수한다.
///
/// - `Ok(Some(bytes))` — 정상 회수.
/// - `Ok(None)` — 오너가 이 타겟을 거절(property == NONE, ICCCM 표준 "지원 안 함"
///   신호) — 호출부가 그 포맷만 건너뛰는 정상 경로.
/// - `Err` — 프로토콜 오류/타임아웃(연결 자체가 안 되는 순수 Wayland 포함).
fn convert_and_read(
    conn: &RustConnection,
    win: u32,
    atoms: &Atoms,
    target: u32,
) -> Result<Option<Vec<u8>>, String> {
    conn.delete_property(win, atoms.TASTY_OTHER_REPLY)
        .map_err(|e| e.to_string())?;
    conn.convert_selection(
        win,
        atoms.CLIPBOARD,
        target,
        atoms.TASTY_OTHER_REPLY,
        Time::CURRENT_TIME,
    )
    .map_err(|e| e.to_string())?;
    conn.sync().map_err(|e| e.to_string())?;

    let mut incr_data: Vec<u8> = Vec::new();
    let mut using_incr = false;
    let mut deadline = Instant::now() + CONVERT_TIMEOUT;

    loop {
        if Instant::now() > deadline {
            return Err("timed out waiting for SelectionNotify/PropertyNotify".to_string());
        }
        let Some(event) = conn.poll_for_event().map_err(|e| e.to_string())? else {
            std::thread::sleep(Duration::from_millis(2));
            continue;
        };
        match event {
            Event::SelectionNotify(ev) => {
                if ev.property == NONE {
                    return Ok(None);
                }
                let reply = conn
                    .get_property(
                        true,
                        win,
                        atoms.TASTY_OTHER_REPLY,
                        AtomEnum::ANY,
                        0,
                        u32::MAX / 4,
                    )
                    .map_err(|e| e.to_string())?
                    .reply()
                    .map_err(|e| e.to_string())?;
                if reply.type_ == atoms.INCR {
                    using_incr = true;
                    deadline = Instant::now() + INCR_SEGMENT_TIMEOUT;
                    continue;
                }
                return Ok(Some(reply.value));
            }
            Event::PropertyNotify(ev)
                if using_incr
                    && ev.atom == atoms.TASTY_OTHER_REPLY
                    && ev.state == Property::NEW_VALUE =>
            {
                let reply = conn
                    .get_property(
                        true,
                        win,
                        atoms.TASTY_OTHER_REPLY,
                        AtomEnum::ANY,
                        0,
                        u32::MAX / 4,
                    )
                    .map_err(|e| e.to_string())?
                    .reply()
                    .map_err(|e| e.to_string())?;
                if reply.value_len == 0 {
                    return Ok(Some(incr_data));
                }
                incr_data.extend(reply.value);
                deadline = Instant::now() + INCR_SEGMENT_TIMEOUT;
            }
            _ => {}
        }
    }
}
