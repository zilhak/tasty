//! 플러그인 enable/disable 실행. GUI·헤드리스·IPC가 같은 함수를 사용한다.
//!
//! 요청 decode와 외부 오류 코드 매핑은 IPC 어댑터가 맡는다. 여기서는 매니저 변경,
//! surface kind 철회, 결과 CoreEvent 생성만 한다. 이벤트 소비는 호출자가 맡는다.

use crate::app::command::CoreEvent;
use crate::plugin::PluginManager;
use crate::runtime::surface_registry::SurfaceKindRegistry;

/// 생명주기 중 enable/disable만 다룬다. 설치·삭제·권한 변경은 별도 경로다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PluginToggle {
    Enable,
    Disable,
}

/// 지정한 토글을 실행한다.
pub(crate) fn toggle(
    mgr: Option<&mut PluginManager>,
    registry: &SurfaceKindRegistry,
    toggle: PluginToggle,
    plugin_id: String,
) -> anyhow::Result<Vec<CoreEvent>> {
    match toggle {
        PluginToggle::Enable => enable(mgr, plugin_id),
        PluginToggle::Disable => disable(mgr, registry, plugin_id),
    }
}

/// 지정한 플러그인만 시작한다.
pub(crate) fn enable(
    mgr: Option<&mut PluginManager>,
    plugin_id: String,
) -> anyhow::Result<Vec<CoreEvent>> {
    let Some(mgr) = mgr else {
        anyhow::bail!("plugin manager not initialized");
    };
    mgr.enable(&plugin_id)?;
    Ok(vec![CoreEvent::PluginEnableToggled {
        plugin_id,
        enabled: true,
    }])
}

/// 실행 중이었다면 PluginUnloaded를 내고 선언한 surface kind도 등록 해제한다.
/// 종료 이유는 User다. remove는 매니저를 직접 호출하므로 App::plugin_remove에서 종류를 해제한다.
pub(crate) fn disable(
    mgr: Option<&mut PluginManager>,
    registry: &SurfaceKindRegistry,
    plugin_id: String,
) -> anyhow::Result<Vec<CoreEvent>> {
    let Some(mgr) = mgr else {
        anyhow::bail!("plugin manager not initialized");
    };
    let was_running = mgr.is_running(&plugin_id);
    mgr.disable(&plugin_id)?;
    registry.withdraw_plugin(&plugin_id);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_toggle_without_a_manager_reports_the_same_error() {
        let registry = SurfaceKindRegistry::new();
        for t in [PluginToggle::Enable, PluginToggle::Disable] {
            let err = toggle(None, &registry, t, "com.example.x".into())
                .expect_err("매니저 없이 성공하면 안 된다");
            assert_eq!(err.to_string(), "plugin manager not initialized", "{t:?}");
        }
    }
}
