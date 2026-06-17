# 의존성 이슈 모니터링

외부 의존성에서 비롯돼 우리 코드로 즉시 해결할 수 없는 빌드 이슈 추적.

## `block v0.1.6` — future-incompatibility (모니터링 중)

`cargo build`/`clippy` 끝의 경고:

```
warning: the following packages contain code that will be rejected by a future version of Rust: block v0.1.6
```

- 원인: transitive 의존 `block v0.1.6`(미유지보수)의 uninhabited static — Rust [#74840](https://github.com/rust-lang/rust/issues/74840) 에 따라 향후 hard error 예정(hard-error toolchain 버전 미정).
- 의존 경로:
  1. `cocoa` ← `drag` — `drag` 신버전이 cocoa 의존을 제거 → `cargo update` 로 해소 예정.
  2. `metal` ← `wgpu-hal` ← `wgpu` — wgpu 가 `block2`(objc2 생태계) 기반 metal 로 올라간 버전 채택 시 해소. wgpu major upgrade 시 점검.
- 대응: **upstream upgrade 대기**(`[patch.crates-io]` 우회·의존 제거는 hard-error 임박 전엔 안 함).

### 점검 / 전환 트리거

```bash
cargo report future-incompatibilities
cargo tree -i block@0.1.6     # 빈 출력이면 본 항목 삭제
```

다음 중 하나면 대기 종료 후 우회 적용: Rust release note 에 hard-error 화 버전 공지 / 경고가 deny 승격으로 빌드 실패. 전환 시 ① `[patch.crates-io]` 로 fixed fork 대체(ABI 호환 필수) ② wgpu 를 block2 기반으로 선제 업그레이드.
