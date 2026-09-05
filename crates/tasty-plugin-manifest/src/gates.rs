//! `contributes` → 요구 권한 게이트 표.
//!
//! 매니페스트의 일부 contribute 는 대응 권한 토큰이 `permissions[]` 에 선언돼 있어야만
//! 로드된다. 그 대응 관계는 두 곳에서 쓰인다 — [`crate::validate`] 의 검증 코드와
//! `docs/dev-guide/plugin-permissions.md` 의 "contributes 권한 게이트" 표. 두 곳이 각자
//! 문자열을 들고 있으면 한쪽만 갱신했을 때 조용히 어긋난다(실제로 `[[contributes.banner]]`
//! 행이 표에서 빠진 채 유지된 적이 있다).
//!
//! 그래서 게이트 목록을 **데이터**로 만들고 양쪽이 같은 출처를 읽게 한다.
//!
//! - 검증 코드는 토큰 문자열을 [`ContributesGate::required`] 로만 얻는다.
//! - 문서 parity 가드(`crates/tasty-doc-guards/tests/contributes_gate_docs_parity.rs`)는 [`ContributesGate::ALL`] 을
//!   순회해 문서 표와 1:1 로 맞는지 본다.
//!
//! 토큰 문자열은 이 표에 **없다** — [`Permission::as_token`] 에서 온다. 그래야 `validate.rs` ·
//! 문서 · 이 표가 같은 출처를 보고, 토큰 개명이 한 곳에서 끝난다.
//!
//! 표 자체의 완전성은 컴파일러가 붙잡는다 — `enum` / `ALL` / `contributes_key` / `token` 이
//! 전부 아래 매크로의 한 행에서 생성되므로, 행을 추가하지 않고 게이트만 늘리는 것은
//! 불가능하고 행을 추가하면 네 가지가 함께 갱신된다.

use crate::types::Permission;

/// 게이트가 요구하는 토큰의 형태.
///
/// 토큰 **문자열** 은 여기 없다 — 어느 쪽도 [`Permission::as_token`] 이 만든다. 이 표가
/// 존재하는 이유가 "같은 문자열을 두 곳이 각자 들면 조용히 어긋난다" 인데, 그 논거를 펴면서
/// 사본을 하나 더 만들 수는 없다.
#[derive(Debug, Clone)]
pub enum GateToken {
    /// 고정 토큰. `permissions[]` 에 이 권한의 토큰이 그대로 있어야 한다.
    Literal(Permission),
    /// 대상 id 를 받아 토큰을 만드는 scoped 권한. 문서에는 `placeholder` 를 대상 자리에
    /// 넣은 형태로 적힌다(`ext:<target>`).
    Scoped {
        make: fn(String) -> Permission,
        placeholder: &'static str,
    },
}

impl GateToken {
    /// 문서 표에 적히는 표기 (`ext:<target>` 처럼 placeholder 를 대상 자리에 넣은 형태).
    pub fn doc_form(&self) -> String {
        match self {
            GateToken::Literal(p) => p.as_token(),
            GateToken::Scoped { make, placeholder } => make((*placeholder).to_string()).as_token(),
        }
    }

    /// `target` 에 대해 실제로 요구되는 토큰. [`GateToken::Literal`] 은 `target` 을 무시한다.
    pub fn resolve(&self, target: &str) -> String {
        match self {
            GateToken::Literal(p) => p.as_token(),
            GateToken::Scoped { make, .. } => make(target.to_string()).as_token(),
        }
    }
}

