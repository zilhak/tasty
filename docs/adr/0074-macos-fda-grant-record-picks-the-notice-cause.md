# ADR-0074: macOS Full Disk Access 보유 기록은 안내 원인을 고르는 데만 쓰고 cdhash로 앱을 식별한다

- **Status**: Accepted — macOS 실 기기 측정 없이 cfg 분기로 구현했다. 아래 재검토 조건의 실제 실행 확인이 남아 있다
- **Date**: 2026-10-08
- **Tags**: macos, permissions, tcc, full-disk-access, boot, codesign, adr-0052
- **Group**: foundation

## Context

부팅 권한 안내는 매 부팅 현재 상태로만 띄운다. 안내를 띄웠다는 기록이나 끄는 토글은 두지 않는다. 그런 기록이 있으면 권한이 나중에 초기화됐을 때 알릴 방법이 없어지기 때문이다([macOS 권한](../features/macos-permissions/index.md) "Full Disk Access — 추정과 안내").

그런데 Full Disk Access(FDA)를 잃는 원인은 하나가 아니고 처방도 다르다. Tasty는 배포 DMG도 ad-hoc 서명이라(`scripts/build-macos-dmg.sh`의 DMG 경로는 `SIGN_IDENTITY="-"`, 공증 없음) TCC의 FDA 행이 서명 해시(cdhash) 하나에 묶인다. 업데이트나 재빌드로 바이너리가 바뀌면 그 행은 시스템 설정 목록에 켜진 채 남아 있어도 더 이상 매칭되지 않는다. 이때 처방은 목록에서 지우고 다시 추가하는 것이다. 반면 같은 앱인데 사용자가 밖에서 껐다면 다시 켜면 된다. 현재 상태만으로는 두 경우가 똑같이 "거부"로 보여 어느 처방을 줄지 고를 수 없다.

원인을 가르려면 이전에 보유했었는지와 그때 앱이 무엇이었는지를 기억해야 한다. 이것이 "기록을 남기지 않는다"는 원칙과 충돌하는지 정해야 했다.

## Decision

FDA 보유를 관측할 때 그때의 자기 서명 해시(cdhash)를 데이터 홈의 별도 파일(`<데이터 홈>/macos-fda-grant`, 한 줄 `cdhash=<16진>`)에 남긴다. 거부로 보이는 부팅에서 이 기록과 지금 해시를 비교해 안내의 FDA 문단 갈래를 고른다.

- 기록 없음 → `Never`(직접 추가하라는 기본 문단).
- 기록의 해시와 지금 해시가 다름 → `Stale`(업데이트·재빌드가 끊었으니 지우고 다시 추가).
- 같음 → `Revoked`(밖에서 꺼졌으니 다시 켜기. `tccutil reset`처럼 목록 항목이 지워졌으면 다시 추가).
- 어느 쪽 해시든 얻지 못했으면 `Stale`로 본다. `Stale`의 처방은 밖에서 꺼진 경우도 고치지만 `Revoked`의 처방은 앱이 바뀐 경우를 고치지 못한다.

이 기록은 안내를 억제하지 않는다. 안내를 띄울지는 지금처럼 매 부팅 현재 상태로 정하고, 기록은 문단 선택과 설정 탭 처방 줄에만 쓴다. 금지한 것은 "안내를 보였다·껐다"는 억제 기록이고, 원인 판별용 관측값은 그 범주가 아니다. 기록은 보유를 관측했을 때만, 값이 달라질 때만 쓴다. 거부나 판단 불가 관측은 기록을 지우지 않는다. 지우면 거부가 이어지는 다음 부팅에서 원인을 가를 수 없다.

cdhash는 `codesign`을 띄우지 않고 Security.framework(`SecCodeCopySelf` → `SecCodeCopyStaticCode` → `SecCodeCopySigningInformation`의 `kSecCodeInfoUnique`)로 구한다. 판정과 기록 형식은 순수 함수(`tasty_platform` 내부 `macos_fda_history`)라 macOS가 아닌 환경에서도 시험하고, 파일 접근과 FFI는 `#[cfg(all(target_os = "macos", feature = "gui"))]` 안에만 둔다.

## Consequences

