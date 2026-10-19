//! attach client가 보낸 입력 사건을 그 surface를 가진 engine에 적용한다. 받는 engine이 없으면 mesh 오류로 답한다.
use crate::app::App;
use tasty_ipc::stream_hub::{InputEvent, StreamHub};

impl App {
    pub(super) fn apply_input_events_batch(
        &mut self,
        events: impl IntoIterator<Item = (u32, InputEvent)>,
        hub: &StreamHub,
    ) {
        for (client, event) in events {
            match event {
                InputEvent::Data(bytes) => self.apply_input_frames_batch([(client, bytes)]),
                InputEvent::Mesh { surface_id, input } => {
                    if !self.any_engine(|engine| {
                        engine.apply_attached_mesh_input(surface_id, client, input.clone())
                    }) {
                        super::event_handler::reply_mesh_error(
                            hub,
                            client,
                            surface_id,
                            "not_attached",
                        );
                    }
                }
                InputEvent::Context(context) => {
                    if !self.any_engine(|engine| {
                        engine.apply_attached_mesh_context(
                            context.surface_id,
                            client,
                            context.width_px,
                            context.height_px,
                            context.pixels_per_point,
                            context.theme.clone(),
                            context.focused,
                        )
                    }) {
                        super::event_handler::reply_mesh_error(
                            hub,
                            client,
                            context.surface_id,
                            "not_attached",
                        );
                    }
                }
                InputEvent::Checkpoint(sequence) => hub.acknowledge_input(client, sequence),
            }
        }
    }
}
