# ADR-0018: Claude Design 세션 자격증명은 평문으로 저장한다

- **Status**: Superseded by [ADR-0057](0057-remove-claude-design-plugin.md) — claude-design 플러그인 자체가 제거되어 본 결정의 대상이 사라짐
- **Date**: 2026-06-22
- **Tags**: claude-design, plugin, secret, security, encryption, auth, trust-boundary

> Claude Design 플러그인(`com.tasty.claude-design`)은 로그인 세션(Playwright `storageState` —
> claude.ai 쿠키 묶음)을 `TASTY_PLUGIN_DATA_DIR/auth.json` 에 **평문**으로 둔다. 초기 설계는
> OS keyring + 암호화를 상정했으나 철회한다. 본 ADR 은 [ADR-0005](0005-memory-secret-not-a-vault.md) 의 결론을 이 플러그인의
> 자격증명에 그대로 적용한 것이다.

## Context

Claude Design 플러그인은 `claude.ai/design` 에 헤드풀 브라우저로 로그인한 뒤, 재사용을 위해
세션을 디스크에 저장해야 한다. 이 세션(`storageState`)은 **살아있는 계정 자격증명**이다 —
유출되면 계정 접근권이 그대로 넘어간다.

두 가지 저장 방식을 검토했다.

1. **OS keyring 직접** — 실측 결과 **불가**. Windows Credential Manager 는 한 항목 blob 이
   ~2560 UTF-16 byte(≈1280 ASCII char)로 제한된다(로컬 실측: 1024B 성공, 2048B 실패).
   `storageState` 는 쿠키 수십 개로 수 KB라 들어가지 않는다.
2. **keyring 키 + AES-256-GCM 암호화 파일** — 키는 keyring(작음), 본문은 파일에 암호화. 크기
   문제는 풀리지만, [ADR-0005](0005-memory-secret-not-a-vault.md) 가 이미 같은 구조
   (AES-GCM + OS keyring master key)를 **철회**했다. 이유: plugin sandbox 가 없는 현재
   trust boundary 에서 같은 user 권한의 악성 프로세스는 키도 파일도 읽을 수 있어 암호화가
   *false sense of security* 일 뿐이고, 키 환경 변동 시 silent decrypt 실패를 부른다.

## Decision

**`storageState` 를 평문 파일(`auth.json`)로 저장한다.** 데이터-앳-레스트 암호화(AES-GCM/
keyring)를 하지 않는다. 보호 범위를 **"OS user / 파일 권한" 한 가지로 정직하게 좁힌다.**

- Unix: 파일 모드 `0600`(소유자만). Windows: 파일이 user 프로필(`data_dir`) 아래라 기본 ACL
  이 user-scoped — 그 이상의 ACL 강화는 약속하지 않는다.
- keyring 가용성에 의존하지 않으므로 Linux 헤드리스/WSL/CI 에서도 동일하게 동작한다(평문/
  암호문 혼재로 인한 손상 위험 없음).
- 코드: 당시 `tasty-plugin-claude-design` 크레이트의 `auth.rs`(모듈 주석 + 회귀 테스트
  `save_load_clear_roundtrip`, `unix_perms_are_owner_only`). 그 plugin 은 [ADR-0057](0057-remove-claude-design-plugin.md)
  로 제거돼 이 좌표는 더 이상 존재하지 않는다.

이는 ADR-0005 의 결론을 자격증명 보관에 일관되게 확장한 것이다. 단, 데이터 성격이 달라
**위협의 우선순위가 다름**을 명시한다(아래 Consequences).

## Consequences

- **얻은 것**: *false sense of security* 제거 — "암호화돼 안전하다"는 지킬 수 없는 약속을
  하지 않는다. keyring 의존이 없어 크로스플랫폼 동작이 단순·균일하다. 향후 plugin sandbox
  (sandbox-exec / landlock / AppContainer)가 도입되면 추가 코드 없이 강해진다.
- **잃은 것 / 명시적 비보장**: `auth.json` 이 **평문**이므로 다음 시나리오에서 세션이 노출된다.
  - 디스크/파일 직접 열람, 기기 도난, 백업·클라우드 sync(OneDrive/Dropbox 등)에 `data_dir` 포함.
  - 같은 user 권한의 악성 프로세스(plugin sandbox 영역 — 본 ADR 범위 밖).
  → 사용자가 신경 쓰는 환경이라면 `data_dir` 을 sync 대상에서 제외해야 한다.
- **운영**: 노출 시 사용자는 `tasty design logout`(파일 삭제) 또는 claude.ai 세션 무효화로
  대응한다. 만료/무효 세션은 `tasty design login` 재실행으로 갱신한다.

## Alternatives Considered

- **OS keyring 직접**: Windows blob 크기 한계로 기각(실측). claude-design 세션은 keyring 한 항목
  에 담기에 너무 크다.
- **keyring 키 + AES-GCM 파일**: ADR-0005 와 동일하게 기각. ① sandbox 부재로 같은 user
  악성 프로세스가 키·파일 모두 접근 → 결정적 보호 아님. ② keyring 환경 변동 시 silent decrypt
  실패. 단 *파일 단독 유출(sync/백업/도난)* 위협에는 유효하다는 반론이 있었고, 채택 시 그
  한계를 ADR 로 명시하는 조건이었다. 최종적으로 **ADR-0005 와의 일관성·정직성**을 우선해 평문
  을 택했다.
- **평문 파일 + Windows DPAPI 등 OS 네이티브 암호화**: OS 별 코드 분기 3벌 + 플랫폼 편차.
  현 단계 비용 대비 이득이 ADR-0005 논리상 제한적이라 보류.

## Reconsideration Triggers

- **plugin sandbox 도입** (sandbox-exec / landlock / AppContainer). 그 시점에 plugin 이
  `data_dir`/keyring 에 임의 접근할 수 없게 되어, 자격증명 보관의 trust boundary 가 바뀐다 —
  "이제 암호화가 의미 있는가"를 재평가한다(ADR-0005 와 동일 트리거).
- **`storageState` 가 keyring 한도(≈1280 char) 안에 드는 형태로 축소 가능**해지고, 그것만으로
  유효 세션이 유지된다면 keyring 직접 보관을 재검토할 수 있다.

## References

- [ADR-0005](0005-memory-secret-not-a-vault.md) — memory secret 영역 평문 결정(본 ADR 의 모태)
- [`dev-guide/plugin-sensitive-data.md`](../dev-guide/plugin-sensitive-data.md) — 민감 데이터 가이드
- 코드: 없음 — plugin 이 [ADR-0057](0057-remove-claude-design-plugin.md) 로 제거됐다.
