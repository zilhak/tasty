# ADR-0318: 핸들러 레지스트리는 크레이트로 내려가고, 번들 기본값은 상대 경로가 아니라 크레이트 상수가 된다

- **Status**: Accepted
- **Date**: 2026-09-20
- **Tags**: architecture, crates, layering, file-handler, include-str, plugin-protocol, orphan-rule, adr-0308

## Context

본 바이너리 안의 파일 모듈 아래에 파일 핸들러의 **정책과 레지스트리**가 있었다 — detector→handler
매핑, 사용자 오버라이드, plugin 기여, 최근 선택, 그리고 그 셋을 patch semantics 로 병합하는
규칙. 호스트·GUI·IPC 를 한 줄도 부르지 않는데 본 바이너리 안에 있었고, 그래서 바이너리가
바이너리와 무관한 도메인 로직을 소유했다.

한 가지가 잘못 세어지기 쉬웠다. 이 모듈은 `crate::poison::` 을 쓰는데 그것은 **GUI 가 아니다**
— `src/main.rs` 의 `pub(crate) use tasty_utils::poison;` 재수출이다. 의존으로 세면 잎
크레이트 하나다.

번들 기본값 `default-file-handlers.toml` 은 **두 자리에서** 상대 경로로
`include_str!` 되고 있었다 — 모듈 자신과 `src/core/state.rs`. 상대 경로는 파일이 움직이는
순간 자리마다 따로 깨지고, 한쪽만 고쳐진 상태가 컴파일된다.

## Decision

핸들러 정책·레지스트리를 `crates/tasty-file-handler` 로 내리고, 본체는 `src/file.rs` 에서
`pub use tasty_file_handler as handler;` 로 이름만 이어 `crate::file::handler::…` 호출부를
건드리지 않는다. 번들 기본값은 크레이트 안으로 함께 옮기고 `pub const HOST_DEFAULTS_TOML`
로 **한 번만** 박아, 소비처가 경로가 아니라 이름을 부르게 한다 — 자매 크레이트
`tasty-file-format` 이 이미 쓰는 모양이다.

새 크레이트의 `tasty-plugin-protocol` 의존은 [ADR-0308](0308-the-format-registry-port-impl-stays-with-the-type.md) 의
**같은 결정을 적용한 것이지 새 결정이 아니다** — plugin 이 레지스트리를 조회하는 port
trait 이 wire 크레이트에 살고, 고아 규칙이 impl 을 타입 소유자 쪽으로 강제한다. layer
예외 명부와 아키텍처 문서의 도메인-IO 절에 그 형태로 등록한다.

## Consequences

- **얻은 것**: 도메인 로직이 바이너리 밖으로 나왔고, GUI 없이 링크된다. 번들 기본값이
  한 자리에만 있으므로 파일이 다시 움직여도 자리마다 따로 깨지지 않는다.
- **잃은 것**: layer 예외가 둘에서 셋이 됐다. 그 셋이 전부 같은 뿌리(고아 규칙)라는 것이
  문서에 적혀야 값이 유지된다 — 안 적으면 다음 사람이 세 번 같은 조사를 한다.
- **운영 비용 / 유지 부담**: 크레이트 수 lockstep 아홉 자리가 함께 움직인다. 그 자리는
  README 배지 둘 · README 본문 넷 · 아키텍처 문서 둘 · `CLAUDE.md` 빌드 절 하나다.

## Alternatives Considered

- **A: port trait impl 을 `tasty-plugin-protocol` 쪽으로 옮겨 예외를 없앤다** — 고아 규칙이
  막는다. 타입을 소유하지 않은 크레이트에 impl 을 둘 수 없다. ADR-0308 이 같은 자리에서
  같은 결론을 냈다.
- **B: 번들 기본값을 크레이트 밖에 두고 양쪽이 상대 경로로 계속 박는다** — 지금 동작하지만
  이번 이동이 그 경로를 크레이트 밖으로 밀어낸다. 같은 함정을 한 칸 옮기는 것뿐이다.
- **C: 핸들러와 dispatch 를 함께 내린다** — dispatch 는 파일을 실제로 여는 실행 경로이고
  호스트 동작에 닿는다. 정책만 내리는 경계가 "GUI 없이 쓸 수 있는가" 와 일치한다.

## Reconsideration Triggers

다음 중 하나가 충족되면 본 ADR 을 재검토한다.

**채널이 붙는 것** — 판정 시점에 레포가 읽을 수 있는 사실이다.

- 고아 규칙 예외가 넷째로 늘어나면, 개별 예외가 아니라 port trait 의 배치 자체를 다시
  본다. `architecture_layer_order_holds.rs` 의 `EXCEPTIONS` 길이가 그 좌변이다.
- `HOST_DEFAULTS_TOML` 을 부르지 않는 새 `include_str!` 이 같은 파일을 가리키면 이
  결정이 무너진 것이다.

**원리적으로 안 붙는 것** — 사람이 관측해야 한다. 재는 법을 함께 적는다.

- 이 크레이트가 정말 호스트 결합 0 인가. 재는 법: `cargo tree -p tasty-file-handler
  --edges normal --prefix none` 의 결과에서 `egui`·`winit`·`wgpu` 를 세고, 같은 술어를
  본 바이너리 폐포에 걸어 판별력이 있는지 확인한다(실측 2026-09-20: 크레이트 0, 바이너리
  6). 컴파일이 통과하는 것은 이 물음에 답하지 않는다.

## References

- 같은 형태의 선행 결정: [ADR-0308](0308-the-format-registry-port-impl-stays-with-the-type.md)
- 크레이트 분할이 의존 방향을 따른다는 결정: [ADR-0089](0089-crate-split-follows-dependency-direction.md)
- 코드 근거(결정이 실현된 **현재 위치**): `tasty-file-handler` 크레이트의
  `HOST_DEFAULTS_TOML` 과 `FileHandlerRegistry`, 본체 쪽 이름 잇기는 `src/file.rs` 의
  `handler` 재수출
- 절 소속·크레이트 수: [architecture](../architecture/index.md)
