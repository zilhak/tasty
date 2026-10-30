//! attach holder 연결로 실행할 수 있는 plugin 메서드 선언. `[contributes.remote_attach]`.
//!
//! 본체는 이 선언으로 허용 목록만 만들고 메서드의 의미·인자·provider 별 훅 이름은 해석하지 않는다.
//! 서버 쪽 host 는 `methods` 에 있는 메서드만 holder 연결 대신 실행한다. 그 plugin 의 host 호출은
//! 매니페스트 권한·사용자 허가 검사를 그대로 받는다. hard 점유 예외는 그 holder 출처의 자식에만 적용된다.
//! client 쪽 host 는 plugin 이 자기 namespace 로만 원격 호출하게 하고, 원격에서 온 plugin 고유 결과는
//! 그 자식을 만든 plugin 의 `receive` 메서드에만 넘긴다. 원격에서 다시 발화할 훅 이벤트는 그 plugin 의
//! `hook_events` 선언, 옮길 메타 키는 `relay_meta` 선언으로 제한한다.
//! namespace 를 하나만 가진 plugin 만 선언할 수 있어 메서드 이름이 가리키는 접두사가 하나로 정해진다.

use serde::Deserialize;

/// 원격 attach 실행 선언. 비어 있으면 이 plugin 은 원격 실행을 받지 않는다.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct RemoteAttachDecl {
    /// holder 가 서버에서 실행할 수 있는 자기 namespace 메서드 이름(접두사 없이).
    #[serde(default)]
    pub methods: Vec<String>,
    /// client 에서 원격 plugin 결과를 받을 자기 namespace 메서드 이름(접두사 없이).
    #[serde(default)]
    pub receive: Option<String>,
    /// 원격 자식에서 client 의 같은 surface 로 옮길 surface 메타 키.
    #[serde(default)]
    pub relay_meta: Vec<String>,
}

impl RemoteAttachDecl {
    pub fn is_empty(&self) -> bool {
        self.methods.is_empty() && self.receive.is_none() && self.relay_meta.is_empty()
    }

    pub fn relays_meta(&self, key: &str) -> bool {
        self.relay_meta.iter().any(|k| k == key)
    }

    /// 실행 허용 여부. `method` 는 namespace 접두사 없는 이름이다.
    pub fn allows(&self, method: &str) -> bool {
        self.methods.iter().any(|m| m == method)
    }
}

/// 메서드 이름 형식과 중복, 그리고 선언할 namespace 가 있는지 검사한다.
pub(crate) fn validate(contributes: &crate::types::Contributes) -> anyhow::Result<()> {
    let decl = &contributes.remote_attach;
    if decl.is_empty() {
        return Ok(());
    }
    if contributes.ipc_namespace.len() != 1 {
        anyhow::bail!(
            "contributes.remote_attach requires exactly one contributes.ipc_namespace — \
             the methods are resolved in the plugin's own namespace"
        );
    }
    for key in &decl.relay_meta {
        if !crate::validators::is_valid_settings_id(key) {
            anyhow::bail!(
                "invalid contributes.remote_attach relay_meta '{key}': must be lowercase ascii, \
                 digits, '_' and '-'"
            );
        }
    }
    let mut seen = std::collections::HashSet::new();
    for name in decl.methods.iter().chain(decl.receive.iter()) {
        if !is_method_name(name) {
            anyhow::bail!(
                "invalid contributes.remote_attach method '{name}': must be lowercase ascii, \
                 digits and '_', start with a letter, no namespace prefix"
            );
        }
    }
    for name in &decl.methods {
        if !seen.insert(name.as_str()) {
            anyhow::bail!("contributes.remote_attach method '{name}' declared twice");
        }
    }
    if let Some(receive) = &decl.receive
        && decl.allows(receive)
    {
        anyhow::bail!(
            "contributes.remote_attach receive '{receive}' must not also be a remotely executable method"
        );
    }
    Ok(())
}

fn is_method_name(s: &str) -> bool {
    (1..=64).contains(&s.len())
        && s.starts_with(|c: char| c.is_ascii_lowercase())
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contributes(toml: &str) -> crate::types::Contributes {
        toml::from_str(toml).unwrap()
    }

    #[test]
    fn methods_are_bare_names_in_the_plugin_namespace() {
        let ok = contributes(
            "[[ipc_namespace]]\nprefix = \"x\"\n[remote_attach]\nmethods = [\"spawn\", \"tell\"]\nreceive = \"remote_receive\"\n",
        );
        validate(&ok).unwrap();
        assert!(ok.remote_attach.allows("spawn"));
        assert!(!ok.remote_attach.allows("x.spawn"));
        for bad in [
            "[[ipc_namespace]]\nprefix = \"x\"\n[remote_attach]\nmethods = [\"x.spawn\"]\n",
            "[[ipc_namespace]]\nprefix = \"x\"\n[remote_attach]\nmethods = [\"a\", \"a\"]\n",
            "[[ipc_namespace]]\nprefix = \"x\"\n[remote_attach]\nmethods = [\"a\"]\nreceive = \"a\"\n",
            "[remote_attach]\nmethods = [\"spawn\"]\n",
            "[[ipc_namespace]]\nprefix = \"x\"\n[[ipc_namespace]]\nprefix = \"y\"\n[remote_attach]\nmethods = [\"a\"]\n",
            "[[ipc_namespace]]\nprefix = \"x\"\n[remote_attach]\nrelay_meta = [\"Restore Command\"]\n",
        ] {
            assert!(validate(&contributes(bad)).is_err(), "{bad}");
        }
        validate(&contributes("")).unwrap();
    }
}
