//! Tasty plugin manifest schema, parse and schema-agnostic validation.
//!
//! 본 crate 는 `tasty-plugin.toml` 의 schema + 파서 + 기본 검증 (id 형식, 중복,
//! permission 매칭 등) 만 제공한다. concrete file::format / file::handler 결합이
//! 필요한 추가 검증 (detector rule schema 등) 은 호스트 본 바이너리의
//! `plugin_bridge::manifest_validate` 가 담당.
//!
//! 모듈 본문은 F.B.6-2 에서 본 바이너리 `src/adapters/plugin/manifest/` 에서
//! `git mv` 로 이동된다.
