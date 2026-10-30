//! attach holder 연결로 실행할 수 있는 plugin 메서드 선언. `[contributes.remote_attach]`.
//!
//! 본체는 이 선언으로 허용 목록만 만들고 메서드의 의미·인자·provider 별 훅 이름은 해석하지 않는다.
//! 서버 쪽 host 는 `methods` 에 있는 자기 namespace 메서드만 holder 권한으로 실행한다.
//! client 쪽 host 는 원격에서 온 plugin 고유 결과를 `receive` 메서드에 넘긴다.

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
}

impl RemoteAttachDecl {
    pub fn is_empty(&self) -> bool {
        self.methods.is_empty() && self.receive.is_none()
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
    if contributes.ipc_namespace.is_empty() {
        anyhow::bail!(
            "contributes.remote_attach requires a contributes.ipc_namespace — \
             the methods are resolved in the plugin's own namespace"
        );
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
        ] {
            assert!(validate(&contributes(bad)).is_err(), "{bad}");
        }
        validate(&contributes("")).unwrap();
    }
}
