# 14. 포트 스캐닝

## cmux 구현 방식

- `ps` + `lsof` 배치 호출
- 셸별 TTY 기반 스캐닝
- 킥-합체-버스트 패턴 (200ms 합체, 6회 버스트)
- 사이드바에 리스닝 포트 표시

## 크로스 플랫폼 구현 방안

### OS별 포트 스캐닝 방법

| OS | 방법 | 세부 |
|----|------|------|
| **Linux** | `/proc/net/tcp` 파싱 + `/proc/{pid}/fd` 조회 | 외부 프로세스 호출 불필요, 빠름 |
| **macOS** | `lsof -i -P -n` 또는 `netstat` | 외부 프로세스 호출 필요 |
| **Windows** | `GetExtendedTcpTable` Win32 API | 네이티브 API, 빠름 |

### Rust 구현

```rust
#[cfg(target_os = "linux")]
fn scan_ports(pid: u32) -> Vec<u16> {
    // /proc/net/tcp 파싱
}

#[cfg(target_os = "macos")]
fn scan_ports(pid: u32) -> Vec<u16> {
    // lsof -i -P -n -p {pid} 호출
}

#[cfg(target_os = "windows")]
fn scan_ports(pid: u32) -> Vec<u16> {
    // GetExtendedTcpTable API
}
```

또는 `netstat2` 크레이트로 크로스 플랫폼 추상화 가능.

### 프로세스 트리 추적

자식 PTY의 PID에서 시작하여 프로세스 트리를 순회하며 리스닝 포트를 수집:

| OS | 프로세스 트리 조회 |
|----|-----------------|
| **Linux** | `/proc/{pid}/children` 또는 `/proc/*/stat` 파싱 |
| **macOS** | `proc_listchildpids()` 또는 `ps -o pid,ppid` |
| **Windows** | `CreateToolhelp32Snapshot` + `Process32Next` |

`sysinfo` 크레이트가 크로스 플랫폼 프로세스 정보를 제공한다.

### GUI에서의 표시

사이드바에 포트를 아이콘과 함께 표시. 클릭하면 시스템 브라우저에서 `http://localhost:{port}`를 연다.

## 최적화 전략

- **스캔 합치기**: 여러 워크스페이스의 포트 스캔 요청을 하나의 OS 호출로 합친다. `/proc/net/tcp` 파싱 또는 `GetExtendedTcpTable` 호출을 한 번만 수행한다.
- **캐싱**: 마지막 스캔 결과를 캐싱하고 TTL 기반(예: 5초)으로 갱신한다. TTL 이내에 요청이 오면 캐시를 반환한다.
- **변경 감지**: 이전 결과와 비교하여 변경된 경우에만 UI를 업데이트한다. 포트 목록이 동일하면 사이드바 재렌더링을 스킵한다.
- **프로세스 트리 캐싱**: 프로세스 부모-자식 관계를 캐싱하고, 변경 시에만 재조회한다. 포트→프로세스 매핑에서 프로세스 정보 조회 비용을 줄인다.

## 구현 가능 여부

| OS | 가능 여부 | 비고 |
|----|-----------|------|
| Windows | ✅ 가능 | Win32 API 직접 호출 |
| macOS | ✅ 가능 | lsof 또는 proc API |
| Linux | ✅ 가능 | /proc 파일시스템으로 가장 효율적 |
