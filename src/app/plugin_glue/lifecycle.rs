//! Plugin lifecycle mutate 의 `App` method wrapper (D.3.C.G.2.c).
//!
//! 옛 `plugin::handler::handle_{install,remove,enable,disable,grant,revoke}`
//! 가 PluginManager 를 직접 mutate 했다. 본 wrapper 는 IPC handler 에서
//! PluginManager 접근을 분리 — handler 는 input parsing 만 하고 *App method*
//! 가 mutate + CoreEvent 발화.
//!
//! 단일 발화 경로: `App::plugin_<op>` → CoreEvent → `handle_core_event` cascade →
//! `PendingHostEvent::Plugin*` enqueue → `host_events.rs` drain →
//! `PluginManager.event_bus broadcast`.

use crate::app::App;
use crate::core::intent::{CoreEvent, PluginRegistryChange};
use crate::plugin::manifest::Permission;
use crate::plugin::{Manifest, PluginManager};

fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let dest_path = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&entry.path(), &dest_path)?;
        } else {
            std::fs::copy(entry.path(), &dest_path)?;
        }
    }
    Ok(())
}

/// 매니페스트 + granted 를 다시 교집합하여 manager 의 in-memory 권한 set 을 갱신.
fn refresh_plugin_permissions(mgr: &mut PluginManager, plugin_id: &str) {
    let Some(pkg) = mgr
        .packages
        .iter()
        .find(|p| p.manifest.id == plugin_id)
        .cloned()
    else {
        return;
    };
    let granted = mgr.config.granted_permissions(plugin_id);
    let perms: std::collections::HashSet<Permission> = pkg
        .manifest
        .parsed_permissions()
        .unwrap_or_default()
        .into_iter()
        .filter(|p| granted.contains(&p.as_token()))
        .collect();
    mgr.set_plugin_permissions(plugin_id, perms);
}

impl App {
    /// `plugin.install` IPC handler 의 본문. 파일 시스템 복사 + manifest 등록 +
    /// auto-grant + 자동 enable. CoreEvent 2종 (Installed + 자동 EnableToggled)
    /// 반환. 에러 시 `(JsonRpcResponse-equivalent String, Vec<>)` 분리 — caller
    /// 가 JSON-RPC 응답 코드 결정.
    pub(crate) fn plugin_install(
        &mut self,
        src_path: std::path::PathBuf,
    ) -> anyhow::Result<Vec<CoreEvent>> {
        let Some(mgr) = self.plugin_manager.as_mut() else {
            anyhow::bail!("plugin manager not initialized (no main window yet)");
        };
        let manifest = Manifest::load(&src_path)
            .map_err(|e| anyhow::anyhow!("invalid plugin at source: {e}"))?;
        let dest_root = crate::plugin::plugin_root()
            .ok_or_else(|| anyhow::anyhow!("could not resolve plugins directory"))?;
        let dest = dest_root.join(&manifest.id);
        if dest.exists() {
            anyhow::bail!(
                "plugin '{}' already installed at {}",
                manifest.id,
                dest.display()
            );
        }
        std::fs::create_dir_all(&dest_root)
            .map_err(|e| anyhow::anyhow!("create dir failed: {e}"))?;
        copy_dir_recursive(&src_path, &dest).map_err(|e| anyhow::anyhow!("copy failed: {e}"))?;

        mgr.packages = crate::plugin::discovery::discover();
        mgr.command_registry.register_plugin(&manifest);
        mgr.recompute_extensions();
        let lang_dir = dest.join(&manifest.lang_dir);
        crate::i18n::register_namespace(&manifest.id, &lang_dir);
        let tokens: Vec<String> = manifest.permissions.clone();
        mgr.config.set_granted(&manifest.id, tokens);
        if let Err(e) = mgr.config.save() {
            tracing::warn!("plugins.toml save failed: {e}");
        }

        let mut events = vec![CoreEvent::PluginRegistryChanged {
            plugin_id: manifest.id.clone(),
            change: PluginRegistryChange::Installed {
                version: manifest.version.clone(),
            },
        }];

        if !mgr.config.is_disabled(&manifest.id) {
            mgr.enable(&manifest.id)
                .map_err(|e| anyhow::anyhow!("enable after install failed: {e}"))?;
            events.push(CoreEvent::PluginEnableToggled {
                plugin_id: manifest.id.clone(),
                enabled: true,
            });
        }

        Ok(events)
    }

