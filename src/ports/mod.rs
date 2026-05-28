//! `ports` — Hexagonal architecture 의 *port (trait)* 정의.
//!
//! Core 가 의존하는 *외부 자원 trait* 들. Adapter (production / test / 다른 plugin
//! impl) 가 trait 을 구현해 Core 에 주입된다.
//!
//! **Inbound port** — 외부 어댑터 (IPC / UI / CLI / Plugin) 가 Core 를 호출하는 진입점.
//! **Outbound port** — Core 가 외부 자원 (PTY / FileSystem / Clock / ...) 에 접근하는 trait.
//!
//! ## 위치 분기
//!
//! - **External crate 의존** — 본 모듈 (`src/ports/`).
//!   PtyService, FileSystem, Clock, ClipboardSystem, ProcessSpawner, HomeDirectory,
//!   TerminalWaker.
//!
//! ## Hub 의 외부 통신
//!
//! TCP IPC server 는 `src/host_api/ipc/server.rs` 의 `IpcServer` 가 자체 완결.
//! port 화 의미 작아 Hub 가 *직접 보유* — Core 외부 영역.
//!
//! - **Internal crate trait (4 port)** — 각 워크스페이스 crate 안.
//!   `tasty_memory::MemoryStorage`, `tasty_presets::PresetStorage`,
//!   `tasty_settings::SettingsStorage`, `tasty_themes::ThemeStorage`.
//!
//! ## Phase D 진행 중
//!
//! 현재는 *trait 정의 만*. production adapter / test mock 은 D.3.A.2 / D.3.A.3 에서.
//! Core 가 trait object 보유는 D.3.A.5 에서.

pub mod clipboard;
pub mod clock;
pub mod fs;
pub mod home;
pub mod inbound;
pub mod process;
pub mod pty;
