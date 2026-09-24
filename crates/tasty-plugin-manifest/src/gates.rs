//! 기능 선언별 요구 권한. 매니페스트 검증과 문서 표 검사가 이 목록을 함께 사용한다.
//! 토큰 문자열은 Permission::as_token에서 얻고, 게이트 목록과 접근자는 같은 매크로 행에서 만든다.

use crate::types::Permission;

/// 고정 권한 또는 대상을 포함하는 권한. 문자열은 Permission에서 만든다.
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

    /// 문서에서 행을 구분할 (키, 권한) 쌍이 중복되지 않아야 한다.
    #[test]
    fn every_gate_has_a_unique_key_and_token_pair() {
        let mut seen = std::collections::HashSet::new();
        for gate in ContributesGate::ALL {
            let pair = (gate.contributes_key(), gate.token().doc_form());
            assert!(seen.insert(pair.clone()), "duplicate gate row: {pair:?}");
        }
        assert_eq!(seen.len(), ContributesGate::ALL.len());
    }

    /// 외부에 사용하는 권한 토큰이 바뀌면 시험이 실패하도록 표기를 직접 비교한다.
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
