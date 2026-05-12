# IPC 안정성 정책

이 문서는 Tasty의 IPC 메서드·CLI 명령·plugin protocol에 대한 break 분류와 deprecation 절차를 정의한다. 묶음 문서 — 명명 규칙은 [`cli-naming.md`](cli-naming.md), 생태계 결정은 [`plugin-ecosystem.md`](plugin-ecosystem.md), plugin protocol schema는 [`crates/tasty-plugin-protocol/CHANGELOG.md`](../../crates/tasty-plugin-protocol/CHANGELOG.md).

## 버전 단계

| 단계 | 상태 | break 정책 |
|------|------|----------|
| **0.x (현재)** | 적극 변경 중 | break는 [CHANGELOG.md](../../CHANGELOG.md)에 `(BREAK)` 머리 표기. **한 minor 이상 deprecation 경고 우선** (보안 예외 시 즉시 제거 가능). |
| **1.0 freeze** | 안정선 | SemVer 엄격. `api_version = "1"` 시리즈 schema는 추가만 가능. |
| **1.x** | 점진 추가 | minor에서 추가, major에서 break. |
| **2.0** | 차세대 | `api_version = "2"` 시리즈 시작. plugin은 매니페스트로 명시 선택. |

## Break 분류

| 변경 | 분류 |
|------|------|
| 새 메서드/명령 추가 | minor |
| 메서드 이름 변경 (alias 있을 때) | minor (deprecation) |
| 메서드 이름 변경 (alias 없이) | **major** |
| 응답에 새 필드 추가 (Option/Default) | minor |
| 응답 필드 의미 변경 / 제거 / 타입 변경 / nullability 변경 | **major** |
| 응답 단위·포맷 변경 (ms ↔ s 등) | **major** |
| 요청에 optional+default 파라미터 추가 | minor |
| 요청에 required 파라미터 추가 | **major** |
| optional → required 승격 | **major** |
| 기존 default 값 변경 (의미 있는 변경) | **major** |
| 새 권한 필요 (기존 plugin 동작 중단) | **major** |
| 에러 코드 의미 변경 / `permission_denied` 조건 변화 | **major** |
| 새 enum variant 추가 (deserialize fail 유발 가능) | **major** |
| 새 enum variant 추가 (`#[serde(other)]` fallback 있음) | minor |
| 컬렉션의 정렬·중복·페이지네이션 의미 변화 | **major** |
| 비동기 이벤트(`surface.event`, `command.invoke`, `ipc.result`) 의미 변화 | **major** |
| 핸드셰이크 / 환경변수(`TASTY_HOST_API_VERSION`, auth token 등) 계약 변경 | **major** |
| 예약 namespace·권한 토큰 정책 변경 (`ipc.invoke:<prefix>`) | **major** |

이 표는 출발선이다. 새 분류가 필요한 변경을 만나면 PR description에 명시하고 표에 항목을 추가한다.

## Deprecation 절차

1. **옛 표면 유지** + 새 표면 추가
2. 옛 표면 호출 시 `tracing::warn!("deprecated: <old>, use <new>")` 출력 (예: `src/ipc/alias.rs`)
3. `CHANGELOG.md`의 `Deprecated` 절에 "1.0 tag 직전 제거" 또는 명시적 기한 기록
4. 1.0 직전 일괄 제거 PR

**deprecation 기간**: "한 minor 이상 또는 일정 기간"이 원칙이지만, 보안 이슈·심각한 버그 수정은 즉시 제거 가능. 1.0 이후로는 "한 minor 이상" 규칙을 엄수.

## 1.0 freeze 진입 체크리스트

다음 조건이 모두 충족된 시점에 1.0 tag를 검토한다. 수량보다 **사건 기반 trigger** + 일관성 지표를 우선시한다.

- [ ] `surface.meta_*` 같은 transitional alias를 모두 제거 (`src/ipc/alias.rs::ALIASES` 비어 있음)
- [ ] `tasty-plugin-protocol`의 baseline schema와 CHANGELOG가 안정화 (최근 1개 minor 동안 major break 0건)
- [ ] plugin SDK(`tasty-plugin-sdk`) 문서 + `docs/agent-guide/api-reference.md` + `docs/dev-guide/plugin-development.md`가 실제 IPC와 일치
- [ ] break 분류표와 deprecation 예외 규칙이 사건 기반으로 정착 (보안 예외 1건 이상 운영 경험)
- [ ] 외부 plugin 1개 이상이 새 정책으로 검증됨 (`tasty-plugin-codex`가 후보)
- [ ] [`cli-naming.md`](cli-naming.md)의 verb·namespace 화이트리스트 위반 0건

## plugin-protocol schema 변경

`crates/tasty-plugin-protocol/CHANGELOG.md`에 별도 기록. `api_version`을 메이저 단위로 올리는 시점:

- 기존 메시지 타입의 필드 의미 변경
- 기존 메서드명 제거 (alias 없이)
- 응답 형식 의미 변경
- handshake/auth 계약 변경

추가만 있는 변경(새 메시지, optional+default 필드)은 같은 `api_version` 내에서 `crates/tasty-plugin-protocol/Cargo.toml`의 `version = "1.x.y"` minor만 올린다.

## 자동화 보조

본 정책은 사람의 손과 자동 도구를 함께 쓴다.

- **CHANGELOG `[Unreleased]` 절 존재 검증 테스트** (`tests/changelog_unreleased.rs`) — 릴리스 도구가 절을 비우거나 누락하지 않도록.
- **PR 템플릿** (예정): "IPC/CLI/plugin-protocol 변경 여부", "BREAK 여부", "CHANGELOG 반영 여부" 체크박스. (`.github/pull_request_template.md` — 미작성)
- **conventional commits → CHANGELOG 초안 도구** (예정): `git-cliff` 같은 도구가 `[Unreleased]` 자동 추가. 사람은 break/deprecation 정확성만 검토.
- **경로 기반 규칙** (예정): `crates/tasty-plugin-protocol/`에 변경이 있으면 해당 CHANGELOG 갱신 강제.

이 자동화 항목은 본 묶음(early-cost-cleanup) 범위가 아니다. 1.0 freeze 진입 직전까지 점진적으로 도입.
