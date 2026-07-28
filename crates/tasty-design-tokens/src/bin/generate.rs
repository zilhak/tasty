//! DTCG → Rust 코드 생성기. `src/generated/` 를 덮어쓴다.
//!
//! 실행: `cargo run -p tasty-design-tokens --bin generate`
//! 출력은 입력(vendor json)에만 의존하는 결정적 텍스트 — 재실행 시 diff 0 (멱등).

use std::fs;
use std::path::Path;
use std::process::ExitCode;

use tasty_design_tokens::{DTCG_JSON, dtcg};

#[allow(clippy::cognitive_complexity)] // complexity-exempt: 일회성 코드생성 스크립트 main — 파싱/디렉토리생성/두 차례 파일쓰기/skip 로그가 순차 early-return 나열. write_generated_files 로 파일쓰기 루프는 이미 분리했고(62→32), 남은 단계를 더 쪼개면 1회성 wrapper 만 늘어남.
fn main() -> ExitCode {
    // C.11: bin 로그도 tracing 경유 — 기본 info 레벨로 wrote/skip 이 항상 보인다.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let set = match dtcg::parse(DTCG_JSON) {
        Ok(set) => set,
        Err(e) => {
            tracing::error!("DTCG 파싱 실패: {e}");
            return ExitCode::FAILURE;
        }
    };
    tracing::info!(
        "parsed {} tokens (primitive {} / semantic {} / component {})",
        set.len(),
        set.tier_count(dtcg::Tier::Primitive),
        set.tier_count(dtcg::Tier::Semantic),
        set.tier_count(dtcg::Tier::Component),
    );

    let generated = dtcg::generate(&set);
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/generated");
    if let Err(e) = fs::create_dir_all(&dir) {
        tracing::error!("{} 생성 실패: {e}", dir.display());
        return ExitCode::FAILURE;
    }
    if let Some(code) = write_generated_files(&dir, &generated.files, "src/generated") {
        return code;
    }

    // component 접근자는 `&Theme` 경유 강제 원칙 때문에 `tasty-type-appearance`
    // 안에 산출한다 (`tasty-design-tokens` → `tasty-type-appearance` 런타임
    // 의존은 금지 — 의존 방향 보존).
    let type_appearance_dir =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../tasty-type-appearance/src");
    if let Some(code) = write_generated_files(
        &type_appearance_dir,
        &generated.type_appearance_files,
        "tasty-type-appearance/src",
    ) {
        return code;
    }

    for skip in &generated.skips {
        tracing::info!("skip: {skip}");
    }
    ExitCode::SUCCESS
}

/// `dir` 밑에 `files`(파일명, 내용) 목록을 쓰고 파일마다 `wrote <label>/<name>` 로그를
/// 남긴다. 실패하면 에러 로그 후 `Some(ExitCode::FAILURE)`, 전부 성공하면 `None`.
fn write_generated_files(
    dir: &Path,
    files: &[(&'static str, String)],
    label: &str,
) -> Option<ExitCode> {
    for (name, content) in files {
        if let Err(e) = fs::write(dir.join(name), content) {
            tracing::error!("{label}/{name} 쓰기 실패: {e}");
            return Some(ExitCode::FAILURE);
        }
        tracing::info!("wrote {label}/{name}");
    }
    None
}
