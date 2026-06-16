# 의존성 이슈 모니터링

외부 의존성에서 비롯된, 우리 코드로 즉시 해결할 수 없는 빌드 이슈의 추적 문서.

## `block v0.1.6` — future-incompatibility (모니터링 중)

`cargo build` / `cargo clippy` 끝의 다음 경고는 **알려진 상태이며 모니터링 대상**이다:

```
warning: the following packages contain code that will be rejected by a future version of Rust: block v0.1.6
```

- 원인: transitive 의존성 `block v0.1.6` (2019 년 이후 미유지보수) 의 uninhabited static (`static _NSConcreteStackBlock: Class;`) — Rust issue [#74840](https://github.com/rust-lang/rust/issues/74840) 에 따라 향후 hard error 화 예정. **hard error 가 되는 toolchain 버전은 미정** (rustc 1.96 기준 경고 단계).
- 의존 경로 2개:
  1. `cocoa v0.26` ← `drag v2.1.0` — **다음 `cargo update` 에서 자동 해소 예정**: `drag v2.1.1` 이 cocoa 의존을 제거했다 (`cargo update --dry-run` 이 `Removing cocoa`/`cocoa-foundation` 표시).
  2. `metal v0.31` ← `wgpu-hal v24` ← `wgpu v24` — wgpu 가 `block2`(objc2 생태계) 기반 metal 로 올라간 버전을 채택할 때 해소. wgpu major upgrade 시점에 함께 점검.
- 채택된 대응: **upstream upgrade 대기** (ADR 성 결정 — `[patch.crates-io]` 우회나 의존 제거는 hard-error 화가 임박하기 전에는 수행하지 않는다).

### 점검 방법

```bash
cargo report future-incompatibilities          # 상세 진단 (id 는 빌드 경고가 알려줌)
cargo tree -i block@0.1.6                      # 의존 경로 잔존 확인 — 빈 출력이면 본 항목 삭제
```

### 전환 트리거

다음 중 하나가 충족되면 대기를 끝내고 우회를 적용한다:

- Rust release note 에 uninhabited statics 의 **hard-error 화 버전이 공지**됨 (toolchain 업그레이드 전에 처리 필요)
- 위 경고가 deny 로 승격되어 빌드가 실패함

전환 시 선택지: ① `[patch.crates-io]` 로 block 의 fixed fork 대체 (cocoa/metal 양쪽에 적용되므로 ABI 호환 필수), ② wgpu 를 block2 기반 버전으로 선제 업그레이드. drag 경유 cocoa 라인은 위 1 의 update 로 이미 소멸 전제.