안내가 원인에 맞는 처방을 준다. 특히 업데이트 뒤 "목록에 켜져 있는데 왜 안 되지"라는 상태를 설명하고 지우고 다시 추가하라고 알린다. 설정 탭의 Full Disk Access 행도 같은 갈래를 읽어 `Stale`일 때 처방 줄을 붙인다.

데이터 홈에 앱이 쓰는 파일이 하나 늘어난다. 사용자 설정(`config.toml`)과 섞지 않았으므로 설정 저장·`debug settings apply`·설정 파일 복구가 이 값을 건드리지 않고, 사용자가 설정을 편집하다 지우는 일도 없다. 파일을 지우면 다음 거부 부팅의 갈래가 `Never`로 떨어질 뿐 안내 자체는 그대로 뜬다.

cdhash 비교는 ad-hoc 서명과 맞물린 선택이다. TCC가 실제로 행을 묶는 값이라, 같은 버전을 다시 빌드한 개발자도 `Stale`로 잡힌다. 대신 Developer ID 서명으로 바뀌어 TCC 행이 식별자·팀 조건으로 묶이면, 업데이트로 cdhash가 바뀌어도 권한은 유지되므로 이 비교는 없는 원인을 `Stale`로 보고하게 된다.

FDA 추정 자체가 경로 접근 우회 판정이라 그 한계(오탐)는 그대로 물려받는다. 이 기록은 그 위에서 문단을 고를 뿐이다.

## Alternatives Considered

- 기록 없이 현재 상태만 본다. 원칙과 가장 단순하게 맞지만 원인을 가를 수 없어, 업데이트로 끊긴 사용자에게 "추가하라"는 문단만 보이고 이미 목록에 켜진 Tasty를 본 사용자가 원인을 찾지 못한다.
- `CFBundleVersion`으로 앱이 바뀌었는지 본다. 배포본 업데이트는 잡지만 같은 버전을 다시 빌드하는 개발자는 잡지 못하고, TCC가 실제로 보는 값도 아니다.
- 기록을 `Settings`(config.toml)에 둔다. 저장 경로가 이미 있지만 사용자 설정과 앱 관측값이 섞이고, 설정 내보내기·복구·debug 설정 적용이 이 값을 함께 옮기거나 덮는다. 이전에 같은 구조체에 있던 `macos_fda_notice_shown`은 안내를 억제하는 기록이라 삭제됐다.
- `codesign -dvvv`를 띄워 cdhash를 읽는다. 부팅 경로에서 프로세스를 띄우고 출력 형식에 기대게 된다.

## Reconsideration Triggers

코드와 설정에서 확인:

- 배포 서명이 Developer ID와 공증으로 바뀌면(`scripts/build-macos-dmg.sh`의 `SIGN_IDENTITY`, `.github/workflows/release.yml`의 `build-macos`) TCC 행이 cdhash가 아닌 조건으로 묶인다. 그때는 비교 대상을 다시 정하거나 `Stale` 갈래를 서명 방식에 따라 끈다.
- 직접 빌드 구분 신호(`notice_inputs()`의 두 번째 값)가 생기면 이 기록과 같은 자리에서 정할지 다시 본다.

실행 결과로 확인:

- macOS 실 기기에서 FDA를 부여한 채 부팅해 기록 파일에 `cdhash=`가 채워지는지, 그 값이 `codesign -dvvv Tasty.app`의 `CDHash`와 같은지 본다. 다르거나 비어 있으면 `kSecCodeInfoUnique` 취득 경로가 틀린 것이다.
- 재빌드로 cdhash를 바꾼 뒤 FDA 프로브가 거부를 보는지, 그때 안내가 `Stale` 문단을 보이는지 본다. 프로브가 계속 보유로 보이면(시스템 경로 TCC.db가 FDA 없이도 열리는 경우) 이 갈래 전체가 발화하지 않으므로 프로브부터 고친다.

## References

- [macOS 권한](../features/macos-permissions/index.md) — 안내 갈래, 설정 탭, 추정 방법의 현재 동작
- [ADR-0052](0052-permission-prompts-are-raised-on-request-not-at-boot.md) — 권한 요청 시점과 부팅 안내의 역할
- `crates/tasty-platform/src/macos_fda_history.rs` — 갈래 판정, 기록 형식, cdhash 취득
- `crates/tasty-platform/src/macos_permissions.rs` — `refresh_permission_snapshot`, `notice_inputs`
