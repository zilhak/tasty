# tasty-design-tokens

디자인 시스템(claude design 산출물)의 W3C DTCG 토큰 export 를 vendor 하고,
치수 계열 토큰을 Rust const 로 생성하는 crate. 구조·역할은 `src/lib.rs` 의
crate 문서 주석 참조.

## vendor 갱신 절차

디자인 폴더는 위치·이름이 매번 바뀐다(재다운로드 시 접미사 변동). **경로를
코드·문서에 박지 말고**, 갱신 시마다 다음 절차를 따른다:

1. **사용자에게 현재 디자인 폴더 위치를 묻는다.** (추측·옛 경로 재사용 금지)
2. 그 루트 기준 `tokens/tasty.tokens.json` 을 `dtcg/tasty.tokens.json` 으로 복사한다.
   - json 은 디자인 측에서 CSS 3파일(`primitives/semantic/components.css`)로부터
     **재생성되는 산출물**이다 — 수동 편집 금지 (디자인 측 안내).
3. 생성기를 재실행한다: `cargo run -p tasty-design-tokens --bin generate`
4. 테스트로 확인한다: `cargo test -p tasty-design-tokens`
   - 토큰 census(488 = 104/127/257)가 바뀌었으면 `tests/freshness.rs` 의
     스냅샷을 의식적으로 갱신한다.
   - `sizing_parity` / `color_drift` 실패는 소스 ↔ 디자인 드리프트 신호 —
     값을 임의로 맞추지 말고 디자인 판정을 먼저 확인한다.
5. vendor json + 재생성된 `src/generated/*.rs` + (필요시) 테스트 스냅샷을
   **같은 커밋**으로 커밋한다.
