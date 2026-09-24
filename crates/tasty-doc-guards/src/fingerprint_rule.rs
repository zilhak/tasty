// 빌드 스크립트와 실행 바이너리가 include!로 공유하는 소스 지문 계산.
// 경로 정렬과 LF 정규화로 플랫폼 차이를 없애고, bin 소스는 각 바이너리에서 따로 비교한다.
// 재빌드 누락 검사용이며 위조 방지용 해시가 아니다.
// include! 소비자와 충돌하지 않도록 use와 테스트는 두지 않는다.

/// 디렉터리의 지문을 계산하며 읽기에 실패하면 None을 반환한다.
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
