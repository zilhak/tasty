//! 영속/세션 상태 저장소들.
//!
//! store/* 의 일부 API (recent_files, clipboard_history, notification 등) 는
//! gui 소비자 (메뉴, popup) 가 주력이라 headless 빌드에선 미사용으로 잡힌다.
//! library API surface — *headless 한정* dead_code 침묵.
#![cfg_attr(not(feature = "gui"), allow(dead_code, unused_imports))]

pub mod clipboard_history;
pub mod notification;
pub mod recent_files;
pub mod scrollback;
