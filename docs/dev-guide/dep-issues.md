# 의존성 이슈 모니터링

외부 의존성에서 비롯돼 우리 코드로 즉시 해결할 수 없는 빌드 이슈 추적.

## `block v0.1.6` — future-incompatibility (모니터링 중)

`cargo build`/`clippy` 끝의 경고:

```
warning: the following packages contain code that will be rejected by a future version of Rust: block v0.1.6
```

- 원인: transitive 의존 `block v0.1.6`(기존 버전)의 uninhabited static — Rust [#74840](https://github.com/rust-lang/rust/issues/74840) 에 따라 향후 hard error 예정(hard-error toolchain 버전 미정).
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

## `winit` — 개인 포크 핀 (한글 IME 수정, 공식 버전 전환 대기)

`Cargo.toml` 의 `[patch.crates-io]` 가 winit 을 개인 포크에 핀:

```toml
winit = { git = "https://github.com/zilhak/winit-ime-fix.git", rev = "dfe2ec8d5bf55bfbca274a7cf56e5fae0d20c1f5" }
```

- **왜 포크인가**: 업스트림 winit 0.30 의 한글 IME 입력 버그를 수정한 포크가 필요. 수정은 업스트림 PR [#4478](https://github.com/rust-windowing/winit/pull/4478) 로 제출돼 있다.
- **왜 `rev` 핀인가**: `branch =` 핀은 브랜치가 움직이면(force-push 포함) 빌드 입력이 조용히 바뀌어 재현성이 깨진다. 고정 commit(`rev`)에 핀해 빌드 입력을 동결한다. 포크 갱신이 필요하면 `rev` 를 의도적으로 갱신한다.
- **리스크**: 포크 레포가 사라지거나 commit 이 GC 되면 빌드 재현 불가. winit 0.30 업스트림 업그레이드는 포크가 따라가야 가능(wgpu/egui-winit 업글과 충돌 여지).

### 점검 / 전환 트리거

- 전환 조건: 업스트림 winit 이 PR #4478(한글 IME 수정)을 머지·릴리스 → `[patch.crates-io]` 항목 제거하고 공식 crates.io 버전으로 교체.
- PR #4478 상태를 주기적으로 확인한다(머지/클로즈/대체 PR 여부).
- **대비책**(포크 레포 소실 시): 해당 commit 을 조직 레포에 미러링하거나 `vendor/` 로 캐싱. 핀 고정으로 충분할 수 있으므로 레포 소실 징후가 보일 때만 착수(과투자 주의).

## `tiny_http` — 레포 사본 + 최소 패치 (공식 버전 전환 대기)

`Cargo.toml` 의 `[patch.crates-io]` 가 tiny_http 를 레포 안의 사본에 핀:

```toml
tiny_http = { path = "vendor/tiny_http" }
```

- **왜 사본인가**: 상류 0.12.0 의 `Content-Length` 리더는 요청을 파괴할 때 읽지 않은 body 를 끝까지 읽는다(drain). 웹훅 413 이 그 drain 없이 연결을 닫게 하는 공개 API(`Request::respond_and_close`)를 더한 최소 패치다. 결정·대안·재검토 조건은 [ADR-0032](../adr/0032-webhook-admission.md), 사본 범위·패치 목록은 [`vendor/tiny_http/PATCHES.md`](../../vendor/tiny_http/PATCHES.md).
- **워크스페이스 멤버가 아니다**: 루트 `Cargo.toml` 의 `exclude` 에 있다 — 워크스페이스 lint·clippy·fmt·파일 SLOC 게이트가 상류 코드를 판정하지 않고, `crates/` 크레이트 수에도 안 들어간다.
- **리스크**: `cargo update` 가 이 의존을 올리지 않고 상류 수정도 자동으로 포함되지 않는다.
- **사본을 고치면 plugin 버전도 오른다**: 번들 plugin 중 `tasty-plugin-agent-stream` 이 이 사본을 링크한다. 사본에서 테스트 전용이 아닌 코드를 고친 커밋은 plugin 버전 게이트가 그 plugin 의 patch +1 을 요구한다 — 검사 대상이 워크스페이스 밖 path 의존까지 닿는다([플러그인 버전 비교](release.md#플러그인-버전-비교)). 테스트 전용 변경과 `PATCHES.md` 는 요구하지 않는다.

### 점검 / 전환 트리거

- 전환 조건: 상류가 요청 단위로 잔여 body 를 읽지 않고 연결을 닫는 공개 API 를 릴리스 → 사본과 `[patch.crates-io]` 줄을 지우고 그 API 로 옮긴다. 옮긴 뒤 `src/webhook/listener_body_tests.rs` 의 `raw_oversize_*` 시험 넷(413)과 `raw_blocked_source_is_closed_without_draining`(429)이 통과해야 한다 — 뒤의 것은 이름 패턴이 달라 `raw_oversize_*` 로 걸리지 않는다.
- RustSec 에 `tiny_http` 권고가 붙으면 사본에 수정을 옮기거나 상류 새 버전 위에 패치를 다시 얹는다.

<a id="은퇴-egui_commonmark--egui-버전-lockstep"></a>

## egui 버전 호환

markdown 본문은 WebView로 그리지만 대용량 파일·파일 열기 확인 팝업은 egui-mesh를 쓴다.
따라서 다른 mesh plugin과 같이 host·plugin의 epaint wire 호환을 확인해야 한다.
기준은 [egui-mesh 채널](egui-mesh-channel.md)을 따른다.
