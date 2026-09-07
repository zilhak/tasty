//! 부팅 시 1회 settings 로드 + i18n 초기화 + 활성 로케일의 프로세스 env 반영.
//!
//! 활성 언어의 단일 출처는 `general.language` 다. 여기서 확정한 값을
//! ① 본 프로세스의 i18n 테이블(`crate::i18n::init`)과
//! ② 자식 프로세스가 상속할 env(`TASTY_LOCALE` / `TASTY_LOCALE_FONT`)에 함께 반영한다.
//!
//! 설정값이 내장 언어(`en`/`ko`/`ja`)가 아니면 `tasty-i18n` 이 `~/.tasty/lang/<code>/pack.toml`
//! 언어팩을 찾는다. 팩이 없거나 형상이 틀리면(`[font]` 부재 등) **영어로 폴백**하고
//! `LoadReport` 로 알린다 — 그 경우 ②의 `TASTY_LOCALE` 도 실제 적용 언어(`en`)를 싣는다.
//! 설정값 자체는 어느 경로에서도 고쳐 쓰지 않는다(GUI 는 부팅 후 경고 토스트 1회 —
//! `app::boot_machine::report_locale_fallback`, headless/CLI 는 로드 시점의 `tracing::warn!`).
//!
//! plugin 프로세스는 host i18n 카탈로그에 접근하지 못하고 env 로만 언어를 받으므로
//! (`crates/tasty-plugin-sdk/src/env.rs`) ② 가 빠지면 plugin UI 는 영어로 고정된다.
//! host-plugin 크레이트는 `tasty-i18n` 에 의존하지 않고 이 env 를 그대로 자식에
//! propagate 한다(`crates/tasty-host-plugin/src/process.rs`). 근거·대안:
//! `docs/adr/0103-plugin-locale-via-host-process-env.md`.

use std::ffi::OsString;
use std::path::PathBuf;

/// 자식 plugin 프로세스가 읽는 활성 언어 코드 env 이름.
pub(crate) const LOCALE_ENV: &str = "TASTY_LOCALE";
/// 언어팩이 폰트 파일을 제공할 때 그 절대경로를 싣는 env 이름. 폰트가 없으면 unset.
pub(crate) const LOCALE_FONT_ENV: &str = "TASTY_LOCALE_FONT";

/// 부팅 시 확정된 로케일 — 본 프로세스 i18n 과 자식 env 의 단일 출처.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedLocale {
    /// 실제 적용된 언어 코드(`en` / `ko` / `ja` / 언어팩 코드 …). 요청 언어의 팩이 없어
    /// 영어로 폴백했으면 `en` — 설정값이 아니라 i18n 테이블이 실제로 담은 언어다.
    pub code: String,
    /// 언어팩이 제공하는 폰트 파일의 절대경로. 언어팩이 폰트를 제공하지 않거나
    /// (`builtin = true`) 내장 언어를 쓰면 `None` — 이때 `TASTY_LOCALE_FONT` 는
    /// 설정하지 않는다(unset). 언어팩 `[font]` 선언을 실제 파일로 resolve·검증하는
    /// 단계는 `init` 에서 `locale_font::resolve` 로 수행한다 — resolve 에 실패하면
    /// (`Failed`) 경고만 하고 `None` 으로 둔다(문자열 자체는 폴백 없이 그대로 로드).
    pub font_file: Option<PathBuf>,
}

impl ResolvedLocale {
    /// i18n 로드 결과에서 로케일을 확정한다 — `effective` 가 곧 코드다(폴백 반영).
    pub fn from_report(report: &crate::i18n::LoadReport) -> Self {
        Self {
            code: report.effective.clone(),
            font_file: None,
        }
    }

