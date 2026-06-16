# 플러그인의 민감 데이터 다루기

플러그인 안에서 비밀번호 / OAuth refresh token / API 결제 key 같은 민감 데이터를 어떻게 저장해야 하는지. 저장 위치 전반은 [plugin-development §6](plugin-development.md#6-데이터-저장-위치).

## 핵심: secret 영역은 "안전 보관소"가 아니다

`memory.secret.*` 는 이름이 오해를 준다. 실제 보호 수준:

| 상황 | 결과 |
|------|------|
| 플러그인 A 가 IPC 로 B 의 secret 요청 | **차단**(owner 분리, 존재조차 모름) |
| 사용자/host(CLI·GUI)의 secret 조회 | 허용(의도된 동작) |
| 플러그인이 `~/.tasty/memory.db` 직접 열기 | **평문 그대로 보임** |
| `~/.tasty/` 백업/cloud sync | **평문 그대로 들어감** |
| 디바이스 분실 + 디스크 암호화 없음 | **평문 노출** |

즉 secret 의 보장은 **"플러그인 간 IPC 격리" 한 가지**뿐.

## 권고

### ✅ secret 에 둬도 되는 것
다른 플러그인이 못 보게만 하면 충분한 것 — UI 옵션, API 응답 캐시, 작업 중 임시 컨텍스트. *디스크에 평문으로 있어도 사용자에게 큰 손해 없는* 데이터.

### ❌ secret 에 두면 안 되는 것
**디스크 평문 정착이 안전한가**로 판단 — master password, OAuth refresh/access token, 결제 정보·API 결제 key, 개인정보(의료/금융/식별), 타 서비스 자격증명.

이 종류는 플러그인이 **직접 OS keyring** 을 호출한다. Rust `keyring` 크레이트:

```rust
let entry = keyring::Entry::new("com.example.myplugin", "refresh_token")?;
entry.set_password(&token)?;
let token = entry.get_password()?;
```

- service 이름은 plugin id prefix, user 이름은 데이터 의미(`refresh_token`).
- 키체인 없는 환경(Linux headless 등)에서 실패 시 사용자에게 명시 알림 + 기능 disable. **평문 파일 폴백 금지.**

### 큰 민감 데이터 — 외부 파일 + memory 링크
값이 큰 것(예: SSH 개인키): ① 파일은 적절한 외부 위치, ② memory 엔 *경로만*, ③ 파일 권한 OS-level 강하게(0600). memory 시스템 원칙("한계 넘는 데이터는 외부 파일 + 링크")과 동일.

## sandbox 가 들어오면

플러그인 sandbox(macOS `sandbox-exec` / Linux `landlock` / Windows `AppContainer`)가 도입되면 플러그인이 `memory.db` 직접 열기·keyring 직접 호출도 capability 로 제어된다 — 그 시점에 secret 의 IPC 격리만으로 진짜 격리가 완성된다. 도입 시기 미정. **그 전까지 위 권고를 따른다.**

## 요약

- secret 영역 = "다른 플러그인한테 안 보이는 자리", 그 이상도 이하도 아니다.
- 진짜 민감 데이터 = OS keyring 직접 호출 또는 외부 파일 + 권한 관리.
- 모호하면 "이 데이터가 평문으로 디스크에 있어도 괜찮은가?" — 괜찮으면 secret, 아니면 keyring.
</content>
