//! 플러그인을 설치·제거하고 권한과 레지스트리를 갱신한다.
//! 결과 이벤트의 후속 처리는 App이 담당한다.

use crate::app::App;
use crate::core::intent::{CoreEvent, PluginRegistryChange};
use crate::plugin::manifest::{Permission, SurfaceKindRendering};
use crate::plugin::{Manifest, PluginManager, PluginPackage};

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

/// 매니페스트에 선언되고 사용자에게 허용된 권한만 매니저에 반영한다.
fn refresh_plugin_permissions(mgr: &mut PluginManager, plugin_id: &str) {
    let Some(pkg) = mgr
        .packages()
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

fn register_plugin_surface_kinds(
    registry: &crate::core::surface_registry::SurfaceKindRegistry,
    plugin_id: &str,
    pkg: &PluginPackage,
    tx: &std::sync::mpsc::Sender<crate::plugin_bridge::host_cmd::HostCmd>,
) -> Vec<CoreEvent> {
    let mut events = Vec::new();
    for decl in &pkg.manifest.surface_kinds {
        if let Some(default) = &decl.default_colors {
            tasty_themes::add_plugin_surface_default(&decl.kind, default.clone());
        }
        let rendering = match decl.rendering {
            SurfaceKindRendering::Remote => {
                crate::plugin_bridge::remote_kind::register_remote_kind(
                    registry,
                    plugin_id,
                    decl,
                    tx.clone(),
                );
                "remote"
            }
            SurfaceKindRendering::Webview => {
                crate::core::surface_registry::webview_kind::register_webview_kind(
                    plugin_id, &decl.kind,
                );
                crate::plugin_bridge::remote_kind::register_remote_kind(
                    registry,
                    plugin_id,
                    decl,
                    tx.clone(),
                );
                "webview"
            }
            SurfaceKindRendering::EguiMesh => {
                crate::core::surface_registry::egui_mesh::register_egui_mesh_kind(
                    registry,
                    plugin_id,
                    decl,
                    &pkg.manifest.api_version,
                );
                "egui-mesh"
            }
        };
        events.push(CoreEvent::PluginSurfaceKindRegistered {
            plugin_id: plugin_id.to_string(),
            kind: decl.kind.clone(),
            rendering: rendering.to_string(),
        });
    }
    events
}

fn collect_window_declared_events(plugin_id: &str, pkg: &PluginPackage) -> Vec<CoreEvent> {
    let mut events = Vec::new();
    for w in &pkg.manifest.contributes.window {
        tracing::info!(
            "plugin '{}' declared window '{}' (window creation is not implemented here)",
            plugin_id,
            w.id
        );
        events.push(CoreEvent::PluginWindowDeclared {
            plugin_id: plugin_id.to_string(),
            window_id: w.id.clone(),
        });
    }
    events
}

/// Windows에서는 실행 중인 파일을 지울 수 없어 disable 뒤 프로세스 회수까지 기다린다.
/// 제거할 플러그인이므로 남아 있는 재시작 예약은 사용하지 않는다.
fn stop_before_remove(mgr: &mut PluginManager, plugin_id: &str) {
    if let Err(e) = mgr.disable(plugin_id) {
        tracing::warn!("disable before remove failed: {e}");
    }
    if mgr.wait_retired(plugin_id) {
        tracing::debug!(
            plugin_id,
            "remove dropped a pending restart — the plugin is being removed"
        );
    }
}

impl App {
    /// 설치 후 권한을 허용하며 비활성 설정이 없으면 실행한다. JSON-RPC 응답 코드는 호출자가 정한다.
    pub(crate) fn plugin_install(
        &mut self,
        src_path: std::path::PathBuf,
    ) -> anyhow::Result<Vec<CoreEvent>> {
        let Some(mgr) = self.plugin_manager.as_mut() else {
            anyhow::bail!("plugin manager not initialized (no main window yet)");
        };
        let manifest = Manifest::load(&src_path)
            .and_then(|m| {
                crate::plugin_bridge::manifest_validate::validate_bin_extras(&m)?;
                Ok(m)
            })
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

        mgr.refresh_packages();
        mgr.command_registry.register_plugin(&manifest);
        let lang_dir = dest.join(&manifest.lang_dir);
        crate::i18n::register_namespace(&manifest.id, &lang_dir);
        let tokens: Vec<String> = manifest.permissions.clone();
        mgr.config.set_granted(&manifest.id, tokens);
        if let Err(e) = mgr.config.save() {
            tracing::warn!("plugins.toml save failed: {e}");
        }
        // 확장자는 packages와 권한 설정에 의존하므로 마지막 설정 변경 뒤 다시 계산한다.
        // 아래 enable은 생략될 수 있어 그 호출의 재계산에만 의존할 수 없다.
        mgr.recompute_extensions();
        mgr.debug_assert_extensions_fresh();

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

    pub(crate) fn plugin_remove(&mut self, plugin_id: String) -> anyhow::Result<Vec<CoreEvent>> {
        let hook_event_registry = self.core_state().plugin_hook_events.clone();
        let surface_registry = self.core_state().surface_registry.clone();
        let Some(mgr) = self.plugin_manager.as_mut() else {
            anyhow::bail!("plugin manager not initialized");
        };
        stop_before_remove(mgr, &plugin_id);
        // IPC disable을 거치지 않고 제거하는 경우에도 등록된 kind를 철회한다.
        surface_registry.withdraw_plugin(&plugin_id);
        let plugin_dir = crate::plugin::plugin_root()
            .ok_or_else(|| anyhow::anyhow!("could not resolve plugins directory"))?
            .join(&plugin_id);
        if !plugin_dir.exists() {
            anyhow::bail!("plugin '{plugin_id}' not installed");
        }
        std::fs::remove_dir_all(&plugin_dir)
            .map_err(|e| anyhow::anyhow!("remove dir failed: {e}"))?;
        // 번들 플러그인이 다음 부팅에 자동 재설치되지 않도록 제거 의사를 기록한다.
        // GUI와 IPC가 공유하는 이 경로에 두며 upgrade_builtins의 restore_removed로 되돌린다.
        crate::plugin::mark_builtin_removed(mgr, &plugin_id);
        // 제거 후 재설치할 때 과거 비활성 설정을 물려받지 않게 지운다.
        if mgr.config.enable(&plugin_id)
            && let Err(e) = mgr.config.save()
        {
            tracing::warn!("plugins.toml save failed after clearing disabled mark: {e}");
        }
        // packages만 지우면 namespace 예약이 남을 수 있어 설치 목록과 파생 표를 함께 갱신한다.
        mgr.refresh_packages();
        mgr.command_registry.unregister_plugin(&plugin_id);
        crate::i18n::unregister_namespace(&plugin_id);
        mgr.recompute_extensions();
        hook_event_registry.unregister(&plugin_id);
        mgr.debug_assert_extensions_fresh();
        Ok(vec![CoreEvent::PluginRegistryChanged {
            plugin_id,
            change: PluginRegistryChange::Removed,
        }])
    }

    /// GUI와 헤드리스가 같은 enable 구현을 사용한다.
    pub(crate) fn plugin_enable(&mut self, plugin_id: String) -> anyhow::Result<Vec<CoreEvent>> {
        crate::ipc::handler::plugin::enable(self.plugin_manager.as_mut(), plugin_id)
    }

    /// GUI와 헤드리스가 같은 disable 구현을 사용한다.
    pub(crate) fn plugin_disable(&mut self, plugin_id: String) -> anyhow::Result<Vec<CoreEvent>> {
        let surface_registry = self.core_state().surface_registry.clone();
        crate::ipc::handler::plugin::disable(
            self.plugin_manager.as_mut(),
            &surface_registry,
            plugin_id,
        )
    }

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
            .packages()
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

    /// 번들 갱신 결과와 함께 실제 Upgraded·Reinstalled 항목마다 Installed 이벤트를 반환한다.
    pub(crate) fn plugin_upgrade_builtins(
        &mut self,
        force: bool,
        restore_removed: Vec<String>,
        restore_all: bool,
        restart_running: bool,
    ) -> anyhow::Result<(tasty_host_plugin::BuiltinUpgradeReport, Vec<CoreEvent>)> {
        let Some(mgr) = self.plugin_manager.as_mut() else {
            anyhow::bail!("plugin manager not initialized");
        };
        let report = tasty_host_plugin::upgrade_builtins(
            mgr,
            force,
            &restore_removed,
            restore_all,
            restart_running,
        );

        let mut events = Vec::new();
        for item in &report.items {
            let new_version = match &item.action {
                tasty_host_plugin::BuiltinUpgradeAction::Upgraded { to, .. } => Some(to.clone()),
                tasty_host_plugin::BuiltinUpgradeAction::Reinstalled { version, .. } => {
                    Some(version.clone())
                }
                _ => None,
            };
            if let Some(version) = new_version {
                events.push(CoreEvent::PluginRegistryChanged {
                    plugin_id: item.id.clone(),
                    change: PluginRegistryChange::Installed { version },
                });
            }
        }
        Ok((report, events))
    }

    /// hello가 끝난 플러그인의 hook·surface kind를 등록하고 Loaded 이벤트를 전달한다.
    /// surface_registry가 없으면 surface 등록은 건너뛰며 이 hello 결과를 재시도용으로 보관하지 않는다.
    pub(crate) fn finalize_plugin_hello(&mut self, hello_pairs: Vec<(String, String)>) {
        if hello_pairs.is_empty() {
            return;
        }
        // 등록 함수는 구체 타입을 요구하므로 매니저의 trait object 대신 CoreState에서 가져온다.
        let core_registry = self.core_state().surface_registry.clone();
        // hook 검증은 화면 렌더링과 독립적이어서 surface_registry가 없어도 등록한다.
        let hook_event_registry = self.core_state().plugin_hook_events.clone();
        let Some(mgr) = self.plugin_manager.as_mut() else {
            return;
        };
        let mut events: Vec<CoreEvent> = Vec::new();

        for (plugin_id, _) in &hello_pairs {
            if let Some(pkg) = mgr.packages().iter().find(|p| &p.manifest.id == plugin_id) {
                let keys: Vec<String> = pkg
                    .manifest
                    .contributes
                    .hook_events
                    .iter()
                    .map(|h| h.key.clone())
                    .collect();
                if !keys.is_empty() {
                    hook_event_registry.register(plugin_id, keys);
                }
            }
        }

        let host_registry = mgr.surface_registry.is_some().then_some(core_registry);
        if let Some(registry) = host_registry {
            let tx = mgr.host_cmd_tx.clone();
            for (plugin_id, version) in &hello_pairs {
                if let Some(pkg) = mgr
                    .packages()
                    .iter()
                    .find(|p| &p.manifest.id == plugin_id)
                    .cloned()
                {
                    events.extend(register_plugin_surface_kinds(
                        &registry, plugin_id, &pkg, &tx,
                    ));
                    events.extend(collect_window_declared_events(plugin_id, &pkg));
                }
                mgr.registered_plugins.insert(plugin_id.clone());
                events.push(CoreEvent::PluginLoaded {
                    plugin_id: plugin_id.clone(),
                    version: version.clone(),
                });
            }
        } else {
            tracing::debug!(
                "plugin manager has no surface_registry; skipping surface registration of {} plugin(s)",
                hello_pairs.len()
            );
            // surface 등록을 생략해도 hello 완료에 대한 Loaded 이벤트는 전달한다.
            for (plugin_id, version) in &hello_pairs {
                events.push(CoreEvent::PluginLoaded {
                    plugin_id: plugin_id.clone(),
                    version: version.clone(),
                });
            }
        }

        self.cascade_plugin_events(events);
    }
}
