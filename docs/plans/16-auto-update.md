# 16. 자동 업데이트

## cmux 구현 방식

- Sparkle 프레임워크 (macOS 전용)
- 1시간 간격 자동 확인
- 타이틀바 업데이트 알림

## 크로스 플랫폼 구현 방안

### 접근 방식

네이티브 GUI 앱이므로 적절한 업데이트 다이얼로그를 제공할 수 있다.
cmux의 Sparkle 프레임워크와 유사한 UX를 크로스 플랫폼으로 구현.

### 업데이트 흐름

1. 시작 시 또는 주기적으로 GitHub Releases API에서 최신 버전 확인
2. 새 버전이 있으면 인앱 알림 표시
3. 사용자가 클릭하면 업데이트 다이얼로그 표시
4. 다운로드 진행률 바 + 릴리즈 노트 표시
5. 다운로드 완료 후 재시작 확인

### GUI 업데이트 다이얼로그

```
┌─ 업데이트 가능 ─────────────────────────┐
│                                        │
│  tasty v0.3.0 사용 가능                 │
│  현재 버전: v0.2.0                      │
│                                        │
│  변경사항:                               │
│  - 새로운 분할 패인 애니메이션            │
│  - IME 한글 입력 개선                    │
│  - 버그 수정: 윈도우 리사이즈 깜빡임      │
│                                        │
│  ████████████░░░░ 67%                  │
│                                        │
│  [나중에]            [업데이트 & 재시작]  │
└────────────────────────────────────────┘
```

### 배포 채널별 업데이트 방법

| 채널 | 업데이트 방법 |
|------|-------------|
| **GitHub Releases** | 인앱 자동 업데이트 (self-update) |
| **Homebrew** (macOS) | `brew upgrade tasty` |
| **Cargo** | `cargo install tasty` |
| **Scoop/WinGet** (Windows) | `scoop update tasty` / `winget upgrade tasty` |
| **AUR** (Arch Linux) | `yay -Syu tasty` |

### self-update 구현

`self_update` 크레이트로 GitHub Releases 기반 자체 업데이트:

```rust
let status = self_update::backends::github::Update::configure()
    .repo_owner("zilhak")
    .repo_name("tasty")
    .bin_name("tasty")
    .current_version(env!("CARGO_PKG_VERSION"))
    .build()?
    .update()?;
```

### OS별 바이너리 교체

| OS | 방법 |
|----|------|
| **Windows** | 실행 중인 바이너리 교체 불가 → 임시 파일로 다운로드 후 재시작 시 교체 |
| **macOS** | 바이너리 교체 가능, 또는 .app 번들 교체 |
| **Linux** | 바이너리 직접 교체 가능 |

## 최적화 전략

- **백그라운드 다운로드**: 업데이트 파일을 백그라운드에서 다운로드하여 UI 블로킹을 방지한다. 다운로드 진행률만 사이드바에 표시한다.
- **델타 업데이트**: 전체 바이너리 대신 변경된 부분만 다운로드한다. bsdiff 알고리즘으로 패치 크기를 최소화한다.
- **체크 빈도 제한**: 업데이트 확인을 적절한 간격으로 제한한다 (예: 1시간). 마지막 체크 시간을 기록하여 불필요한 네트워크 요청을 줄인다.
- **대역폭 제한**: 업데이트 다운로드 중 네트워크 대역폭을 제한하여 다른 작업(SSH 세션 등)을 방해하지 않는다.

## 구현 가능 여부

| OS | 가능 여부 | 비고 |
|----|-----------|------|
| Windows | ✅ 가능 | self-update + GUI 다이얼로그 |
| macOS | ✅ 가능 | self-update + GUI 다이얼로그 |
| Linux | ✅ 가능 | self-update + GUI 다이얼로그 |

GUI 앱이므로 cmux의 Sparkle과 유사한 매끄러운 업데이트 UX가 가능하다.
