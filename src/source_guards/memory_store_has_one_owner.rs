//! 제품 코드에서 memory.db 저장소는 memory writer 하나가 소유한다. 다른 소비자는 writer 요청으로만
//! 닿는다(docs/design/systems/storage-contract.md의 "저장 실행기" 절).
//!
//! `tasty-memory`의 공유 잠금 경로(`WriterStore::shared`·`MemoryAccess::inline`)는 `test-support`
//! 기능 뒤에 있어 제품 빌드에서 컴파일되지 않는다. 이 검사는 그 밖에서 저장소를 잠금으로 다시
//! 공유하는 형태를 찾는다. 테스트 전용 파일과 cfg(test) 항목은 제외한다. 타입 별칭이나 제네릭으로
//! 감춘 공유는 추적하지 않는다. 본체의 `AppServices::with_memory`는 cfg(test)라 컴파일 조건이 막고,
//! task runtime의 같은 이름 함수는 writer 요청이라 이름으로 찾지 않는다.

use std::path::PathBuf;

use tasty_doc_guards::cfg_predicate::cfg_gated_lines;
use tasty_doc_guards::shipping_scope;
use tasty_doc_guards::source_text::mask_non_code;

/// 저장소를 소유하고 공유 경로를 시험 기능 뒤에 두는 크레이트. 컴파일 조건이 이 규칙을 강제한다.
const OWNER_CRATE: &str = "crates/tasty-memory/";

const SHARED_FORMS: &[&str] = &[
    "Mutex<dyn MemoryStorage",
    "Mutex<dyn tasty_memory::MemoryStorage",
    "Mutex<tasty_memory::MemoryStore>",
    "Mutex<MemoryStore>",
    "WriterStore::shared(",
    "MemoryAccess::inline(",
    "lock_inline_for_tests(",
];

/// 부팅이 저장소를 열어 writer에 넘기기 전까지 잠시 감싸는 자리. writer를 만들 때 `Arc::try_unwrap`으로
/// 소유를 넘기며, 다른 사본이 남아 있으면 부팅을 실패시킨다.
const HANDOVER: &[(&str, &str)] = &[
    (
        "src/boot.rs",
        "init_with_config가 돌려준 저장소를 부팅 정비 후 넘긴다",
    ),
    (
        "src/boot/wiring.rs",
        "진단 핸들을 꺼낸 뒤 try_unwrap으로 writer에 소유를 넘긴다",
    ),
    (
        "src/app.rs",
        "부팅에서 받은 저장소를 wiring에 그대로 넘긴다",
    ),
];

fn shipping_hits() -> (Vec<String>, usize) {
    let root = super::repo_root();
    let sources = super::rust_sources();
    let test_only = shipping_scope::test_only_files(&root, &sources);
    let mut hits = Vec::new();
    let mut files = 0usize;
    for (path, raw) in &sources {
        let rel = path.to_string_lossy().replace('\\', "/");
        if rel.starts_with(OWNER_CRATE) || test_only.contains(path) {
            continue;
        }
        files += 1;
        let masked = mask_non_code(raw);
        let lines: Vec<&str> = masked.lines().collect();
        let gated = cfg_gated_lines(&lines, "test");
        for (i, line) in lines.iter().enumerate() {
            if gated.get(i).copied().unwrap_or(false) {
                continue;
            }
            if SHARED_FORMS.iter().any(|form| line.contains(form)) {
                hits.push(format!("{rel}:{}  {}", i + 1, line.trim()));
            }
        }
    }
    (hits, files)
}

#[test]
fn only_the_memory_writer_owns_the_store_in_shipping_code() {
    let (hits, files) = shipping_hits();
    assert!(
        files >= 200,
        "제품 소스를 {files}개만 수집했다(하한 200). 순회 범위를 확인한다."
    );
    let offenders: Vec<&String> = hits
        .iter()
        .filter(|hit| {
            !HANDOVER
                .iter()
                .any(|(file, _)| hit.starts_with(&format!("{file}:")))
        })
        .collect();
    assert!(
        offenders.is_empty(),
        "제품 코드가 memory.db 저장소를 잠금으로 공유한다:\n{}\n저장소는 memory writer가 소유한다. \
         읽기·쓰기는 MemoryAccess로 writer에 요청을 넣는다.",
        offenders
            .iter()
            .map(|s| format!("  {s}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    for (file, reason) in HANDOVER {
        assert!(
            hits.iter().any(|hit| hit.starts_with(&format!("{file}:"))),
            "인계 자리로 적힌 `{file}`({reason})에 저장소 감싸기가 없다. 옮겨졌으면 명부를 고친다."
        );
    }
}

/// 검사가 cfg(test) 항목과 테스트 전용 파일을 제외하고 제품 줄은 잡는지 실제 파일로 대조한다.
#[test]
fn the_scan_skips_test_items_but_sees_shipping_lines() {
    let root = super::repo_root();
    let sources = super::rust_sources();
    let test_only = shipping_scope::test_only_files(&root, &sources);
    let services = PathBuf::from("src/app/services.rs");
    assert!(
        !test_only.contains(&services),
        "src/app/services.rs가 테스트 전용으로 분류됐다"
    );
    let (_, raw) = sources
        .iter()
        .find(|(p, _)| *p == services)
        .expect("services.rs scanned");
    let masked = mask_non_code(raw);
    let lines: Vec<&str> = masked.lines().collect();
    let gated = cfg_gated_lines(&lines, "test");
    let field = lines
        .iter()
        .position(|l| l.contains("memory: Option<Arc<Mutex<dyn MemoryStorage>>>"))
        .expect("the test-only shared field exists");
    assert!(gated[field], "cfg(test) 필드가 제품 줄로 잡혔다");
    let (hits, _) = shipping_hits();
    assert!(
        hits.iter().any(|h| h.starts_with("src/boot/wiring.rs:")),
        "제품 줄(부팅 인계)을 잡지 못했다"
    );
}
