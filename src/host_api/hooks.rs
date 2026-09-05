// hooks 시스템 API (global hooks / lua hooks). gui 의 dispatcher 가 주
// 활성화 경로 — headless 빌드에선 미사용. library API surface — *headless 한정*
// dead_code 침묵.

pub mod autofire;
pub mod global;
pub mod lua;
