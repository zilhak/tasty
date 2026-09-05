#![forbid(unsafe_code)]

//! `tasty-plugin-claude` 와 `tasty-plugin-codex` 가 공유하는 헬퍼.
//!
//! 두 plugin 은 **다른 CLI**(claude / codex)를 감싸지만, 그 CLI 를 자식 surface 에서
//! 기동·감시·재시작하는 뼈대는 CLI 와 무관하다 — prompt 를 임시파일로 넘기는 법,
//! 완료 알림 hook 형제를 정리하는 법, `terminal.children` 응답을 읽는 법, reboot
//! 인자를 파싱하는 법. 이것들이 양쪽에 **글자 그대로 같은 사본**으로 있었고, 한쪽만
//! 고쳐지면 조용히 갈라지는 자리였다.
//!
//! ## 무엇이 여기 오고 무엇이 안 오는가
//!
//! 여기 오는 것은 **CLI 를 몰라도 성립하는 코드**뿐이다. 두 plugin 이 지금 같은 값을
//! 쓴다는 사실만으로는 여기 오지 않는다 — 예를 들어 reboot 의 `EXIT_WAIT` 는 claude 가
//! 5 초, codex 가 8 초로 **다르고**, 같았더라도 그것은 각 CLI 의 종료 속도라 공유하면
//! 안 되는 값이다. 같은 이유로 `CTRL_C_COUNT` 처럼 지금 값이 우연히 같은 상수도
//! 각 plugin 에 남긴다. 공유는 "같은 원인을 가진 코드" 에만 적용하고, "지금 같은 값" 은
//! 근거로 쓰지 않는다.
//!
//! IPC 에러 문구도 여기 없다 — claude 는 번역해서 내보내고 codex 는 영어로 박아
//! **두 plugin 의 규약이 지금 서로 반대**다. 합치면 한쪽 동작이 조용히 바뀐다.
//!
//! ## 이 crate 는 plugin 이 아니다
//!
//! 이름이 `tasty-plugin-` 으로 시작하지만 `tasty-plugin.toml` 매니페스트가 없다 —
//! `tasty-plugin-sdk` / `tasty-plugin-protocol` / `tasty-plugin-manifest` 와 같은
//! 라이브러리 크레이트다. 번들 plugin 탐색(빌드 스크립트·버전 bump 검사·번들 가드)은
//! 전부 매니페스트 유무로 거르므로 여기는 자연히 빠진다.
//!
//! **버전 정책 주의**: 이 crate 만 고치면 두 plugin 바이너리의 산출물이 바뀌는데도
//! `scripts/check-plugin-version-bump.sh` 는 변경 목록에서 `crates/tasty-plugin-<name>/`
//! 경로를 못 봐서 bump 를 요구하지 않는다. `tasty-plugin-sdk` · `tasty-utils` 에 대해
//! 이미 있던 성질이고 여기도 같다 — 이 crate 를 단독으로 고쳤다면 두 plugin 의 patch 를
//! 손으로 올려야 라이브 재sync 가 동작한다.

pub mod children;
pub mod host_call;
pub mod params;
pub mod prompt_file;
pub mod reboot;
