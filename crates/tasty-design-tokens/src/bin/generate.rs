//! DTCG → Rust 코드 생성기. `src/generated/` 를 덮어쓴다.
//!
//! 실행: `cargo run -p tasty-design-tokens --bin generate`
//! 출력은 입력(vendor json)에만 의존하는 결정적 텍스트 — 재실행 시 diff 0 (멱등).

use std::fs;
use std::path::Path;
use std::process::ExitCode;

use tasty_design_tokens::{DTCG_JSON, dtcg};

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
    for (name, content) in &generated.files {
        if let Err(e) = fs::write(dir.join(name), content) {
            tracing::error!("src/generated/{name} 쓰기 실패: {e}");
            return ExitCode::FAILURE;
        }
        tracing::info!("wrote src/generated/{name}");
    }

    // component 접근자는 `&Theme` 경유 강제 원칙 때문에 `tasty-type-appearance`
    // 안에 산출한다 (`tasty-design-tokens` → `tasty-type-appearance` 런타임
    // 의존은 금지 — 의존 방향 보존).
    let type_appearance_dir =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../tasty-type-appearance/src");
    for (name, content) in &generated.type_appearance_files {
        if let Err(e) = fs::write(type_appearance_dir.join(name), content) {
            tracing::error!("tasty-type-appearance/src/{name} 쓰기 실패: {e}");
            return ExitCode::FAILURE;
        }
        tracing::info!("wrote tasty-type-appearance/src/{name}");
    }

    for skip in &generated.skips {
        tracing::info!("skip: {skip}");
    }
    ExitCode::SUCCESS
}