    /// 자식 프로세스가 상속할 env 항목. `Some` 은 set, `None` 은 unset — 셸에서
    /// export 된 stale 값이 자식에 흘러가지 않도록 두 경우를 모두 명시한다.
    pub fn env_entries(&self) -> [(&'static str, Option<OsString>); 2] {
        [
            (LOCALE_ENV, Some(OsString::from(&self.code))),
            (
                LOCALE_FONT_ENV,
                self.font_file.as_ref().map(|p| p.clone().into_os_string()),
            ),
        ]
    }
}

/// 부팅 때 export 한 `TASTY_LOCALE_FONT` 를 되읽어 언어팩 폰트 경로를 돌려준다 —
/// 없으면 `None`. host 의 두 egui 폰트 셋업 경로(`src/gfx/gpu/fonts.rs` ·
/// `src/adapters/ui/font_registry.rs`)가 같은 값을 읽어 체인 뒤에 붙이는 단일 출처다.
/// plugin 프로세스도 같은 env 를 상속받아 자기 미러에서 읽는다.
pub(crate) fn font_env_path() -> Option<PathBuf> {
    std::env::var_os(LOCALE_FONT_ENV)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// 설정 문자열을 요청 코드로 정규화한다. 공백/빈 값은 `en` — 빈 코드는 host i18n 과
/// plugin SDK 양쪽에서 "언어 파일 없음" 으로 영어와 같게 동작하지만, env 로 빈 문자열이
/// 흘러가면 소비처마다 해석이 갈린다.
pub(crate) fn normalize_code(language: &str) -> String {
    let code = language.trim();
    if code.is_empty() {
        "en".to_string()
    } else {
        code.to_string()
    }
}

use std::sync::Once;

static INIT: Once = Once::new();

/// settings 를 읽어 i18n 테이블을 올린다. `cli_routing::parse_or_route` 진입부와 각
/// mode helper 가 모두 부르므로 `Once` 로 1회만 실행한다 — 두 번째 호출부터는 settings
/// 파일을 다시 읽지 않아 config 파싱 경고가 중복으로 찍히지 않는다(`tasty_i18n::init`
/// 자체도 `OnceLock` 이라 재호출은 무해하지만, 그 앞의 `Settings::load` 는 아니다).
pub(crate) fn init() {
    INIT.call_once(|| {
        let lang_settings = crate::settings::Settings::load();
        let requested = normalize_code(&lang_settings.general.language);
        let report = crate::i18n::init(&requested);
        let mut locale = ResolvedLocale::from_report(&report);
        // `[font]` 선언을 실제 파일로 resolve·검증한다(egui 가 보기 전에 깨진 폰트를
        // 거른다). 실패는 폴백 없이 경고만 — 문자열은 그대로 뜨고 UI 폰트만 안 붙는다.
        // GUI 의 사용자 향 경고 토스트는 부팅 후 `app::boot_machine` 이 별도로 낸다.
        match crate::boot::locale_font::resolve(&report.outcome) {
            crate::boot::locale_font::FontResolution::Resolved(path) => {
                locale.font_file = Some(path);
            }
            crate::boot::locale_font::FontResolution::Failed { detail } => {
                tracing::warn!(
                    "locale '{}' declares a [font] that could not be resolved: {detail}",
                    locale.code
                );
            }
            crate::boot::locale_font::FontResolution::None => {}
        }
        export_to_process_env(&locale);
    });
}

/// 확정된 로케일을 본 프로세스 env 에 반영한다 — 이후 spawn 되는 모든 자식(plugin
/// 프로세스, PTY 셸 — `Command` 의 env 상속)이 이 값을 본다.
///
/// `std::env::set_var` / `remove_var` 는 edition 2024 에서 `unsafe` 다: 다른 스레드가
/// 동시에 env 를 읽는 중이면 data race 가 된다. 이 함수는 부팅 시퀀스에서 이벤트 루프 ·
/// IPC accept 스레드 · plugin spawner 스레드 · PTY reader 가 하나도 생기기 전, 즉
/// 프로세스가 아직 **단일 스레드**인 구간에서만 호출된다(`boot::run_gui` /
/// `run_headless` / `run_subcommand` 의 첫 단계 `locale::init`). 부팅 이후 언어 변경은
/// 재시작 전까지 반영하지 않는다 — 스레드가 살아 있는 시점의 env 변경은 이 안전 조건을
/// 깨므로, 이 함수를 부팅 밖에서 다시 부르면 안 된다.
fn export_to_process_env(locale: &ResolvedLocale) {
    for (key, value) in locale.env_entries() {
        match value {
            Some(value) => {
                // SAFETY: 부팅 단일 스레드 구간 — 다른 스레드가 없으므로 env 를 동시에
                // 읽거나 쓰는 주체가 없다(위 doc 의 호출 위치 제약).
                unsafe { std::env::set_var(key, &value) };
            }
            None => {
                // SAFETY: 위와 동일 — 부팅 단일 스레드 구간.
                unsafe { std::env::remove_var(key) };
            }
        }
    }
    tracing::debug!(
        "locale: {} exported to process env ({LOCALE_ENV}; font={})",
        locale.code,
        locale
            .font_file
            .as_ref()
            .map_or_else(|| "unset".to_string(), |p| p.display().to_string())
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::{LoadOutcome, LoadReport};

    fn report(requested: &str, effective: &str, outcome: LoadOutcome) -> LoadReport {
        LoadReport {
            requested: requested.to_string(),
            effective: effective.to_string(),
            outcome,
        }
    }

    #[test]
    fn from_report_keeps_effective_code_and_has_no_font() {
        let l = ResolvedLocale::from_report(&report("ko", "ko", LoadOutcome::Builtin));
        assert_eq!(l.code, "ko");
        assert_eq!(l.font_file, None);
    }

    #[test]
    fn from_report_uses_fallback_language_not_the_requested_one() {
        let l = ResolvedLocale::from_report(&report(
            "zz",
            "en",
            LoadOutcome::PackMissing {
                expected: PathBuf::from("lang/zz/pack.toml"),
            },
        ));
        assert_eq!(l.code, "en");
    }

    #[test]
    fn normalize_code_maps_blank_to_en() {
        assert_eq!(normalize_code(""), "en");
        assert_eq!(normalize_code("  "), "en");
        assert_eq!(normalize_code(" ja "), "ja");
        assert_eq!(normalize_code("xx"), "xx");
    }

    #[test]
    fn env_entries_set_locale_and_unset_font_when_absent() {
        let l = ResolvedLocale::from_report(&report("ja", "ja", LoadOutcome::Builtin));
        let [(k1, v1), (k2, v2)] = l.env_entries();
        assert_eq!(k1, "TASTY_LOCALE");
        assert_eq!(v1, Some(OsString::from("ja")));
        assert_eq!(k2, "TASTY_LOCALE_FONT");
        assert_eq!(v2, None);
    }

    #[test]
    fn env_entries_carry_font_path_when_resolved() {
        let font = crate::test_support::abs_path("lang/ko/fonts/x.ttf");
        let l = ResolvedLocale {
            code: "ko".to_string(),
            font_file: Some(font.clone()),
        };
        let [_, (k, v)] = l.env_entries();
        assert_eq!(k, "TASTY_LOCALE_FONT");
        assert_eq!(v, Some(font.into_os_string()));
    }
}