    /// `plugin.remove` IPC handler 의 본문. graceful shutdown + 디스크 삭제 +
    /// registry 갱신. CoreEvent::PluginRegistryChanged 반환.
    pub(crate) fn plugin_remove(&mut self, plugin_id: String) -> anyhow::Result<Vec<CoreEvent>> {
        let Some(mgr) = self.plugin_manager.as_mut() else {
            anyhow::bail!("plugin manager not initialized");
        };
        if let Err(e) = mgr.disable(&plugin_id) {
            tracing::warn!("disable before remove failed: {e}");
        }
        let plugin_dir = crate::plugin::plugin_root()
            .ok_or_else(|| anyhow::anyhow!("could not resolve plugins directory"))?
            .join(&plugin_id);
        if !plugin_dir.exists() {
            anyhow::bail!("plugin '{plugin_id}' not installed");
        }
        std::fs::remove_dir_all(&plugin_dir)
            .map_err(|e| anyhow::anyhow!("remove dir failed: {e}"))?;
        mgr.packages.retain(|p| p.manifest.id != plugin_id);
        mgr.command_registry.unregister_plugin(&plugin_id);
        crate::i18n::unregister_namespace(&plugin_id);
        mgr.recompute_extensions();
        Ok(vec![CoreEvent::PluginRegistryChanged {
            plugin_id,
            change: PluginRegistryChange::Removed,
        }])
    }

    /// `plugin.enable` IPC handler 의 본문. spawn 실패 시 Err 즉시 반환.
    pub(crate) fn plugin_enable(&mut self, plugin_id: String) -> anyhow::Result<Vec<CoreEvent>> {
        let Some(mgr) = self.plugin_manager.as_mut() else {
            anyhow::bail!("plugin manager not initialized");
        };
        mgr.enable(&plugin_id)?;
        Ok(vec![CoreEvent::PluginEnableToggled {
            plugin_id,
            enabled: true,
        }])
    }

    /// `plugin.disable` IPC handler 의 본문. graceful shutdown. unloaded 도
    /// 함께 발화 (was_running 분기 — 옛 lifecycle.rs:317 의 의미를 cascade 가
    /// 흡수). 결정 §7.2: reason 은 항상 `User`.
    pub(crate) fn plugin_disable(&mut self, plugin_id: String) -> anyhow::Result<Vec<CoreEvent>> {
        let Some(mgr) = self.plugin_manager.as_mut() else {
            anyhow::bail!("plugin manager not initialized");
        };
        let was_running = mgr.is_running(&plugin_id);
        mgr.disable(&plugin_id)?;
        let mut events = vec![CoreEvent::PluginEnableToggled {
            plugin_id: plugin_id.clone(),
            enabled: false,
        }];
        if was_running {
            events.push(CoreEvent::PluginUnloaded {
                plugin_id,
                reason: tasty_plugin_protocol::events::LifecycleReason::User,
            });
        }
        Ok(events)
    }

    /// `plugin.grant` IPC handler 의 본문. permission 토큰 검증 + grant +
    /// in-memory 권한 갱신 + (ext:* 이면) extension 재계산.
    pub(crate) fn plugin_grant(
        &mut self,
        plugin_id: String,
        token: String,
    ) -> anyhow::Result<Vec<CoreEvent>> {
        let Some(mgr) = self.plugin_manager.as_mut() else {
            anyhow::bail!("plugin manager not initialized");
        };
        if Permission::from_token(&token).is_none() {
            anyhow::bail!("unknown permission '{token}'");
        }
        let pkg = mgr
            .packages
            .iter()
            .find(|p| p.manifest.id == plugin_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("plugin '{plugin_id}' not installed"))?;
        if !pkg.manifest.permissions.iter().any(|p| p == &token) {
            anyhow::bail!(
                "plugin '{plugin_id}' does not declare permission '{token}' in its manifest"
            );
        }
        let _added = mgr.config.grant(&plugin_id, &token);
        if let Err(e) = mgr.config.save() {
            tracing::warn!("plugins.toml save failed: {e}");
        }
        refresh_plugin_permissions(mgr, &plugin_id);
        if token.starts_with("ext:") {
            mgr.recompute_extensions();
        }
        Ok(vec![CoreEvent::PluginRegistryChanged {
            plugin_id,
            change: PluginRegistryChange::PermissionGranted { permission: token },
        }])
    }

    /// `plugin.revoke` IPC handler 의 본문.
    pub(crate) fn plugin_revoke(
        &mut self,
        plugin_id: String,
        token: String,
    ) -> anyhow::Result<Vec<CoreEvent>> {
        let Some(mgr) = self.plugin_manager.as_mut() else {
            anyhow::bail!("plugin manager not initialized");
        };
        let _removed = mgr.config.revoke(&plugin_id, &token);
        if let Err(e) = mgr.config.save() {
            tracing::warn!("plugins.toml save failed: {e}");
        }
        refresh_plugin_permissions(mgr, &plugin_id);
        if token.starts_with("ext:") {
            mgr.recompute_extensions();
        }
        Ok(vec![CoreEvent::PluginRegistryChanged {
            plugin_id,
            change: PluginRegistryChange::PermissionRevoked { permission: token },
        }])
    }
}
