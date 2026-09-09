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
//! 한 함수 안에서도 갈린다. 완료 알림 hook 등록(`host_call::register_completion_hooks`)은
//! 등록 **루프**만 여기 있고 **이벤트 목록**은 각 plugin 이 인자로 준다 — 그 목록의 근거는
//! 각자의 매니페스트 `contributes.hook_events` 이고, host 는 매니페스트에 없는 이벤트
//! 구독을 거부한다. codex 에 `needs-input` 이 없는 것은 표류가 아니라 **의도된 비대칭**이다
//! (대응하는 codex hook 이벤트가 없어 거짓 계약을 만들지 않으려고 선언하지 않았다).
//!
//! ## 짝이 갈린 채 남는 것 — 그리고 무엇이 그것을 지키는가
//!
//! 위 항들은 **왜 안 합치는지**가 정해진 자리다. 아래 둘은 다르다 — 합칠지 말지가
//! 아직 안 정해졌고, 그래서 여기 값으로 적어 둔다. 둘 다 **응답 shape** 이 갈린다.
//!
//! | 자리 | claude | codex |
//! |---|---|---|
//! | `handle_children` | 화이트리스트로 remap 하고 자식마다 `surface.foreground_process` 를 한 번 더 불러 `foreground_process`·`foreground_pid` 를 덧씌운 뒤 **bare 배열**로 답한다 | 호스트 응답을 그대로 흘린다(`{"children": […]}`) |
//! | `handle_kill` | `error_scan` 을 내리고(그 하위 시스템은 claude 에만 있다) 응답을 `{"killed": true}` 로 바꾼다 | 호스트 응답을 그대로 흘린다(`{"killed_surface_id", "child_index"}`) |
//!
//! 갈래를 갈라 읽어야 한다. **`handle_kill` 의 `error_scan` 부분은 의도된 비대칭**이다 —
//! codex 에는 그 하위 시스템이 없고, 근거가 `docs/plugins/claude/index.md` 에 적혀 있다
//! (성공 응답의 `killed_surface_id` 로 즉시 `disable` 해 최대 800ms 의 잔여 발화 창을
//! 없앤다). 갈린 채 남는 것은 그 옆의 **응답 shape** 이다.
//!
//! 실측(2026-09-08): 레포 안에 `{"killed": true}` 를 읽는 소비자가 **없다** — CLI 는
//! 응답 JSON 을 그대로 찍는다. 즉 그 변환은 지금 `killed_surface_id` 와 `child_index` 를
//! **버리기만 한다.** 그런데 맞추려면 claude 쪽을 바꿔야 하고 그것은 에이전트가 읽는
//! 표면의 변경이라, 여기서 정하지 않는다.
//!
//! **★ 무엇이 이 둘을 지키는가.** 이 문단이 오래 유일한 기록이었고 — 그동안 한쪽이
//! shape 을 바꿔도 빨개지는 것이 없었다. 이제 네 시험이 **네 값을 각각 못박는다**:
//! `tasty-plugin-claude` 의 `children_response_is_a_bare_remapped_array` ·
//! `kill_response_is_reduced_to_a_killed_flag`, `tasty-plugin-codex` 의
//! `children_response_is_the_host_response_verbatim` ·
//! `kill_response_is_the_host_response_verbatim`. 넷 다 호스트 원본 응답만 돌려주는
//! mock(`ShapeHost`) 위에서 돌아 **호스트를 안 바꾸고 plugin 의 변환만** 잰다.
//!
//! 그래서 지금 갈린 채 남는 것은 **표에 적힌 그대로 고정**돼 있다. 위 표를 바꾸려면
//! 대응하는 시험이 먼저 빨개지고, 합치기로 정해지면 그때 시험 넷 중 둘이 지워진다.
//! 시험은 shape 이 **옳다**고 주장하지 않는다 — 지금 무엇인지를 말할 뿐이고, 어느
//! 쪽으로 맞출지는 여전히 여기서 정하지 않는다.
//!
//! ## 이 crate 는 plugin 이 아니다
//!
//! 이름이 `tasty-plugin-` 으로 시작하지만 `tasty-plugin.toml` 매니페스트가 없다 —
//! `tasty-plugin-sdk` / `tasty-plugin-protocol` / `tasty-plugin-manifest` 와 같은
//! 라이브러리 크레이트다. 번들 plugin 탐색(빌드 스크립트·버전 bump 검사·번들 가드)은
//! 전부 매니페스트 유무로 거르므로 여기는 자연히 빠진다.
//!
//! **버전 정책 주의**: 이 crate 만 고쳐도 두 plugin 의 patch 를 함께 올려야 한다.
//! 산출물이 실제로 달라지기 때문이다 —
//! `scripts/check-plugin-version-bump.sh` 는 그것을 자동으로 잡는다(워크스페이스 내부
//! 의존 폐포를 판정 대상에 넣는다: `docs/adr/0166-the-plugin-version-gate-judges-the-artifact-not-the-directory.md`).
//! 실측: 이 crate 한 줄만 바꾸면 게이트가 `tasty-plugin-claude` · `tasty-plugin-codex`
//! 둘을 위반으로 낸다. `tasty-plugin-sdk` · `tasty-utils` 도 같다.

pub mod children;
pub mod host_call;
pub mod params;
pub mod prompt_file;
pub mod reboot;