macro_rules! contributes_gates {
    ($(
        $(#[$meta:meta])*
        $variant:ident => ($key:expr, $token:expr)
    ),+ $(,)?) => {
        /// 권한 게이트가 걸린 contribute 종류.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum ContributesGate {
            $( $(#[$meta])* $variant, )+
        }

        impl ContributesGate {
            /// 표에 있는 모든 게이트. 문서 parity 가드가 이 순회로 표를 검사한다.
            pub const ALL: &'static [ContributesGate] = &[ $( ContributesGate::$variant, )+ ];

            /// 이 게이트가 걸리는 매니페스트 키 (문서 표의 왼쪽 열).
            pub fn contributes_key(self) -> &'static str {
                match self { $( ContributesGate::$variant => $key, )+ }
            }

            /// 이 게이트가 요구하는 토큰의 형태 (문서 표의 오른쪽 열).
            pub fn token(self) -> GateToken {
                match self { $( ContributesGate::$variant => $token, )+ }
            }
        }
    };
}

contributes_gates! {
    Tool => ("[[contributes.tool]]", GateToken::Literal(Permission::UiToolItem)),
    Popup => ("[[contributes.popup]]", GateToken::Literal(Permission::UiPopup)),
    /// `action.kind = "open_popup"` 인 command 만 해당 — command 선언 자체는 게이트가 없다.
    CommandOpenPopup => ("[[contributes.commands]]", GateToken::Literal(Permission::UiPopup)),
    Banner => ("[[contributes.banner]]", GateToken::Literal(Permission::UiBanner)),
    SettingsPage => (
        "[[contributes.settings_pages]]",
        GateToken::Literal(Permission::UiSettingsPage)
    ),
    Window => ("[[contributes.window]]", GateToken::Literal(Permission::WindowSpawn)),
    Extends => (
        "[extends]",
        GateToken::Scoped { make: Permission::Extension, placeholder: "<target>" }
    ),
    /// detector 는 신규 정의와 기존 id 재선언이 서로 다른 토큰을 요구한다(둘 중 하나).
    DetectorDefine => (
        "[[contributes.detector]]",
        GateToken::Literal(Permission::FileHandlerDefine)
    ),
    DetectorExtend => (
        "[[contributes.detector]]",
        GateToken::Scoped { make: Permission::FileHandlerExtend, placeholder: "<id>" }
    ),
    Handler => (
        "[[contributes.handler]]",
        GateToken::Scoped { make: Permission::FileHandlerHandle, placeholder: "<detector>" }
    ),
    HookHandler => (
        "[[contributes.hook_handler]]",
        GateToken::Literal(Permission::HookHandlerDefine)
    ),
    CompletionStrategy => (
        "[[contributes.completion_strategy]]",
        GateToken::Literal(Permission::CompletionStrategyDefine)
    ),
}

impl ContributesGate {
    /// `target` 에 대해 이 게이트가 요구하는 실제 토큰.
    pub fn required(self, target: &str) -> String {
        self.token().resolve(target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 문서 parity 가드는 (키, 토큰 표기) 쌍으로 표 행을 식별한다. 두 게이트가 같은 쌍을
    /// 가지면 그 식별이 무너지므로(한 행이 두 게이트를 만족시키고 다른 행은 미매칭으로
    /// 남는다) 표 자체가 그 쌍의 유일성을 지켜야 한다.
    #[test]
    fn every_gate_has_a_unique_key_and_token_pair() {
        let mut seen = std::collections::HashSet::new();
        for gate in ContributesGate::ALL {
            let pair = (gate.contributes_key(), gate.token().doc_form());
            assert!(seen.insert(pair.clone()), "duplicate gate row: {pair:?}");
        }
        assert_eq!(seen.len(), ContributesGate::ALL.len());
    }

    /// 아래 두 테스트의 문자열은 표의 **사본이 아니라 핀**이다 — `Permission` 쪽에서
    /// 토큰을 개명하면 여기서 요란하게 깨져 개명 범위를 알려준다. 프로덕션 경로는
    /// 어느 것도 이 문자열을 읽지 않는다.
    #[test]
    fn scoped_tokens_resolve_to_the_prefix_plus_target() {
        assert_eq!(
            ContributesGate::Extends.required("com.example.host"),
            "ext:com.example.host"
        );
        assert_eq!(
            ContributesGate::Handler.required("markdown"),
            "file_handler.handle:markdown"
        );
        assert_eq!(ContributesGate::Extends.token().doc_form(), "ext:<target>");
    }

    /// 고정 토큰 게이트는 대상과 무관하게 같은 토큰을 요구한다.
    #[test]
    fn literal_tokens_ignore_the_target() {
        assert_eq!(ContributesGate::Tool.required("anything"), "ui.tool_item");
        assert_eq!(ContributesGate::Tool.token().doc_form(), "ui.tool_item");
    }
}
