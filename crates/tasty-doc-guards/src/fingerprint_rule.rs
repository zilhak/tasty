// 라이브러리 소스 지문의 **계산 규칙** — 한 벌만 둔다.
//
// 이 규칙은 두 곳에서 돌아야 한다: 빌드 스크립트가 값을 **구울 때**와, 판정기가
// 실행 중에 디스크와 **대조할 때**. 두 벌로 두면 한쪽만 바뀌었을 때 값이 영영 안 맞아
// 판정기가 상시 낡은 것으로 나오고, 그 상태는 폴백만 조용히 켠다.
//
// 그래서 규칙을 이 파일 하나에 두고 양쪽이 **부른다** — 빌드 스크립트는 크레이트를
// 의존할 수 없으므로 `include!` 로 같은 원문을 가져간다. 주석으로 "같은 규칙" 이라고
// 적는 대신 원문을 하나로 만든 것이라, 규칙을 고치면 양쪽이 함께 고쳐진다.
//
// 규칙의 정본은 **아래 구현**이다. 눈에 띄는 것을 적어 두면: 경로 **정렬**(readdir 순서는
// 파일시스템마다 다르다) · `bin/` **제외**(판정기를 하나 추가한 것이 다른 판정기를 낡게
// 만들면 안 된다) · 줄바꿈 **LF 정규화**(체크아웃 설정이 같은 내용에 다른 지문을 주면
// 안 된다). 이 목록은 안내이지 명부가 아니다 — 규칙이 하나 늘면 이 주석이 아니라
// 아래 코드가 정답이다.
//
// 암호학적 해시가 아니다. 여기서 막는 것은 **사고**(다시 안 지음)이지 위조가 아니다.
//
// `include!` 로 들어가므로 이 파일에는 `use` 와 테스트를 두지 않는다 — 경로는 전부
// 절대 경로로 적고, 테스트는 부르는 쪽에 둔다.

/// 디렉토리 하나의 지문. 읽을 수 없는 자리가 있으면 `None`(= 물을 수 없다).
pub(crate) fn fingerprint(lib_dir: &std::path::Path) -> Option<String> {
    let mut files = Vec::new();
    collect(lib_dir, lib_dir, &mut files)?;
    files.sort();
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for (rel, bytes) in &files {
        mix(&mut h, rel.as_bytes());
        mix(&mut h, bytes);
    }
    Some(format!("{h:016x}"))
}

fn collect(
    root: &std::path::Path,
    dir: &std::path::Path,
    out: &mut Vec<(String, Vec<u8>)>,
) -> Option<()> {
    for entry in std::fs::read_dir(dir).ok()? {
        let path = entry.ok()?.path();
        if path.is_dir() {
            // 바이너리 소스는 지문에서 뺀다 — 위 모듈 문서 참조.
            if path.file_name().is_some_and(|n| n == "bin") {
                continue;
            }
            collect(root, &path, out)?;
        } else if path.extension().is_some_and(|e| e == "rs") {
            let rel = path
                .strip_prefix(root)
                .ok()?
                .to_string_lossy()
                .replace('\\', "/");
            out.push((rel, normalize(&std::fs::read(&path).ok()?)));
        }
    }
    Some(())
}

/// 줄바꿈을 LF 로 맞춘다 — 같은 내용이 체크아웃 설정 때문에 다른 지문을 갖지 않게.
fn normalize(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\r' && bytes.get(i + 1) == Some(&b'\n') {
            i += 1;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

fn mix(h: &mut u64, bytes: &[u8]) {
    for b in bytes {
        *h ^= u64::from(*b);
        *h = h.wrapping_mul(0x100_0000_01b3);
    }
}
