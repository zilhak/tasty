//! Tasty plugin manager — 호스트 측 lifecycle/IPC routing/manifest registry.
//!
//! 본 crate 는 본 바이너리 `src/adapters/plugin/` 의 manager / handle_channel /
//! process / listener / protocol / discovery / builtin / event_bus 등 다수
//! 모듈을 흡수한다. host 본 바이너리 결합은 Phase F.B.0 의 6 host_port trait
//! (SurfaceRegistry / FileFormatRegistryPort / FileHandlerRegistryPort /
//! I18nNamespaceRegistrar / IpcHostFacade) + plugin_bridge/ 잔존 5 모듈로 격리.
//!
//! 모듈 본문은 F.B.11-2 ~ F.B.11-4 에서 본 바이너리에서 `git mv` 로 이동된다.
