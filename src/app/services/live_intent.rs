//! Live effects only. Structural commands are admitted by JournalApplication.
use super::*;
impl AppServices {
    /// intent를 적용하고 후속 처리용 이벤트를 반환한다. 일부 작업은 여기서 상태를 바꾸고,
    /// 설정·알림 등은 이벤트를 받은 App dispatcher가 적용한다.
    pub(crate) fn apply_live(
        &mut self,
        engine: &mut EngineMut<'_>,
        intent: DomainIntent,
    ) -> anyhow::Result<Vec<CoreEvent>> {
        match intent {
            DomainIntent::UpdateSettings(new_settings) => {
                Ok(vec![CoreEvent::SettingsUpdated(new_settings)])
            }
            DomainIntent::PushNotification {
                ws_id,
                surface_id,
                title,
                body,
                source,
            } => Ok(vec![CoreEvent::NotificationPushRequested {
                ws_id,
                surface_id,
                title,
                body,
                source,
            }]),
            #[cfg(feature = "gui")]
            DomainIntent::MarkNotificationRead { id } => {
                Ok(vec![CoreEvent::NotificationReadRequested { id }])
            }
            #[cfg(feature = "gui")]
            DomainIntent::MarkAllNotificationsRead => {
                Ok(vec![CoreEvent::AllNotificationsReadRequested])
            }
            #[cfg(feature = "gui")]
            DomainIntent::SurfaceCwdChanged {
                surface_id,
                generation,
            } => {
                if !engine
                    .runtime
                    .terminals
                    .matches_generation(surface_id, generation)
                {
                    return Ok(Vec::new());
                }
                Ok(vec![CoreEvent::SurfaceCwdChanged {
                    surface_id,
                    generation,
                }])
            }
            DomainIntent::SetTerminalMark { surface_id } => {
                Ok(vec![CoreEvent::TerminalMarkSet { surface_id }])
            }
            DomainIntent::SurfaceCompletion { surface_id, kind } => {
                Ok(vec![CoreEvent::SurfaceCompletionRequested {
                    surface_id,
                    kind,
                }])
            }
            DomainIntent::SurfaceAttentionClear { surface_id, kind } => {
                Ok(vec![CoreEvent::SurfaceAttentionClearRequested {
                    surface_id,
                    kind,
                }])
            }
            DomainIntent::SendToSurface {
                surface_id,
                payload,
            } => Ok(vec![Self::apply_send_to_surface(
                engine, surface_id, payload,
            )]),

            #[cfg(feature = "gui")]
            DomainIntent::UpdateTabName {
                surface_id,
                name,
                generation,
            } => {
                if !engine
                    .runtime
                    .terminals
                    .matches_generation(surface_id, generation)
                {
                    return Ok(Vec::new());
                }
                Ok(vec![Self::apply_update_tab_name(engine, surface_id, name)])
            }
            // 결과를 이벤트 루프로 돌려주는 identify worker는 GUI에만 있다.
            #[cfg(feature = "gui")]
            DomainIntent::DispatchFile {
                target,
                depth,
                origin_surface_id,
                dispatch_origin,
                ignore_size_limit,
            } => {
                if let Some(sid) = origin_surface_id {
                    crate::core::origin::require_origin_pane(engine, sid)
                        .map_err(anyhow::Error::msg)?;
                }
                // mirror origin의 경로는 원격 파일이다. 로컬 파일시스템으로 판정하지 않는다(ADR-0022).
                let depth = if origin_surface_id.is_some_and(|sid| engine.is_mirror_surface(sid)) {
                    crate::file::format::DetectDepth::Name
                } else {
                    depth
                };
                match engine.runtime.identify_worker.as_ref() {
                    Some(worker) => {
                        worker.spawn_identify(
                            target,
                            depth,
                            origin_surface_id,
                            dispatch_origin,
                            ignore_size_limit,
                        );
                    }
                    None => {
                        tracing::warn!(
                            target = %target.display(),
                            "DispatchFile: identify_worker not injected — drop",
                        );
                    }
                }
                Ok(vec![])
            }
            _ => anyhow::bail!("structural intent requires committed journal admission"),
        }
    }
}
