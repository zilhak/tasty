//! Tasty CLI — commands / transport / request / run wiring.
//!
//! 본 crate 는 본 바이너리 `src/adapters/cli/` 의 8 파일/디렉터리 (commands/,
//! request/, run.rs, transport.rs, format.rs, help.rs, dynamic.rs, plugin.rs)
//! 를 흡수한다. 호스트 도메인 결합은 tasty-ipc / tasty-plugin-manifest /
//! tasty-host-plugin 의 trait + Manifest 표면을 경유.
//!
//! 모듈 본문은 F.B.13-3 에서 본 바이너리에서 `git mv` 로 이동된다.
