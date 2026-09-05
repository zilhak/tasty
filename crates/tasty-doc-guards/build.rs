//! 판정기 바이너리가 **자기가 무엇으로 지어졌는지**를 알게 한다.
//!
//! 게이트는 "이 판정기가 지금 소스와 맞는가" 를 물어야 한다. 그 물음을 mtime 으로 재면
//! **git 이 파일을 다시 쓰기만 해도 낡은 것으로 나온다** — 내용이 같아도 그렇다. 이
//! 저장소의 표준 흐름(브랜치를 main 으로 다시 잡고 체리픽)은 파일을 옛 내용으로 되돌렸다
//! 다시 쓰므로 내용은 제자리인데 mtime 만 새것이 된다(실측으로 밟았다). 그러면 경고가
//! 상시 켜지고, 폴백이 상시 동작해서 게이트가 하던 일을 안 하게 된다.
//!
//! 그래서 라이브러리 소스의 지문을 **빌드할 때 구워 넣고**, 실행할 때 디스크와 대조한다.
//! `src/bin/` 은 빼는데, 판정기를 하나 **추가**한 것이 다른 판정기를 낡게 만들면 안 되기
//! 때문이다 — 각 바이너리는 자기 소스 하나를 따로 확인한다.
//!
//! 암호학적 해시가 아니다. 여기서 막는 것은 **사고**(다시 안 지음)이지 위조가 아니다.

use std::path::{Path, PathBuf};

fn main() {
    // 라이브러리 소스가 바뀌면 다시 돈다. `src/bin/` 도 이 감시에 들어가지만, 지문
    // 계산에서는 빠지므로 값이 안 바뀐다(다시 도는 비용만 든다).
    println!("cargo::rerun-if-changed=src");
    let src =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    collect(&src, &src, &mut files);
    files.sort();
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for (rel, bytes) in &files {
        mix(&mut h, rel.as_bytes());
        mix(&mut h, bytes);
    }
    println!("cargo::rustc-env=TASTY_DOC_GUARDS_LIB_FINGERPRINT={h:016x}");
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // 바이너리 소스는 지문에서 뺀다 — 위 doc 참조.
            if path.file_name().is_some_and(|n| n == "bin") {
                continue;
            }
            collect(root, &path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            let rel = path
                .strip_prefix(root)
                .expect("스캔 경로는 src 안이어야 한다")
                .to_string_lossy()
                .replace('\\', "/");
            let bytes = std::fs::read(&path).expect("소스를 읽을 수 없다");
            out.push((rel, normalize(&bytes)));
        }
    }
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
