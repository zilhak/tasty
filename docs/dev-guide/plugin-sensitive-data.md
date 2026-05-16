# Plugin 의 민감 데이터 다루기

이 문서는 **plugin 개발자** 를 대상으로 한다. plugin 안에서 사용자 비밀번호 / OAuth refresh token / API 결제 key 같은 민감한 데이터를 어떻게 저장해야 하는지 가이드한다.

## 핵심: secret 영역은 "민감 데이터 안전 보관소" 가 아니다

Tasty 의 `memory.secret.*` 영역은 이름이 오해를 준다. 실제 보호 수준은 다음과 같다:

| 보장 | 결과 |
|---|---|
| Plugin A 가 IPC 로 plugin B 의 secret 요청 | **차단됨** (owner 분리, 존재 자체를 모름) |
| 사용자 / host (CLI, GUI) 의 secret 조회 | 허용 (의도된 동작) |
| Plugin 이 `~/.tasty/memory.db` 파일을 직접 열기 | **평문이 그대로 보임** |
| `~/.tasty/` 가 백업 / cloud sync 됨 | **평문이 그대로 들어감** |
| 디바이스 분실 + 디스크 암호화 없음 | **평문 노출** |

즉 secret 영역의 보호 약속은 **"plugin 간 IPC 격리"** 한 가지로 좁혀져 있다. 이게 전부다.

자세한 설계 배경은 `docs/design/memory-system.md` 의 "왜 암호화를 하지 않는가" 섹션을 참고.

## 권고

### ✅ secret 영역에 두기 좋은 것

다른 plugin 이 보지 못하게만 하면 충분한 데이터:

- 사용자가 선택한 UI 옵션 (다른 plugin 이 알 필요 없음)
- plugin 자체 캐시 (예: API 호출 결과 메모이즈)
- 작업 도중 누적되는 임시 컨텍스트
- 다른 plugin 에 노출되면 곤란하지만 디스크에 평문으로 있어도 사용자에게 큰 손해는 없는 것

### ❌ secret 영역에 두지 말아야 할 것

**디스크에 평문으로 정착해도 안전한가** 를 기준으로 판단:

- 사용자 master password / passphrase
- OAuth refresh token / API access token (특히 만료 없거나 긴 것)
- 결제 정보, API 결제 key
- 사용자 개인정보 (의료, 금융, 식별 가능 정보)
- 다른 서비스의 인증 자격 증명

이 종류 데이터는 plugin 이 **자체적으로 OS keyring 을 호출** 해서 보관해야 한다. Rust 에서는 `keyring` 크레이트가 표준 선택:

```rust
// Cargo.toml
[dependencies]
keyring = { version = "3", features = ["apple-native", "windows-native", "linux-native"] }

// 사용
let entry = keyring::Entry::new("com.example.myplugin", "refresh_token")?;
entry.set_password(&refresh_token)?;
let token = entry.get_password()?;
```

- service 이름은 plugin id 를 prefix 로 (`com.example.myplugin`).
- user 이름은 데이터 의미를 표현 (`refresh_token`, `api_key`).
- 키체인 없는 환경 (Linux 헤드리스 등) 에서 실패 시 사용자에게 명시적으로 알리고 기능 disable. **평문 파일에 폴백하지 말 것.**

### 외부 파일 + memory 링크 패턴

값이 큰 민감 데이터 (예: 사용자가 업로드한 SSH 개인키 파일) 는:

1. 파일 자체는 plugin 디렉토리 외부의 적절한 위치에 두기 (예: 사용자 홈 디렉토리, 사용자가 명시한 경로).
2. memory 에는 *경로만* 저장 (regular 또는 secret).
3. 파일 자체의 권한은 OS-level 로 강하게 (0600 등).

memory 시스템 자체의 원칙 ("지원 한계 넘는 데이터는 외부 파일 + memory 에 링크") 과 일치한다.

## sandbox 가 들어오면 어떻게 되나

언젠가 plugin sandbox (macOS `sandbox-exec`, Linux `landlock`, Windows `AppContainer`) 가 도입되면:

- plugin 이 `~/.tasty/memory.db` 자체를 못 열게 됨.
- plugin 이 OS keyring 도 못 호출하게 됨 (또는 capability 로 제어).
- 그 시점에 secret 영역의 IPC 격리만으로 진짜 격리 완성.

sandbox 도입은 별도 큰 작업이고 시기 미정. **그 전까지는 위 권고를 따른다.** sandbox 가 들어오는 시점에는 이 문서를 갱신해 "이제 secret 영역도 안전한가" 를 재평가한다.

## 요약

- secret 영역 = "다른 plugin 한테 안 보이는 자리". 그 이상도 이하도 아니다.
- 진짜 민감 데이터 = OS keyring 직접 호출 또는 외부 파일 + 권한 관리.
- 모호하면: "이 데이터가 평문으로 디스크에 있어도 괜찮은가?" 를 자문할 것. 괜찮으면 secret, 아니면 keyring.
