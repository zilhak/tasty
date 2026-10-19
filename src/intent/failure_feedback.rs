//! User failure copy is fixed and localized; diagnostic details stay in the log.
use super::IntentOrigin;

#[derive(Clone, Copy)]
// reason: only General is raised by the headless dispatcher; GUI paths use the other contexts.
#[cfg_attr(not(feature = "gui"), expect(dead_code))]
pub(crate) enum FailureContext {
    General,
    Layout,
    Divider,
    CategoryReset,
    RemoteConnect,
}
impl FailureContext {
    fn label(self) -> &'static str {
        match self {
            Self::General => "intent application",
            Self::Layout => "journal structural intent",
            Self::Divider => "divider commit",
            Self::CategoryReset => "journal category reset",
            Self::RemoteConnect => "remote connection admission",
        }
    }
    #[cfg(feature = "gui")]
    pub(crate) fn message(self) -> &'static str {
        crate::i18n::t(match self {
            Self::General => "toast.action_failed",
            Self::Layout => "toast.layout_failed",
            Self::Divider => "toast.divider_failed",
            Self::CategoryReset => "toast.category_reset_failed",
            Self::RemoteConnect => "toast.remote_connect_failed",
        })
    }
}

/// Report one failure. A caller supplies state only while its intended View exists.
/// Agent/System and absent/parked/headless Views retain diagnostics without a user toast.
/// Error strings and internal target identifiers are never used as generic UI copy.
// reason: headless logs failures but has no toast state or UI origin consumer.
#[cfg_attr(not(feature = "gui"), allow(unused_variables))]
pub(crate) fn report_apply_error(
    state: Option<&mut crate::state::RequestContext>,
    origin: &IntentOrigin,
    context: FailureContext,
    err: &anyhow::Error,
) {
    let withdrawn = err.downcast_ref::<crate::runtime::surface_registry::SurfaceKindWithdrawn>();
    if let Some(withdrawn) = withdrawn
        && origin.is_user()
    {
        tracing::info!(
            kind = %withdrawn.kind,
            plugin_id = %withdrawn.plugin_id,
            "{} refused: the surface kind's plugin is off or has not reconnected",
            context.label()
        );
    } else {
        tracing::warn!("{} failed: {err}", context.label());
    }
    #[cfg(feature = "gui")]
    if origin.is_user()
        && let Some(state) = state
    {
        use crate::model::toast_kind::{ToastKind, ToastScope};
        let (message, kind) = match withdrawn {
            Some(withdrawn) => (
                crate::i18n::t_fmt2(
                    "surface.kind_toast.withdrawn",
                    &withdrawn.kind,
                    &withdrawn.plugin_id,
                ),
                ToastKind::Warning,
            ),
            None => (context.message().to_owned(), ToastKind::Error),
        };
        state.toasts.push(message, kind, ToastScope::Window);
    }
}
