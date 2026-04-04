use super::{AppState, SurfaceMessage};

impl AppState {
    /// Send a message from one surface to another. Returns the assigned message ID.
    pub fn send_message(&mut self, from: u32, to: u32, content: String) -> u32 {
        self.engine.surface_next_message_id += 1;
        let id = self.engine.surface_next_message_id;
        let msg = SurfaceMessage { id, from_surface_id: from, content };
        self.engine.surface_messages.entry(to).or_default().push(msg);
        id
    }

    /// Read (and optionally consume) messages queued for a surface.
    /// If `from` is Some, only return messages from that sender.
    /// If `peek` is false, the returned messages are removed from the queue.
    pub fn read_messages(&mut self, surface_id: u32, from: Option<u32>, peek: bool) -> Vec<SurfaceMessage> {
        let queue = match self.engine.surface_messages.get_mut(&surface_id) {
            Some(q) => q,
            None => return vec![],
        };

        if peek {
            queue
                .iter()
                .filter(|m| from.map_or(true, |f| m.from_surface_id == f))
                .cloned()
                .collect()
        } else {
            let mut retained = Vec::new();
            let mut taken = Vec::new();
            for msg in queue.drain(..) {
                if from.map_or(true, |f| msg.from_surface_id == f) {
                    taken.push(msg);
                } else {
                    retained.push(msg);
                }
            }
            *queue = retained;
            taken
        }
    }

    /// Count messages queued for a surface.
    pub fn message_count(&self, surface_id: u32) -> usize {
        self.engine.surface_messages.get(&surface_id).map(|v| v.len()).unwrap_or(0)
    }

    /// Clear all messages queued for a surface.
    pub fn clear_messages(&mut self, surface_id: u32) {
        self.engine.surface_messages.remove(&surface_id);
    }
}
