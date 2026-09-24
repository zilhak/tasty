//! DTCG에서 토큰 상수와 `tasty-type-appearance`의 `Theme` 접근자를 생성한다.
//! 실행: `cargo run -p tasty-design-tokens --bin generate`

use std::fs;
use std::path::Path;
use std::process::ExitCode;

use tasty_design_tokens::{DTCG_JSON, dtcg};

#[allow(clippy::cognitive_complexity)] // complexity-exempt: 파싱, 디렉터리 생성, 파일 쓰기의 실패를 순서대로 처리한다.
fn main() -> ExitCode {
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

    // 런타임 의존 방향을 유지하려고 Theme 접근자는 type-appearance에 생성한다.
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

/// 파일을 쓰고 경로를 기록한다. 쓰기에 실패하면 `Some(ExitCode::FAILURE)`를 반환한다.
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
