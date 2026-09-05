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

/// `pkg.manifest.surface_kinds` 선언들을 `registry` 에 등록 (rendering
/// 방식별 remote/webview/egui-mesh 분기) 하고, 각 등록에 대한
/// `PluginSurfaceKindRegistered` CoreEvent 를 모아 반환한다.
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

/// `pkg.manifest.contributes.window` 선언들을 로그로 남기고 각각에 대한
/// `PluginWindowDeclared` CoreEvent 를 모아 반환한다.
fn collect_window_declared_events(plugin_id: &str, pkg: &PluginPackage) -> Vec<CoreEvent> {
    let mut events = Vec::new();
    for w in &pkg.manifest.contributes.window {
        tracing::info!(
            "plugin '{}' declared window '{}' (runtime spawn: pending — schema-only in 1.0)",
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
        // 유도는 **원본을 바꾸는 마지막 쓰기 뒤**에 온다. `extensions` 는 packages 와
        // config(비활성 여부 · `ext:` 권한) 둘 다에서 계산되므로, `set_granted` 앞에서
        // 계산하면 방금 준 권한을 안 본 값이 남는다. 지금까지 그것이 안 보이던 이유는
        // 아래 `enable` 이 한 번 더 계산하기 때문인데, 그 호출은 `is_disabled` 일 때
        // 건너뛴다 — 즉 무해함이 다른 분기에 얹혀 있었다.
        mgr.recompute_extensions();

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
        let hook_event_registry = self.core_state().plugin_hook_events.clone();
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
        // 설치 목록을 **다시 발견**한다 — 손으로 `packages` 만 지우면 안 된다.
        // `ipc_namespaces` 는 이제 설치된 매니페스트에서 유도되는 표라
        // (ADR-0173) `packages` 를 바꾸는 자리가 그 유도를 같이 돌리지 않으면
        // 지운 plugin 의 prefix 가 남아, 그 이름의 호출이 `-32002 plugin '<id>'
        // is not running` 으로 거절된다 — 설치조차 안 돼 있는데. 호스트가 같은
        // 이름에 구현을 갖고 있으면 그 구현이 그 상태에서 가려진다.
        // `plugin_install` 이 이미 같은 함수를 쓴다(두 방향을 대칭으로 둔다).
        mgr.refresh_packages();
        mgr.command_registry.unregister_plugin(&plugin_id);
        crate::i18n::unregister_namespace(&plugin_id);
        mgr.recompute_extensions();
        hook_event_registry.unregister(&plugin_id);
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

    /// `plugin.upgrade_builtins` IPC handler 의 본문. bundle 기준으로 builtin plugin
    /// 디렉토리를 재설치하고 디스크에 적용된 항목별 `BuiltinUpgradeAction` 리포트를
    /// 반환한다. `Upgraded` / `Reinstalled` 항목은 *per-id N 회* `PluginRegistryChange::
    /// Installed { version }` CoreEvent 를 발화 (verify §0 결정 1·2 반영 — 신 batch
    /// variant 추가 없이 기존 `Installed` 재사용).
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

    /// `PluginManager::pump` 가 반환한 (plugin_id, version) 쌍 리스트로부터
    /// surface_kind registry 등록 + `registered_plugins.insert` + CoreEvent
    /// (PluginLoaded / PluginSurfaceKindRegistered) 발화 처리. surface_registry
    /// 가 set 안 된 상태 (test/headless) 면 등록 skip — 다음 pump tick 에서 다시
    /// 시도하지 못하므로 (hello_pairs 는 1회성) 본 substep 범위에서는 그대로
    /// 옛 pump.rs 동작과 일치 (deferred 등록 없이 무시).
    pub(crate) fn finalize_plugin_hello(&mut self, hello_pairs: Vec<(String, String)>) {
        if hello_pairs.is_empty() {
            return;
        }
        // concrete registry 는 본 바이너리 CoreState 에서 직접 가져온다.
        // (plugin manager 가 가진 surface_registry 필드는 trait object 라
        // remote_kind/egui_mesh 등 closure 등록 함수가 받지 못한다.)
        // mgr 차용 전에 미리 추출.
        let core_registry = self.core_state().surface_registry.clone();
        // hook 이벤트 레지스트리는 surface_registry 유무와 무관하게 (headless 포함)
        // 항상 집계한다 — hook 검증은 surface 렌더링과 독립적이다.
        let hook_event_registry = self.core_state().plugin_hook_events.clone();
        let Some(mgr) = self.plugin_manager.as_mut() else {
            return;
        };
        let mut events: Vec<CoreEvent> = Vec::new();

        for (plugin_id, _) in &hello_pairs {
            if let Some(pkg) = mgr.packages.iter().find(|p| &p.manifest.id == plugin_id) {
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
                    .packages
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
                "plugin manager has no surface_registry; deferring registration of {} plugin(s)",
                hello_pairs.len()
            );
            // surface_registry 없으면 surface_kind 등록은 skip — 옛 pump.rs 동작.
            // PluginLoaded 는 그래도 발화 (process spawn 자체는 성공).
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
