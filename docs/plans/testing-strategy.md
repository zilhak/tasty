# 테스트 전략

## 테스트 피라미드

```
         ╱╲
        ╱  ╲         E2E (소수, 느림)
       ╱────╲
      ╱      ╲       통합 (중간)
     ╱────────╲
    ╱          ╲      단위 (다수, 빠름)
   ╱────────────╲
```

## 단위 테스트

- 대상: 순수 로직 (레이아웃 계산, 설정 파싱, 키맵 조회, 알림 합치기, 퍼지 매칭 등)
- 프레임워크: Rust 내장 `#[test]` + `cargo test`
- 목표: 핵심 로직의 80%+ 커버리지
- 모킹: `mockall` 크레이트 (trait 기반 인터페이스에 대해)

예시:

```rust
#[cfg(test)]
mod tests {
    #[test]
    fn split_tree_layout_calculation() { ... }

    #[test]
    fn notification_coalescing_within_window() { ... }

    #[test]
    fn fuzzy_match_scoring() { ... }

    #[test]
    fn config_toml_parsing() { ... }
}
```

## 통합 테스트

- 대상: 컴포넌트 간 상호작용
  - termwiz + PTY: 셸 시작 → 명령 실행 → 출력 파싱 → 셀 그리드 확인
  - IPC: CLI → 소켓 → GUI 명령 처리 → 응답
  - 세션 복원: 저장 → 로드 → 레이아웃 비교
- 위치: `tests/` 디렉토리
- 주의: GPU 테스트는 CI에서 headless로 실행하기 어렵다. GPU 로직과 렌더링을 분리하여 로직만 테스트한다.

## 시각적 회귀 테스트

- 대상: GPU 렌더링 출력이 의도대로인지 확인
- 방법:
  - 참조 스크린샷과 현재 렌더링 결과 비교
  - 소프트웨어 렌더러 (wgpu의 llvmpipe 백엔드)로 headless 렌더링
  - 픽셀 diff 허용 범위 설정 (서브픽셀 렌더링 차이 고려)
- 도구: 자체 스크린샷 비교 또는 `image` 크레이트 + 커스텀 diff
- 실행: 수동 또는 릴리즈 전 검증 (CI에서 자동화 가능하지만 초기에는 수동)

## 성능 벤치마크

- 대상: 렌더링 프레임 시간, PTY I/O 처리량, 시작 시간
- 프레임워크: `criterion` 크레이트
- 벤치마크 항목:
  - `cat /dev/urandom | head -c 10M` 처리 시간
  - 프레임 렌더링 시간 (평균, P99)
  - 콜드 스타트 → 첫 프레임 시간
  - 워크스페이스 전환 지연
  - 글리프 아틀라스 채우기 속도
- 회귀 감지: 이전 결과와 비교하여 10% 이상 저하 시 경고

## PTY 테스트

PTY는 OS별 동작이 다르므로 크로스 플랫폼 테스트가 중요하다.

테스트 항목:

- 셸 생성/종료
- 리사이즈 이벤트 전달
- 대량 출력 처리
- 유니코드/CJK 입출력
- ConPTY 특이 동작 (Windows)

## 테스트 인프라

| 항목 | 도구 |
|------|------|
| 단위/통합 | `cargo test` |
| 벤치마크 | `criterion` |
| 모킹 | `mockall` |
| 시각적 | 자체 스크린샷 diff |
| 커버리지 | `cargo-tarpaulin` 또는 `cargo-llvm-cov` |
